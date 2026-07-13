//! Reversible command stack for undo/redo.

use crate::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use crate::clips::{Clip, MidiNote, Track};
use crate::graph::{GraphEdge, GraphNode};
use crate::ids::{AutomationLaneId, ClipId, NodeId, NoteId, TrackId};
use crate::project::{PluginStateBlob, Project};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    AddTrack {
        track: Track,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    },
    RemoveTrack {
        track: Track,
        clips: Vec<Clip>,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    },
    AddClip {
        clip: Clip,
    },
    RemoveClip {
        clip: Clip,
    },
    MoveClip {
        clip_id: ClipId,
        old_start: f64,
        new_start: f64,
        old_length: f64,
        new_length: f64,
    },
    /// Move a clip onto a different track.
    SetClipTrack {
        clip_id: ClipId,
        old_track: TrackId,
        new_track: TrackId,
    },
    AddNote {
        clip_id: ClipId,
        note: MidiNote,
    },
    /// Add a group of notes as one undoable operation (for chord stamps).
    AddNotes {
        clip_id: ClipId,
        notes: Vec<MidiNote>,
    },
    RemoveNote {
        clip_id: ClipId,
        note: MidiNote,
    },
    /// Remove a group of notes as one undoable operation.
    RemoveNotes {
        clip_id: ClipId,
        notes: Vec<MidiNote>,
    },
    EditNote {
        clip_id: ClipId,
        note_id: NoteId,
        before: MidiNote,
        after: MidiNote,
    },
    /// Edit a group of notes as one undoable operation (multi-note drag).
    EditNotes {
        clip_id: ClipId,
        before: Vec<MidiNote>,
        after: Vec<MidiNote>,
    },
    SetGainPan {
        node_id: NodeId,
        old_gain: f32,
        new_gain: f32,
        old_pan: f32,
        new_pan: f32,
        old_mute: bool,
        new_mute: bool,
    },
    Connect {
        edge: GraphEdge,
    },
    Disconnect {
        edge: GraphEdge,
    },
    /// Connect that replaced existing wires into the destination port.
    ConnectReplace {
        edge: GraphEdge,
        replaced: Vec<GraphEdge>,
    },
    AddNode {
        node: GraphNode,
    },
    RemoveNode {
        node: GraphNode,
        edges: Vec<GraphEdge>,
        /// If this node was a track instrument, restore the track link on undo.
        instrument_track: Option<TrackId>,
        plugin_state: Option<PluginStateBlob>,
    },
    SetTempo {
        old_bpm: f64,
        new_bpm: f64,
    },
    SetTimeSignature {
        old_beats_per_bar: u32,
        new_beats_per_bar: u32,
        old_beat_unit: u32,
        new_beat_unit: u32,
    },
    SetAutomationPoint {
        lane_id: AutomationLaneId,
        target: AutomationTarget,
        created_lane: bool,
        old_point: Option<AutomationPoint>,
        new_point: AutomationPoint,
    },
}

pub struct CommandStack {
    undo: Vec<Command>,
    redo: Vec<Command>,
    pub max_depth: usize,
}

impl Default for CommandStack {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_depth: 256,
        }
    }
}

impl CommandStack {
    pub fn push(&mut self, project: &mut Project, cmd: Command) {
        apply(project, &cmd, false);
        self.record(cmd);
        project.touch();
    }

    /// Record a command that was already applied to the project (for undo).
    pub fn record(&mut self, cmd: Command) {
        self.undo.push(cmd);
        if self.undo.len() > self.max_depth {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        let Some(cmd) = self.undo.pop() else {
            return false;
        };
        apply(project, &cmd, true);
        self.redo.push(cmd);
        project.touch();
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(cmd) = self.redo.pop() else {
            return false;
        };
        apply(project, &cmd, false);
        self.undo.push(cmd);
        project.touch();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

fn apply(project: &mut Project, cmd: &Command, reverse: bool) {
    match cmd {
        Command::AddTrack {
            track,
            nodes,
            edges,
        } => {
            if reverse {
                project.tracks.retain(|t| t.id != track.id);
                for n in nodes {
                    project.graph.remove_node(n.id);
                }
            } else {
                for n in nodes {
                    project.graph.add_node(n.clone());
                }
                for e in edges {
                    if let Err(err) =
                        project
                            .graph
                            .connect(e.from_node, e.from_port, e.to_node, e.to_port)
                    {
                        tracing::warn!("undo/redo reconnect failed: {err}");
                    }
                }
                if !project.tracks.iter().any(|t| t.id == track.id) {
                    project.tracks.push(track.clone());
                }
            }
        }
        Command::RemoveTrack {
            track,
            clips,
            nodes,
            edges,
        } => {
            if reverse {
                for n in nodes {
                    project.graph.add_node(n.clone());
                }
                for e in edges {
                    if let Err(err) =
                        project
                            .graph
                            .connect(e.from_node, e.from_port, e.to_node, e.to_port)
                    {
                        tracing::warn!("undo/redo reconnect failed: {err}");
                    }
                }
                project.tracks.push(track.clone());
                project.clips.extend(clips.iter().cloned());
            } else {
                project.tracks.retain(|t| t.id != track.id);
                project.clips.retain(|c| c.track_id != track.id);
                for n in nodes {
                    project.graph.remove_node(n.id);
                }
            }
        }
        Command::AddClip { clip } => {
            if reverse {
                project.clips.retain(|c| c.id != clip.id);
            } else if !project.clips.iter().any(|c| c.id == clip.id) {
                project.clips.push(clip.clone());
            }
        }
        Command::RemoveClip { clip } => {
            if reverse {
                project.clips.push(clip.clone());
            } else {
                project.clips.retain(|c| c.id != clip.id);
            }
        }
        Command::MoveClip {
            clip_id,
            old_start,
            new_start,
            old_length,
            new_length,
        } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if reverse {
                    clip.start_beats = *old_start;
                    clip.length_beats = *old_length;
                } else {
                    clip.start_beats = *new_start;
                    clip.length_beats = *new_length;
                }
            }
        }
        Command::SetClipTrack {
            clip_id,
            old_track,
            new_track,
        } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                clip.track_id = if reverse { *old_track } else { *new_track };
            }
        }
        Command::AddNote { clip_id, note } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if let Some(notes) = clip.notes_mut() {
                    if reverse {
                        notes.retain(|n| n.id != note.id);
                    } else if !notes.iter().any(|n| n.id == note.id) {
                        notes.push(note.clone());
                    }
                }
            }
        }
        Command::AddNotes {
            clip_id,
            notes: added,
        } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if let Some(notes) = clip.notes_mut() {
                    if reverse {
                        notes.retain(|note| !added.iter().any(|added| added.id == note.id));
                    } else {
                        for note in added {
                            if !notes.iter().any(|existing| existing.id == note.id) {
                                notes.push(note.clone());
                            }
                        }
                    }
                }
            }
        }
        Command::RemoveNote { clip_id, note } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if let Some(notes) = clip.notes_mut() {
                    if reverse {
                        notes.push(note.clone());
                    } else {
                        notes.retain(|n| n.id != note.id);
                    }
                }
            }
        }
        Command::RemoveNotes {
            clip_id,
            notes: removed,
        } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if let Some(notes) = clip.notes_mut() {
                    if reverse {
                        for note in removed {
                            if !notes.iter().any(|existing| existing.id == note.id) {
                                notes.push(note.clone());
                            }
                        }
                    } else {
                        notes.retain(|note| !removed.iter().any(|removed| removed.id == note.id));
                    }
                }
            }
        }
        Command::EditNote {
            clip_id,
            note_id,
            before,
            after,
        } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if let Some(notes) = clip.notes_mut() {
                    if let Some(n) = notes.iter_mut().find(|n| n.id == *note_id) {
                        *n = if reverse {
                            before.clone()
                        } else {
                            after.clone()
                        };
                    }
                }
            }
        }
        Command::EditNotes {
            clip_id,
            before,
            after,
        } => {
            if let Some(clip) = project.clips.iter_mut().find(|c| c.id == *clip_id) {
                if let Some(notes) = clip.notes_mut() {
                    let source = if reverse { before } else { after };
                    for edited in source {
                        if let Some(n) = notes.iter_mut().find(|n| n.id == edited.id) {
                            *n = edited.clone();
                        }
                    }
                }
            }
        }
        Command::SetGainPan {
            node_id,
            old_gain,
            new_gain,
            old_pan,
            new_pan,
            old_mute,
            new_mute,
        } => {
            if let Some(node) = project.graph.nodes.get_mut(node_id) {
                if let crate::graph::NodeKind::GainPan {
                    gain_db, pan, mute, ..
                } = &mut node.kind
                {
                    if reverse {
                        *gain_db = *old_gain;
                        *pan = *old_pan;
                        *mute = *old_mute;
                    } else {
                        *gain_db = *new_gain;
                        *pan = *new_pan;
                        *mute = *new_mute;
                    }
                }
            }
        }
        Command::Connect { edge } => {
            if reverse {
                project.graph.disconnect(edge.id);
            } else if let Err(err) =
                project
                    .graph
                    .connect(edge.from_node, edge.from_port, edge.to_node, edge.to_port)
            {
                tracing::warn!("undo/redo connect failed: {err}");
            }
        }
        Command::Disconnect { edge } => {
            if reverse {
                if let Err(err) = project.graph.connect(
                    edge.from_node,
                    edge.from_port,
                    edge.to_node,
                    edge.to_port,
                ) {
                    tracing::warn!("undo/redo reconnect failed: {err}");
                }
            } else {
                project.graph.disconnect(edge.id);
            }
        }
        Command::ConnectReplace { edge, replaced } => {
            if reverse {
                project.graph.disconnect(edge.id);
                for e in replaced {
                    if let Err(err) =
                        project
                            .graph
                            .connect(e.from_node, e.from_port, e.to_node, e.to_port)
                    {
                        tracing::warn!("undo/redo reconnect failed: {err}");
                    }
                }
            } else {
                project
                    .graph
                    .disconnect_inputs_to(edge.to_node, edge.to_port);
                if let Err(err) = project.graph.connect(
                    edge.from_node,
                    edge.from_port,
                    edge.to_node,
                    edge.to_port,
                ) {
                    tracing::warn!("undo/redo connect-replace failed: {err}");
                }
            }
        }
        Command::AddNode { node } => {
            if reverse {
                project.graph.remove_node(node.id);
            } else {
                project.graph.add_node(node.clone());
            }
        }
        Command::RemoveNode {
            node,
            edges,
            instrument_track,
            plugin_state,
        } => {
            if reverse {
                project.graph.add_node(node.clone());
                for e in edges {
                    if let Err(err) =
                        project
                            .graph
                            .connect(e.from_node, e.from_port, e.to_node, e.to_port)
                    {
                        tracing::warn!("undo/redo reconnect failed: {err}");
                    }
                }
                if let Some(track_id) = instrument_track {
                    if let Some(track) = project.tracks.iter_mut().find(|t| t.id == *track_id) {
                        track.instrument_node = Some(node.id);
                    }
                }
                if let Some(blob) = plugin_state {
                    project.plugin_states.insert(blob.instance_id, blob.clone());
                }
            } else {
                project.graph.remove_node(node.id);
                if let Some(track_id) = instrument_track {
                    if let Some(track) = project.tracks.iter_mut().find(|t| t.id == *track_id) {
                        if track.instrument_node == Some(node.id) {
                            track.instrument_node = None;
                        }
                    }
                }
                if let Some(blob) = plugin_state {
                    project.plugin_states.shift_remove(&blob.instance_id);
                }
            }
        }
        Command::SetTempo { old_bpm, new_bpm } => {
            project.tempo.bpm = if reverse { *old_bpm } else { *new_bpm };
        }
        Command::SetTimeSignature {
            old_beats_per_bar,
            new_beats_per_bar,
            old_beat_unit,
            new_beat_unit,
        } => {
            if reverse {
                project.tempo.beats_per_bar = (*old_beats_per_bar).max(1);
                project.tempo.beat_unit = (*old_beat_unit).max(1);
            } else {
                project.tempo.beats_per_bar = (*new_beats_per_bar).max(1);
                project.tempo.beat_unit = (*new_beat_unit).max(1);
            }
        }
        Command::SetAutomationPoint {
            lane_id,
            target,
            created_lane,
            old_point,
            new_point,
        } => {
            if reverse {
                if *created_lane {
                    project.automation.retain(|l| l.id != *lane_id);
                } else if let Some(lane) = project.automation.iter_mut().find(|l| l.id == *lane_id)
                {
                    lane.remove_point_at(new_point.beat);
                    if let Some(old) = old_point {
                        lane.add_point(old.beat, old.value);
                    }
                }
            } else {
                let lane =
                    if let Some(lane) = project.automation.iter_mut().find(|l| l.id == *lane_id) {
                        lane
                    } else {
                        project.automation.push(AutomationLane {
                            id: *lane_id,
                            target: target.clone(),
                            points: Vec::new(),
                            enabled: true,
                        });
                        project.automation.last_mut().unwrap()
                    };
                if let Some(old) = old_point {
                    lane.remove_point_at(old.beat);
                }
                lane.add_point(new_point.beat, new_point.value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clips::Clip;

    #[test]
    fn undo_redo_clip() {
        let mut project = Project::new("t");
        let track = project.add_midi_track("A");
        let mut stack = CommandStack::default();
        let clip = Clip::new_midi(track, "C", 0.0, 4.0);
        stack.push(&mut project, Command::AddClip { clip: clip.clone() });
        assert_eq!(project.clips.len(), 1);
        assert!(stack.undo(&mut project));
        assert!(project.clips.is_empty());
        assert!(stack.redo(&mut project));
        assert_eq!(project.clips.len(), 1);
    }

    #[test]
    fn chord_notes_undo_as_one_command() {
        let mut project = Project::new("t");
        let track = project.add_midi_track("A");
        let clip = Clip::new_midi(track, "C", 0.0, 4.0);
        let clip_id = clip.id;
        project.clips.push(clip);
        let notes = [60, 64, 67]
            .map(|pitch| MidiNote::new(pitch, 100, 0.0, 1.0))
            .to_vec();
        let mut stack = CommandStack::default();

        stack.push(
            &mut project,
            Command::AddNotes {
                clip_id,
                notes: notes.clone(),
            },
        );
        assert_eq!(project.clips[0].notes().unwrap().len(), 3);
        assert!(stack.undo(&mut project));
        assert!(project.clips[0].notes().unwrap().is_empty());
        assert!(stack.redo(&mut project));
        assert_eq!(project.clips[0].notes().unwrap().len(), 3);
    }

    #[test]
    fn remove_notes_undo_as_one_command() {
        let mut project = Project::new("t");
        let track = project.add_midi_track("A");
        let clip = Clip::new_midi(track, "C", 0.0, 4.0);
        let clip_id = clip.id;
        project.clips.push(clip);
        let notes = [60, 64, 67]
            .map(|pitch| MidiNote::new(pitch, 100, 0.0, 1.0))
            .to_vec();
        let mut stack = CommandStack::default();
        stack.push(
            &mut project,
            Command::AddNotes {
                clip_id,
                notes: notes.clone(),
            },
        );
        stack.push(
            &mut project,
            Command::RemoveNotes {
                clip_id,
                notes: notes.clone(),
            },
        );
        assert!(project.clips[0].notes().unwrap().is_empty());
        assert!(stack.undo(&mut project));
        assert_eq!(project.clips[0].notes().unwrap().len(), 3);
        assert!(stack.redo(&mut project));
        assert!(project.clips[0].notes().unwrap().is_empty());
    }

    #[test]
    fn edit_notes_undo_as_one_command() {
        let mut project = Project::new("t");
        let track = project.add_midi_track("A");
        let clip = Clip::new_midi(track, "C", 0.0, 4.0);
        let clip_id = clip.id;
        project.clips.push(clip);
        let before = [60, 64]
            .map(|pitch| MidiNote::new(pitch, 100, 0.0, 1.0))
            .to_vec();
        let after: Vec<_> = before
            .iter()
            .map(|n| MidiNote {
                pitch: n.pitch + 2,
                start_beats: n.start_beats + 1.0,
                ..n.clone()
            })
            .collect();
        let mut stack = CommandStack::default();
        stack.push(
            &mut project,
            Command::AddNotes {
                clip_id,
                notes: before.clone(),
            },
        );
        stack.push(
            &mut project,
            Command::EditNotes {
                clip_id,
                before: before.clone(),
                after: after.clone(),
            },
        );
        let notes = project.clips[0].notes().unwrap();
        assert_eq!(notes[0].pitch, 62);
        assert!((notes[0].start_beats - 1.0).abs() < 1e-9);
        assert!(stack.undo(&mut project));
        let notes = project.clips[0].notes().unwrap();
        assert_eq!(notes[0].pitch, 60);
        assert!(notes[0].start_beats.abs() < 1e-9);
    }

    #[test]
    fn set_clip_track_moves_and_undoes() {
        let mut project = Project::new("t");
        let a = project.add_midi_track("A");
        let b = project.add_midi_track("B");
        let clip = Clip::new_midi(a, "C", 0.0, 4.0);
        let clip_id = clip.id;
        let mut stack = CommandStack::default();
        stack.push(&mut project, Command::AddClip { clip });

        stack.push(
            &mut project,
            Command::SetClipTrack {
                clip_id,
                old_track: a,
                new_track: b,
            },
        );
        assert_eq!(
            project
                .clips
                .iter()
                .find(|c| c.id == clip_id)
                .unwrap()
                .track_id,
            b
        );
        assert!(stack.undo(&mut project));
        assert_eq!(
            project
                .clips
                .iter()
                .find(|c| c.id == clip_id)
                .unwrap()
                .track_id,
            a
        );
        assert!(stack.redo(&mut project));
        assert_eq!(
            project
                .clips
                .iter()
                .find(|c| c.id == clip_id)
                .unwrap()
                .track_id,
            b
        );
    }

    #[test]
    fn remove_track_takes_clips_and_nodes_and_undoes() {
        let mut project = Project::new("t");
        let track_id = project.add_midi_track("Synth");
        let track = project
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .unwrap()
            .clone();
        let clip = Clip::new_midi(track_id, "C", 0.0, 4.0);
        project.clips.push(clip);

        let node_ids: Vec<_> = [
            track.midi_source_node,
            track.gain_node,
            track.instrument_node,
            track.audio_source_node,
        ]
        .into_iter()
        .flatten()
        .collect();
        let nodes: Vec<_> = node_ids
            .iter()
            .filter_map(|nid| project.graph.nodes.get(nid).cloned())
            .collect();
        let edges: Vec<_> = project
            .graph
            .edges
            .values()
            .filter(|e| node_ids.contains(&e.from_node) || node_ids.contains(&e.to_node))
            .cloned()
            .collect();
        let clips: Vec<_> = project
            .clips
            .iter()
            .filter(|c| c.track_id == track_id)
            .cloned()
            .collect();

        let mut stack = CommandStack::default();
        stack.push(
            &mut project,
            Command::RemoveTrack {
                track,
                clips,
                nodes,
                edges,
            },
        );
        assert!(!project.tracks.iter().any(|t| t.id == track_id));
        assert!(!project.clips.iter().any(|c| c.track_id == track_id));
        for nid in &node_ids {
            assert!(!project.graph.nodes.contains_key(nid));
        }

        assert!(stack.undo(&mut project));
        assert!(project.tracks.iter().any(|t| t.id == track_id));
        assert_eq!(
            project
                .clips
                .iter()
                .filter(|c| c.track_id == track_id)
                .count(),
            1
        );
        for nid in &node_ids {
            assert!(project.graph.nodes.contains_key(nid));
        }
    }

    #[test]
    fn undo_redo_add_node() {
        let mut project = Project::new("t");
        let mut stack = CommandStack::default();
        let mut node = crate::graph::GraphNode::stereo_gain_pan("Gain");
        node.position = [10.0, 20.0];
        let id = node.id;
        stack.push(&mut project, Command::AddNode { node });
        assert!(project.graph.nodes.contains_key(&id));
        assert!(stack.undo(&mut project));
        assert!(!project.graph.nodes.contains_key(&id));
        assert!(stack.redo(&mut project));
        assert!(project.graph.nodes.contains_key(&id));
    }

    #[test]
    fn record_add_track_undo_removes_track() {
        let mut project = Project::new("t");
        let id = project.add_midi_track("MIDI 2");
        let track = project.tracks.iter().find(|t| t.id == id).unwrap().clone();
        let node_ids: Vec<_> = [
            track.midi_source_node,
            track.gain_node,
            track.instrument_node,
            track.audio_source_node,
        ]
        .into_iter()
        .flatten()
        .collect();
        let nodes: Vec<_> = node_ids
            .iter()
            .filter_map(|nid| project.graph.nodes.get(nid).cloned())
            .collect();
        let edges: Vec<_> = project
            .graph
            .edges
            .values()
            .filter(|e| node_ids.contains(&e.from_node) || node_ids.contains(&e.to_node))
            .cloned()
            .collect();
        let mut stack = CommandStack::default();
        stack.record(Command::AddTrack {
            track,
            nodes,
            edges,
        });
        assert!(project.tracks.iter().any(|t| t.id == id));
        assert!(stack.undo(&mut project));
        assert!(!project.tracks.iter().any(|t| t.id == id));
        for nid in &node_ids {
            assert!(!project.graph.nodes.contains_key(nid));
        }
    }

    #[test]
    fn undo_remove_node_restores_instrument_track_link() {
        let mut project = Project::new("t");
        let track = project.add_midi_track("Synth");
        let (node_id, _) = project
            .attach_instrument(track, "uid".into(), "/tmp/x.vst3".into(), "X".into())
            .unwrap();
        let node = project.graph.nodes[&node_id].clone();
        let plugin_state = match &node.kind {
            crate::graph::NodeKind::PluginInstrument { instance_id, .. } => {
                project.plugin_states.get(instance_id).cloned()
            }
            _ => None,
        };
        let edges: Vec<_> = project
            .graph
            .edges
            .values()
            .filter(|e| e.from_node == node_id || e.to_node == node_id)
            .cloned()
            .collect();
        let mut stack = CommandStack::default();
        stack.push(
            &mut project,
            Command::RemoveNode {
                node,
                edges,
                instrument_track: Some(track),
                plugin_state,
            },
        );
        assert!(
            project
                .tracks
                .iter()
                .find(|t| t.id == track)
                .unwrap()
                .instrument_node
                .is_none()
        );
        assert!(stack.undo(&mut project));
        assert_eq!(
            project
                .tracks
                .iter()
                .find(|t| t.id == track)
                .unwrap()
                .instrument_node,
            Some(node_id)
        );
        assert!(project.graph.nodes.contains_key(&node_id));
    }
}
