//! Floating X11 parent window for VST3 `kPlatformTypeX11EmbedWindowID`.
//!
//! When the host does not supply an embed parent (egui has no easy X11 id),
//! the worker creates a top-level shell plus an embed child, matching the
//! Steinberg editorhost layout.

use anyhow::{Context, Result, anyhow};
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::raw::{c_uint, c_ulong};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use x11_dl::xlib::{
    ButtonMotionMask, ButtonPressMask, ButtonReleaseMask, CWBackPixel, CWBorderPixel, CWColormap,
    CWEventMask, ClientMessage, ConfigureNotify, Display, ExposureMask, False, InputOutput,
    KeyPressMask, PointerMotionMask, StructureNotifyMask, SubstructureNotifyMask, Window, XEvent,
    XSetWindowAttributes, Xlib,
};

/// Match CottSynth's default egui size so we rarely need a post-attach resize.
const DEFAULT_W: u32 = 440;
const DEFAULT_H: u32 = 520;
/// Match CottSynth / dark plugin UIs so WM resize doesn't flash white.
const BG_PIXEL: u64 = 0x0016_181c; // #16181c

pub struct FloatingEditorWindow {
    xlib: Xlib,
    display: *mut Display,
    /// Top-level window managed by the WM (title bar, close button).
    shell: Window,
    /// Child passed to `IPlugView::attached` as `X11EmbedWindowID`.
    embed: Window,
    wm_delete: c_ulong,
    width: u32,
    height: u32,
    /// Set when the user closes the shell via the window manager.
    user_closed: bool,
}

/// Result of draining X events for the floating editor.
#[derive(Debug, Clone, Copy)]
pub struct EditorPumpResult {
    pub closed: bool,
    /// New shell size from `ConfigureNotify`, if any.
    pub resized_to: Option<(u32, u32)>,
}

// Xlib Display is not Send across threads; we keep it on the worker UI thread.
unsafe impl Send for FloatingEditorWindow {}

impl FloatingEditorWindow {
    pub fn create(title: &str, width: u32, height: u32) -> Result<Self> {
        let width = width.max(80);
        let height = height.max(80);
        let xlib = Xlib::open().context("load libX11")?;

        unsafe {
            let display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return Err(anyhow!(
                    "XOpenDisplay failed — run under X11/XWayland (WINIT_UNIX_BACKEND=x11)"
                ));
            }

            let screen = (xlib.XDefaultScreen)(display);
            let root = (xlib.XRootWindow)(display, screen);
            let black = (xlib.XBlackPixel)(display, screen);
            let depth = (xlib.XDefaultDepth)(display, screen);
            let visual = (xlib.XDefaultVisual)(display, screen);
            let colormap = (xlib.XDefaultColormap)(display, screen);

            let mut attrs: XSetWindowAttributes = MaybeUninit::zeroed().assume_init();
            attrs.background_pixel = BG_PIXEL;
            attrs.border_pixel = black;
            attrs.colormap = colormap;
            attrs.event_mask = ExposureMask | StructureNotifyMask | SubstructureNotifyMask;

            let valuemask = CWBackPixel | CWBorderPixel | CWColormap | CWEventMask;
            let shell = (xlib.XCreateWindow)(
                display,
                root,
                100,
                100,
                width as c_uint,
                height as c_uint,
                1,
                depth,
                InputOutput as c_uint,
                visual,
                valuemask,
                &mut attrs,
            );
            if shell == 0 {
                (xlib.XCloseDisplay)(display);
                return Err(anyhow!("XCreateWindow (shell) failed"));
            }

            let title_c = CString::new(title).unwrap_or_else(|_| CString::new("Plugin").unwrap());
            (xlib.XStoreName)(display, shell, title_c.as_ptr());

            let wm_delete = {
                let atom_name = CString::new("WM_DELETE_WINDOW").unwrap();
                (xlib.XInternAtom)(display, atom_name.as_ptr(), False)
            };
            let mut protocols = [wm_delete];
            (xlib.XSetWMProtocols)(display, shell, protocols.as_mut_ptr(), 1);

            // Embed parent: normal child of shell (NOT override_redirect).
            // override_redirect + GL child often paints black under NVIDIA/XWayland.
            attrs.event_mask = ExposureMask
                | KeyPressMask
                | ButtonPressMask
                | ButtonReleaseMask
                | PointerMotionMask
                | ButtonMotionMask
                | SubstructureNotifyMask
                | StructureNotifyMask;
            let embed_mask = CWBackPixel | CWBorderPixel | CWColormap | CWEventMask;
            let embed = (xlib.XCreateWindow)(
                display,
                shell,
                0,
                0,
                width as c_uint,
                height as c_uint,
                0,
                depth,
                InputOutput as c_uint,
                visual,
                embed_mask,
                &mut attrs,
            );
            if embed == 0 {
                (xlib.XDestroyWindow)(display, shell);
                (xlib.XCloseDisplay)(display);
                return Err(anyhow!("XCreateWindow (embed) failed"));
            }

            (xlib.XMapWindow)(display, embed);
            (xlib.XMapWindow)(display, shell);
            // Ensure the server has created/mapped the windows before a VST3
            // view attaches GL to `embed` — otherwise reopen can yield a black box.
            (xlib.XSync)(display, False);

            info!(
                title,
                width, height, shell, embed, "created floating X11 editor parent"
            );

            Ok(Self {
                xlib,
                display,
                shell,
                embed,
                wm_delete,
                width,
                height,
                user_closed: false,
            })
        }
    }

    pub fn create_default(title: &str) -> Result<Self> {
        Self::create(title, DEFAULT_W, DEFAULT_H)
    }

    /// Window id passed to `WindowHandle::X11` / `IPlugView::attached`.
    pub fn embed_window_id(&self) -> u64 {
        self.embed as u64
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Flush + sync the X connection (use before destroy / after map).
    pub fn sync(&self) {
        unsafe {
            (self.xlib.XSync)(self.display, False);
        }
    }

    /// Number of direct children of the embed window (baseview's GL window).
    pub fn embed_child_count(&self) -> u32 {
        unsafe {
            let mut root: Window = 0;
            let mut parent: Window = 0;
            let mut children: *mut Window = ptr::null_mut();
            let mut nchildren: c_uint = 0;
            if (self.xlib.XQueryTree)(
                self.display,
                self.embed,
                &mut root,
                &mut parent,
                &mut children,
                &mut nchildren,
            ) == 0
            {
                return 0;
            }
            if !children.is_null() {
                (self.xlib.XFree)(children as *mut _);
            }
            nchildren
        }
    }

    /// After `IPlugView::removed`, baseview only sets a close atomic — its event
    /// loop thread may still hold a GL context on a child of `embed`. Destroying
    /// the parent while that thread is alive (and XID-recycling the next open)
    /// produces a black editor. Wait until the child is gone.
    pub fn wait_for_embed_children_gone(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let _ = self.pump_events();
            self.sync();
            let n = self.embed_child_count();
            if n == 0 {
                debug!("embed has no plugin children — safe to destroy parent");
                return true;
            }
            if Instant::now() >= deadline {
                warn!(
                    children = n,
                    "timed out waiting for plugin GL child to detach"
                );
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Ensure the embed (and its GL child) is mapped and on top of the shell.
    pub fn raise_embed(&self) {
        unsafe {
            (self.xlib.XMapRaised)(self.display, self.embed);
            (self.xlib.XFlush)(self.display);
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(80);
        let height = height.max(80);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        unsafe {
            (self.xlib.XResizeWindow)(self.display, self.shell, width as c_uint, height as c_uint);
            (self.xlib.XResizeWindow)(self.display, self.embed, width as c_uint, height as c_uint);
            (self.xlib.XFlush)(self.display);
        }
        debug!(width, height, "resized floating editor window");
    }

    /// Keep the embed child matched to the shell after a WM `ConfigureNotify`.
    fn sync_embed_to_shell_size(&mut self, width: u32, height: u32) {
        let width = width.max(80);
        let height = height.max(80);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        unsafe {
            (self.xlib.XResizeWindow)(self.display, self.embed, width as c_uint, height as c_uint);
            (self.xlib.XFlush)(self.display);
        }
        debug!(width, height, "synced embed to shell ConfigureNotify");
    }

    /// Drain pending X events.
    pub fn pump_events(&mut self) -> EditorPumpResult {
        if self.user_closed {
            return EditorPumpResult {
                closed: true,
                resized_to: None,
            };
        }
        let mut resized_to = None;
        unsafe {
            while (self.xlib.XPending)(self.display) > 0 {
                let mut event = MaybeUninit::<XEvent>::uninit();
                (self.xlib.XNextEvent)(self.display, event.as_mut_ptr());
                let event = event.assume_init();
                match event.get_type() {
                    t if t == ClientMessage => {
                        let cm = event.client_message;
                        if cm.window == self.shell
                            && cm.data.get_long(0) as c_ulong == self.wm_delete
                        {
                            info!("editor window closed by user");
                            self.user_closed = true;
                            return EditorPumpResult {
                                closed: true,
                                resized_to: None,
                            };
                        }
                    }
                    t if t == ConfigureNotify => {
                        let cfg = event.configure;
                        if cfg.window == self.shell {
                            let w = cfg.width.max(1) as u32;
                            let h = cfg.height.max(1) as u32;
                            if w != self.width || h != self.height {
                                self.sync_embed_to_shell_size(w, h);
                                resized_to = Some((w, h));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        EditorPumpResult {
            closed: false,
            resized_to,
        }
    }
}

impl Drop for FloatingEditorWindow {
    fn drop(&mut self) {
        unsafe {
            if !self.display.is_null() {
                if self.embed != 0 {
                    (self.xlib.XDestroyWindow)(self.display, self.embed);
                }
                if self.shell != 0 {
                    (self.xlib.XDestroyWindow)(self.display, self.shell);
                }
                (self.xlib.XFlush)(self.display);
                (self.xlib.XCloseDisplay)(self.display);
                self.display = ptr::null_mut();
                self.shell = 0;
                self.embed = 0;
            }
        }
    }
}
