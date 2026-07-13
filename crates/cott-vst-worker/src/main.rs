//! CottDAW VST worker process.
//!
//! Usage:
//!   cott-vst-worker --shm <name> --sock <path>

mod classify;
mod host;
mod vst;
mod x11_editor;

use anyhow::{Context, Result};
use cott_ipc::{
    HostToWorker, PROTOCOL_VERSION, WorkerToHost, posix::SharedAudioRegion, try_decode_message,
};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info, warn};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("cott_vst_worker=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(e) = run() {
        error!("worker fatal: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut shm_name = None;
    let mut sock_path = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--shm" => {
                i += 1;
                shm_name = args.get(i).cloned();
            }
            "--sock" => {
                i += 1;
                sock_path = args.get(i).cloned();
            }
            other => warn!("unknown arg: {other}"),
        }
        i += 1;
    }
    let shm_name = shm_name.context("missing --shm")?;
    let sock_path = PathBuf::from(sock_path.context("missing --sock")?);

    let mut shm = SharedAudioRegion::open(&shm_name).context("open shm")?;
    let mut stream = UnixStream::connect(&sock_path).context("connect sock")?;
    stream.set_nonblocking(false)?;

    send(
        &mut stream,
        &WorkerToHost::HelloAck {
            version: PROTOCOL_VERSION,
        },
    )?;

    let mut backend: Option<vst::VstPlugin> = None;
    let mut read_buf = Vec::new();
    let mut tmp = [0u8; 65536];

    loop {
        // Keep Linux IRunLoop timers ticking whenever a VST is loaded,
        // and also while a floating editor window is open.
        let needs_pump = backend.is_some();
        if needs_pump {
            stream.set_read_timeout(Some(Duration::from_millis(8)))?;
        } else {
            stream.set_read_timeout(None)?;
        }

        match stream.read(&mut tmp) {
            Ok(0) => {
                info!("host disconnected");
                break;
            }
            Ok(n) => {
                read_buf.extend_from_slice(&tmp[..n]);
                while let Some(msg) = try_decode_message::<HostToWorker>(&mut read_buf)? {
                    if !handle_message(msg, &mut stream, &mut shm, &mut backend)? {
                        return Ok(());
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                // Idle wake for editor / run-loop pump.
            }
            Err(e) => return Err(e.into()),
        }

        if let Some(plugin) = backend.as_mut() {
            let _ = plugin.pump_editor();
        }
    }
    Ok(())
}

fn handle_message(
    msg: HostToWorker,
    stream: &mut UnixStream,
    shm: &mut SharedAudioRegion,
    backend: &mut Option<vst::VstPlugin>,
) -> Result<bool> {
    match msg {
        HostToWorker::Hello { version } => {
            if version != PROTOCOL_VERSION {
                warn!("protocol mismatch host={version}");
            }
            send(
                stream,
                &WorkerToHost::HelloAck {
                    version: PROTOCOL_VERSION,
                },
            )?;
        }
        HostToWorker::ScanPaths { paths } => {
            let plugins = vst::scan_paths(&paths).unwrap_or_else(|e| {
                warn!("scan failed: {e:#}");
                Vec::new()
            });
            send(stream, &WorkerToHost::ScanResult { plugins })?;
        }
        HostToWorker::Load {
            path,
            uid,
            sample_rate,
            block_size,
            state,
        } => {
            match vst::VstPlugin::load(&path, &uid, sample_rate, block_size, state.as_deref()) {
                Ok(plugin) => {
                    let (name, latency, params, has_editor, is_instrument) = plugin.meta();
                    *backend = Some(plugin);
                    send(
                        stream,
                        &WorkerToHost::Loaded {
                            name,
                            latency,
                            params,
                            has_editor,
                            is_instrument,
                        },
                    )?;
                }
                Err(e) => {
                    send(
                        stream,
                        &WorkerToHost::LoadFailed {
                            message: format!("{e:#}"),
                        },
                    )?;
                }
            }
        }
        HostToWorker::Unload => {
            *backend = None;
        }
        HostToWorker::SetParam { id, value } => {
            if let Some(plugin) = backend {
                plugin.set_param(id, value);
            }
        }
        HostToWorker::GetParams => {
            let params = backend.as_ref().map(|p| p.params()).unwrap_or_default();
            send(stream, &WorkerToHost::Params { params })?;
        }
        HostToWorker::GetState => {
            let data = backend.as_ref().map(|p| p.get_state()).unwrap_or_default();
            send(stream, &WorkerToHost::State { data })?;
        }
        HostToWorker::SetState { data } => {
            if let Some(plugin) = backend {
                plugin.set_state(&data);
            }
        }
        HostToWorker::OpenEditor { parent_x11_window } => {
            let result = match backend.as_mut() {
                Some(plugin) => plugin.open_editor(parent_x11_window),
                None => Err(anyhow::anyhow!("no plugin loaded")),
            };
            match result {
                Ok(()) => send(stream, &WorkerToHost::EditorOpened)?,
                Err(e) => send(
                    stream,
                    &WorkerToHost::EditorFailed {
                        message: format!("{e:#}"),
                    },
                )?,
            }
        }
        HostToWorker::CloseEditor => {
            if let Some(plugin) = backend.as_mut() {
                plugin.close_editor();
            }
            send(stream, &WorkerToHost::EditorClosed)?;
        }
        HostToWorker::ProcessNotify { transport }
        | HostToWorker::OfflineProcess { transport, .. } => {
            let ok = if let Some(plugin) = backend.as_mut() {
                plugin.process(shm, &transport)
            } else {
                // Silence planar stereo output (L then R at MAX_BLOCK_FRAMES).
                let frames = transport.block_size as usize;
                let out = shm.audio_out_mut();
                let frames = frames.min(cott_ipc::MAX_BLOCK_FRAMES);
                out[..frames].fill(0.0);
                let r0 = cott_ipc::MAX_BLOCK_FRAMES;
                out[r0..r0 + frames].fill(0.0);
                true
            };
            let latency = backend.as_ref().map(|p| p.latency()).unwrap_or(0);
            {
                let header = shm.header_mut();
                header.worker_seq = header.host_seq;
                header.flags |= cott_ipc::ShmFlags::PROCESS_DONE.bits();
                if !ok {
                    header.flags |= cott_ipc::ShmFlags::WORKER_FAILED.bits();
                }
            }
            send(
                stream,
                &WorkerToHost::ProcessDone {
                    latency,
                    ok,
                    message: None,
                },
            )?;
        }
        HostToWorker::Shutdown => {
            info!("shutdown requested");
            return Ok(false);
        }
    }
    Ok(true)
}

fn send(stream: &mut UnixStream, msg: &WorkerToHost) -> Result<()> {
    let bytes = cott_ipc::encode_message(msg)?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}
