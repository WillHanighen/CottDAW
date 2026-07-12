use cott_ipc::{
    ParamInfo, PluginDescriptor, ShmMidiEvent, TransportInfo, posix::SharedAudioRegion,
};
use std::path::PathBuf;

pub struct FakePlugin {
    uid: String,
    is_instrument: bool,
    sample_rate: f64,
    gain: f32,
    phase: f32,
    freq: f32,
    amp: f32,
    state: Vec<u8>,
}

pub fn scan_fake() -> Vec<PluginDescriptor> {
    vec![
        PluginDescriptor {
            uid: "fake.sine".into(),
            name: "Fake Sine Instrument".into(),
            vendor: "CottDAW".into(),
            path: PathBuf::from("fake://sine"),
            is_instrument: true,
            is_effect: false,
            has_editor: false,
        },
        PluginDescriptor {
            uid: "fake.gain".into(),
            name: "Fake Gain Effect".into(),
            vendor: "CottDAW".into(),
            path: PathBuf::from("fake://gain"),
            is_instrument: false,
            is_effect: true,
            has_editor: false,
        },
    ]
}

impl FakePlugin {
    pub fn load(
        uid: &str,
        sample_rate: f64,
        _block_size: u32,
        state: Option<&[u8]>,
    ) -> anyhow::Result<Self> {
        let is_instrument = uid.contains("sine") || uid.ends_with(".sine");
        let mut p = Self {
            uid: uid.to_string(),
            is_instrument,
            sample_rate,
            gain: 0.8,
            phase: 0.0,
            freq: 440.0,
            amp: 0.0,
            state: state.unwrap_or(&[]).to_vec(),
        };
        if let Some(s) = state {
            p.set_state(s);
        }
        Ok(p)
    }

    pub fn meta(&self) -> (String, u32, Vec<ParamInfo>, bool, bool) {
        let name = if self.is_instrument {
            "Fake Sine Instrument"
        } else {
            "Fake Gain Effect"
        };
        (
            name.into(),
            0,
            vec![ParamInfo {
                id: 0,
                name: "Gain".into(),
                default: 0.8,
                min: 0.0,
                max: 1.0,
            }],
            false,
            self.is_instrument,
        )
    }

    pub fn params(&self) -> Vec<ParamInfo> {
        self.meta().2
    }

    pub fn set_param(&mut self, id: u32, value: f32) {
        if id == 0 {
            self.gain = value.clamp(0.0, 1.0);
        }
    }

    pub fn get_state(&self) -> Vec<u8> {
        self.gain.to_le_bytes().to_vec()
    }

    pub fn set_state(&mut self, data: &[u8]) {
        if data.len() >= 4 {
            let mut b = [0u8; 4];
            b.copy_from_slice(&data[..4]);
            self.gain = f32::from_le_bytes(b).clamp(0.0, 1.0);
        }
        self.state = data.to_vec();
    }

    pub fn latency(&self) -> u32 {
        0
    }

    pub fn process(&mut self, shm: &mut SharedAudioRegion, transport: &TransportInfo) -> bool {
        let frames = transport.block_size as usize;
        let midi_count = shm
            .header()
            .midi_count
            .min(cott_ipc::MAX_MIDI_EVENTS as u32) as usize;
        let midi: Vec<ShmMidiEvent> = shm.midi_mut()[..midi_count].to_vec();
        let channels_in = shm.header().channels_in.max(1) as usize;
        let mut input = vec![0.0f32; frames * 2];
        {
            let ain = shm.audio_in_mut();
            for i in 0..frames {
                input[i] = ain[i];
                input[frames + i] = if channels_in > 1 {
                    ain[cott_ipc::MAX_BLOCK_FRAMES + i]
                } else {
                    ain[i]
                };
            }
        }

        let mut out_l = vec![0.0f32; frames];
        let mut out_r = vec![0.0f32; frames];

        if self.is_instrument {
            for ev in &midi {
                let offset = ev.sample_offset as usize;
                let status = ev.status & 0xf0;
                if status == 0xb0 && (ev.data1 == 120 || ev.data1 == 123) {
                    // All Sound Off / All Notes Off — silence immediately.
                    self.amp = 0.0;
                    let _ = offset;
                } else if status == 0x90 && ev.data2 > 0 {
                    self.freq = 440.0 * 2f32.powf((ev.data1 as f32 - 69.0) / 12.0);
                    self.amp = (ev.data2 as f32 / 127.0) * self.gain * 0.25;
                    // Apply from offset by rendering whole block with current amp — MVP.
                    let _ = offset;
                } else if status == 0x80 || (status == 0x90 && ev.data2 == 0) {
                    self.amp = 0.0;
                }
            }
            for i in 0..frames {
                let s = (self.phase * std::f32::consts::TAU).sin() * self.amp;
                out_l[i] = s;
                out_r[i] = s;
                self.phase = (self.phase + (self.freq / self.sample_rate as f32)) % 1.0;
            }
        } else {
            for i in 0..frames {
                out_l[i] = input[i] * self.gain;
                out_r[i] = input[frames + i] * self.gain;
            }
        }

        {
            let aout = shm.audio_out_mut();
            for i in 0..frames {
                aout[i] = out_l[i];
                aout[cott_ipc::MAX_BLOCK_FRAMES + i] = out_r[i];
            }
        }
        let header = shm.header_mut();
        header.frames = frames as u32;
        header.channels_out = 2;
        true
    }
}
