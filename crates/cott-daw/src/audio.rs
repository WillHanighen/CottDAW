//! PipeWire/cpal audio output bridge (F32).

use crate::plugins::HostPluginAudio;
use anyhow::{Context, Result, anyhow};
use cott_core::engine::{AudioProcessor, EngineCommand, EngineEvent, SharedTransport};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::{info, warn};

pub struct AudioEngine {
    pub shared: Arc<SharedTransport>,
    pub cmd_tx: Producer<EngineCommand>,
    pub evt_rx: Consumer<EngineEvent>,
    _stream: Option<cpal::Stream>,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub plugin_host: Arc<Mutex<crate::plugins::PluginHost>>,
    pub running: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn start(plugin_host: Arc<Mutex<crate::plugins::PluginHost>>) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default output device"))?;
        let device_id = device
            .id()
            .map(|id| format!("{id:?}"))
            .unwrap_or_else(|_| "unknown".into());
        info!("audio device: {device_id} (host={})", host.id().name());

        let supported = device.default_output_config()?;
        if supported.sample_format() != SampleFormat::F32 {
            return Err(anyhow!(
                "need F32 output, device is {:?}",
                supported.sample_format()
            ));
        }
        let sample_rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let buffer_size = 256u32;

        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate,
            buffer_size: BufferSize::Fixed(buffer_size),
        };

        let shared = Arc::new(SharedTransport::new());
        let (cmd_tx, mut cmd_rx) = rtrb::RingBuffer::new(256);
        let (mut evt_tx, evt_rx) = rtrb::RingBuffer::new(256);
        let running = Arc::new(AtomicBool::new(true));

        let mut processor = AudioProcessor::new(Arc::clone(&shared));
        let plugin_host_cb = Arc::clone(&plugin_host);
        let err_fn = |e| warn!("audio stream error: {e}");

        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                processor.handle_commands(&mut cmd_rx);
                let frames = data.len() / channels.max(1);
                let mut host_audio = HostPluginAudio {
                    inner: Arc::clone(&plugin_host_cb),
                };
                processor.process(
                    data,
                    channels,
                    frames,
                    sample_rate,
                    &mut host_audio,
                    &mut evt_tx,
                );
            },
            err_fn,
            None,
        )?;

        stream.play().context("play stream")?;
        info!("audio running @ {sample_rate} Hz, buffer={buffer_size}, ch={channels}");

        Ok(Self {
            shared,
            cmd_tx,
            evt_rx,
            _stream: Some(stream),
            sample_rate,
            buffer_size,
            plugin_host,
            running,
        })
    }
}
