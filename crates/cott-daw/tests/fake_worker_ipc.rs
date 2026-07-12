//! Integration-style IPC test with the fake worker binary.

use cott_ipc::posix::SharedAudioRegion;
use cott_ipc::{
    HostToWorker, PROTOCOL_VERSION, WorkerToHost, encode_message, shm_name_for, try_decode_message,
};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

fn worker_bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.join("target/debug/cott-vst-worker")
}

#[test]
fn fake_worker_scan_and_process() {
    let bin = worker_bin();
    if !bin.exists() {
        eprintln!("skip: worker not built at {}", bin.display());
        return;
    }

    let id = Uuid::new_v4();
    let shm_name = shm_name_for(id);
    let sock_path = std::env::temp_dir().join(format!("cott-test-{}.sock", id));
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).unwrap();
    let mut shm = SharedAudioRegion::create(&shm_name).unwrap();

    let mut child = Command::new(&bin)
        .args([
            "--shm",
            &shm_name,
            "--sock",
            sock_path.to_str().unwrap(),
            "--fake",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");

    listener.set_nonblocking(false).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    // Hello
    let hello = recv(&mut stream);
    assert!(matches!(hello, WorkerToHost::HelloAck { version } if version == PROTOCOL_VERSION));

    send(&mut stream, &HostToWorker::ScanPaths { paths: vec![] });
    let scan = recv(&mut stream);
    match scan {
        WorkerToHost::ScanResult { plugins } => {
            assert!(plugins.iter().any(|p| p.uid == "fake.sine"));
        }
        other => panic!("unexpected {other:?}"),
    }

    send(
        &mut stream,
        &HostToWorker::Load {
            path: PathBuf::from("fake://sine"),
            uid: "fake.sine".into(),
            sample_rate: 48_000.0,
            block_size: 128,
            state: None,
        },
    );
    match recv(&mut stream) {
        WorkerToHost::Loaded { is_instrument, .. } => assert!(is_instrument),
        other => panic!("unexpected {other:?}"),
    }

    {
        let header = shm.header_mut();
        header.frames = 128;
        header.channels_in = 2;
        header.channels_out = 2;
        header.midi_count = 1;
        header.host_seq = 1;
    }
    {
        let midi = shm.midi_mut();
        midi[0] = cott_ipc::ShmMidiEvent {
            sample_offset: 0,
            status: 0x90,
            data1: 60,
            data2: 100,
            _pad: 0,
        };
    }

    send(
        &mut stream,
        &HostToWorker::ProcessNotify {
            transport: cott_ipc::TransportInfo {
                sample_rate: 48_000.0,
                tempo: 120.0,
                project_time_samples: 0,
                playing: true,
                cycle: false,
                block_size: 128,
            },
        },
    );
    match recv(&mut stream) {
        WorkerToHost::ProcessDone { ok, .. } => assert!(ok),
        other => panic!("unexpected {other:?}"),
    }

    let out = shm.audio_out();
    let peak = out[..128]
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0f32, f32::max);
    assert!(peak > 0.0, "expected audible sine output");

    send(&mut stream, &HostToWorker::Shutdown);
    let _ = child.wait();
    let _ = std::fs::remove_file(&sock_path);
}

fn send(stream: &mut UnixStream, msg: &HostToWorker) {
    let bytes = encode_message(msg).unwrap();
    stream.write_all(&bytes).unwrap();
    stream.flush().unwrap();
}

fn recv(stream: &mut UnixStream) -> WorkerToHost {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 65536];
    loop {
        let n = stream.read(&mut tmp).unwrap();
        assert!(n > 0);
        buf.extend_from_slice(&tmp[..n]);
        if let Some(msg) = try_decode_message::<WorkerToHost>(&mut buf).unwrap() {
            return msg;
        }
    }
}
