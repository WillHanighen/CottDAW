//! Typed acyclic audio/MIDI routing graph.

use crate::ids::{EdgeId, NodeId, PluginInstanceId, PortId, TrackId};
use cott_synth_dsp::SynthParams;
use indexmap::IndexMap;
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortType {
    Audio,
    Midi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub id: PortId,
    pub name: String,
    pub port_type: PortType,
    pub is_input: bool,
    pub channel: u32,
}

impl Port {
    pub fn audio_in(name: impl Into<String>, channel: u32) -> Self {
        Self {
            id: PortId::new(),
            name: name.into(),
            port_type: PortType::Audio,
            is_input: true,
            channel,
        }
    }

    pub fn audio_out(name: impl Into<String>, channel: u32) -> Self {
        Self {
            id: PortId::new(),
            name: name.into(),
            port_type: PortType::Audio,
            is_input: false,
            channel,
        }
    }

    pub fn midi_in(name: impl Into<String>) -> Self {
        Self {
            id: PortId::new(),
            name: name.into(),
            port_type: PortType::Midi,
            is_input: true,
            channel: 0,
        }
    }

    pub fn midi_out(name: impl Into<String>) -> Self {
        Self {
            id: PortId::new(),
            name: name.into(),
            port_type: PortType::Midi,
            is_input: false,
            channel: 0,
        }
    }
}

/// Number of stereo input strips on a bus mixer.
pub const MIXER_STRIP_COUNT: usize = 4;

/// Number of MIDI input jacks on a MIDI mixer.
pub const MIDI_MIXER_INPUT_COUNT: usize = 4;

/// One stereo strip on a [`NodeKind::SumMixer`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MixerStrip {
    pub gain_db: f32,
    /// −1 = full left, 0 = center, +1 = full right.
    pub pan: f32,
    pub mute: bool,
}

impl Default for MixerStrip {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
        }
    }
}

impl MixerStrip {
    pub fn clamped(self) -> Self {
        Self {
            gain_db: self.gain_db.clamp(-60.0, 12.0),
            pan: self.pan.clamp(-1.0, 1.0),
            mute: self.mute,
        }
    }
}

/// Stereo A/B branch controls for [`NodeKind::StereoSplitter`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SplitterBranch {
    pub gain_db: f32,
    pub pan: f32,
}

impl Default for SplitterBranch {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
        }
    }
}

impl SplitterBranch {
    pub fn clamped(self) -> Self {
        Self {
            gain_db: self.gain_db.clamp(-60.0, 12.0),
            pan: self.pan.clamp(-1.0, 1.0),
        }
    }
}

fn default_mixer_strips() -> [MixerStrip; MIXER_STRIP_COUNT] {
    [MixerStrip::default(); MIXER_STRIP_COUNT]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    MidiClipSource {
        track_id: TrackId,
    },
    AudioClipSource {
        track_id: TrackId,
    },
    GainPan {
        gain_db: f32,
        pan: f32,
        mute: bool,
        solo: bool,
    },
    /// Bus mixer with [`MIXER_STRIP_COUNT`] stereo inputs (each with gain/pan/mute).
    SumMixer {
        #[serde(default = "default_mixer_strips")]
        strips: [MixerStrip; MIXER_STRIP_COUNT],
        #[serde(default)]
        master_gain_db: f32,
        #[serde(default)]
        master_pan: f32,
        #[serde(default)]
        mute: bool,
    },
    /// Fan-out: one stereo in → two stereo outs (A/B), each with gain/pan.
    StereoSplitter {
        #[serde(default)]
        a: SplitterBranch,
        #[serde(default)]
        b: SplitterBranch,
    },
    /// MIDI merge bus — [`MIDI_MIXER_INPUT_COUNT`] inputs into one output (no processing).
    MidiMixer,
    /// MIDI fan-out — one input to A/B outputs (no processing).
    MidiSplitter,
    MasterOutput,
    /// First-party CottSynth (same DSP as the redistributable VST3).
    BuiltinSynth {
        #[serde(default)]
        params: SynthParams,
    },
    #[serde(alias = "Vst3Instrument")]
    PluginInstrument {
        instance_id: PluginInstanceId,
        #[serde(default = "default_vst3_format")]
        plugin_format: String,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
        failed: bool,
    },
    #[serde(alias = "Vst3Effect")]
    PluginEffect {
        instance_id: PluginInstanceId,
        #[serde(default = "default_vst3_format")]
        plugin_format: String,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
        bypass: bool,
        failed: bool,
    },
}

impl NodeKind {
    /// Master accepts multiple wires on the same input ports (DSP sums).
    pub fn allows_input_fan_in(&self) -> bool {
        matches!(self, Self::MasterOutput)
    }

    /// Pure MIDI routers (no parameters / no editor).
    pub fn is_midi_router(&self) -> bool {
        matches!(self, Self::MidiMixer | Self::MidiSplitter)
    }

    /// Built-in nodes that have an in-app floating editor (no Plugins tab required).
    pub fn has_builtin_editor(&self) -> bool {
        matches!(
            self,
            Self::GainPan { .. }
                | Self::SumMixer { .. }
                | Self::StereoSplitter { .. }
                | Self::BuiltinSynth { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub name: String,
    /// Deserialized with a compat shim so legacy unit `SumMixer` still loads.
    #[serde(deserialize_with = "deserialize_node_kind")]
    pub kind: NodeKind,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    /// UI position in the graph editor.
    pub position: [f32; 2],
    /// Declared processing latency in samples.
    pub latency_samples: u32,
}

fn deserialize_node_kind<'de, D>(deserializer: D) -> Result<NodeKind, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    // Legacy projects stored the mixer as a unit variant: `"SumMixer"`.
    if value.as_str() == Some("SumMixer") {
        return Ok(NodeKind::SumMixer {
            strips: default_mixer_strips(),
            master_gain_db: 0.0,
            master_pan: 0.0,
            mute: false,
        });
    }
    // Intermediate form had bus-level gain/pan before per-strip controls.
    if let Some(obj) = value.as_object()
        && let Some(mixer) = obj.get("SumMixer")
        && mixer.get("strips").is_none()
    {
        let master_gain_db = mixer
            .get("gain_db")
            .or_else(|| mixer.get("master_gain_db"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let master_pan = mixer
            .get("pan")
            .or_else(|| mixer.get("master_pan"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let mute = mixer.get("mute").and_then(|v| v.as_bool()).unwrap_or(false);
        return Ok(NodeKind::SumMixer {
            strips: default_mixer_strips(),
            master_gain_db,
            master_pan,
            mute,
        });
    }
    // Flat a_gain_db / a_pan fields → nested A/B branches.
    if let Some(obj) = value.as_object()
        && let Some(split) = obj.get("StereoSplitter")
        && split.get("a").is_none()
    {
        let a = SplitterBranch {
            gain_db: split
                .get("a_gain_db")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32,
            pan: split.get("a_pan").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        };
        let b = SplitterBranch {
            gain_db: split
                .get("b_gain_db")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32,
            pan: split.get("b_pan").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        };
        return Ok(NodeKind::StereoSplitter { a, b });
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

impl GraphNode {
    pub fn stereo_gain_pan(name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::GainPan {
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
                solo: false,
            },
            inputs: vec![Port::audio_in(stereo_in_name(0), 0), Port::audio_in(stereo_in_name(1), 1)],
            outputs: vec![
                Port::audio_out(stereo_out_name(0), 0),
                Port::audio_out(stereo_out_name(1), 1),
            ],
            position: [0.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn midi_clip_source(track_id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::MidiClipSource { track_id },
            inputs: vec![],
            outputs: vec![Port::midi_out("MIDI Out")],
            position: [0.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn audio_clip_source(track_id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::AudioClipSource { track_id },
            inputs: vec![],
            outputs: vec![
                Port::audio_out(stereo_out_name(0), 0),
                Port::audio_out(stereo_out_name(1), 1),
            ],
            position: [0.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn master_output() -> Self {
        Self {
            id: NodeId::new(),
            name: "Master".into(),
            kind: NodeKind::MasterOutput,
            inputs: vec![Port::audio_in(stereo_in_name(0), 0), Port::audio_in(stereo_in_name(1), 1)],
            outputs: vec![],
            position: [layout::column_x(3), layout::ORIGIN_Y],
            latency_samples: 0,
        }
    }

    pub fn sum_mixer(name: impl Into<String>) -> Self {
        let mut inputs = Vec::with_capacity(MIXER_STRIP_COUNT * 2);
        for i in 0..MIXER_STRIP_COUNT {
            inputs.push(Port::audio_in(stereo_in_branch_name(i, 0), (i * 2) as u32));
            inputs.push(Port::audio_in(stereo_in_branch_name(i, 1), (i * 2 + 1) as u32));
        }
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::SumMixer {
                strips: default_mixer_strips(),
                master_gain_db: 0.0,
                master_pan: 0.0,
                mute: false,
            },
            inputs,
            outputs: vec![
                Port::audio_out(stereo_out_name(0), 0),
                Port::audio_out(stereo_out_name(1), 1),
            ],
            position: [400.0, 100.0],
            latency_samples: 0,
        }
    }

    pub fn stereo_splitter(name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::StereoSplitter {
                a: SplitterBranch::default(),
                b: SplitterBranch::default(),
            },
            inputs: vec![Port::audio_in(stereo_in_name(0), 0), Port::audio_in(stereo_in_name(1), 1)],
            outputs: vec![
                Port::audio_out(stereo_out_branch_name(0, 0), 0),
                Port::audio_out(stereo_out_branch_name(0, 1), 1),
                Port::audio_out(stereo_out_branch_name(1, 0), 2),
                Port::audio_out(stereo_out_branch_name(1, 1), 3),
            ],
            position: [400.0, 200.0],
            latency_samples: 0,
        }
    }

    pub fn midi_mixer(name: impl Into<String>) -> Self {
        let mut inputs = Vec::with_capacity(MIDI_MIXER_INPUT_COUNT);
        for i in 0..MIDI_MIXER_INPUT_COUNT {
            inputs.push(Port::midi_in(format!("MIDI In {}", branch_letter(i))));
        }
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::MidiMixer,
            inputs,
            outputs: vec![Port::midi_out("MIDI Out")],
            position: [300.0, 160.0],
            latency_samples: 0,
        }
    }

    pub fn midi_splitter(name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::MidiSplitter,
            inputs: vec![Port::midi_in("MIDI In")],
            outputs: vec![
                Port::midi_out("MIDI Out A"),
                Port::midi_out("MIDI Out B"),
            ],
            position: [300.0, 240.0],
            latency_samples: 0,
        }
    }

    pub fn builtin_synth(name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::BuiltinSynth {
                params: SynthParams::default(),
            },
            inputs: vec![Port::midi_in("MIDI In")],
            outputs: vec![
                Port::audio_out(stereo_out_name(0), 0),
                Port::audio_out(stereo_out_name(1), 1),
            ],
            position: [200.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn plugin_instrument(
        instance_id: PluginInstanceId,
        plugin_format: String,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
    ) -> Self {
        Self {
            id: NodeId::new(),
            name: plugin_name.clone(),
            kind: NodeKind::PluginInstrument {
                instance_id,
                plugin_format,
                plugin_uid,
                plugin_path,
                plugin_name,
                failed: false,
            },
            inputs: vec![Port::midi_in("MIDI In")],
            outputs: vec![
                Port::audio_out(stereo_out_name(0), 0),
                Port::audio_out(stereo_out_name(1), 1),
            ],
            position: [0.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn plugin_effect(
        instance_id: PluginInstanceId,
        plugin_format: String,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
    ) -> Self {
        Self {
            id: NodeId::new(),
            name: plugin_name.clone(),
            kind: NodeKind::PluginEffect {
                instance_id,
                plugin_format,
                plugin_uid,
                plugin_path,
                plugin_name,
                bypass: false,
                failed: false,
            },
            inputs: vec![Port::audio_in(stereo_in_name(0), 0), Port::audio_in(stereo_in_name(1), 1)],
            outputs: vec![
                Port::audio_out(stereo_out_name(0), 0),
                Port::audio_out(stereo_out_name(1), 1),
            ],
            position: [0.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn vst3_effect(
        instance_id: PluginInstanceId,
        plugin_uid: String,
        plugin_path: String,
        plugin_name: String,
    ) -> Self {
        Self::plugin_effect(
            instance_id,
            default_vst3_format(),
            plugin_uid,
            plugin_path,
            plugin_name,
        )
    }

    pub fn find_port(&self, id: PortId) -> Option<&Port> {
        self.inputs
            .iter()
            .chain(self.outputs.iter())
            .find(|p| p.id == id)
    }

    /// Expand legacy mixer port layouts and normalize port labels after load
    /// (keeps existing port IDs).
    pub fn migrate_ports(&mut self) {
        match &self.kind {
            NodeKind::SumMixer { .. } => {
                if self.inputs.len() < MIXER_STRIP_COUNT * 2 {
                    let mut by_ch: IndexMap<u32, Port> = IndexMap::new();
                    for port in self.inputs.drain(..) {
                        if port.port_type == PortType::Audio {
                            by_ch.insert(port.channel, port);
                        }
                    }
                    let mut inputs = Vec::with_capacity(MIXER_STRIP_COUNT * 2);
                    for i in 0..MIXER_STRIP_COUNT {
                        let l_ch = (i * 2) as u32;
                        let r_ch = (i * 2 + 1) as u32;
                        let mut l = by_ch
                            .shift_remove(&l_ch)
                            .unwrap_or_else(|| Port::audio_in(stereo_in_branch_name(i, 0), l_ch));
                        l.channel = l_ch;
                        l.is_input = true;
                        inputs.push(l);
                        let mut r = by_ch
                            .shift_remove(&r_ch)
                            .unwrap_or_else(|| Port::audio_in(stereo_in_branch_name(i, 1), r_ch));
                        r.channel = r_ch;
                        r.is_input = true;
                        inputs.push(r);
                    }
                    self.inputs = inputs;
                }
                for (i, port) in self.inputs.iter_mut().enumerate() {
                    let strip = i / 2;
                    let ch = (i % 2) as u32;
                    port.name = stereo_in_branch_name(strip, ch);
                }
                for port in &mut self.outputs {
                    port.name = stereo_out_name(port.channel);
                }
            }
            NodeKind::StereoSplitter { .. } => {
                for port in &mut self.inputs {
                    port.name = stereo_in_name(port.channel);
                }
                for (i, port) in self.outputs.iter_mut().enumerate() {
                    port.name = stereo_out_branch_name(i / 2, (i % 2) as u32);
                }
            }
            NodeKind::MidiMixer => {
                for (i, port) in self.inputs.iter_mut().enumerate() {
                    port.name = format!("MIDI In {}", branch_letter(i));
                }
                for port in &mut self.outputs {
                    port.name = "MIDI Out".into();
                }
            }
            NodeKind::MidiSplitter => {
                for port in &mut self.inputs {
                    port.name = "MIDI In".into();
                }
                for (i, port) in self.outputs.iter_mut().enumerate() {
                    port.name = format!("MIDI Out {}", branch_letter(i));
                }
            }
            NodeKind::MidiClipSource { .. } => {
                for port in &mut self.outputs {
                    port.name = "MIDI Out".into();
                }
            }
            NodeKind::AudioClipSource { .. } => {
                for port in &mut self.outputs {
                    port.name = stereo_out_name(port.channel);
                }
            }
            NodeKind::MasterOutput => {
                for port in &mut self.inputs {
                    port.name = stereo_in_name(port.channel);
                }
            }
            NodeKind::GainPan { .. }
            | NodeKind::PluginEffect { .. } => {
                for port in &mut self.inputs {
                    port.name = stereo_in_name(port.channel);
                }
                for port in &mut self.outputs {
                    port.name = stereo_out_name(port.channel);
                }
            }
            NodeKind::BuiltinSynth { .. } | NodeKind::PluginInstrument { .. } => {
                for port in &mut self.inputs {
                    port.name = "MIDI In".into();
                }
                for port in &mut self.outputs {
                    port.name = stereo_out_name(port.channel);
                }
            }
        }
    }
}

fn branch_letter(index: usize) -> char {
    char::from(b'A' + (index as u8))
}

fn stereo_in_name(channel: u32) -> String {
    if channel == 0 {
        "L In".into()
    } else {
        "R In".into()
    }
}

fn stereo_out_name(channel: u32) -> String {
    if channel == 0 {
        "L Out".into()
    } else {
        "R Out".into()
    }
}

fn stereo_in_branch_name(strip: usize, channel: u32) -> String {
    let side = if channel % 2 == 0 { "L" } else { "R" };
    format!("{side} In {}", branch_letter(strip))
}

fn stereo_out_branch_name(branch: usize, channel: u32) -> String {
    let side = if channel % 2 == 0 { "L" } else { "R" };
    format!("{side} Out {}", branch_letter(branch))
}

fn default_vst3_format() -> String {
    "vst3".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: EdgeId,
    pub from_node: NodeId,
    pub from_port: PortId,
    pub to_node: NodeId,
    pub to_port: PortId,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GraphError {
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("port not found")]
    PortNotFound,
    #[error("type mismatch: cannot connect {from:?} to {to:?}")]
    TypeMismatch { from: PortType, to: PortType },
    #[error("direction mismatch: must connect output to input")]
    DirectionMismatch,
    #[error("self-connection is not allowed")]
    SelfConnection,
    #[error("connection would create a feedback loop")]
    Cycle,
    #[error("duplicate edge")]
    DuplicateEdge,
}

/// Geometry of the routing editor's left-to-right layout.
///
/// Shared so that nodes spawned before the editor has ever drawn (new tracks,
/// attached instruments) land on the same grid the auto-arrange uses.
pub mod layout {
    /// Drawn width of a node body in the routing editor.
    pub const NODE_WIDTH: f32 = 176.0;
    pub const ORIGIN_X: f32 = 40.0;
    pub const ORIGIN_Y: f32 = 40.0;
    pub const COL_GAP: f32 = 80.0;
    pub const ROW_GAP: f32 = 36.0;
    pub const MIN_COL_WIDTH: f32 = 140.0;
    /// Vertical pitch between tracks when nothing has measured the nodes yet.
    pub const ROW_PITCH: f32 = 112.0;

    /// Left edge of signal-flow column `col` (0 = sources).
    pub fn column_x(col: usize) -> f32 {
        ORIGIN_X + col as f32 * (NODE_WIDTH + COL_GAP)
    }

    /// Top edge of track row `row`.
    pub fn row_y(row: usize) -> f32 {
        ORIGIN_Y + row as f32 * ROW_PITCH
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioGraph {
    pub nodes: IndexMap<NodeId, GraphNode>,
    pub edges: IndexMap<EdgeId, GraphEdge>,
    /// Set once the user positions a node by hand. Until then the editor is
    /// free to lay the graph out itself when a project is opened.
    #[serde(default)]
    pub user_arranged: bool,
}

impl AudioGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn migrate_nodes(&mut self) {
        for node in self.nodes.values_mut() {
            node.migrate_ports();
        }
    }

    pub fn add_node(&mut self, node: GraphNode) -> NodeId {
        let id = node.id;
        self.nodes.insert(id, node);
        id
    }

    pub fn remove_node(&mut self, id: NodeId) -> Option<GraphNode> {
        let node = self.nodes.shift_remove(&id)?;
        self.edges
            .retain(|_, e| e.from_node != id && e.to_node != id);
        Some(node)
    }

    pub fn connect(
        &mut self,
        from_node: NodeId,
        from_port: PortId,
        to_node: NodeId,
        to_port: PortId,
    ) -> Result<EdgeId, GraphError> {
        if from_node == to_node {
            return Err(GraphError::SelfConnection);
        }
        let from = self
            .nodes
            .get(&from_node)
            .ok_or_else(|| GraphError::NodeNotFound(from_node.to_string()))?;
        let to = self
            .nodes
            .get(&to_node)
            .ok_or_else(|| GraphError::NodeNotFound(to_node.to_string()))?;
        let out = from.find_port(from_port).ok_or(GraphError::PortNotFound)?;
        let inp = to.find_port(to_port).ok_or(GraphError::PortNotFound)?;
        if out.is_input || !inp.is_input {
            return Err(GraphError::DirectionMismatch);
        }
        if out.port_type != inp.port_type {
            return Err(GraphError::TypeMismatch {
                from: out.port_type,
                to: inp.port_type,
            });
        }
        for edge in self.edges.values() {
            if edge.from_node == from_node
                && edge.from_port == from_port
                && edge.to_node == to_node
                && edge.to_port == to_port
            {
                return Err(GraphError::DuplicateEdge);
            }
        }

        let edge = GraphEdge {
            id: EdgeId::new(),
            from_node,
            from_port,
            to_node,
            to_port,
        };
        let id = edge.id;
        self.edges.insert(id, edge);
        if self.has_cycle() {
            self.edges.shift_remove(&id);
            return Err(GraphError::Cycle);
        }
        Ok(id)
    }

    pub fn disconnect(&mut self, id: EdgeId) -> Option<GraphEdge> {
        self.edges.shift_remove(&id)
    }

    /// Remove every edge that lands on the given input port.
    pub fn disconnect_inputs_to(&mut self, to_node: NodeId, to_port: PortId) {
        self.edges
            .retain(|_, edge| !(edge.to_node == to_node && edge.to_port == to_port));
    }

    /// Connect, replacing any existing wires into the destination input port.
    pub fn connect_replace(
        &mut self,
        from_node: NodeId,
        from_port: PortId,
        to_node: NodeId,
        to_port: PortId,
    ) -> Result<EdgeId, GraphError> {
        self.disconnect_inputs_to(to_node, to_port);
        self.connect(from_node, from_port, to_node, to_port)
    }

    pub fn has_cycle(&self) -> bool {
        let (pg, _) = self.to_petgraph();
        is_cyclic_directed(&pg)
    }

    pub fn topological_order(&self) -> Result<Vec<NodeId>, GraphError> {
        let (pg, index_to_id) = self.to_petgraph();
        let order = toposort(&pg, None).map_err(|_| GraphError::Cycle)?;
        Ok(order
            .into_iter()
            .map(|idx| index_to_id[idx.index()])
            .collect())
    }

    fn to_petgraph(&self) -> (DiGraph<(), ()>, Vec<NodeId>) {
        let mut pg = DiGraph::new();
        let mut id_to_index: IndexMap<NodeId, NodeIndex> = IndexMap::new();
        let mut index_to_id = Vec::new();
        for id in self.nodes.keys() {
            let idx = pg.add_node(());
            id_to_index.insert(*id, idx);
            index_to_id.push(*id);
        }
        for edge in self.edges.values() {
            if let (Some(&a), Some(&b)) = (
                id_to_index.get(&edge.from_node),
                id_to_index.get(&edge.to_node),
            ) {
                pg.add_edge(a, b, ());
            }
        }
        (pg, index_to_id)
    }

    /// Propagate latency through the DAG; returns total latency to master.
    pub fn compute_latencies(&mut self) -> u32 {
        let Ok(order) = self.topological_order() else {
            return 0;
        };
        let mut arrival: IndexMap<NodeId, u32> = IndexMap::new();
        let mut max_to_master = 0u32;
        for node_id in order {
            let mut input_latency = 0u32;
            for edge in self.edges.values() {
                if edge.to_node == node_id {
                    let upstream = arrival.get(&edge.from_node).copied().unwrap_or(0);
                    input_latency = input_latency.max(upstream);
                }
            }
            let node_latency = self
                .nodes
                .get(&node_id)
                .map(|n| n.latency_samples)
                .unwrap_or(0);
            let out = input_latency.saturating_add(node_latency);
            arrival.insert(node_id, out);
            if matches!(
                self.nodes.get(&node_id).map(|n| &n.kind),
                Some(NodeKind::MasterOutput)
            ) {
                max_to_master = out;
            }
        }
        max_to_master
    }

    pub fn connect_stereo(
        &mut self,
        from: NodeId,
        to: NodeId,
    ) -> Result<(EdgeId, EdgeId), GraphError> {
        let from_node = self
            .nodes
            .get(&from)
            .ok_or_else(|| GraphError::NodeNotFound(from.to_string()))?;
        let to_node = self
            .nodes
            .get(&to)
            .ok_or_else(|| GraphError::NodeNotFound(to.to_string()))?;
        let from_l = from_node
            .outputs
            .iter()
            .find(|p| p.port_type == PortType::Audio && p.channel == 0)
            .map(|p| p.id)
            .ok_or(GraphError::PortNotFound)?;
        let from_r = from_node
            .outputs
            .iter()
            .find(|p| p.port_type == PortType::Audio && p.channel == 1)
            .map(|p| p.id)
            .ok_or(GraphError::PortNotFound)?;
        let to_l = to_node
            .inputs
            .iter()
            .find(|p| p.port_type == PortType::Audio && p.channel == 0)
            .map(|p| p.id)
            .ok_or(GraphError::PortNotFound)?;
        let to_r = to_node
            .inputs
            .iter()
            .find(|p| p.port_type == PortType::Audio && p.channel == 1)
            .map(|p| p.id)
            .ok_or(GraphError::PortNotFound)?;
        let e0 = self.connect(from, from_l, to, to_l)?;
        let e1 = self.connect(from, from_r, to, to_r)?;
        Ok((e0, e1))
    }

    pub fn connect_midi(&mut self, from: NodeId, to: NodeId) -> Result<EdgeId, GraphError> {
        let from_port = self
            .nodes
            .get(&from)
            .and_then(|n| n.outputs.iter().find(|p| p.port_type == PortType::Midi))
            .map(|p| p.id)
            .ok_or(GraphError::PortNotFound)?;
        let to_port = self
            .nodes
            .get(&to)
            .and_then(|n| n.inputs.iter().find(|p| p.port_type == PortType::Midi))
            .map(|p| p.id)
            .ok_or(GraphError::PortNotFound)?;
        self.connect(from, from_port, to, to_port)
    }

    /// Lay the graph out unless the user has arranged it themselves.
    ///
    /// Called when a project is opened, so a graph nobody has touched always
    /// comes up readable — node sizes change between releases, and stored
    /// positions from an older build would otherwise overlap.
    pub fn auto_arrange_if_untouched(&mut self, size_of: impl Fn(&GraphNode) -> [f32; 2]) -> bool {
        if self.user_arranged {
            return false;
        }
        self.apply_arrangement(size_of)
    }

    /// Move every node onto the computed layout. Returns whether anything moved.
    pub fn apply_arrangement(&mut self, size_of: impl Fn(&GraphNode) -> [f32; 2]) -> bool {
        let laid_out = self.arranged_positions(size_of);
        let mut moved = false;
        for (id, pos) in laid_out {
            if let Some(node) = self.nodes.get_mut(&id)
                && ((node.position[0] - pos[0]).abs() > 0.5
                    || (node.position[1] - pos[1]).abs() > 0.5)
            {
                node.position = pos;
                moved = true;
            }
        }
        moved
    }

    /// Left-to-right layered layout following signal flow.
    ///
    /// Sources sit in the leftmost column; the master output is always on the
    /// right. Nodes in later columns align vertically with their upstream
    /// connections when there is room.
    pub fn arranged_positions(
        &self,
        size_of: impl Fn(&GraphNode) -> [f32; 2],
    ) -> IndexMap<NodeId, [f32; 2]> {
        use layout::{COL_GAP, MIN_COL_WIDTH, ORIGIN_X, ORIGIN_Y, ROW_GAP};

        let mut result = IndexMap::new();
        if self.nodes.is_empty() {
            return result;
        }

        let order = self
            .topological_order()
            .unwrap_or_else(|_| self.nodes.keys().copied().collect());

        let mut rank: IndexMap<NodeId, u32> = IndexMap::new();
        for id in &order {
            let r = self
                .edges
                .values()
                .filter(|e| e.to_node == *id)
                .filter_map(|e| rank.get(&e.from_node).copied())
                .max()
                .map(|m| m + 1)
                .unwrap_or(0);
            rank.insert(*id, r);
        }
        for id in self.nodes.keys() {
            rank.entry(*id).or_insert(0);
        }

        let max_other = self
            .nodes
            .iter()
            .filter(|(_, n)| !matches!(n.kind, NodeKind::MasterOutput))
            .filter_map(|(id, _)| rank.get(id).copied())
            .max()
            .unwrap_or(0);
        for (id, n) in &self.nodes {
            if matches!(n.kind, NodeKind::MasterOutput) {
                rank.insert(*id, max_other.saturating_add(1));
            }
        }

        let max_rank = rank.values().copied().max().unwrap_or(0);
        let n_cols = max_rank as usize + 1;
        let mut col_width = vec![MIN_COL_WIDTH; n_cols];
        for (id, r) in &rank {
            if let Some(node) = self.nodes.get(id) {
                let i = *r as usize;
                col_width[i] = col_width[i].max(size_of(node)[0]);
            }
        }
        let mut col_x = vec![ORIGIN_X; n_cols];
        for i in 1..n_cols {
            col_x[i] = col_x[i - 1] + col_width[i - 1] + COL_GAP;
        }

        for r in 0..=max_rank {
            // (id, desired_y, height, source_out_idx, dest_in_idx)
            let mut layer: Vec<(NodeId, f32, f32, u32, u32)> = rank
                .iter()
                .filter(|(_, rr)| **rr == r)
                .filter_map(|(id, _)| {
                    let node = self.nodes.get(id)?;
                    let height = size_of(node)[1];
                    let mut source_out_idx = u32::MAX;
                    let mut dest_in_idx = u32::MAX;
                    let mut pred_ys = Vec::new();
                    for edge in self.edges.values().filter(|e| e.to_node == *id) {
                        let Some(from) = self.nodes.get(&edge.from_node) else {
                            continue;
                        };
                        let Some(&from_pos) = result.get(&edge.from_node) else {
                            continue;
                        };
                        let out_i = from
                            .outputs
                            .iter()
                            .position(|p| p.id == edge.from_port)
                            .unwrap_or(0) as u32;
                        let in_i = node
                            .inputs
                            .iter()
                            .position(|p| p.id == edge.to_port)
                            .unwrap_or(0) as u32;
                        source_out_idx = source_out_idx.min(out_i);
                        dest_in_idx = dest_in_idx.min(in_i);
                        pred_ys.push(from_pos[1]);
                    }
                    let desired_y = if pred_ys.is_empty() {
                        node.position[1]
                    } else {
                        pred_ys.iter().sum::<f32>() / pred_ys.len() as f32
                    };
                    Some((*id, desired_y, height, source_out_idx, dest_in_idx))
                })
                .collect();

            // Fan-out order first (splitter A above B), then mixer strip order,
            // then geometric Y — keeps wires from crossing.
            layer.sort_by(|a, b| {
                a.3.cmp(&b.3)
                    .then_with(|| a.4.cmp(&b.4))
                    .then_with(|| {
                        a.1.partial_cmp(&b.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| a.0.as_uuid().cmp(&b.0.as_uuid()))
            });

            let x = col_x[r as usize];
            let mut y_cursor = ORIGIN_Y;
            for (id, want, h, _, _) in layer {
                let y = if r == 0 { y_cursor } else { want.max(y_cursor) };
                result.insert(id, [x, y]);
                y_cursor = y + h + ROW_GAP;
            }
        }

        result
    }
}

/// Immutable compiled plan safe to swap onto the audio thread.
#[derive(Debug, Clone)]
pub struct CompiledPlan {
    pub order: Vec<NodeId>,
    pub nodes: IndexMap<NodeId, GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Per-node delay (samples) for PDC relative to graph max latency.
    pub delay_compensation: IndexMap<NodeId, u32>,
    pub total_latency: u32,
}

impl CompiledPlan {
    pub fn compile(graph: &AudioGraph) -> Result<Self, GraphError> {
        let mut graph = graph.clone();
        let total_latency = graph.compute_latencies();
        let mut order = graph.topological_order()?;

        // Floating plugins must not consume a worker round-trip every block.
        // Schedule only nodes that can contribute to a master output.
        let mut active = HashSet::new();
        let mut pending: Vec<NodeId> = graph
            .nodes
            .iter()
            .filter_map(|(id, node)| matches!(node.kind, NodeKind::MasterOutput).then_some(*id))
            .collect();
        while let Some(node_id) = pending.pop() {
            if !active.insert(node_id) {
                continue;
            }
            pending.extend(
                graph
                    .edges
                    .values()
                    .filter(|edge| edge.to_node == node_id)
                    .map(|edge| edge.from_node),
            );
        }
        order.retain(|id| active.contains(id));

        let mut arrival: IndexMap<NodeId, u32> = IndexMap::new();
        for node_id in &order {
            let mut input_latency = 0u32;
            for edge in graph.edges.values() {
                if edge.to_node == *node_id {
                    input_latency =
                        input_latency.max(arrival.get(&edge.from_node).copied().unwrap_or(0));
                }
            }
            let node_latency = graph
                .nodes
                .get(node_id)
                .map(|n| n.latency_samples)
                .unwrap_or(0);
            arrival.insert(*node_id, input_latency.saturating_add(node_latency));
        }
        let mut delay_compensation = IndexMap::new();
        for id in &order {
            delay_compensation.insert(*id, 0);
        }
        // PDC aligns sibling audio branches at each fan-in. Applying the
        // graph's total latency to every early node also delays serial chains
        // repeatedly (instrument -> effect -> gain), which is incorrect.
        for destination in &order {
            let incoming: Vec<NodeId> = graph
                .edges
                .values()
                .filter(|edge| {
                    edge.to_node == *destination
                        && graph
                            .nodes
                            .get(&edge.from_node)
                            .and_then(|node| node.find_port(edge.from_port))
                            .is_some_and(|port| port.port_type == PortType::Audio)
                })
                .map(|edge| edge.from_node)
                .collect();
            let max_arrival = incoming
                .iter()
                .filter_map(|id| arrival.get(id))
                .copied()
                .max()
                .unwrap_or(0);
            for source in incoming {
                let delay = max_arrival.saturating_sub(arrival.get(&source).copied().unwrap_or(0));
                if let Some(current) = delay_compensation.get_mut(&source) {
                    *current = (*current).max(delay);
                }
            }
        }
        Ok(Self {
            order,
            nodes: graph.nodes,
            edges: graph.edges.values().cloned().collect(),
            delay_compensation,
            total_latency,
        })
    }

    pub fn empty() -> Self {
        Self {
            order: Vec::new(),
            nodes: IndexMap::new(),
            edges: Vec::new(),
            delay_compensation: IndexMap::new(),
            total_latency: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cycles() {
        let mut g = AudioGraph::new();
        let a = g.add_node(GraphNode::stereo_gain_pan("A"));
        let b = g.add_node(GraphNode::stereo_gain_pan("B"));
        g.connect_stereo(a, b).unwrap();
        let err = g.connect_stereo(b, a).unwrap_err();
        assert_eq!(err, GraphError::Cycle);
    }

    #[test]
    fn rejects_type_mismatch() {
        let mut g = AudioGraph::new();
        let midi = g.add_node(GraphNode::midi_clip_source(TrackId::new(), "MIDI"));
        let gain = g.add_node(GraphNode::stereo_gain_pan("Gain"));
        let from_port = g.nodes[&midi].outputs[0].id;
        let to_port = g.nodes[&gain].inputs[0].id;
        let err = g.connect(midi, from_port, gain, to_port).unwrap_err();
        assert!(matches!(err, GraphError::TypeMismatch { .. }));
    }

    #[test]
    fn topological_order_respects_edges() {
        let mut g = AudioGraph::new();
        let a = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "Clip"));
        let b = g.add_node(GraphNode::stereo_gain_pan("Gain"));
        let c = g.add_node(GraphNode::master_output());
        g.connect_stereo(a, b).unwrap();
        g.connect_stereo(b, c).unwrap();
        let order = g.topological_order().unwrap();
        let ai = order.iter().position(|x| *x == a).unwrap();
        let bi = order.iter().position(|x| *x == b).unwrap();
        let ci = order.iter().position(|x| *x == c).unwrap();
        assert!(ai < bi && bi < ci);
    }

    #[test]
    fn latency_compensation() {
        let mut g = AudioGraph::new();
        let a = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "Clip"));
        let mut effect = GraphNode::stereo_gain_pan("FX");
        effect.latency_samples = 128;
        let b = g.add_node(effect);
        let c = g.add_node(GraphNode::master_output());
        g.connect_stereo(a, b).unwrap();
        g.connect_stereo(b, c).unwrap();
        let plan = CompiledPlan::compile(&g).unwrap();
        assert_eq!(plan.total_latency, 128);
        assert_eq!(plan.delay_compensation[&a], 0);
        assert_eq!(plan.delay_compensation[&b], 0);
        assert_eq!(plan.delay_compensation[&c], 0);
    }

    #[test]
    fn legacy_unit_sum_mixer_deserializes() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "name": "Bus",
            "kind": "SumMixer",
            "inputs": [
                {"id":"00000000-0000-0000-0000-000000000002","name":"L","port_type":"Audio","is_input":true,"channel":0},
                {"id":"00000000-0000-0000-0000-000000000003","name":"R","port_type":"Audio","is_input":true,"channel":1}
            ],
            "outputs": [
                {"id":"00000000-0000-0000-0000-000000000004","name":"L","port_type":"Audio","is_input":false,"channel":0},
                {"id":"00000000-0000-0000-0000-000000000005","name":"R","port_type":"Audio","is_input":false,"channel":1}
            ],
            "position": [0.0, 0.0],
            "latency_samples": 0
        }"#;
        let mut node: GraphNode = serde_json::from_str(json).unwrap();
        assert!(matches!(node.kind, NodeKind::SumMixer { .. }));
        node.migrate_ports();
        assert_eq!(node.inputs.len(), MIXER_STRIP_COUNT * 2);
        assert_eq!(node.inputs[0].name, "L In A");
        assert_eq!(node.inputs[1].name, "R In A");
        assert_eq!(node.outputs[0].name, "L Out");
        assert_eq!(node.outputs[1].name, "R Out");
        // Original port IDs preserved on strip 1.
        assert_eq!(
            node.inputs[0].id.to_string(),
            "00000000-0000-0000-0000-000000000002"
        );
    }

    #[test]
    fn disconnected_plugins_are_not_scheduled() {
        let mut g = AudioGraph::new();
        let source = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "Clip"));
        let master = g.add_node(GraphNode::master_output());
        let floating = g.add_node(GraphNode::vst3_effect(
            PluginInstanceId::new(),
            "effect".into(),
            "/effect.vst3".into(),
            "Floating effect".into(),
        ));
        g.connect_stereo(source, master).unwrap();

        let plan = CompiledPlan::compile(&g).unwrap();

        assert!(plan.order.contains(&source));
        assert!(plan.order.contains(&master));
        assert!(!plan.order.contains(&floating));
    }

    #[test]
    fn arranged_positions_flow_left_to_right() {
        let mut g = AudioGraph::new();
        let mut src = GraphNode::midi_clip_source(TrackId::new(), "MIDI");
        src.position = [400.0, 80.0];
        let mut synth = GraphNode::builtin_synth("Synth");
        synth.position = [10.0, 10.0];
        let mut gain = GraphNode::stereo_gain_pan("Gain");
        gain.position = [50.0, 500.0];
        let mut master = GraphNode::master_output();
        master.position = [0.0, 0.0];

        let src_id = g.add_node(src);
        let synth_id = g.add_node(synth);
        let gain_id = g.add_node(gain);
        let master_id = g.add_node(master);
        g.connect_midi(src_id, synth_id).unwrap();
        g.connect_stereo(synth_id, gain_id).unwrap();
        g.connect_stereo(gain_id, master_id).unwrap();

        let pos = g.arranged_positions(|_| [140.0, 56.0]);
        assert!(pos[&src_id][0] < pos[&synth_id][0]);
        assert!(pos[&synth_id][0] < pos[&gain_id][0]);
        assert!(pos[&gain_id][0] < pos[&master_id][0]);
        let ys = [pos[&src_id][1], pos[&synth_id][1], pos[&gain_id][1]];
        let spread = ys.iter().copied().fold(f32::MIN, f32::max)
            - ys.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            spread < 1.0,
            "serial chain should stay aligned, spread={spread}"
        );
    }

    #[test]
    fn arranged_positions_respects_splitter_port_order() {
        let mut g = AudioGraph::new();
        let src = g.add_node(GraphNode::midi_clip_source(TrackId::new(), "MIDI Out"));
        let split = g.add_node(GraphNode::midi_splitter("Split"));
        // Intentionally place "A" target below "B" target before organising.
        let mut a_inst = GraphNode::builtin_synth("MIDI In A");
        a_inst.position = [0.0, 400.0];
        let mut b_inst = GraphNode::builtin_synth("MIDI In B");
        b_inst.position = [0.0, 0.0];
        let a_id = g.add_node(a_inst);
        let b_id = g.add_node(b_inst);
        let mut mixer = GraphNode::sum_mixer("Bus");
        mixer.position = [0.0, 200.0];
        let mix_id = g.add_node(mixer);
        let master = g.add_node(GraphNode::master_output());

        g.connect_midi(src, split).unwrap();
        // Split A → bottom-placed synth, Split B → top-placed synth (crossed).
        g.connect_midi(split, a_id).unwrap();
        let split_b = g.nodes[&split].outputs[1].id;
        let b_midi = g.nodes[&b_id].inputs[0].id;
        g.connect(split, split_b, b_id, b_midi).unwrap();
        // Crossed into mixer strips too: A → In2, B → In1.
        let a_l = g.nodes[&a_id].outputs[0].id;
        let a_r = g.nodes[&a_id].outputs[1].id;
        let b_l = g.nodes[&b_id].outputs[0].id;
        let b_r = g.nodes[&b_id].outputs[1].id;
        let in1_l = g.nodes[&mix_id].inputs[0].id;
        let in1_r = g.nodes[&mix_id].inputs[1].id;
        let in2_l = g.nodes[&mix_id].inputs[2].id;
        let in2_r = g.nodes[&mix_id].inputs[3].id;
        g.connect(a_id, a_l, mix_id, in2_l).unwrap();
        g.connect(a_id, a_r, mix_id, in2_r).unwrap();
        g.connect(b_id, b_l, mix_id, in1_l).unwrap();
        g.connect(b_id, b_r, mix_id, in1_r).unwrap();
        g.connect_stereo(mix_id, master).unwrap();

        let pos = g.arranged_positions(|_| [140.0, 56.0]);
        assert!(
            pos[&a_id][1] < pos[&b_id][1],
            "split A target should sit above split B target"
        );
    }

    /// Node footprint used by the routing editor.
    fn editor_size(node: &GraphNode) -> [f32; 2] {
        let ports = node.inputs.len().max(node.outputs.len()).max(1);
        [layout::NODE_WIDTH, 56.0 + (ports as f32 - 1.0) * 14.0]
    }

    fn overlapping_pair(g: &AudioGraph) -> Option<(String, String)> {
        let nodes: Vec<_> = g.nodes.values().collect();
        for (i, a) in nodes.iter().enumerate() {
            for b in nodes.iter().skip(i + 1) {
                let (sa, sb) = (editor_size(a), editor_size(b));
                let overlap_x = a.position[0] < b.position[0] + sb[0]
                    && b.position[0] < a.position[0] + sa[0];
                let overlap_y = a.position[1] < b.position[1] + sb[1]
                    && b.position[1] < a.position[1] + sa[1];
                if overlap_x && overlap_y {
                    return Some((a.name.clone(), b.name.clone()));
                }
            }
        }
        None
    }

    fn typical_session() -> AudioGraph {
        let mut g = AudioGraph::new();
        let src = g.add_node(GraphNode::midi_clip_source(TrackId::new(), "MIDI 1 MIDI"));
        let synth = g.add_node(GraphNode::builtin_synth("CottSynth"));
        let gain = g.add_node(GraphNode::stereo_gain_pan("MIDI 1 Gain"));
        let audio = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "Audio 1"));
        let audio_gain = g.add_node(GraphNode::stereo_gain_pan("Audio 1 Gain"));
        let master = g.add_node(GraphNode::master_output());
        g.connect_midi(src, synth).unwrap();
        g.connect_stereo(synth, gain).unwrap();
        g.connect_stereo(gain, master).unwrap();
        g.connect_stereo(audio, audio_gain).unwrap();
        g.connect_stereo(audio_gain, master).unwrap();
        g
    }

    #[test]
    fn untouched_graphs_are_laid_out_without_overlaps() {
        let mut g = typical_session();
        // Positions from an older build, when nodes were drawn narrower.
        for (i, node) in g.nodes.values_mut().enumerate() {
            node.position = [40.0 + (i % 3) as f32 * 160.0, (i / 3) as f32 * 100.0];
        }
        assert!(overlapping_pair(&g).is_some(), "test setup should overlap");

        assert!(g.auto_arrange_if_untouched(editor_size));
        assert_eq!(
            overlapping_pair(&g),
            None,
            "auto-arrange left nodes on top of each other"
        );
    }

    #[test]
    fn a_hand_arranged_graph_is_left_alone() {
        let mut g = typical_session();
        let before: Vec<_> = g.nodes.values().map(|n| n.position).collect();
        g.user_arranged = true;

        assert!(!g.auto_arrange_if_untouched(editor_size));
        let after: Vec<_> = g.nodes.values().map(|n| n.position).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn user_arranged_survives_a_save_and_load() {
        let mut g = typical_session();
        g.user_arranged = true;
        let json = serde_json::to_string(&g).unwrap();
        let restored: AudioGraph = serde_json::from_str(&json).unwrap();
        assert!(restored.user_arranged);

        // Projects written before the flag existed load as "never arranged".
        let legacy = json.replace("\"user_arranged\":true", "\"unused\":true");
        let restored: AudioGraph = serde_json::from_str(&legacy).unwrap();
        assert!(!restored.user_arranged);
    }
}
