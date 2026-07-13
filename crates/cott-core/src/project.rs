//! Versioned project document and asset registry.

use crate::automation::AutomationLane;
use crate::clips::{Clip, ClipContent, Track, TrackKind};
use crate::graph::{AudioGraph, CompiledPlan, GraphNode, NodeKind};
use crate::ids::{AssetId, NodeId, PluginInstanceId, TrackId};
use crate::time::{TempoMap, TransportState};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const PROJECT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    /// Path relative to the project directory.
    pub relative_path: PathBuf,
    pub kind: AssetKind,
    pub sample_rate: u32,
    pub channels: u16,
    pub length_samples: u64,
    pub missing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    Audio,
    MidiFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStateBlob {
    pub instance_id: PluginInstanceId,
    pub plugin_uid: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub version: u32,
    pub name: String,
    pub created_unix_ms: u64,
    pub modified_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub meta: ProjectMeta,
    pub tempo: TempoMap,
    pub transport: TransportState,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub graph: AudioGraph,
    pub master_node: NodeId,
    pub assets: IndexMap<AssetId, Asset>,
    pub automation: Vec<AutomationLane>,
    pub plugin_states: IndexMap<PluginInstanceId, PluginStateBlob>,
    /// Working directory for live asset I/O (runtime only).
    ///
    /// For `.ctgdaw` projects this is an extracted temporary workspace; the
    /// user-facing path lives in the host app as `project_path`.
    #[serde(skip)]
    pub root_dir: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialize error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("unsupported project version {0}")]
    UnsupportedVersion(u32),
    #[error("invalid project: {0}")]
    Invalid(String),
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        let mut graph = AudioGraph::new();
        let master = GraphNode::master_output();
        let master_id = master.id;
        graph.add_node(master);
        let now = unix_ms();
        Self {
            meta: ProjectMeta {
                version: PROJECT_VERSION,
                name: name.into(),
                created_unix_ms: now,
                modified_unix_ms: now,
            },
            tempo: TempoMap::default(),
            transport: TransportState::Stopped,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 16.0,
            tracks: Vec::new(),
            clips: Vec::new(),
            graph,
            master_node: master_id,
            assets: IndexMap::new(),
            automation: Vec::new(),
            plugin_states: IndexMap::new(),
            root_dir: None,
        }
    }

    pub fn touch(&mut self) {
        self.meta.modified_unix_ms = unix_ms();
    }

    /// Suggested arrangement loop end with half a bar of tail after content.
    ///
    /// MIDI clips use their last note rather than the clip boundary so trailing
    /// editable space does not create a long silent section in the loop.
    pub fn suggested_loop_end_beats(&self) -> f64 {
        let content_end = self
            .clips
            .iter()
            .filter_map(|clip| match &clip.content {
                ClipContent::Midi { notes } => notes
                    .iter()
                    .map(|note| clip.start_beats + note.end_beats())
                    .reduce(f64::max),
                ClipContent::Audio { .. } => Some(clip.end_beats()),
            })
            .fold(self.loop_start_beats, f64::max);
        content_end + self.tempo.bar_length_beats() * 0.5
    }

    pub fn add_midi_track(&mut self, name: impl Into<String>) -> TrackId {
        let name = name.into();
        let mut track = Track::new_midi(&name);
        let y = self.tracks.len() as f32 * 100.0;

        let midi_src = GraphNode::midi_clip_source(track.id, format!("{name} MIDI"));
        let mut midi_src = midi_src;
        midi_src.position = [40.0, y];
        let midi_id = midi_src.id;

        let mut gain = GraphNode::stereo_gain_pan(format!("{name} Gain"));
        gain.position = [360.0, y];
        let gain_id = gain.id;

        track.midi_source_node = Some(midi_id);
        track.gain_node = Some(gain_id);

        self.graph.add_node(midi_src);
        self.graph.add_node(gain);
        let _ = self.graph.connect_stereo(gain_id, self.master_node);

        let id = track.id;
        self.tracks.push(track);
        self.touch();
        id
    }

    pub fn add_audio_track(&mut self, name: impl Into<String>) -> TrackId {
        let name = name.into();
        let mut track = Track::new_audio(&name);
        let y = self.tracks.len() as f32 * 100.0;

        let mut audio_src = GraphNode::audio_clip_source(track.id, format!("{name} Audio"));
        audio_src.position = [40.0, y];
        let audio_id = audio_src.id;

        let mut gain = GraphNode::stereo_gain_pan(format!("{name} Gain"));
        gain.position = [360.0, y];
        let gain_id = gain.id;

        track.audio_source_node = Some(audio_id);
        track.gain_node = Some(gain_id);

        self.graph.add_node(audio_src);
        self.graph.add_node(gain);
        let _ = self.graph.connect_stereo(audio_id, gain_id);
        let _ = self.graph.connect_stereo(gain_id, self.master_node);

        let id = track.id;
        self.tracks.push(track);
        self.touch();
        id
    }

    pub fn attach_plugin_instrument(
        &mut self,
        track_id: TrackId,
        plugin_format: String,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
    ) -> Option<(NodeId, Option<PluginInstanceId>)> {
        let track = self.tracks.iter().find(|t| t.id == track_id)?;
        if track.kind != TrackKind::Midi {
            return None;
        }
        let midi_src = track.midi_source_node?;
        let gain = track.gain_node?;
        let old_instrument = track.instrument_node;
        let y = self
            .graph
            .nodes
            .get(&gain)
            .map(|n| n.position[1])
            .unwrap_or(0.0);

        // Tear down any previous instrument on this track so we don't orphan nodes/workers.
        let mut unloaded: Option<PluginInstanceId> = None;
        if let Some(old_id) = old_instrument {
            if let Some(old) = self.graph.remove_node(old_id) {
                if let NodeKind::PluginInstrument { instance_id, .. } = old.kind {
                    self.plugin_states.shift_remove(&instance_id);
                    unloaded = Some(instance_id);
                }
            }
        }

        let instance_id = PluginInstanceId::new();
        let inst = GraphNode {
            id: NodeId::new(),
            name: plugin_name.clone(),
            kind: NodeKind::PluginInstrument {
                instance_id,
                plugin_format,
                plugin_uid: plugin_uid.clone(),
                plugin_path,
                plugin_name,
                failed: false,
            },
            inputs: vec![crate::graph::Port::midi_in("MIDI")],
            outputs: vec![
                crate::graph::Port::audio_out("L", 0),
                crate::graph::Port::audio_out("R", 1),
            ],
            position: [200.0, y],
            latency_samples: 0,
        };
        let inst_id = inst.id;
        // Drop only edges leaving the MIDI source (re-route through the new instrument).
        self.graph.edges.retain(|_, e| e.from_node != midi_src);
        self.graph.add_node(inst);
        let _ = self.graph.connect_midi(midi_src, inst_id);
        let _ = self.graph.connect_stereo(inst_id, gain);

        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.instrument_node = Some(inst_id);
        }
        self.plugin_states.insert(
            instance_id,
            PluginStateBlob {
                instance_id,
                plugin_uid,
                data: Vec::new(),
            },
        );
        self.touch();
        Some((inst_id, unloaded))
    }

    pub fn attach_instrument(
        &mut self,
        track_id: TrackId,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
    ) -> Option<(NodeId, Option<PluginInstanceId>)> {
        self.attach_plugin_instrument(
            track_id,
            "vst3".into(),
            plugin_uid,
            plugin_path,
            plugin_name,
        )
    }

    pub fn add_plugin_effect(
        &mut self,
        plugin_format: String,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
        position: [f32; 2],
    ) -> NodeId {
        // Effects stay floating for now: there is no track insert-slot model yet,
        // and auto-wiring before GainPan vs after GainPan into master is ambiguous
        // without an explicit target. Connect via the graph editor until then.
        let instance_id = PluginInstanceId::new();
        let mut effect = GraphNode::plugin_effect(
            instance_id,
            plugin_format,
            plugin_uid.clone(),
            plugin_path,
            plugin_name,
        );
        effect.position = position;
        let node_id = self.graph.add_node(effect);
        self.plugin_states.insert(
            instance_id,
            PluginStateBlob {
                instance_id,
                plugin_uid,
                data: Vec::new(),
            },
        );
        self.touch();
        node_id
    }

    pub fn add_effect(
        &mut self,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
        position: [f32; 2],
    ) -> NodeId {
        self.add_plugin_effect(
            "vst3".into(),
            plugin_uid,
            plugin_path,
            plugin_name,
            position,
        )
    }

    pub fn compiled_plan(&self) -> CompiledPlan {
        match CompiledPlan::compile(&self.graph) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::warn!("graph compile failed, using empty plan: {e}");
                CompiledPlan::empty()
            }
        }
    }

    pub fn try_compiled_plan(&self) -> Result<CompiledPlan, crate::graph::GraphError> {
        CompiledPlan::compile(&self.graph)
    }

    /// Ensure a live workspace directory exists for asset import / decoding.
    ///
    /// When `root_dir` is unset, creates `dir` (typically a temp workspace).
    /// When set to a different path, migrates asset files into `dir`.
    pub fn ensure_workspace(&mut self, dir: &Path) -> Result<(), ProjectError> {
        std::fs::create_dir_all(dir)?;
        std::fs::create_dir_all(dir.join("assets"))?;
        let old_root = self.root_dir.clone();
        if old_root.as_deref() != Some(dir) {
            self.copy_asset_files_into(dir, old_root.as_deref())?;
        }
        self.root_dir = Some(dir.to_path_buf());
        Ok(())
    }

    /// Atomically pack the current workspace into a `.ctgdaw` archive.
    pub fn save_to_archive(&mut self, archive_path: &Path) -> Result<(), ProjectError> {
        let workspace = self.root_dir.clone().ok_or_else(|| {
            ProjectError::Invalid("project has no workspace; save requires a root".into())
        })?;
        self.meta.version = PROJECT_VERSION;
        self.touch();
        // Keep a readable manifest in the workspace for debugging / tooling.
        let manifest = workspace.join(crate::archive::MANIFEST_NAME);
        let tmp_manifest = workspace.join("project.json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_manifest, &json)?;
        std::fs::rename(&tmp_manifest, &manifest)?;
        crate::archive::pack_workspace(self, &workspace, archive_path)
    }

    /// Extract a `.ctgdaw` archive into `workspace` and return the project.
    pub fn load_from_archive(archive_path: &Path, workspace: &Path) -> Result<Self, ProjectError> {
        crate::archive::unpack_archive(archive_path, workspace)
    }

    /// Legacy directory project write (tests / migration helpers).
    ///
    /// User-facing saves should use [`Self::save_to_archive`].
    pub fn save_to_dir(&mut self, dir: &Path) -> Result<(), ProjectError> {
        self.ensure_workspace(dir)?;
        self.meta.version = PROJECT_VERSION;
        self.touch();
        let path = dir.join(crate::archive::MANIFEST_NAME);
        let tmp = dir.join("project.json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Load a legacy directory project (`project.json` + `assets/`).
    pub fn load_from_dir(dir: &Path) -> Result<Self, ProjectError> {
        let path = dir.join(crate::archive::MANIFEST_NAME);
        let data = std::fs::read_to_string(&path)?;
        let mut project: Project = serde_json::from_str(&data)?;
        if project.meta.version > PROJECT_VERSION {
            return Err(ProjectError::UnsupportedVersion(project.meta.version));
        }
        if project.meta.version < PROJECT_VERSION {
            project.meta.version = PROJECT_VERSION;
        }
        project.root_dir = Some(dir.to_path_buf());
        for asset in project.assets.values_mut() {
            crate::archive::validate_archive_path(&asset.relative_path)?;
            let full = dir.join(&asset.relative_path);
            asset.missing = !full.exists();
        }
        Ok(project)
    }

    /// Import a legacy folder project into a fresh workspace for conversion to `.ctgdaw`.
    pub fn load_legacy_into_workspace(
        src_dir: &Path,
        workspace: &Path,
    ) -> Result<Self, ProjectError> {
        crate::archive::copy_legacy_project_into(src_dir, workspace)
    }

    /// Copy registered asset files into `dest_root`, reading from `src_root` when set.
    fn copy_asset_files_into(
        &self,
        dest_root: &Path,
        src_root: Option<&Path>,
    ) -> Result<(), ProjectError> {
        for asset in self.assets.values() {
            crate::archive::validate_archive_path(&asset.relative_path)?;
            let dest = crate::archive::safe_join(dest_root, &asset.relative_path)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if dest.exists() {
                continue;
            }
            let Some(src_root) = src_root else {
                continue;
            };
            let src = src_root.join(&asset.relative_path);
            if src.exists() && src != dest {
                std::fs::copy(&src, &dest)?;
            }
        }
        Ok(())
    }

    /// Write an atomic `.ctgdaw` autosave snapshot under `autosave_dir`.
    pub fn atomic_autosave(&self, autosave_dir: &Path) -> Result<PathBuf, ProjectError> {
        let Some(workspace) = self.root_dir.as_deref() else {
            // Nothing on disk yet — skip quietly by returning the autosave dir.
            std::fs::create_dir_all(autosave_dir)?;
            return Ok(autosave_dir.to_path_buf());
        };
        std::fs::create_dir_all(autosave_dir)?;
        let stamp = self.meta.modified_unix_ms;
        let path = autosave_dir.join(format!(
            "autosave-{stamp}.{}",
            crate::archive::ARCHIVE_EXTENSION
        ));
        crate::archive::pack_workspace(self, workspace, &path)?;
        Ok(path)
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let mut p = Project::new("Test");
        p.add_midi_track("Synth");
        p.add_audio_track("Drums");
        p.save_to_dir(dir.path()).unwrap();
        let loaded = Project::load_from_dir(dir.path()).unwrap();
        assert_eq!(loaded.tracks.len(), 2);
        assert_eq!(loaded.meta.name, "Test");
        assert!(!loaded.graph.nodes.is_empty());
    }

    #[test]
    fn archive_save_load_roundtrip() {
        let workspace = tempdir().unwrap();
        let mut p = Project::new("ArchiveTest");
        p.add_midi_track("Synth");
        p.add_audio_track("Drums");
        p.ensure_workspace(workspace.path()).unwrap();
        let out = tempdir().unwrap();
        let archive = out.path().join("song.ctgdaw");
        p.save_to_archive(&archive).unwrap();

        let extract = tempdir().unwrap();
        let loaded = Project::load_from_archive(&archive, extract.path()).unwrap();
        assert_eq!(loaded.tracks.len(), 2);
        assert_eq!(loaded.meta.name, "ArchiveTest");
        assert_eq!(loaded.root_dir.as_deref(), Some(extract.path()));
    }

    #[test]
    fn suggested_loop_end_is_half_bar_after_last_note() {
        let mut project = Project::new("Loop");
        let track = project.add_midi_track("Synth");
        let mut clip = Clip::new_midi(track, "Clip", 0.0, 16.0);
        clip.notes_mut()
            .unwrap()
            .push(crate::clips::MidiNote::new(60, 100, 5.0, 1.0));
        project.clips.push(clip);

        assert_eq!(project.suggested_loop_end_beats(), 8.0);
    }

    #[test]
    fn adding_effect_creates_stereo_node_and_plugin_state() {
        let mut project = Project::new("Effects");
        let node_id = project.add_effect(
            "test.effect".into(),
            "/tmp/test.vst3".into(),
            "Test Effect".into(),
            [120.0, 80.0],
        );

        let node = &project.graph.nodes[&node_id];
        assert_eq!(node.position, [120.0, 80.0]);
        assert_eq!(node.inputs.len(), 2);
        assert_eq!(node.outputs.len(), 2);
        let NodeKind::PluginEffect { instance_id, .. } = &node.kind else {
            panic!("expected a plugin effect node");
        };
        assert!(project.plugin_states.contains_key(instance_id));
    }

    #[test]
    fn autosave_writes_ctgdaw_with_assets() {
        let workspace = tempdir().unwrap();
        let mut p = Project::new("AutosaveAssets");
        p.ensure_workspace(workspace.path()).unwrap();
        let wav = workspace.path().join("assets/beep.wav");
        std::fs::write(&wav, b"RIFF....WAVEfmt ").unwrap();
        let id = crate::ids::AssetId::new();
        p.assets.insert(
            id,
            Asset {
                id,
                name: "beep".into(),
                relative_path: PathBuf::from("assets/beep.wav"),
                kind: AssetKind::Audio,
                sample_rate: 48000,
                channels: 2,
                length_samples: 100,
                missing: false,
            },
        );
        p.touch();
        let auto = tempdir().unwrap();
        let saved = p.atomic_autosave(auto.path()).unwrap();
        assert!(
            saved.extension().and_then(|e| e.to_str()) == Some(crate::archive::ARCHIVE_EXTENSION)
        );
        let extract = tempdir().unwrap();
        let loaded = Project::load_from_archive(&saved, extract.path()).unwrap();
        assert!(extract.path().join("assets/beep.wav").exists());
        assert_eq!(loaded.meta.name, "AutosaveAssets");
    }

    #[test]
    fn save_to_new_workspace_migrates_asset_files() {
        let old = tempdir().unwrap();
        let mut p = Project::new("Migrate");
        p.ensure_workspace(old.path()).unwrap();
        let wav = old.path().join("assets/tone.wav");
        std::fs::write(&wav, b"RIFF....WAVEfmt ").unwrap();
        let id = crate::ids::AssetId::new();
        p.assets.insert(
            id,
            Asset {
                id,
                name: "tone".into(),
                relative_path: PathBuf::from("assets/tone.wav"),
                kind: AssetKind::Audio,
                sample_rate: 48000,
                channels: 2,
                length_samples: 100,
                missing: false,
            },
        );
        let new_dir = tempdir().unwrap();
        p.ensure_workspace(new_dir.path()).unwrap();
        let migrated = new_dir.path().join("assets/tone.wav");
        assert!(migrated.exists());
        assert_eq!(p.root_dir.as_deref(), Some(new_dir.path()));
    }

    #[test]
    fn legacy_folder_converts_via_archive_save() {
        let legacy = tempdir().unwrap();
        let mut p = Project::new("Convert");
        p.save_to_dir(legacy.path()).unwrap();
        let wav = legacy.path().join("assets/hit.wav");
        std::fs::write(&wav, b"hit-bytes").unwrap();
        let id = crate::ids::AssetId::new();
        p.assets.insert(
            id,
            Asset {
                id,
                name: "hit".into(),
                relative_path: PathBuf::from("assets/hit.wav"),
                kind: AssetKind::Audio,
                sample_rate: 48000,
                channels: 1,
                length_samples: 10,
                missing: false,
            },
        );
        p.save_to_dir(legacy.path()).unwrap();

        let workspace = tempdir().unwrap();
        let mut loaded =
            Project::load_legacy_into_workspace(legacy.path(), workspace.path()).unwrap();
        let out = tempdir().unwrap();
        let archive = out.path().join("converted.ctgdaw");
        loaded.save_to_archive(&archive).unwrap();

        let extract = tempdir().unwrap();
        let roundtrip = Project::load_from_archive(&archive, extract.path()).unwrap();
        assert_eq!(
            std::fs::read(extract.path().join("assets/hit.wav")).unwrap(),
            b"hit-bytes"
        );
        assert_eq!(roundtrip.meta.name, "Convert");
    }

    #[test]
    fn attach_instrument_tears_down_previous() {
        let mut project = Project::new("Orphan");
        let track = project.add_midi_track("Synth");
        let (first_id, unloaded) = project
            .attach_instrument(track, "a.uid".into(), "/tmp/a.vst3".into(), "A".into())
            .expect("first attach");
        assert!(unloaded.is_none());
        let first_instance = match &project.graph.nodes[&first_id].kind {
            NodeKind::PluginInstrument { instance_id, .. } => *instance_id,
            other => panic!("expected instrument, got {other:?}"),
        };
        assert!(project.plugin_states.contains_key(&first_instance));

        let (second_id, unloaded) = project
            .attach_instrument(track, "b.uid".into(), "/tmp/b.vst3".into(), "B".into())
            .expect("second attach");
        assert_eq!(unloaded, Some(first_instance));
        assert!(!project.graph.nodes.contains_key(&first_id));
        assert!(project.graph.nodes.contains_key(&second_id));
        assert!(!project.plugin_states.contains_key(&first_instance));
        let track_ref = project.tracks.iter().find(|t| t.id == track).unwrap();
        assert_eq!(track_ref.instrument_node, Some(second_id));
    }

    #[test]
    fn version_one_vst3_node_migrates_to_generic_plugin_node() {
        let mut project = Project::new("Migration");
        let track = project.add_midi_track("Synth");
        let (node_id, _) = project
            .attach_instrument(
                track,
                "legacy.uid".into(),
                "/tmp/legacy.vst3".into(),
                "Legacy".into(),
            )
            .unwrap();
        let node = &project.graph.nodes[&node_id];
        let legacy_json = serde_json::to_string(node)
            .unwrap()
            .replace("\"PluginInstrument\"", "\"Vst3Instrument\"")
            .replace("\"plugin_format\":\"vst3\",", "");

        let migrated: GraphNode = serde_json::from_str(&legacy_json).unwrap();
        let NodeKind::PluginInstrument { plugin_format, .. } = migrated.kind else {
            panic!("expected migrated plugin instrument");
        };
        assert_eq!(plugin_format, "vst3");
    }
}
