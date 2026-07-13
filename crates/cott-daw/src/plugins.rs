//! Host-side sandboxed plugin process manager.

use anyhow::{Context, Result, anyhow};
use cott_core::dsp::{AudioBuffer, PluginAudioHost, TransportBlockInfo};
use cott_core::ids::PluginInstanceId;
use cott_ipc::posix::SharedAudioRegion;
use cott_ipc::{
    HostToWorker, PROTOCOL_VERSION, ParamInfo, PluginDescriptor, PluginFormat, ShmFlags,
    ShmMidiEvent, TransportInfo, WorkerToHost, encode_message, shm_name_for, try_decode_message,
};
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use uuid::Uuid;

pub struct PluginInstance {
    pub id: PluginInstanceId,
    pub format: PluginFormat,
    pub uid: String,
    pub path: PathBuf,
    pub name: String,
    pub has_editor: bool,
    pub params: Vec<ParamInfo>,
    pub param_values: IndexMap<u32, f32>,
    pub latency: u32,
    pub failed: bool,
    pub fail_message: Option<String>,
    child: Option<Child>,
    stream: Option<UnixStream>,
    shm: Option<SharedAudioRegion>,
    sock_path: PathBuf,
    shm_name: String,
}

pub struct PluginHost {
    pub catalog: Vec<PluginDescriptor>,
    pub instances: IndexMap<PluginInstanceId, PluginInstance>,
    worker_bin: PathBuf,
    scan_blacklist: Vec<String>,
}

impl PluginHost {
    pub fn new() -> Self {
        let worker_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("cott-vst-worker")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from("cott-vst-worker"));
        Self {
            catalog: Vec::new(),
            instances: IndexMap::new(),
            worker_bin,
            scan_blacklist: Vec::new(),
        }
    }

    pub fn set_worker_bin(&mut self, path: PathBuf) {
        self.worker_bin = path;
    }

    pub fn worker_bin(&self) -> &Path {
        &self.worker_bin
    }

    pub fn scan_blacklist(&self) -> &[String] {
        &self.scan_blacklist
    }

    #[allow(dead_code)]
    pub fn scan(&mut self) -> Result<()> {
        self.catalog = Self::scan_catalog(&self.worker_bin, &self.scan_blacklist)?;
        Ok(())
    }

    /// Run a disposable scanner worker and return the catalog (no host mutation).
    /// Safe to call from a background thread.
    pub fn scan_catalog(
        worker_bin: &Path,
        scan_blacklist: &[String],
    ) -> Result<Vec<PluginDescriptor>> {
        // Disposable scanner worker.
        let instance_uuid = Uuid::new_v4();
        let shm_name = shm_name_for(instance_uuid);
        let sock_path = std::env::temp_dir().join(format!("cott-scan-{}.sock", instance_uuid));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path)?;
        restrict_socket_permissions(&sock_path);
        listener.set_nonblocking(false)?;

        let shm = SharedAudioRegion::create(&shm_name)?;
        let mut cmd = Command::new(worker_bin);
        cmd.arg("--shm")
            .arg(&shm_name)
            .arg("--sock")
            .arg(&sock_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "spawn scanner worker at {} (build cott-vst-worker or set PATH)",
                worker_bin.display()
            )
        })?;

        let (mut stream, _) = listener.accept()?;
        wait_hello(&mut stream).with_context(|| {
            format!(
                "plugin worker handshake failed for {}. Rebuild both binaries with \
                 `cargo build -p cott-daw -p cott-vst-worker`",
                worker_bin.display()
            )
        })?;

        send(
            &mut stream,
            &HostToWorker::ScanPaths {
                paths: standard_vst3_dirs(),
            },
        )?;
        // Yabridge plugins spawn Wine per bundle; allow a long budget.
        let msg = recv_timeout(&mut stream, Duration::from_secs(300))?;
        let catalog = match msg {
            WorkerToHost::ScanResult { plugins } => {
                let catalog: Vec<_> = plugins
                    .into_iter()
                    .filter(|p| !scan_blacklist.iter().any(|b| b == &p.uid))
                    .collect();
                info!("scanned {} plugins", catalog.len());
                catalog
            }
            other => {
                warn!("unexpected scan response: {other:?}");
                Vec::new()
            }
        };
        let _ = send(&mut stream, &HostToWorker::Shutdown);
        let _ = child.wait();
        drop(shm);
        let _ = std::fs::remove_file(&sock_path);
        Ok(catalog)
    }

    pub fn load(
        &mut self,
        id: PluginInstanceId,
        format: PluginFormat,
        uid: &str,
        path: &Path,
        sample_rate: f64,
        block_size: u32,
        state: Option<Vec<u8>>,
    ) -> Result<()> {
        if let Some(existing) = self.instances.get_mut(&id) {
            existing.shutdown();
        }

        let instance_uuid = id.as_uuid();
        let shm_name = shm_name_for(instance_uuid);
        let sock_path = std::env::temp_dir().join(format!("cott-plug-{}.sock", instance_uuid));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path)?;
        restrict_socket_permissions(&sock_path);

        let shm = SharedAudioRegion::create(&shm_name)?;
        let mut cmd = Command::new(&self.worker_bin);
        cmd.arg("--shm")
            .arg(&shm_name)
            .arg("--sock")
            .arg(&sock_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().context("spawn plugin worker")?;
        if let Some(stderr) = child.stderr.take() {
            std::thread::Builder::new()
                .name(format!("cott-worker-log-{id}"))
                .spawn(move || forward_worker_stderr(stderr))
                .ok();
        }
        let (mut stream, _) = listener.accept()?;
        wait_hello(&mut stream).with_context(|| {
            format!(
                "plugin worker handshake failed for {}. Rebuild both binaries with \
                 `cargo build -p cott-daw -p cott-vst-worker`",
                self.worker_bin.display()
            )
        })?;
        send(
            &mut stream,
            &HostToWorker::Load {
                format,
                path: path.to_path_buf(),
                uid: uid.to_string(),
                sample_rate,
                block_size,
                state,
            },
        )?;
        let msg = recv_timeout(&mut stream, Duration::from_secs(30))?;
        let (name, latency, params, has_editor) = match msg {
            WorkerToHost::Loaded {
                name,
                latency,
                params,
                has_editor,
                ..
            } => (name, latency, params, has_editor),
            WorkerToHost::LoadFailed { message } => {
                let _ = child;
                return Err(anyhow!("load failed: {message}"));
            }
            other => return Err(anyhow!("unexpected load response: {other:?}")),
        };

        let mut param_values = IndexMap::new();
        for p in &params {
            param_values.insert(p.id, p.default);
        }

        self.instances.insert(
            id,
            PluginInstance {
                id,
                format,
                uid: uid.to_string(),
                path: path.to_path_buf(),
                name,
                has_editor,
                params,
                param_values,
                latency,
                failed: false,
                fail_message: None,
                child: Some(child),
                stream: Some(stream),
                shm: Some(shm),
                sock_path,
                shm_name,
            },
        );
        Ok(())
    }

    pub fn unload(&mut self, id: PluginInstanceId) {
        if let Some(mut inst) = self.instances.shift_remove(&id) {
            inst.shutdown();
        }
    }

    pub fn set_param(&mut self, id: PluginInstanceId, param_id: u32, value: f32) {
        self.set_param_inner(id, param_id, value, false);
    }

    fn set_param_normalized(&mut self, id: PluginInstanceId, param_id: u32, value: f32) {
        self.set_param_inner(id, param_id, value, true);
    }

    fn set_param_inner(
        &mut self,
        id: PluginInstanceId,
        param_id: u32,
        value: f32,
        normalized: bool,
    ) {
        if let Some(inst) = self.instances.get_mut(&id) {
            let display_value = if normalized {
                inst.params
                    .iter()
                    .find(|parameter| parameter.id == param_id)
                    .map(|parameter| {
                        parameter.min + value.clamp(0.0, 1.0) * (parameter.max - parameter.min)
                    })
                    .unwrap_or(value)
            } else {
                value
            };
            inst.param_values.insert(param_id, display_value);
            if let Some(stream) = inst.stream.as_mut() {
                let _ = send(
                    stream,
                    &HostToWorker::SetParam {
                        id: param_id,
                        value,
                        normalized,
                    },
                );
            }
        }
    }

    pub fn open_editor(&mut self, id: PluginInstanceId, parent_x11: Option<u64>) -> Result<()> {
        let inst = self
            .instances
            .get_mut(&id)
            .ok_or_else(|| anyhow!("instance missing"))?;
        let stream = inst.stream.as_mut().ok_or_else(|| anyhow!("no stream"))?;
        send(
            stream,
            &HostToWorker::OpenEditor {
                parent_x11_window: parent_x11,
            },
        )?;
        match recv_timeout(stream, Duration::from_secs(5))? {
            WorkerToHost::EditorOpened => Ok(()),
            WorkerToHost::EditorFailed { message } => Err(anyhow!(message)),
            other => Err(anyhow!("unexpected: {other:?}")),
        }
    }

    pub fn save_state(&mut self, id: PluginInstanceId) -> Option<Vec<u8>> {
        let inst = self.instances.get_mut(&id)?;
        let stream = inst.stream.as_mut()?;
        let _ = send(stream, &HostToWorker::GetState);
        match recv_timeout(stream, Duration::from_secs(2)).ok()? {
            WorkerToHost::State { data } => Some(data),
            _ => None,
        }
    }

    pub fn restart_failed(
        &mut self,
        id: PluginInstanceId,
        sample_rate: f64,
        block_size: u32,
        state: Option<Vec<u8>>,
    ) -> Result<()> {
        let (format, uid, path) = {
            let inst = self
                .instances
                .get(&id)
                .ok_or_else(|| anyhow!("missing instance"))?;
            (inst.format, inst.uid.clone(), inst.path.clone())
        };
        self.load(id, format, &uid, &path, sample_rate, block_size, state)
    }
}

impl PluginInstance {
    fn shutdown(&mut self) {
        if let Some(stream) = self.stream.as_mut() {
            let _ = send(stream, &HostToWorker::Shutdown);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.stream = None;
        self.shm = None;
        let _ = std::fs::remove_file(&self.sock_path);
    }

    fn process_block(
        &mut self,
        midi: &[cott_core::clips::ScheduledMidiEvent],
        input: Option<&AudioBuffer>,
        output: &mut AudioBuffer,
        ctx: &TransportBlockInfo,
    ) -> bool {
        if self.failed {
            render_fallback(input, output);
            return false;
        }
        // Check if child still alive.
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.failed = true;
                    self.fail_message = Some(format!("worker exited: {status}"));
                    render_fallback(input, output);
                    return false;
                }
                Err(e) => {
                    self.failed = true;
                    self.fail_message = Some(format!("worker wait error: {e}"));
                    render_fallback(input, output);
                    return false;
                }
                Ok(None) => {}
            }
        }

        let Some(shm) = self.shm.as_mut() else {
            render_fallback(input, output);
            return false;
        };
        let frames = (ctx.block_len as usize)
            .min(output.frames())
            .min(cott_ipc::MAX_BLOCK_FRAMES);
        let expected_sequence = {
            let header = shm.header_mut();
            header.frames = frames as u32;
            header.channels_in = 2;
            header.channels_out = 2;
            header.midi_count = midi.len().min(cott_ipc::MAX_MIDI_EVENTS) as u32;
            header.host_seq = header.host_seq.wrapping_add(1);
            header.flags = ShmFlags::REQUEST_PROCESS.bits();
            header.host_seq
        };
        {
            let midi_buf = shm.midi_mut();
            for (i, ev) in midi.iter().take(cott_ipc::MAX_MIDI_EVENTS).enumerate() {
                midi_buf[i] = ShmMidiEvent {
                    sample_offset: ev.sample_offset,
                    status: ev.status,
                    data1: ev.data1,
                    data2: ev.data2,
                    _pad: 0,
                };
            }
        }
        {
            let ain = shm.audio_in_mut();
            ain.fill(0.0);
            if let Some(input) = input {
                for i in 0..frames.min(input.frames()) {
                    ain[i] = input.channels.first().map(|c| c[i]).unwrap_or(0.0);
                    ain[cott_ipc::MAX_BLOCK_FRAMES + i] =
                        input.channels.get(1).map(|c| c[i]).unwrap_or(ain[i]);
                }
            }
        }

        let transport = TransportInfo {
            sample_rate: ctx.sample_rate as f64,
            tempo: ctx.bpm,
            time_sig_numerator: ctx.time_sig_numerator,
            time_sig_denominator: ctx.time_sig_denominator,
            project_time_samples: ctx.block_start.0,
            playing: ctx.playing,
            cycle: false,
            block_size: frames as u32,
        };

        let Some(stream) = self.stream.as_mut() else {
            render_fallback(input, output);
            return false;
        };
        if send(stream, &HostToWorker::ProcessNotify { transport }).is_err() {
            self.failed = true;
            self.fail_message = Some("IPC send failed".into());
            render_fallback(input, output);
            return false;
        }
        match recv_process_done(stream, expected_sequence, Duration::from_millis(50)) {
            Ok((latency, ok, message)) => {
                self.latency = latency;
                if !ok {
                    self.failed = true;
                    self.fail_message = message;
                    render_fallback(input, output);
                    return false;
                }
            }
            Err(e) => {
                // Timeout — treat as failure for this block but don't mark permanently yet.
                warn!("plugin {} process failed: {e}", self.name);
                render_fallback(input, output);
                return false;
            }
        }

        let completion = shm.header();
        let flags = ShmFlags::from_bits_truncate(completion.flags);
        if completion.worker_seq != expected_sequence || !flags.contains(ShmFlags::PROCESS_DONE) {
            warn!(
                "plugin {} stale process result: expected sequence {}, got {}",
                self.name, expected_sequence, completion.worker_seq
            );
            render_fallback(input, output);
            return false;
        }

        {
            let aout = shm.audio_out();
            for i in 0..frames.min(output.frames()) {
                if !output.channels.is_empty() {
                    output.channels[0][i] = aout[i];
                }
                if output.channels.len() > 1 {
                    output.channels[1][i] = aout[cott_ipc::MAX_BLOCK_FRAMES + i];
                }
            }
        }
        true
    }
}

fn render_fallback(input: Option<&AudioBuffer>, output: &mut AudioBuffer) {
    if let Some(input) = input {
        *output = input.clone();
    } else {
        output.clear();
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Adapter used by the DSP graph.
pub struct HostPluginAudio {
    pub inner: Arc<Mutex<PluginHost>>,
}

impl PluginAudioHost for HostPluginAudio {
    fn process_instrument(
        &mut self,
        instance: PluginInstanceId,
        midi: &[cott_core::clips::ScheduledMidiEvent],
        output: &mut AudioBuffer,
        ctx: &TransportBlockInfo,
    ) -> bool {
        let Some(mut host) = self.inner.try_lock() else {
            // Avoid blocking the realtime thread if the UI holds the host lock.
            output.clear();
            return false;
        };
        if let Some(inst) = host.instances.get_mut(&instance) {
            inst.process_block(midi, None, output, ctx)
        } else {
            output.clear();
            false
        }
    }

    fn process_effect(
        &mut self,
        instance: PluginInstanceId,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        ctx: &TransportBlockInfo,
    ) -> bool {
        let Some(mut host) = self.inner.try_lock() else {
            *output = input.clone();
            return false;
        };
        if let Some(inst) = host.instances.get_mut(&instance) {
            inst.process_block(&[], Some(input), output, ctx)
        } else {
            *output = input.clone();
            false
        }
    }

    fn set_param(&mut self, instance: PluginInstanceId, param_id: u32, value: f32) {
        let Some(mut host) = self.inner.try_lock() else {
            return;
        };
        host.set_param_normalized(instance, param_id, value);
    }
}

fn standard_vst3_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/lib/vst3"),
        PathBuf::from("/usr/local/lib/vst3"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".vst3"));
    }
    dirs
}

fn send(stream: &mut UnixStream, msg: &HostToWorker) -> Result<()> {
    let bytes = encode_message(msg)?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn forward_worker_stderr(stderr: ChildStderr) {
    let reader = BufReader::new(stderr);
    for line in reader.lines() {
        match line {
            Ok(line) => {
                // Worker already formats with tracing; surface as host-side info.
                info!(target: "cott_vst_worker", "{line}");
            }
            Err(_) => break,
        }
    }
}

fn wait_hello(stream: &mut UnixStream) -> Result<()> {
    match recv_timeout(stream, Duration::from_secs(5))? {
        WorkerToHost::HelloAck { version } => {
            if version != PROTOCOL_VERSION {
                return Err(anyhow!(
                    "worker protocol mismatch: host {}, worker {version}",
                    PROTOCOL_VERSION
                ));
            }
            Ok(())
        }
        other => Err(anyhow!("expected hello, got {other:?}")),
    }
}

/// Wait for the completion matching this shared-memory generation.
///
/// A worker may finish a timed-out request after the host has submitted the
/// next block. Drain those stale notifications without discarding a newer
/// frame that arrived in the same socket read.
fn recv_process_done(
    stream: &mut UnixStream,
    expected_sequence: u64,
    timeout: Duration,
) -> Result<(u32, bool, Option<String>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 65536];
    let start = Instant::now();

    loop {
        while let Some(message) = try_decode_message::<WorkerToHost>(&mut buf)? {
            match message {
                WorkerToHost::ProcessDone {
                    sequence,
                    latency,
                    ok,
                    message,
                } if sequence == expected_sequence => return Ok((latency, ok, message)),
                WorkerToHost::ProcessDone { .. } => {
                    // Completion for a request that already timed out.
                }
                other => warn!("unexpected plugin process response: {other:?}"),
            }
        }

        let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
            return Err(anyhow!("timeout"));
        };
        if remaining.is_zero() {
            return Err(anyhow!("timeout"));
        }
        stream.set_read_timeout(Some(remaining))?;

        match stream.read(&mut tmp) {
            Ok(0) => return Err(anyhow!("worker disconnected")),
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Err(anyhow!("timeout"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn recv_timeout(stream: &mut UnixStream, timeout: Duration) -> Result<WorkerToHost> {
    stream.set_read_timeout(Some(timeout))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 65536];
    let start = Instant::now();
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => return Err(anyhow!("worker disconnected")),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(msg) = try_decode_message::<WorkerToHost>(&mut buf)? {
                    return Ok(msg);
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if start.elapsed() > timeout {
                    return Err(anyhow!("timeout"));
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_receive_drains_stale_completion_from_same_read() {
        let (mut host, mut worker) = UnixStream::pair().unwrap();
        let stale = WorkerToHost::ProcessDone {
            sequence: 41,
            latency: 0,
            ok: true,
            message: None,
        };
        let current = WorkerToHost::ProcessDone {
            sequence: 42,
            latency: 128,
            ok: true,
            message: None,
        };
        let mut bytes = encode_message(&stale).unwrap();
        bytes.extend(encode_message(&current).unwrap());
        worker.write_all(&bytes).unwrap();

        let result = recv_process_done(&mut host, 42, Duration::from_millis(100)).unwrap();
        assert_eq!(result, (128, true, None));
    }
}

/// Restrict IPC socket to the current user (owner read/write only).
fn restrict_socket_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
