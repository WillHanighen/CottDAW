//! IPC protocol between CottDAW host and per-plugin VST worker processes.

use bitflags::bitflags;
use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_BLOCK_FRAMES: usize = 4096;
pub const MAX_CHANNELS: usize = 2;
pub const MAX_MIDI_EVENTS: usize = 512;
pub const SHM_MAGIC: u32 = 0xC0_77_DA_01;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bincode: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("protocol mismatch: got {0}")]
    Protocol(u32),
    #[error("shared memory: {0}")]
    Shm(String),
    #[error("timeout")]
    Timeout,
    #[error("worker crashed or disconnected")]
    Disconnected,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub uid: String,
    pub name: String,
    pub vendor: String,
    pub path: PathBuf,
    pub is_instrument: bool,
    pub is_effect: bool,
    pub has_editor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportInfo {
    pub sample_rate: f64,
    pub tempo: f64,
    pub project_time_samples: i64,
    pub playing: bool,
    pub cycle: bool,
    pub block_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostToWorker {
    Hello {
        version: u32,
    },
    ScanPaths {
        paths: Vec<PathBuf>,
    },
    Load {
        path: PathBuf,
        uid: String,
        sample_rate: f64,
        block_size: u32,
        state: Option<Vec<u8>>,
    },
    Unload,
    SetParam {
        id: u32,
        value: f32,
    },
    GetParams,
    GetState,
    SetState {
        data: Vec<u8>,
    },
    OpenEditor {
        parent_x11_window: Option<u64>,
    },
    CloseEditor,
    Shutdown,
    /// Process one block using the shared-memory ring (signal only).
    ProcessNotify {
        transport: TransportInfo,
    },
    OfflineProcess {
        transport: TransportInfo,
        frames: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerToHost {
    HelloAck {
        version: u32,
    },
    ScanResult {
        plugins: Vec<PluginDescriptor>,
    },
    Loaded {
        name: String,
        latency: u32,
        params: Vec<ParamInfo>,
        has_editor: bool,
        is_instrument: bool,
    },
    LoadFailed {
        message: String,
    },
    Params {
        params: Vec<ParamInfo>,
    },
    State {
        data: Vec<u8>,
    },
    ProcessDone {
        latency: u32,
        ok: bool,
        message: Option<String>,
    },
    EditorOpened,
    EditorClosed,
    EditorFailed {
        message: String,
    },
    Crashed {
        message: String,
    },
    Log {
        level: String,
        message: String,
    },
    Pong,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ShmHeader {
    pub magic: u32,
    pub version: u32,
    pub frames: u32,
    pub channels_in: u32,
    pub channels_out: u32,
    pub midi_count: u32,
    pub host_seq: u64,
    pub worker_seq: u64,
    pub flags: u32,
    pub _pad: u32,
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ShmFlags: u32 {
        const REQUEST_PROCESS = 0b0001;
        const PROCESS_DONE = 0b0010;
        const WORKER_FAILED = 0b0100;
        const BYPASS = 0b1000;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ShmMidiEvent {
    pub sample_offset: u32,
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
    pub _pad: u8,
}

/// Layout of the shared memory region.
pub struct ShmLayout {
    pub header_offset: usize,
    pub midi_offset: usize,
    pub audio_in_offset: usize,
    pub audio_out_offset: usize,
    pub total_size: usize,
}

impl ShmLayout {
    pub fn new() -> Self {
        let header = std::mem::size_of::<ShmHeader>();
        let midi = MAX_MIDI_EVENTS * std::mem::size_of::<ShmMidiEvent>();
        let audio = MAX_CHANNELS * MAX_BLOCK_FRAMES * std::mem::size_of::<f32>();
        let header_offset = 0;
        let midi_offset = align(header, 64);
        let audio_in_offset = align(midi_offset + midi, 64);
        let audio_out_offset = align(audio_in_offset + audio, 64);
        let total_size = align(audio_out_offset + audio, 64);
        Self {
            header_offset,
            midi_offset,
            audio_in_offset,
            audio_out_offset,
            total_size,
        }
    }
}

impl Default for ShmLayout {
    fn default() -> Self {
        Self::new()
    }
}

fn align(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

pub fn shm_name_for(instance: Uuid) -> String {
    format!("/cott-daw-{}", instance.simple())
}

pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>, IpcError> {
    let payload = bincode::serialize(msg)?;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

pub fn try_decode_message<T: for<'de> Deserialize<'de>>(
    buffer: &mut Vec<u8>,
) -> Result<Option<T>, IpcError> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    if buffer.len() < 4 + len {
        return Ok(None);
    }
    let msg: T = bincode::deserialize(&buffer[4..4 + len])?;
    buffer.drain(..4 + len);
    Ok(Some(msg))
}

pub mod posix {
    use super::*;
    use nix::fcntl::OFlag;
    use nix::sys::mman::{MapFlags, ProtFlags, mmap, munmap, shm_open, shm_unlink};
    use nix::sys::stat::Mode;
    use nix::unistd::ftruncate;
    use std::ffi::CString;
    use std::os::fd::OwnedFd;
    use std::ptr::NonNull;

    pub struct SharedAudioRegion {
        pub name: String,
        pub fd: OwnedFd,
        pub ptr: NonNull<u8>,
        pub size: usize,
        pub layout: ShmLayout,
        pub owner: bool,
    }

    // Shared memory is intentionally cross-thread; access is synchronized by the
    // host/worker protocol (seq numbers + Unix socket acknowledgements).
    unsafe impl Send for SharedAudioRegion {}
    unsafe impl Sync for SharedAudioRegion {}

    impl SharedAudioRegion {
        pub fn create(name: &str) -> Result<Self, IpcError> {
            let layout = ShmLayout::new();
            let cname = CString::new(name).map_err(|e| IpcError::Shm(e.to_string()))?;
            let fd = shm_open(
                cname.as_c_str(),
                OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
                Mode::S_IRUSR | Mode::S_IWUSR,
            )
            .map_err(|e| IpcError::Shm(e.to_string()))?;
            ftruncate(&fd, layout.total_size as i64).map_err(|e| IpcError::Shm(e.to_string()))?;
            let ptr = unsafe {
                mmap(
                    None,
                    std::num::NonZeroUsize::new(layout.total_size)
                        .ok_or_else(|| IpcError::Shm("zero size".into()))?,
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_SHARED,
                    &fd,
                    0,
                )
                .map_err(|e| IpcError::Shm(e.to_string()))?
            };
            let mut region = Self {
                name: name.to_string(),
                fd,
                ptr: ptr.cast(),
                size: layout.total_size,
                layout,
                owner: true,
            };
            unsafe {
                std::ptr::write_bytes(region.ptr.as_ptr(), 0, region.size);
                let header = region.header_mut();
                header.magic = SHM_MAGIC;
                header.version = PROTOCOL_VERSION;
            }
            Ok(region)
        }

        pub fn open(name: &str) -> Result<Self, IpcError> {
            let layout = ShmLayout::new();
            let cname = CString::new(name).map_err(|e| IpcError::Shm(e.to_string()))?;
            let fd = shm_open(cname.as_c_str(), OFlag::O_RDWR, Mode::empty())
                .map_err(|e| IpcError::Shm(e.to_string()))?;
            let ptr = unsafe {
                mmap(
                    None,
                    std::num::NonZeroUsize::new(layout.total_size)
                        .ok_or_else(|| IpcError::Shm("zero size".into()))?,
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_SHARED,
                    &fd,
                    0,
                )
                .map_err(|e| IpcError::Shm(e.to_string()))?
            };
            Ok(Self {
                name: name.to_string(),
                fd,
                ptr: ptr.cast(),
                size: layout.total_size,
                layout,
                owner: false,
            })
        }

        pub fn header(&self) -> &ShmHeader {
            unsafe { &*(self.ptr.as_ptr().add(self.layout.header_offset) as *const ShmHeader) }
        }

        pub fn header_mut(&mut self) -> &mut ShmHeader {
            unsafe { &mut *(self.ptr.as_ptr().add(self.layout.header_offset) as *mut ShmHeader) }
        }

        pub fn midi_mut(&mut self) -> &mut [ShmMidiEvent] {
            unsafe {
                std::slice::from_raw_parts_mut(
                    self.ptr.as_ptr().add(self.layout.midi_offset) as *mut ShmMidiEvent,
                    MAX_MIDI_EVENTS,
                )
            }
        }

        pub fn audio_in_mut(&mut self) -> &mut [f32] {
            let floats = MAX_CHANNELS * MAX_BLOCK_FRAMES;
            unsafe {
                std::slice::from_raw_parts_mut(
                    self.ptr.as_ptr().add(self.layout.audio_in_offset) as *mut f32,
                    floats,
                )
            }
        }

        pub fn audio_out_mut(&mut self) -> &mut [f32] {
            let floats = MAX_CHANNELS * MAX_BLOCK_FRAMES;
            unsafe {
                std::slice::from_raw_parts_mut(
                    self.ptr.as_ptr().add(self.layout.audio_out_offset) as *mut f32,
                    floats,
                )
            }
        }

        pub fn audio_out(&self) -> &[f32] {
            let floats = MAX_CHANNELS * MAX_BLOCK_FRAMES;
            unsafe {
                std::slice::from_raw_parts(
                    self.ptr.as_ptr().add(self.layout.audio_out_offset) as *const f32,
                    floats,
                )
            }
        }
    }

    impl Drop for SharedAudioRegion {
        fn drop(&mut self) {
            unsafe {
                let _ = munmap(self.ptr.cast(), self.size);
            }
            if self.owner {
                if let Ok(cname) = CString::new(self.name.as_str()) {
                    let _ = shm_unlink(cname.as_c_str());
                }
            }
        }
    }
}
