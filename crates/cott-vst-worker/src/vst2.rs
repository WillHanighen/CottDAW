//! Legacy VST 2.4 host used primarily for yabridge's `.so` wrappers.

use anyhow::{Context, Result, anyhow};
use cott_ipc::{
    ParamInfo, PluginDescriptor, PluginFormat, TransportInfo, posix::SharedAudioRegion,
};
use parking_lot::Mutex as ParkingMutex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use vst::api::{TimeInfo, TimeInfoFlags};
use vst::buffer::SendEventBuffer;
use vst::editor::Editor;
use vst::event::MidiEvent;
use vst::host::{Host, HostBuffer, PluginInstance, PluginLoader};
use vst::plugin::{Category, Plugin, PluginParameters};

use crate::classify::{name_looks_like_effect, name_looks_like_instrument};
use crate::x11_editor::FloatingEditorWindow;

const STATE_MAGIC: &[u8; 8] = b"COTTVST2";

#[derive(Clone, Copy)]
struct HostRuntime {
    sample_rate: f64,
    block_size: isize,
    sample_pos: f64,
    tempo: f64,
    time_sig_numerator: i32,
    time_sig_denominator: i32,
    playing: bool,
    cycle: bool,
}

struct Vst2Host {
    runtime: ParkingMutex<HostRuntime>,
}

impl Host for Vst2Host {
    fn get_info(&self) -> (isize, String, String) {
        (1, "Cottage End".into(), "CottDAW".into())
    }

    fn get_block_size(&self) -> isize {
        self.runtime.lock().block_size
    }

    fn get_time_info(&self, _mask: i32) -> Option<TimeInfo> {
        let rt = *self.runtime.lock();
        let mut flags = TimeInfoFlags::TEMPO_VALID
            | TimeInfoFlags::PPQ_POS_VALID
            | TimeInfoFlags::TIME_SIG_VALID;
        if rt.playing {
            flags |= TimeInfoFlags::TRANSPORT_PLAYING;
        }
        if rt.cycle {
            flags |= TimeInfoFlags::TRANSPORT_CYCLE_ACTIVE;
        }
        Some(TimeInfo {
            sample_pos: rt.sample_pos,
            sample_rate: rt.sample_rate,
            ppq_pos: rt.sample_pos / rt.sample_rate.max(1.0) * rt.tempo / 60.0,
            tempo: rt.tempo,
            time_sig_numerator: rt.time_sig_numerator,
            time_sig_denominator: rt.time_sig_denominator,
            flags: flags.bits(),
            ..TimeInfo::default()
        })
    }
}

pub struct Vst2Plugin {
    instance: PluginInstance,
    params: Arc<dyn PluginParameters>,
    info: vst::plugin::Info,
    host: Arc<Mutex<Vst2Host>>,
    host_buffer: HostBuffer<f32>,
    midi_buffer: SendEventBuffer,
    has_editor: bool,
    editor: Option<Box<dyn Editor>>,
    owned_editor: Option<FloatingEditorWindow>,
}

pub fn scan_paths(paths: &[PathBuf]) -> Vec<PluginDescriptor> {
    let mut libraries = Vec::new();
    for root in paths {
        collect_libraries(root, &mut libraries);
    }
    libraries.sort();
    libraries.dedup();

    let mut out = Vec::new();
    for path in libraries {
        if crate::vst::looks_like_yabridge_path(&path) {
            out.push(path_descriptor(&path));
            continue;
        }
        match inspect(&path) {
            Ok(desc) => out.push(desc),
            Err(err) => tracing::warn!("VST2 scan {}: {err:#}", path.display()),
        }
    }
    out
}

fn collect_libraries(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_libraries(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("so") {
            out.push(path);
        }
    }
}

fn path_descriptor(path: &Path) -> PluginDescriptor {
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("VST2 Plugin")
        .trim_end_matches(".64")
        .to_owned();
    let instrument_hint = name_looks_like_instrument(&name);
    let effect_hint = name_looks_like_effect(&name);
    PluginDescriptor {
        format: PluginFormat::Vst2,
        uid: format!("vst2-path:{}", crate::vst::stable_path_hash(path)),
        name,
        vendor: "yabridge".into(),
        path: path.to_owned(),
        is_instrument: instrument_hint || !effect_hint,
        is_effect: effect_hint || !instrument_hint,
        has_editor: true,
    }
}

fn inspect(path: &Path) -> Result<PluginDescriptor> {
    let host = make_host(44_100.0, 512);
    let mut loader = PluginLoader::load(path, host).map_err(|e| anyhow!(e))?;
    let mut instance = loader.instance().map_err(|e| anyhow!(e))?;
    instance.init();
    let info = instance.get_info();
    let has_editor = instance.get_editor().is_some();
    let is_instrument = matches!(info.category, Category::Synth)
        || info.inputs == 0
        || name_looks_like_instrument(&info.name);
    Ok(PluginDescriptor {
        format: PluginFormat::Vst2,
        uid: format!("vst2:{:08x}", info.unique_id as u32),
        name: info.name,
        vendor: info.vendor,
        path: path.to_owned(),
        is_instrument,
        is_effect: !is_instrument,
        has_editor,
    })
}

fn make_host(sample_rate: f64, block_size: u32) -> Arc<Mutex<Vst2Host>> {
    Arc::new(Mutex::new(Vst2Host {
        runtime: ParkingMutex::new(HostRuntime {
            sample_rate,
            block_size: block_size as isize,
            sample_pos: 0.0,
            tempo: 120.0,
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            playing: false,
            cycle: false,
        }),
    }))
}

impl Vst2Plugin {
    pub fn load(
        path: &Path,
        sample_rate: f64,
        block_size: u32,
        state: Option<&[u8]>,
    ) -> Result<Self> {
        let host = make_host(sample_rate, block_size);
        let mut loader = PluginLoader::load(path, Arc::clone(&host)).map_err(|e| anyhow!(e))?;
        let mut instance = loader.instance().map_err(|e| anyhow!(e))?;
        instance.init();
        instance.set_sample_rate(sample_rate as f32);
        instance.set_block_size(block_size as i64);
        let info = instance.get_info();
        let has_editor = instance.get_editor().is_some();
        let params = instance.get_parameter_object();
        if let Some(data) = state.filter(|data| !data.is_empty()) {
            restore_state(params.as_ref(), &info, data);
        }
        instance.resume();
        let host_buffer = HostBuffer::from_info(&info);
        Ok(Self {
            instance,
            params,
            info,
            host,
            host_buffer,
            midi_buffer: SendEventBuffer::new(cott_ipc::MAX_MIDI_EVENTS),
            has_editor,
            editor: None,
            owned_editor: None,
        })
    }

    pub fn descriptor_meta(&mut self) -> (String, u32, Vec<ParamInfo>, bool, bool) {
        let params = self.params();
        let instrument = matches!(self.info.category, Category::Synth)
            || self.info.inputs == 0
            || name_looks_like_instrument(&self.info.name);
        (
            self.info.name.clone(),
            self.latency(),
            params,
            self.has_editor,
            instrument,
        )
    }

    pub fn params(&self) -> Vec<ParamInfo> {
        (0..self.info.parameters.max(0))
            .map(|index| ParamInfo {
                id: index as u32,
                name: self.params.get_parameter_name(index),
                default: self.params.get_parameter(index),
                min: 0.0,
                max: 1.0,
            })
            .collect()
    }

    pub fn set_param(&self, id: u32, value: f32) {
        if id < self.info.parameters.max(0) as u32 {
            self.params.set_parameter(id as i32, value.clamp(0.0, 1.0));
        }
    }

    pub fn get_state(&self) -> Vec<u8> {
        let mut state = Vec::new();
        state.extend_from_slice(STATE_MAGIC);
        if self.info.preset_chunks {
            state.push(1);
            state.extend_from_slice(&self.params.get_bank_data());
        } else {
            state.push(0);
            let count = self.info.parameters.max(0) as u32;
            state.extend_from_slice(&count.to_le_bytes());
            for index in 0..count {
                state.extend_from_slice(&self.params.get_parameter(index as i32).to_le_bytes());
            }
        }
        state
    }

    pub fn set_state(&self, data: &[u8]) {
        if !data.is_empty() {
            restore_state(self.params.as_ref(), &self.info, data);
        }
    }

    pub fn latency(&self) -> u32 {
        self.info.initial_delay.max(0) as u32
    }

    pub fn open_editor(&mut self, parent_x11: Option<u64>) -> Result<()> {
        self.close_editor();
        let (parent, owned) = match parent_x11 {
            Some(id) => (id, None),
            None => {
                let window = FloatingEditorWindow::create_default(&self.info.name)
                    .context("create VST2 editor parent")?;
                (window.embed_window_id(), Some(window))
            }
        };
        let mut editor = self
            .instance
            .get_editor()
            .ok_or_else(|| anyhow!("plugin has no VST2 editor"))?;
        if !editor.open(parent as usize as *mut std::ffi::c_void) {
            return Err(anyhow!("VST2 editor refused to open"));
        }
        if let Some(mut window) = owned {
            let (width, height) = editor.size();
            if width > 0 && height > 0 {
                window.resize(width as u32, height as u32);
            }
            self.owned_editor = Some(window);
        }
        self.editor = Some(editor);
        Ok(())
    }

    pub fn close_editor(&mut self) {
        if let Some(mut editor) = self.editor.take() {
            editor.close();
        }
        self.owned_editor = None;
    }

    pub fn pump_editor(&mut self) -> bool {
        if let Some(window) = self.owned_editor.as_mut()
            && !window.pump_events()
        {
            self.close_editor();
            return false;
        }
        if let Some(editor) = self.editor.as_mut() {
            editor.idle();
        }
        true
    }

    pub fn process(&mut self, shm: &mut SharedAudioRegion, transport: &TransportInfo) -> bool {
        let frames = transport.block_size as usize;
        if frames == 0 || frames > cott_ipc::MAX_BLOCK_FRAMES {
            return false;
        }
        if let Ok(host) = self.host.lock() {
            *host.runtime.lock() = HostRuntime {
                sample_rate: transport.sample_rate,
                block_size: frames as isize,
                sample_pos: transport.project_time_samples as f64,
                tempo: transport.tempo,
                time_sig_numerator: transport.time_sig_numerator as i32,
                time_sig_denominator: transport.time_sig_denominator as i32,
                playing: transport.playing,
                cycle: transport.cycle,
            };
        }

        let midi_count = shm
            .header()
            .midi_count
            .min(cott_ipc::MAX_MIDI_EVENTS as u32) as usize;
        let midi_events: Vec<MidiEvent> = shm.midi_mut()[..midi_count]
            .iter()
            .map(|event| MidiEvent {
                data: [event.status, event.data1, event.data2],
                delta_frames: event.sample_offset.min(frames as u32 - 1) as i32,
                live: !transport.playing,
                note_length: None,
                note_offset: None,
                detune: 0,
                note_off_velocity: 0,
            })
            .collect();
        self.midi_buffer.store_events(midi_events);
        self.instance.process_events(self.midi_buffer.events());

        let input_count = self.info.inputs.max(0) as usize;
        let output_count = self.info.outputs.max(0) as usize;
        if output_count == 0 {
            return false;
        }
        let mut inputs = vec![vec![0.0f32; frames]; input_count];
        let mut outputs = vec![vec![0.0f32; frames]; output_count];
        {
            let shared = shm.audio_in_mut();
            for (channel, input) in inputs.iter_mut().enumerate() {
                let source = channel.min(1) * cott_ipc::MAX_BLOCK_FRAMES;
                input.copy_from_slice(&shared[source..source + frames]);
            }
        }
        let mut buffer = self.host_buffer.bind(&inputs, &mut outputs);
        self.instance.process(&mut buffer);
        let shared = shm.audio_out_mut();
        shared[..frames].copy_from_slice(&outputs[0]);
        let right = if output_count > 1 {
            &outputs[1]
        } else {
            &outputs[0]
        };
        shared[cott_ipc::MAX_BLOCK_FRAMES..cott_ipc::MAX_BLOCK_FRAMES + frames]
            .copy_from_slice(right);
        true
    }
}

fn restore_state(params: &dyn PluginParameters, info: &vst::plugin::Info, data: &[u8]) {
    let Some(payload) = data.strip_prefix(STATE_MAGIC) else {
        // Projects saved before the multi-format host stored raw VST2 chunks.
        if info.preset_chunks {
            params.load_bank_data(data);
        }
        return;
    };
    match payload.split_first() {
        Some((&1, chunk)) if info.preset_chunks => params.load_bank_data(chunk),
        Some((&0, values)) if values.len() >= 4 => {
            let count = u32::from_le_bytes(values[..4].try_into().unwrap())
                .min(info.parameters.max(0) as u32);
            for (index, encoded) in values[4..].chunks_exact(4).take(count as usize).enumerate() {
                params.set_parameter(
                    index as i32,
                    f32::from_le_bytes(encoded.try_into().unwrap()).clamp(0.0, 1.0),
                );
            }
        }
        _ => {}
    }
}

impl Drop for Vst2Plugin {
    fn drop(&mut self) {
        self.close_editor();
        self.instance.suspend();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestParams(std::sync::Mutex<Vec<f32>>);

    impl PluginParameters for TestParams {
        fn get_parameter(&self, index: i32) -> f32 {
            self.0.lock().unwrap()[index as usize]
        }

        fn set_parameter(&self, index: i32, value: f32) {
            self.0.lock().unwrap()[index as usize] = value;
        }
    }

    #[test]
    fn restores_parameter_state_for_plugins_without_chunks() {
        let params = TestParams(std::sync::Mutex::new(vec![0.0, 0.0]));
        let info = vst::plugin::Info {
            parameters: 2,
            preset_chunks: false,
            ..Default::default()
        };
        let mut state = STATE_MAGIC.to_vec();
        state.push(0);
        state.extend_from_slice(&2u32.to_le_bytes());
        state.extend_from_slice(&0.25f32.to_le_bytes());
        state.extend_from_slice(&0.75f32.to_le_bytes());

        restore_state(&params, &info, &state);

        assert_eq!(*params.0.lock().unwrap(), vec![0.25, 0.75]);
    }
}
