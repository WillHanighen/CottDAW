//! Application state and eframe App impl.

use crate::audio::AudioEngine;
use crate::plugins::PluginHost;
use crate::ui::{self, UiState};
use cott_core::clips::{MidiNote, TrackKind};
use cott_core::commands::{Command, CommandStack};
use cott_core::dsp::SampleCache;
use cott_core::engine::{EngineCommand, push_project_snapshot};
use cott_core::export::{
    ExportFormat, ExportOptions, export_gonio_mp4, export_opus, write_wav_file,
};
use cott_core::graph::NodeKind;
use cott_core::ids::{ClipId, NodeId, PluginInstanceId, TrackId};
use cott_core::import::{
    add_audio_asset_to_project, cache_put, create_audio_clip_from_import,
    create_midi_clip_from_notes, import_audio_file, import_midi_file, rebuild_sample_cache,
};
use cott_core::project::Project;
use cott_core::time::TransportState;
use cott_ipc::PluginDescriptor;
use eframe::egui;
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

pub struct CottApp {
    pub project: Project,
    pub commands: CommandStack,
    pub ui: UiState,
    pub audio: Option<AudioEngine>,
    pub plugin_host: Arc<Mutex<PluginHost>>,
    pub sample_cache: Arc<SampleCache>,
    pub status: String,
    pub project_path: Option<PathBuf>,
    pub last_autosave: Instant,
    pub meters: IndexMap<NodeId, cott_core::dsp::MeterState>,
    /// Background VST scan result (UI must stay responsive while Wine loads).
    plugin_scan_rx: Option<mpsc::Receiver<Result<Vec<PluginDescriptor>, String>>>,
    /// Background export result.
    export_rx: Option<mpsc::Receiver<Result<PathBuf, String>>>,
}

impl CottApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

        let plugin_host = Arc::new(Mutex::new(PluginHost::new()));
        // Prefer workspace target path for worker.
        if let Ok(exe) = std::env::current_exe() {
            let candidates = [
                exe.parent().map(|p| p.join("cott-vst-worker")),
                Some(PathBuf::from("target/debug/cott-vst-worker")),
                Some(PathBuf::from("target/release/cott-vst-worker")),
            ];
            for c in candidates.into_iter().flatten() {
                if c.exists() {
                    plugin_host.lock().set_worker_bin(c);
                    break;
                }
            }
        }

        let audio = match AudioEngine::start(Arc::clone(&plugin_host)) {
            Ok(a) => Some(a),
            Err(e) => {
                warn!("audio start failed: {e:#} (UI-only mode)");
                None
            }
        };

        let mut project = Project::new("Untitled");
        if let Some(audio) = &audio {
            project.tempo.sample_rate = audio.sample_rate;
        }
        project.add_midi_track("MIDI 1");
        project.add_audio_track("Audio 1");

        let mut app = Self {
            project,
            commands: CommandStack::default(),
            ui: UiState::default(),
            audio,
            plugin_host,
            sample_cache: Arc::new(SampleCache::default()),
            status: "Ready".into(),
            project_path: None,
            last_autosave: Instant::now(),
            meters: IndexMap::new(),
            plugin_scan_rx: None,
            export_rx: None,
        };
        app.sync_engine();
        // Never block window creation on VST scan (yabridge/Wine can take minutes).
        app.start_plugin_scan();
        app
    }

    pub fn start_plugin_scan(&mut self) {
        if self.plugin_scan_rx.is_some() {
            self.status = "Plugin scan already running…".into();
            return;
        }
        let (worker_bin, blacklist) = {
            let host = self.plugin_host.lock();
            (
                host.worker_bin().to_path_buf(),
                host.scan_blacklist().to_vec(),
            )
        };
        let (tx, rx) = mpsc::channel();
        self.plugin_scan_rx = Some(rx);
        self.status = "Scanning plugins…".into();
        std::thread::Builder::new()
            .name("cott-plugin-scan".into())
            .spawn(move || {
                let result = PluginHost::scan_catalog(&worker_bin, &blacklist)
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(result);
            })
            .expect("spawn plugin scan thread");
    }

    pub fn poll_plugin_scan(&mut self) {
        let Some(rx) = &self.plugin_scan_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(catalog)) => {
                self.plugin_scan_rx = None;
                let n = catalog.len();
                self.plugin_host.lock().catalog = catalog;
                self.status = format!("Found {n} plugins");
                info!("plugin scan finished: {n}");
            }
            Ok(Err(e)) => {
                self.plugin_scan_rx = None;
                self.status = format!("Plugin scan: {e}");
                warn!("plugin scan failed: {e}");
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.plugin_scan_rx = None;
                self.status = "Plugin scan interrupted".into();
            }
        }
    }

    pub fn is_scanning_plugins(&self) -> bool {
        self.plugin_scan_rx.is_some()
    }

    pub fn sync_engine(&mut self) {
        if let Err(e) = self.project.try_compiled_plan() {
            self.status = format!("Graph compile error: {e}");
        }
        if let Some(audio) = &mut self.audio {
            push_project_snapshot(
                &mut audio.cmd_tx,
                &self.project,
                Arc::clone(&self.sample_cache),
            );
        }
    }

    /// Mirror live plugin worker failure/latency into the project graph for UI/DSP.
    pub fn sync_plugin_runtime_state(&mut self) {
        let host = self.plugin_host.lock();
        let mut dirty = false;
        for node in self.project.graph.nodes.values_mut() {
            let (instance_id, failed_flag) = match &mut node.kind {
                NodeKind::Vst3Instrument {
                    instance_id,
                    failed,
                    ..
                }
                | NodeKind::Vst3Effect {
                    instance_id,
                    failed,
                    ..
                } => (*instance_id, failed),
                _ => continue,
            };
            let Some(inst) = host.instances.get(&instance_id) else {
                continue;
            };
            if *failed_flag != inst.failed {
                *failed_flag = inst.failed;
                dirty = true;
            }
            if node.latency_samples != inst.latency {
                node.latency_samples = inst.latency;
                dirty = true;
            }
        }
        drop(host);
        if dirty {
            self.sync_engine();
        }
    }

    pub fn play(&mut self) {
        self.project.transport = TransportState::Playing;
        if let Some(audio) = &mut self.audio {
            let _ = audio
                .cmd_tx
                .push(EngineCommand::SetTransport(TransportState::Playing));
            audio.shared.set_state(TransportState::Playing);
        }
        self.status = "Playing".into();
    }

    pub fn toggle_play_stop(&mut self) {
        if self.project.transport == TransportState::Playing {
            self.stop();
        } else {
            self.play();
        }
    }

    pub fn stop(&mut self) {
        self.project.transport = TransportState::Stopped;
        if let Some(audio) = &mut self.audio {
            let _ = audio
                .cmd_tx
                .push(EngineCommand::SetTransport(TransportState::Stopped));
            let _ = audio
                .cmd_tx
                .push(EngineCommand::Seek(cott_core::time::SamplePos(0)));
            audio.shared.set_state(TransportState::Stopped);
            audio.shared.set_position(cott_core::time::SamplePos(0));
        }
        self.status = "Stopped".into();
    }

    pub fn toggle_loop(&mut self) {
        self.project.loop_enabled = !self.project.loop_enabled;
        self.sync_engine();
        self.status = if self.project.loop_enabled {
            "Loop enabled".into()
        } else {
            "Loop disabled".into()
        };
    }

    pub fn undo(&mut self) {
        if self.commands.undo(&mut self.project) {
            self.sync_engine();
            self.status = "Undo".into();
        }
    }

    pub fn redo(&mut self) {
        if self.commands.redo(&mut self.project) {
            self.sync_engine();
            self.status = "Redo".into();
        }
    }

    pub fn poll_audio_events(&mut self) {
        if let Some(audio) = &mut self.audio {
            while let Ok(evt) = audio.evt_rx.pop() {
                match evt {
                    cott_core::engine::EngineEvent::Meters(m) => self.meters = m,
                    cott_core::engine::EngineEvent::Position(_) => {}
                    cott_core::engine::EngineEvent::XRun
                    | cott_core::engine::EngineEvent::Underrun => {
                        self.status = "XRun".into();
                    }
                }
            }
        }
    }

    pub fn maybe_autosave(&mut self) {
        if self.last_autosave.elapsed() < Duration::from_secs(60) {
            return;
        }
        self.last_autosave = Instant::now();
        let dir = directories::ProjectDirs::from("dev", "Cottage", "CottDAW")
            .map(|d| d.data_dir().join("autosave"));
        if let Some(dir) = dir {
            match self.project.atomic_autosave(&dir) {
                Ok(p) => info!("autosaved to {}", p.display()),
                Err(e) => warn!("autosave failed: {e}"),
            }
        }
    }

    pub fn save_project(&mut self) {
        let path = if let Some(p) = &self.project_path {
            p.clone()
        } else if let Some(folder) = rfd::FileDialog::new()
            .set_title("Save Project")
            .pick_folder()
        {
            folder
        } else {
            return;
        };
        // Persist plugin states.
        {
            let mut host = self.plugin_host.lock();
            for (id, blob) in self.project.plugin_states.iter_mut() {
                if let Some(data) = host.save_state(*id) {
                    blob.data = data;
                }
            }
        }
        match self.project.save_to_dir(&path) {
            Ok(()) => {
                self.project_path = Some(path.clone());
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    pub fn load_project(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Open Project")
            .pick_folder()
        else {
            return;
        };
        match Project::load_from_dir(&folder) {
            Ok(p) => {
                self.stop();
                self.project = p;
                self.project_path = Some(folder.clone());
                self.commands = CommandStack::default();
                let sr = self
                    .audio
                    .as_ref()
                    .map(|a| a.sample_rate)
                    .unwrap_or(self.project.tempo.sample_rate);
                match rebuild_sample_cache(&self.project, sr) {
                    Ok(cache) => self.sample_cache = Arc::new(cache),
                    Err(e) => {
                        warn!("rebuild sample cache: {e:#}");
                        self.sample_cache = Arc::new(SampleCache::default());
                    }
                }
                self.sync_engine();
                self.reload_plugins_from_project();
                self.status = "Project loaded".into();
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    fn reload_plugins_from_project(&mut self) {
        let sr = self.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48_000) as f64;
        let bs = self.audio.as_ref().map(|a| a.buffer_size).unwrap_or(256);
        let targets: Vec<_> = self
            .project
            .graph
            .nodes
            .iter()
            .filter_map(|(node_id, node)| match &node.kind {
                NodeKind::Vst3Instrument {
                    instance_id,
                    plugin_uid,
                    plugin_path,
                    ..
                }
                | NodeKind::Vst3Effect {
                    instance_id,
                    plugin_uid,
                    plugin_path,
                    ..
                } => Some((
                    *node_id,
                    *instance_id,
                    plugin_uid.clone(),
                    plugin_path.clone(),
                )),
                _ => None,
            })
            .collect();
        let mut host = self.plugin_host.lock();
        let mut any_failed = false;
        for (node_id, instance_id, plugin_uid, plugin_path) in targets {
            let state = self
                .project
                .plugin_states
                .get(&instance_id)
                .map(|b| b.data.clone());
            if let Err(e) = host.load(
                instance_id,
                &plugin_uid,
                PathBuf::from(&plugin_path).as_path(),
                sr,
                bs,
                state,
            ) {
                warn!("reload plugin: {e:#}");
                any_failed = true;
                if let Some(node) = self.project.graph.nodes.get_mut(&node_id) {
                    match &mut node.kind {
                        NodeKind::Vst3Instrument { failed, .. }
                        | NodeKind::Vst3Effect { failed, .. } => {
                            *failed = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        drop(host);
        if any_failed {
            self.sync_engine();
        }
    }

    pub fn import_audio(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "flac", "ogg", "mp3", "aiff", "m4a"])
            .pick_file()
        else {
            return;
        };
        let track_id = self.ui.selected_track.or_else(|| {
            self.project
                .tracks
                .iter()
                .find(|t| t.kind == TrackKind::Audio)
                .map(|t| t.id)
        });
        let Some(track_id) = track_id else {
            self.status = "Select an audio track".into();
            return;
        };
        // Ensure project has a root for assets.
        if self.project.root_dir.is_none() {
            let dir = std::env::temp_dir().join(format!("cott-proj-{}", uuid::Uuid::new_v4()));
            if let Err(e) = self.project.save_to_dir(&dir) {
                self.status = format!("temp project: {e}");
                return;
            }
            self.project_path = Some(dir);
        }
        match import_audio_file(&path, self.project.tempo.sample_rate) {
            Ok(decoded) => match add_audio_asset_to_project(&mut self.project, &path, &decoded) {
                Ok(asset_id) => {
                    let mut cache = SampleCache {
                        buffers: self.sample_cache.buffers.clone(),
                    };
                    cache_put(&mut cache, asset_id, decoded.buffer);
                    self.sample_cache = Arc::new(cache);
                    let start = self.playhead_beats();
                    if let Some(clip) =
                        create_audio_clip_from_import(&mut self.project, track_id, asset_id, start)
                    {
                        self.commands
                            .push(&mut self.project, Command::AddClip { clip });
                        self.sync_engine();
                        self.status = "Audio imported".into();
                    }
                }
                Err(e) => self.status = format!("Asset: {e}"),
            },
            Err(e) => self.status = format!("Import failed: {e:#}"),
        }
    }

    pub fn import_midi(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MIDI", &["mid", "midi"])
            .pick_file()
        else {
            return;
        };
        let track_id = self.ui.selected_track.or_else(|| {
            self.project
                .tracks
                .iter()
                .find(|t| t.kind == TrackKind::Midi)
                .map(|t| t.id)
        });
        let Some(track_id) = track_id else {
            self.status = "Select a MIDI track".into();
            return;
        };
        match import_midi_file(&path, &self.project.tempo) {
            Ok(notes) => {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("MIDI")
                    .to_string();
                let clip =
                    create_midi_clip_from_notes(track_id, name, notes, self.playhead_beats());
                self.commands
                    .push(&mut self.project, Command::AddClip { clip });
                self.sync_engine();
                self.status = "MIDI imported".into();
            }
            Err(e) => self.status = format!("MIDI import: {e:#}"),
        }
    }

    pub fn playhead_beats(&self) -> f64 {
        let sample = self
            .audio
            .as_ref()
            .map(|a| a.shared.position())
            .unwrap_or_default();
        self.project.tempo.sample_to_beat(sample).0
    }

    pub fn add_midi_track(&mut self) {
        let n = self.project.tracks.len() + 1;
        let id = self.project.add_midi_track(format!("MIDI {n}"));
        if let Some(cmd) = track_add_command(&self.project, id) {
            self.commands.record(cmd);
        }
        self.sync_engine();
    }

    pub fn add_audio_track(&mut self) {
        let n = self.project.tracks.len() + 1;
        let id = self.project.add_audio_track(format!("Audio {n}"));
        if let Some(cmd) = track_add_command(&self.project, id) {
            self.commands.record(cmd);
        }
        self.sync_engine();
    }

    pub fn load_instrument_on_selected_track(
        &mut self,
        uid: String,
        path: PathBuf,
        name: String,
        position: [f32; 2],
    ) {
        let track_id = self
            .ui
            .selected_track
            .and_then(|id| {
                self.project
                    .tracks
                    .iter()
                    .find(|t| t.id == id && t.kind == TrackKind::Midi)
                    .map(|t| t.id)
            })
            .or_else(|| {
                self.project
                    .tracks
                    .iter()
                    .find(|t| t.kind == TrackKind::Midi)
                    .map(|t| t.id)
            });
        let Some(track_id) = track_id else {
            self.status = "Add a MIDI track before loading an instrument".into();
            return;
        };
        self.ui.selected_track = Some(track_id);
        let Some((node_id, unloaded)) = self.project.attach_instrument(
            track_id,
            uid.clone(),
            path.display().to_string(),
            name.clone(),
        ) else {
            self.status = "Could not attach instrument (need MIDI track)".into();
            return;
        };
        if let Some(old_instance) = unloaded {
            self.plugin_host.lock().unload(old_instance);
        }
        if let Some(node) = self.project.graph.nodes.get_mut(&node_id) {
            node.position = position;
        }
        let instance_id = match &self.project.graph.nodes[&node_id].kind {
            NodeKind::Vst3Instrument { instance_id, .. } => *instance_id,
            _ => PluginInstanceId::new(),
        };
        let sr = self.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48_000) as f64;
        let bs = self.audio.as_ref().map(|a| a.buffer_size).unwrap_or(256);
        let load_result = {
            let mut host = self.plugin_host.lock();
            host.load(instance_id, &uid, &path, sr, bs, None)
        };
        match load_result {
            Ok(()) => {
                self.sync_engine();
                self.ui.selected_node = Some(node_id);
                self.status = format!("Loaded instrument {name}");
            }
            Err(e) => {
                error!("plugin load: {e:#}");
                self.project.plugin_states.shift_remove(&instance_id);
                self.project.graph.remove_node(node_id);
                if let Some(track) = self
                    .project
                    .tracks
                    .iter_mut()
                    .find(|t| t.instrument_node == Some(node_id))
                {
                    track.instrument_node = None;
                }
                self.sync_engine();
                self.status = format!("Plugin load failed: {e}");
            }
        }
    }

    pub fn load_effect(&mut self, uid: String, path: PathBuf, name: String, position: [f32; 2]) {
        let node_id = self.project.add_effect(
            uid.clone(),
            path.display().to_string(),
            name.clone(),
            position,
        );
        let instance_id = match &self.project.graph.nodes[&node_id].kind {
            NodeKind::Vst3Effect { instance_id, .. } => *instance_id,
            _ => {
                self.status = "Could not create effect node".into();
                return;
            }
        };
        let sr = self.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48_000) as f64;
        let bs = self.audio.as_ref().map(|a| a.buffer_size).unwrap_or(256);
        let load_result = {
            let mut host = self.plugin_host.lock();
            host.load(instance_id, &uid, &path, sr, bs, None)
        };
        match load_result {
            Ok(()) => {
                self.sync_engine();
                self.ui.selected_node = Some(node_id);
                self.status = format!("Loaded effect {name}");
            }
            Err(e) => {
                self.project.graph.remove_node(node_id);
                self.project.plugin_states.shift_remove(&instance_id);
                error!("plugin load: {e:#}");
                self.status = format!("Effect load failed: {e}");
            }
        }
    }

    pub fn can_remove_graph_node(&self, node_id: NodeId) -> bool {
        let Some(node) = self.project.graph.nodes.get(&node_id) else {
            return false;
        };
        match &node.kind {
            NodeKind::MasterOutput
            | NodeKind::MidiClipSource { .. }
            | NodeKind::AudioClipSource { .. } => false,
            NodeKind::GainPan { .. } => !self
                .project
                .tracks
                .iter()
                .any(|track| track.gain_node == Some(node_id)),
            NodeKind::SumMixer | NodeKind::Vst3Instrument { .. } | NodeKind::Vst3Effect { .. } => {
                true
            }
        }
    }

    pub fn remove_graph_node(&mut self, node_id: NodeId) {
        if !self.can_remove_graph_node(node_id) {
            self.status = "Cannot delete required track/master nodes".into();
            return;
        }
        let Some(node) = self.project.graph.nodes.get(&node_id).cloned() else {
            return;
        };
        let edges: Vec<_> = self
            .project
            .graph
            .edges
            .values()
            .filter(|e| e.from_node == node_id || e.to_node == node_id)
            .cloned()
            .collect();
        let name = node.name.clone();
        let mut instrument_track = None;
        let mut plugin_state = None;
        match &node.kind {
            NodeKind::Vst3Instrument { instance_id, .. }
            | NodeKind::Vst3Effect { instance_id, .. } => {
                let instance_id = *instance_id;
                self.plugin_host.lock().unload(instance_id);
                plugin_state = self.project.plugin_states.shift_remove(&instance_id);
                for track in &mut self.project.tracks {
                    if track.instrument_node == Some(node_id) {
                        instrument_track = Some(track.id);
                        track.instrument_node = None;
                    }
                }
            }
            _ => {}
        }
        self.project.graph.remove_node(node_id);
        self.commands.record(Command::RemoveNode {
            node,
            edges,
            instrument_track,
            plugin_state,
        });
        if self.ui.selected_node == Some(node_id) {
            self.ui.selected_node = None;
        }
        self.sync_engine();
        self.status = format!("Deleted {name}");
    }

    pub fn remove_selected_graph_node(&mut self) {
        let Some(node_id) = self.ui.selected_node else {
            return;
        };
        self.remove_graph_node(node_id);
    }

    pub fn open_export_dialog(&mut self) {
        if self.export_rx.is_some() {
            self.status = "Export already running…".into();
            return;
        }
        self.ui.show_export_dialog = true;
    }

    pub fn is_exporting(&self) -> bool {
        self.export_rx.is_some()
    }

    /// After the settings window: pick destination, then start the bounce.
    pub fn confirm_export(&mut self) {
        if self.export_rx.is_some() {
            self.status = "Export already running…".into();
            return;
        }
        let format = self.ui.export_dialog.format;
        let Some(path) = rfd::FileDialog::new()
            .add_filter(format.filter_name(), &[format.extension()])
            .set_file_name(format.default_file_name())
            .save_file()
        else {
            // Re-open settings if the user cancels the save dialog.
            self.ui.show_export_dialog = true;
            return;
        };

        let mut opts = ExportOptions {
            format,
            bitrate_bps: self.ui.export_dialog.bitrate_bps,
            tail_beats: self.ui.export_dialog.tail_beats,
            gonio: self.ui.export_dialog.gonio.clone(),
            ..Default::default()
        };
        opts.gonio = opts.gonio.clamp();

        let project = self.project.clone();
        let sample_cache = Arc::clone(&self.sample_cache);
        let plugin_host = Arc::clone(&self.plugin_host);
        let (tx, rx) = mpsc::channel();
        self.export_rx = Some(rx);
        self.status = format!("Exporting {}…", path.display());
        std::thread::Builder::new()
            .name("cott-export".into())
            .spawn(move || {
                let mut live = crate::plugins::HostPluginAudio { inner: plugin_host };
                let result = match opts.format {
                    ExportFormat::Wav => {
                        let buf = cott_core::export::render_project_stereo(
                            &project,
                            &sample_cache,
                            &mut live,
                            &opts,
                        );
                        write_wav_file(&buf, project.tempo.sample_rate, &path).map(|_| path)
                    }
                    ExportFormat::Opus => {
                        export_opus(&project, &sample_cache, &mut live, &path, &opts).map(|_| path)
                    }
                    ExportFormat::GonioMp4 => {
                        export_gonio_mp4(&project, &sample_cache, &mut live, &path, &opts)
                            .map(|_| path)
                    }
                }
                .map_err(|e| format!("{e:#}"));
                let _ = tx.send(result);
            })
            .expect("spawn export thread");
    }

    pub fn poll_export(&mut self) {
        let Some(rx) = &self.export_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.export_rx = None;
                // Bounce shares the live PluginHost — flush any latched voices.
                self.flush_instrument_voices();
                self.status = format!("Exported {}", path.display());
            }
            Ok(Err(e)) => {
                self.export_rx = None;
                self.flush_instrument_voices();
                self.status = format!("Export failed: {e}");
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.export_rx = None;
                self.status = "Export interrupted".into();
            }
        }
    }

    /// Ask the engine to send MIDI panic so VST voices don't stay latched.
    fn flush_instrument_voices(&mut self) {
        if let Some(audio) = &mut self.audio {
            let pos = audio.shared.position();
            let _ = audio.cmd_tx.push(EngineCommand::Seek(pos));
        }
    }

    pub fn add_note_at(&mut self, clip_id: ClipId, pitch: u8, start_beats: f64, length: f64) {
        self.ensure_clip_length(clip_id, start_beats + length);
        let note = MidiNote::new(pitch, 100, start_beats, length);
        self.commands
            .push(&mut self.project, Command::AddNote { clip_id, note });
        self.sync_engine();
        if let Some(track_id) = self
            .project
            .clips
            .iter()
            .find(|c| c.id == clip_id)
            .map(|c| c.track_id)
        {
            self.preview_note(track_id, pitch);
        }
    }

    /// Grow a clip so notes stay inside it.
    pub fn ensure_clip_length(&mut self, clip_id: ClipId, needed_end_beats: f64) {
        let Some(clip) = self.project.clips.iter().find(|c| c.id == clip_id) else {
            return;
        };
        if needed_end_beats <= clip.length_beats {
            return;
        }
        let old_start = clip.start_beats;
        let old_length = clip.length_beats;
        self.commands.push(
            &mut self.project,
            Command::MoveClip {
                clip_id,
                old_start,
                new_start: old_start,
                old_length,
                new_length: needed_end_beats,
            },
        );
    }

    /// Shrink a MIDI clip so trailing empty beats are removed after notes change.
    /// Always keeps at least one bar so an empty clip remains editable.
    pub fn shrink_clip_to_notes(&mut self, clip_id: ClipId) {
        let Some(clip) = self.project.clips.iter().find(|c| c.id == clip_id) else {
            return;
        };
        let Some(notes) = clip.notes() else {
            return;
        };
        let content_end = notes
            .iter()
            .map(MidiNote::end_beats)
            .fold(0.0_f64, f64::max);
        let min_len = self.project.tempo.bar_length_beats();
        let new_length = content_end.max(min_len);
        if new_length >= clip.length_beats - 1e-9 {
            return;
        }
        let old_start = clip.start_beats;
        let old_length = clip.length_beats;
        self.commands.push(
            &mut self.project,
            Command::MoveClip {
                clip_id,
                old_start,
                new_start: old_start,
                old_length,
                new_length,
            },
        );
    }

    pub fn remove_clip(&mut self, clip_id: ClipId) {
        let Some(clip) = self
            .project
            .clips
            .iter()
            .find(|c| c.id == clip_id)
            .cloned()
        else {
            return;
        };
        let name = clip.name.clone();
        self.commands
            .push(&mut self.project, Command::RemoveClip { clip });
        if self.ui.selected_clip == Some(clip_id) {
            self.ui.selected_clip = None;
            self.ui.piano_drag = None;
        }
        if self.ui.clip_drag.as_ref().map(|d| d.clip_id) == Some(clip_id) {
            self.ui.clip_drag = None;
        }
        self.sync_engine();
        self.status = format!("Deleted clip {name}");
    }

    pub fn remove_selected_clip(&mut self) {
        let Some(clip_id) = self.ui.selected_clip else {
            return;
        };
        self.remove_clip(clip_id);
    }

    pub fn move_clip(
        &mut self,
        clip_id: ClipId,
        new_start: f64,
        new_length: f64,
    ) {
        let Some(clip) = self.project.clips.iter().find(|c| c.id == clip_id) else {
            return;
        };
        let old_start = clip.start_beats;
        let old_length = clip.length_beats;
        let new_start = new_start.max(0.0);
        let new_length = new_length.max(0.25);
        if (old_start - new_start).abs() < 1e-9 && (old_length - new_length).abs() < 1e-9 {
            return;
        }
        self.commands.push(
            &mut self.project,
            Command::MoveClip {
                clip_id,
                old_start,
                new_start,
                old_length,
                new_length,
            },
        );
        self.sync_engine();
    }

    pub fn edit_note(&mut self, clip_id: ClipId, before: MidiNote, after: MidiNote) {
        if before.pitch == after.pitch
            && before.start_beats == after.start_beats
            && before.length_beats == after.length_beats
            && before.velocity == after.velocity
            && before.channel == after.channel
        {
            return;
        }
        self.ensure_clip_length(clip_id, after.start_beats + after.length_beats);
        let note_id = before.id;
        self.commands.push(
            &mut self.project,
            Command::EditNote {
                clip_id,
                note_id,
                before,
                after: after.clone(),
            },
        );
        self.shrink_clip_to_notes(clip_id);
        self.sync_engine();
        if let Some(track_id) = self
            .project
            .clips
            .iter()
            .find(|c| c.id == clip_id)
            .map(|c| c.track_id)
        {
            self.preview_note(track_id, after.pitch);
        }
    }

    /// Audition a MIDI pitch through the track's instrument (works while stopped).
    pub fn preview_note(&mut self, track_id: TrackId, pitch: u8) {
        let sample_rate = self
            .audio
            .as_ref()
            .map(|a| a.sample_rate)
            .unwrap_or(48_000);
        let duration_samples = ((sample_rate as f64) * 0.18).round() as u32;
        if let Some(audio) = &mut self.audio {
            let _ = audio.cmd_tx.push(EngineCommand::PreviewNote {
                track_id,
                pitch,
                velocity: 100,
                duration_samples: duration_samples.max(1),
            });
        }
        self.ui.piano_preview_pitch = Some(pitch);
    }

    pub fn preview_note_if_new_pitch(&mut self, track_id: TrackId, pitch: u8) {
        if self.ui.piano_preview_pitch != Some(pitch) {
            self.preview_note(track_id, pitch);
        }
    }

    /// Open the native VST editor for an instrument/effect node (floating window).
    pub fn open_plugin_editor_for_node(&mut self, node_id: NodeId) {
        let Some(node) = self.project.graph.nodes.get(&node_id) else {
            self.status = "No node selected".into();
            return;
        };
        let (instance_id, failed) = match &node.kind {
            NodeKind::Vst3Instrument {
                instance_id,
                failed,
                ..
            }
            | NodeKind::Vst3Effect {
                instance_id,
                failed,
                ..
            } => (*instance_id, *failed),
            _ => {
                self.status = "Not a plugin node — select a VST instrument/effect".into();
                return;
            }
        };
        if failed {
            self.status = "Plugin failed — use Restart in the Plugins tab first".into();
            return;
        }
        match self.plugin_host.lock().open_editor(instance_id, None) {
            Ok(()) => self.status = format!("Opened editor for {}", node.name),
            Err(e) => {
                self.status = format!("Editor: {e} (try Plugins tab for generic params)");
            }
        }
    }

    pub fn set_gain(&mut self, node_id: NodeId, gain_db: f32) {
        let (old_gain, old_pan, old_mute) = match self.project.graph.nodes.get(&node_id) {
            Some(n) => match &n.kind {
                NodeKind::GainPan {
                    gain_db, pan, mute, ..
                } => (*gain_db, *pan, *mute),
                _ => return,
            },
            None => return,
        };
        self.commands.push(
            &mut self.project,
            Command::SetGainPan {
                node_id,
                old_gain,
                new_gain: gain_db,
                old_pan,
                new_pan: old_pan,
                old_mute,
                new_mute: old_mute,
            },
        );
        self.sync_engine();
    }
}

impl eframe::App for CottApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_plugin_scan();
        self.poll_export();
        self.sync_plugin_runtime_state();
        self.poll_audio_events();
        self.maybe_autosave();
        ui::draw(self, ctx);
        if self.is_scanning_plugins() || self.export_rx.is_some() {
            // Keep animating status while Wine finishes scanning / export runs.
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}

fn track_add_command(project: &Project, track_id: TrackId) -> Option<Command> {
    let track = project.tracks.iter().find(|t| t.id == track_id)?.clone();
    let mut node_ids = Vec::new();
    if let Some(id) = track.midi_source_node {
        node_ids.push(id);
    }
    if let Some(id) = track.audio_source_node {
        node_ids.push(id);
    }
    if let Some(id) = track.gain_node {
        node_ids.push(id);
    }
    if let Some(id) = track.instrument_node {
        node_ids.push(id);
    }
    let nodes: Vec<_> = node_ids
        .iter()
        .filter_map(|id| project.graph.nodes.get(id).cloned())
        .collect();
    let edges: Vec<_> = project
        .graph
        .edges
        .values()
        .filter(|e| node_ids.contains(&e.from_node) || node_ids.contains(&e.to_node))
        .cloned()
        .collect();
    Some(Command::AddTrack {
        track,
        nodes,
        edges,
    })
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(28, 30, 34);
    visuals.window_fill = egui::Color32::from_rgb(34, 36, 40);
    ctx.set_visuals(visuals);
}
