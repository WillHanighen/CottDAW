//! Floating X11 parent window for VST3 `kPlatformTypeX11EmbedWindowID`.
//!
//! Creates a top-level WM window and passes it as the embed parent. JUCE OpenGL
//! editors (Surge XT) need:
//! 1. A GLX-capable visual/colormap on that parent
//! 2. XEmbed handshake (`XEMBED_EMBEDDED_NOTIFY` + map) when the plugin child appears
//!
//! Nested non-GLX intermediate windows commonly produce a black view under
//! NVIDIA + XWayland even when audio works.

use anyhow::{Context, Result, anyhow};
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_long, c_uint, c_ulong};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use x11_dl::xlib::{
    Atom, CWBackPixel, CWBorderPixel, CWColormap, CWEventMask, ClientMessage, ClientMessageData,
    ConfigureNotify, CreateNotify, CurrentTime, Display, ExposureMask, False, InputOutput,
    NoEventMask, PropertyChangeMask, ReparentNotify, StructureNotifyMask, SubstructureNotifyMask,
    True, Window, XA_CARDINAL, XClientMessageEvent, XEvent, XSetWindowAttributes, Xlib,
};

/// Fallback when a plugin does not report a size before attach.
const DEFAULT_W: u32 = 1024;
const DEFAULT_H: u32 = 700;
const BG_PIXEL: u64 = 0x0016_181c; // #16181c

const XEMBED_EMBEDDED_NOTIFY: c_long = 0;
const XEMBED_WINDOW_ACTIVATE: c_long = 1;
const XEMBED_FOCUS_IN: c_long = 4;
const XEMBED_FOCUS_CURRENT: c_long = 0;
const XEMBED_MAPPED: c_long = 1 << 0;
const MAX_XEMBED_VERSION: c_long = 0;

pub struct FloatingEditorWindow {
    xlib: Xlib,
    display: *mut Display,
    /// Top-level WM window — also the X11Embed parent (no nested intermediate).
    shell: Window,
    wm_delete: c_ulong,
    xembed_msg: Atom,
    xembed_info: Atom,
    /// Plugin-created client window currently adopted for XEmbed.
    client: Window,
    width: u32,
    height: u32,
    user_closed: bool,
    used_glx_visual: bool,
}

/// Result of draining X events for the floating editor.
#[derive(Debug, Clone, Copy)]
pub struct EditorPumpResult {
    pub closed: bool,
    pub resized_to: Option<(u32, u32)>,
}

unsafe impl Send for FloatingEditorWindow {}

impl FloatingEditorWindow {
    pub fn create(title: &str, width: u32, height: u32) -> Result<Self> {
        let width = width.max(80);
        let height = height.max(80);
        let xlib = Xlib::open().context("load libX11")?;

        unsafe {
            // MUST be before any other Xlib call in this process. JUCE OpenGL
            // editors (Surge XT) create their own Display + GL thread; without
            // XInitThreads the child window stays blank forever.
            let threads_ok = (xlib.XInitThreads)();
            if threads_ok == 0 {
                warn!("XInitThreads failed — OpenGL plugin editors may stay black");
            } else {
                info!("XInitThreads enabled for plugin editor hosting");
            }

            let display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return Err(anyhow!(
                    "XOpenDisplay failed — run under X11/XWayland (WINIT_UNIX_BACKEND=x11)"
                ));
            }

            let screen = (xlib.XDefaultScreen)(display);
            let root = (xlib.XRootWindow)(display, screen);
            let black = (xlib.XBlackPixel)(display, screen);

            // Default visual/colormap: JUCE creates its own GLX child window.
            let used_glx_visual = false;
            let visual = (xlib.XDefaultVisual)(display, screen);
            let depth = (xlib.XDefaultDepth)(display, screen);
            let colormap = (xlib.XDefaultColormap)(display, screen);
            info!(depth, "using default visual for floating editor parent");

            let mut swa: XSetWindowAttributes = MaybeUninit::zeroed().assume_init();
            swa.background_pixel = BG_PIXEL;
            swa.border_pixel = black;
            swa.colormap = colormap;
            swa.event_mask = ExposureMask
                | StructureNotifyMask
                | SubstructureNotifyMask
                | PropertyChangeMask;

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
                &mut swa,
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

            let xembed_msg = {
                let name = CString::new("_XEMBED").unwrap();
                (xlib.XInternAtom)(display, name.as_ptr(), False)
            };
            let xembed_info = {
                let name = CString::new("_XEMBED_INFO").unwrap();
                (xlib.XInternAtom)(display, name.as_ptr(), False)
            };

            (xlib.XMapWindow)(display, shell);
            (xlib.XSync)(display, False);

            info!(
                title,
                width,
                height,
                shell,
                used_glx_visual,
                "created floating X11 editor parent (shell=X11Embed)"
            );

            Ok(Self {
                xlib,
                display,
                shell,
                wm_delete,
                xembed_msg,
                xembed_info,
                client: 0,
                width,
                height,
                user_closed: false,
                used_glx_visual,
            })
        }
    }

    pub fn create_default(title: &str) -> Result<Self> {
        Self::create(title, DEFAULT_W, DEFAULT_H)
    }

    /// Window id passed to `WindowHandle::X11` / `IPlugView::attached`.
    pub fn embed_window_id(&self) -> u64 {
        self.shell as u64
    }

    pub fn used_glx_visual(&self) -> bool {
        self.used_glx_visual
    }

    pub fn sync(&self) {
        unsafe {
            (self.xlib.XSync)(self.display, False);
        }
    }

    pub fn embed_child_count(&self) -> u32 {
        unsafe {
            let mut root: Window = 0;
            let mut parent: Window = 0;
            let mut children: *mut Window = ptr::null_mut();
            let mut nchildren: c_uint = 0;
            if (self.xlib.XQueryTree)(
                self.display,
                self.shell,
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

    /// Adopt plugin-created children and run XEmbed handshake.
    pub fn sync_xembed_clients(&mut self) {
        for child in self.list_shell_children() {
            if child != 0 && child != self.shell {
                self.adopt_xembed_client(child);
            }
        }
    }

    /// Log geometry / map state of the adopted client (diagnostics).
    pub fn log_client_state(&self) {
        if self.client == 0 {
            info!(
                children = self.embed_child_count(),
                "XEmbed: no client adopted yet"
            );
            return;
        }
        unsafe {
            let mut attrs = MaybeUninit::<x11_dl::xlib::XWindowAttributes>::uninit();
            if (self.xlib.XGetWindowAttributes)(self.display, self.client, attrs.as_mut_ptr()) == 0
            {
                warn!(client = self.client, "XGetWindowAttributes failed");
                return;
            }
            let attrs = attrs.assume_init();
            info!(
                client = self.client,
                x = attrs.x,
                y = attrs.y,
                width = attrs.width,
                height = attrs.height,
                map_state = attrs.map_state,
                depth = attrs.depth,
                glx_parent = self.used_glx_visual,
                "XEmbed client window state"
            );
        }
    }

    fn list_shell_children(&self) -> Vec<Window> {
        unsafe {
            let mut root: Window = 0;
            let mut parent: Window = 0;
            let mut children: *mut Window = ptr::null_mut();
            let mut nchildren: c_uint = 0;
            if (self.xlib.XQueryTree)(
                self.display,
                self.shell,
                &mut root,
                &mut parent,
                &mut children,
                &mut nchildren,
            ) == 0
                || children.is_null()
            {
                return Vec::new();
            }
            let slice = std::slice::from_raw_parts(children, nchildren as usize);
            let out = slice.to_vec();
            (self.xlib.XFree)(children as *mut _);
            out
        }
    }

    fn adopt_xembed_client(&mut self, client: Window) {
        if client == 0 {
            return;
        }
        if client == self.client {
            self.map_and_size_client(client);
            return;
        }

        self.client = client;
        info!(
            client,
            shell = self.shell,
            "adopting XEmbed client window for plugin editor"
        );

        unsafe {
            (self.xlib.XSelectInput)(
                self.display,
                client,
                StructureNotifyMask | PropertyChangeMask,
            );
        }

        self.map_and_size_client(client);
        self.send_xembed(
            client,
            XEMBED_EMBEDDED_NOTIFY,
            0,
            self.shell as c_long,
            MAX_XEMBED_VERSION,
        );
        self.send_xembed(client, XEMBED_WINDOW_ACTIVATE, 0, 0, 0);
        self.send_xembed(client, XEMBED_FOCUS_IN, XEMBED_FOCUS_CURRENT, 0, 0);
        self.sync();
        self.log_client_state();
    }

    fn map_and_size_client(&self, client: Window) {
        unsafe {
            let _ = self.client_wants_mapped(client);
            (self.xlib.XMapWindow)(self.display, client);
            (self.xlib.XMapRaised)(self.display, client);
            (self.xlib.XMoveResizeWindow)(
                self.display,
                client,
                0,
                0,
                self.width as c_uint,
                self.height as c_uint,
            );
            (self.xlib.XFlush)(self.display);
        }
    }

    fn client_wants_mapped(&self, client: Window) -> bool {
        unsafe {
            let mut actual_type: Atom = 0;
            let mut actual_format: c_int = 0;
            let mut nitems: c_ulong = 0;
            let mut bytes_after: c_ulong = 0;
            let mut prop: *mut u8 = ptr::null_mut();
            let status = (self.xlib.XGetWindowProperty)(
                self.display,
                client,
                self.xembed_info,
                0,
                2,
                False,
                XA_CARDINAL,
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            );
            if status != 0 || prop.is_null() || actual_format != 32 || nitems < 2 {
                if !prop.is_null() {
                    (self.xlib.XFree)(prop as *mut _);
                }
                return true;
            }
            let longs = prop as *const c_long;
            let flags = *longs.add(1);
            (self.xlib.XFree)(prop as *mut _);
            (flags & XEMBED_MAPPED) != 0
        }
    }

    fn send_xembed(
        &self,
        client: Window,
        opcode: c_long,
        detail: c_long,
        data1: c_long,
        data2: c_long,
    ) {
        unsafe {
            let mut data = ClientMessageData::new();
            data.set_long(0, CurrentTime as c_long);
            data.set_long(1, opcode);
            data.set_long(2, detail);
            data.set_long(3, data1);
            data.set_long(4, data2);

            let mut event = XEvent {
                client_message: XClientMessageEvent {
                    type_: ClientMessage,
                    serial: 0,
                    send_event: True,
                    display: self.display,
                    window: client,
                    message_type: self.xembed_msg,
                    format: 32,
                    data,
                },
            };
            (self.xlib.XSendEvent)(self.display, client, False, NoEventMask, &mut event);
            debug!(client, opcode, "sent XEmbed message");
        }
    }

    pub fn wait_for_embed_children_gone(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let _ = self.pump_events();
            self.sync();
            let n = self.embed_child_count();
            if n == 0 {
                debug!("shell has no plugin children — safe to destroy parent");
                self.client = 0;
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

    pub fn raise_embed(&self) {
        unsafe {
            (self.xlib.XMapRaised)(self.display, self.shell);
            if self.client != 0 {
                (self.xlib.XMapRaised)(self.display, self.client);
            }
            (self.xlib.XFlush)(self.display);
        }
    }

    pub fn sync_wine_coordinates(&self) {
        unsafe {
            let mut root: Window = 0;
            let mut x = 0i32;
            let mut y = 0i32;
            let mut width = 0u32;
            let mut height = 0u32;
            let mut border = 0u32;
            let mut depth = 0u32;
            if (self.xlib.XGetGeometry)(
                self.display,
                self.shell,
                &mut root,
                &mut x,
                &mut y,
                &mut width,
                &mut height,
                &mut border,
                &mut depth,
            ) == 0
            {
                return;
            }
            (self.xlib.XMoveWindow)(self.display, self.shell, x + 1, y);
            (self.xlib.XSync)(self.display, False);
            (self.xlib.XMoveWindow)(self.display, self.shell, x, y);
            (self.xlib.XSync)(self.display, False);
            debug!(x, y, "nudged floating editor to sync Wine mouse coords");
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
            if self.client != 0 {
                (self.xlib.XMoveResizeWindow)(
                    self.display,
                    self.client,
                    0,
                    0,
                    width as c_uint,
                    height as c_uint,
                );
            }
            (self.xlib.XFlush)(self.display);
        }
        debug!(width, height, "resized floating editor window");
    }

    fn sync_shell_size(&mut self, width: u32, height: u32) {
        let width = width.max(80);
        let height = height.max(80);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        unsafe {
            if self.client != 0 {
                (self.xlib.XMoveResizeWindow)(
                    self.display,
                    self.client,
                    0,
                    0,
                    width as c_uint,
                    height as c_uint,
                );
            }
            (self.xlib.XFlush)(self.display);
        }
        debug!(width, height, "synced client to shell ConfigureNotify");
    }

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
                                self.sync_shell_size(w, h);
                                resized_to = Some((w, h));
                            }
                        }
                    }
                    t if t == CreateNotify => {
                        let ev = event.create_window;
                        if ev.parent == self.shell && ev.window != self.shell {
                            debug!(window = ev.window, "CreateNotify under shell");
                            self.adopt_xembed_client(ev.window);
                        }
                    }
                    t if t == ReparentNotify => {
                        let ev = event.reparent;
                        if ev.parent == self.shell && ev.window != self.shell {
                            debug!(window = ev.window, "ReparentNotify into shell");
                            self.adopt_xembed_client(ev.window);
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
                if self.shell != 0 {
                    (self.xlib.XDestroyWindow)(self.display, self.shell);
                }
                (self.xlib.XFlush)(self.display);
                (self.xlib.XCloseDisplay)(self.display);
                self.display = ptr::null_mut();
                self.shell = 0;
                self.client = 0;
            }
        }
    }
}
