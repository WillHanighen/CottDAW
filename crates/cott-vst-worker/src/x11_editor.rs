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
use tracing::{debug, info};
use x11_dl::xlib::{
    ButtonMotionMask, ButtonPressMask, ButtonReleaseMask, CWBackPixel, CWBorderPixel, CWColormap,
    CWEventMask, CWOverrideRedirect, ClientMessage, Display, ExposureMask, False, InputOutput,
    KeyPressMask, PointerMotionMask, StructureNotifyMask, SubstructureNotifyMask, True, Window,
    XEvent, XSetWindowAttributes, Xlib,
};

const DEFAULT_W: u32 = 800;
const DEFAULT_H: u32 = 600;

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
            let white = (xlib.XWhitePixel)(display, screen);
            let depth = (xlib.XDefaultDepth)(display, screen);
            let visual = (xlib.XDefaultVisual)(display, screen);
            let colormap = (xlib.XDefaultColormap)(display, screen);

            let mut attrs: XSetWindowAttributes = MaybeUninit::zeroed().assume_init();
            attrs.background_pixel = white;
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

            // Embed parent: child of shell, what VST3 attaches into.
            // Include motion masks so drag gestures reach plugin child windows
            // that rely on the embedder's event mask / XEmbed focus path.
            attrs.override_redirect = True;
            attrs.event_mask = ExposureMask
                | KeyPressMask
                | ButtonPressMask
                | ButtonReleaseMask
                | PointerMotionMask
                | ButtonMotionMask
                | SubstructureNotifyMask
                | StructureNotifyMask;
            let embed_mask =
                CWBackPixel | CWBorderPixel | CWColormap | CWEventMask | CWOverrideRedirect;
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
            (xlib.XFlush)(display);

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

    /// Drain pending X events. Returns `false` if the user closed the window.
    pub fn pump_events(&mut self) -> bool {
        if self.user_closed {
            return false;
        }
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
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
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
