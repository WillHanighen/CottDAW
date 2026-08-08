//! Host-side VST3 callbacks: parameter sync, Linux run loop, host context.
//!
//! Without [`IComponentHandler`], GUI edits never reach the audio processor.
//! Without Linux [`IRunLoop`], plugin UI timers (and often drag interactions)
//! never fire.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vst3::Steinberg::Linux::{
    FileDescriptor, IEventHandler, IEventHandlerTrait, IRunLoop, IRunLoopTrait, ITimerHandler,
    ITimerHandlerTrait, TimerInterval,
};
use vst3::Steinberg::Vst::{
    IComponentHandler, IComponentHandlerTrait, IHostApplication, IHostApplicationTrait,
    IParamValueQueue, IParamValueQueueTrait, IParameterChanges, IParameterChangesTrait, ParamID,
    ParamValue, String128,
};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, TUID, ViewRect, kNoInterface,
    kResultFalse, kResultOk,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

// ---------------------------------------------------------------------------
// Pending GUI → processor parameter changes
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct PendingParams {
    inner: Arc<Mutex<Vec<(ParamID, ParamValue)>>>,
}

impl PendingParams {
    pub fn push(&self, id: ParamID, value: ParamValue) {
        if let Ok(mut q) = self.inner.lock() {
            // Collapse duplicates — keep the latest value for this id.
            if let Some(slot) = q.iter_mut().find(|(i, _)| *i == id) {
                slot.1 = value;
            } else {
                q.push((id, value));
            }
        }
    }

    pub fn take(&self) -> Vec<(ParamID, ParamValue)> {
        self.inner
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// IComponentHandler — receives beginEdit / performEdit / endEdit from GUI
// ---------------------------------------------------------------------------

pub struct ComponentHandler {
    pending: PendingParams,
}

impl ComponentHandler {
    pub fn new(pending: PendingParams) -> Self {
        Self { pending }
    }
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, _id: ParamID) -> i32 {
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value_normalized: ParamValue) -> i32 {
        self.pending.push(id, value_normalized);
        kResultOk
    }

    unsafe fn endEdit(&self, _id: ParamID) -> i32 {
        kResultOk
    }

    unsafe fn restartComponent(&self, _flags: i32) -> i32 {
        // Bus/latency restarts are not yet wired; acknowledge so plugins
        // don't treat the host as broken.
        kResultOk
    }
}

/// Create the handler COM object and return `(pending queue, keep-alive ptr)`.
pub fn install_component_handler(
    controller: &ComPtr<vst3::Steinberg::Vst::IEditController>,
) -> Result<(PendingParams, ComPtr<IComponentHandler>), i32> {
    use vst3::Steinberg::Vst::IEditControllerTrait;

    let pending = PendingParams::default();
    let wrapper = ComWrapper::new(ComponentHandler::new(pending.clone()));
    let handler = wrapper
        .to_com_ptr::<IComponentHandler>()
        .ok_or(kResultFalse)?;
    let status = unsafe { controller.setComponentHandler(handler.as_ptr()) };
    if status != kResultOk {
        return Err(status);
    }
    Ok((pending, handler))
}

// ---------------------------------------------------------------------------
// IParameterChanges / IParamValueQueue — delivered in process()
// ---------------------------------------------------------------------------

struct ParamValueQueueImpl {
    id: ParamID,
    points: RefCell<Vec<(i32, ParamValue)>>,
}

impl Class for ParamValueQueueImpl {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamValueQueueImpl {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }

    unsafe fn getPointCount(&self) -> i32 {
        i32::try_from(self.points.borrow().len()).unwrap_or(i32::MAX)
    }

    unsafe fn getPoint(
        &self,
        index: i32,
        sample_offset: *mut i32,
        value: *mut ParamValue,
    ) -> i32 {
        if index < 0 || sample_offset.is_null() || value.is_null() {
            return kResultFalse;
        }
        let points = self.points.borrow();
        let Some(&(offset, val)) = points.get(index as usize) else {
            return kResultFalse;
        };
        unsafe {
            *sample_offset = offset;
            *value = val;
        }
        kResultOk
    }

    unsafe fn addPoint(
        &self,
        sample_offset: i32,
        value: ParamValue,
        index: *mut i32,
    ) -> i32 {
        let mut points = self.points.borrow_mut();
        let idx = points.len();
        points.push((sample_offset, value));
        if !index.is_null() {
            unsafe {
                *index = i32::try_from(idx).unwrap_or(i32::MAX);
            }
        }
        kResultOk
    }
}

pub struct ParameterChangesImpl {
    queues: RefCell<Vec<ComPtr<IParamValueQueue>>>,
}

impl ParameterChangesImpl {
    fn empty() -> Self {
        Self {
            queues: RefCell::new(Vec::new()),
        }
    }

    fn from_pending(pending: Vec<(ParamID, ParamValue)>) -> Self {
        let mut queues = Vec::new();
        for (id, value) in pending {
            let wrapper = ComWrapper::new(ParamValueQueueImpl {
                id,
                points: RefCell::new(vec![(0, value)]),
            });
            if let Some(ptr) = wrapper.to_com_ptr::<IParamValueQueue>() {
                queues.push(ptr);
            }
        }
        Self {
            queues: RefCell::new(queues),
        }
    }
}

impl Class for ParameterChangesImpl {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for ParameterChangesImpl {
    unsafe fn getParameterCount(&self) -> i32 {
        i32::try_from(self.queues.borrow().len()).unwrap_or(i32::MAX)
    }

    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        if index < 0 {
            return std::ptr::null_mut();
        }
        self.queues
            .borrow()
            .get(index as usize)
            .map(ComPtr::as_ptr)
            .unwrap_or(std::ptr::null_mut())
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut i32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        let id = unsafe { *id };

        {
            let queues = self.queues.borrow();
            for (i, q) in queues.iter().enumerate() {
                if unsafe { q.getParameterId() } == id {
                    if !index.is_null() {
                        unsafe {
                            *index = i32::try_from(i).unwrap_or(i32::MAX);
                        }
                    }
                    return q.as_ptr();
                }
            }
        }

        let wrapper = ComWrapper::new(ParamValueQueueImpl {
            id,
            points: RefCell::new(Vec::new()),
        });
        let Some(ptr) = wrapper.to_com_ptr::<IParamValueQueue>() else {
            return std::ptr::null_mut();
        };
        let raw = ptr.as_ptr();
        let mut queues = self.queues.borrow_mut();
        if !index.is_null() {
            unsafe {
                *index = i32::try_from(queues.len()).unwrap_or(i32::MAX);
            }
        }
        queues.push(ptr);
        raw
    }
}

/// Build input + output parameter change lists for one `process()` call.
///
/// Returned `ComWrapper`s must stay alive for the duration of `process()`.
pub fn make_process_param_changes(
    pending: Vec<(ParamID, ParamValue)>,
) -> (
    ComWrapper<ParameterChangesImpl>,
    ComPtr<IParameterChanges>,
    ComWrapper<ParameterChangesImpl>,
    ComPtr<IParameterChanges>,
) {
    let input = ComWrapper::new(ParameterChangesImpl::from_pending(pending));
    let input_ptr = input
        .to_com_ptr::<IParameterChanges>()
        .expect("ParameterChangesImpl exposes IParameterChanges");
    let output = ComWrapper::new(ParameterChangesImpl::empty());
    let output_ptr = output
        .to_com_ptr::<IParameterChanges>()
        .expect("ParameterChangesImpl exposes IParameterChanges");
    (input, input_ptr, output, output_ptr)
}

// ---------------------------------------------------------------------------
// Linux IRunLoop — timers + FD handlers for plugin UI
// ---------------------------------------------------------------------------

struct TimerEntry {
    handler: ComPtr<ITimerHandler>,
    interval: Duration,
    next_fire: Instant,
}

struct FdEntry {
    fd: FileDescriptor,
    handler: ComPtr<IEventHandler>,
}

pub(crate) struct RunLoopState {
    timers: Mutex<Vec<TimerEntry>>,
    fds: Mutex<Vec<FdEntry>>,
}

impl Default for RunLoopState {
    fn default() -> Self {
        Self {
            timers: Mutex::new(Vec::new()),
            fds: Mutex::new(Vec::new()),
        }
    }
}

impl RunLoopState {
    fn pump(&self) {
        let now = Instant::now();
        let due: Vec<ComPtr<ITimerHandler>> = {
            let Ok(mut timers) = self.timers.lock() else {
                return;
            };
            let mut due = Vec::new();
            for t in timers.iter_mut() {
                if now >= t.next_fire {
                    due.push(t.handler.clone());
                    t.next_fire = now + t.interval;
                }
            }
            due
        };
        for handler in due {
            unsafe { handler.onTimer() };
        }

        // Non-blocking poll of registered FDs (X11 connection, etc.).
        let fds: Vec<(FileDescriptor, ComPtr<IEventHandler>)> = {
            let Ok(list) = self.fds.lock() else {
                return;
            };
            list.iter()
                .map(|e| (e.fd, e.handler.clone()))
                .collect()
        };
        if fds.is_empty() {
            return;
        }
        let mut pollfds: Vec<libc::pollfd> = fds
            .iter()
            .map(|(fd, _)| libc::pollfd {
                fd: *fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let n = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, 0) };
        if n <= 0 {
            return;
        }
        let mut stale = Vec::new();
        for (i, (fd, handler)) in fds.into_iter().enumerate() {
            let Some(p) = pollfds.get(i) else {
                continue;
            };
            // Drop closed/invalid FDs left behind if a plugin unregistered poorly.
            if p.revents & (libc::POLLNVAL | libc::POLLHUP | libc::POLLERR) != 0 {
                stale.push(handler.as_ptr());
                continue;
            }
            if p.revents != 0 {
                unsafe { handler.onFDIsSet(fd) };
            }
        }
        if !stale.is_empty() {
            if let Ok(mut list) = self.fds.lock() {
                list.retain(|e| !stale.contains(&e.handler.as_ptr()));
            }
        }
    }

    fn register_event_handler(&self, handler: *mut IEventHandler, fd: FileDescriptor) -> i32 {
        let Some(href) = (unsafe { ComRef::from_raw(handler) }) else {
            return kResultFalse;
        };
        let kept = href.to_com_ptr();
        if let Ok(mut fds) = self.fds.lock() {
            let raw = kept.as_ptr();
            fds.retain(|e| e.handler.as_ptr() != raw);
            fds.push(FdEntry {
                fd,
                handler: kept,
            });
        }
        kResultOk
    }

    fn unregister_event_handler(&self, handler: *mut IEventHandler) -> i32 {
        if let Ok(mut fds) = self.fds.lock() {
            fds.retain(|e| e.handler.as_ptr() != handler);
        }
        kResultOk
    }

    fn register_timer(&self, handler: *mut ITimerHandler, milliseconds: TimerInterval) -> i32 {
        let Some(href) = (unsafe { ComRef::from_raw(handler) }) else {
            return kResultFalse;
        };
        let kept = href.to_com_ptr();
        let interval = Duration::from_millis(milliseconds.max(1));
        if let Ok(mut timers) = self.timers.lock() {
            let raw = kept.as_ptr();
            timers.retain(|t| t.handler.as_ptr() != raw);
            timers.push(TimerEntry {
                handler: kept,
                interval,
                next_fire: Instant::now() + interval,
            });
        }
        kResultOk
    }

    fn unregister_timer(&self, handler: *mut ITimerHandler) -> i32 {
        if let Ok(mut timers) = self.timers.lock() {
            timers.retain(|t| t.handler.as_ptr() != handler);
        }
        kResultOk
    }
}

/// Host context passed to `IPluginBase::initialize`.
///
/// Exposes both `IHostApplication` and Linux `IRunLoop` via queryInterface.
pub struct HostContext {
    run_loop: Arc<RunLoopState>,
}

impl HostContext {
    pub fn new() -> (Self, Arc<RunLoopState>) {
        let run_loop = Arc::new(RunLoopState::default());
        (
            Self {
                run_loop: Arc::clone(&run_loop),
            },
            run_loop,
        )
    }
}

impl Class for HostContext {
    type Interfaces = (IHostApplication, IRunLoop);
}

impl IHostApplicationTrait for HostContext {
    unsafe fn getName(&self, name: *mut String128) -> i32 {
        if name.is_null() {
            return kResultFalse;
        }
        let src: Vec<u16> = "CottDAW".encode_utf16().chain(std::iter::once(0)).collect();
        let out = unsafe { &mut *name };
        out.fill(0);
        for (dst, src) in out.iter_mut().zip(src.iter()) {
            *dst = *src;
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        obj: *mut *mut std::ffi::c_void,
    ) -> i32 {
        if !obj.is_null() {
            unsafe {
                *obj = std::ptr::null_mut();
            }
        }
        kNoInterface
    }
}

impl IRunLoopTrait for HostContext {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: FileDescriptor,
    ) -> i32 {
        self.run_loop.register_event_handler(handler, fd)
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> i32 {
        self.run_loop.unregister_event_handler(handler)
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: TimerInterval,
    ) -> i32 {
        self.run_loop.register_timer(handler, milliseconds)
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> i32 {
        self.run_loop.unregister_timer(handler)
    }
}

/// Keep-alive handle for the host context + run-loop pump.
pub struct HostServices {
    _host: ComPtr<IHostApplication>,
    run_loop: Arc<RunLoopState>,
}

impl HostServices {
    pub fn create() -> (Self, *mut vst3::Steinberg::FUnknown) {
        let (ctx, run_loop) = HostContext::new();
        let wrapper = ComWrapper::new(ctx);
        let host = wrapper
            .to_com_ptr::<IHostApplication>()
            .expect("HostContext exposes IHostApplication");
        let raw = host.as_ptr() as *mut vst3::Steinberg::FUnknown;
        (
            Self {
                _host: host,
                run_loop,
            },
            raw,
        )
    }

    pub fn pump(&self) {
        self.run_loop.pump();
    }

    /// Shared with `IPlugFrame` so Linux plugins can `queryInterface` for `IRunLoop`.
    pub fn run_loop(&self) -> Arc<RunLoopState> {
        Arc::clone(&self.run_loop)
    }
}

// ---------------------------------------------------------------------------
// IPlugFrame — plugin → host editor resize requests (+ Linux IRunLoop)
// ---------------------------------------------------------------------------

/// Latest size requested by the plugin via `IPlugFrame::resizeView`.
#[derive(Clone, Default)]
pub struct PendingViewResize {
    inner: Arc<Mutex<Option<(u32, u32)>>>,
}

impl PendingViewResize {
    pub fn push(&self, width: u32, height: u32) {
        if let Ok(mut slot) = self.inner.lock() {
            *slot = Some((width.max(1), height.max(1)));
        }
    }

    pub fn take(&self) -> Option<(u32, u32)> {
        self.inner.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// Steinberg requires Linux hosts to expose `IRunLoop` from the same object as
/// `IPlugFrame`. nih-plug casts the frame to `IRunLoop` in `setFrame`; without
/// that, the egui/OpenGL editor often comes back as a black window after close.
pub struct PlugFrame {
    pending: PendingViewResize,
    run_loop: Arc<RunLoopState>,
}

impl PlugFrame {
    pub fn new(pending: PendingViewResize, run_loop: Arc<RunLoopState>) -> Self {
        Self { pending, run_loop }
    }
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame, IRunLoop);
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> i32 {
        if new_size.is_null() {
            return kResultFalse;
        }
        let rect = unsafe { *new_size };
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        self.pending.push(width, height);
        // Confirm to the plugin; the host applies the platform window size on
        // the next UI pump.
        if let Some(view) = unsafe { ComRef::from_raw(view) } {
            let mut confirmed = ViewRect {
                left: 0,
                top: 0,
                right: rect.right - rect.left,
                bottom: rect.bottom - rect.top,
            };
            let _ = unsafe { view.onSize(&raw mut confirmed) };
        }
        kResultOk
    }
}

impl IRunLoopTrait for PlugFrame {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: FileDescriptor,
    ) -> i32 {
        self.run_loop.register_event_handler(handler, fd)
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> i32 {
        self.run_loop.unregister_event_handler(handler)
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: TimerInterval,
    ) -> i32 {
        self.run_loop.register_timer(handler, milliseconds)
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> i32 {
        self.run_loop.unregister_timer(handler)
    }
}

/// Create an `IPlugFrame` (+ `IRunLoop`) sharing the host run-loop state.
pub fn create_plug_frame(
    run_loop: Arc<RunLoopState>,
) -> (PendingViewResize, ComPtr<IPlugFrame>) {
    let pending = PendingViewResize::default();
    let wrapper = ComWrapper::new(PlugFrame::new(pending.clone(), run_loop));
    let frame = wrapper
        .to_com_ptr::<IPlugFrame>()
        .expect("PlugFrame exposes IPlugFrame");
    (pending, frame)
}

/// Apply outgoing parameter changes from `process()` back onto the controller.
pub fn apply_output_param_changes(
    controller: &ComPtr<vst3::Steinberg::Vst::IEditController>,
    output: &ComPtr<IParameterChanges>,
) {
    use vst3::Steinberg::Vst::IEditControllerTrait;

    let count = unsafe { output.getParameterCount() };
    for i in 0..count {
        let queue = unsafe { output.getParameterData(i) };
        let Some(qref) = (unsafe { ComRef::from_raw(queue) }) else {
            continue;
        };
        let id = unsafe { qref.getParameterId() };
        let points = unsafe { qref.getPointCount() };
        if points <= 0 {
            continue;
        }
        let mut offset = 0i32;
        let mut value = 0.0f64;
        if unsafe { qref.getPoint(points - 1, &raw mut offset, &raw mut value) } == kResultOk {
            let _ = unsafe { controller.setParamNormalized(id, value) };
        }
    }
}
