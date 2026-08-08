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
        let mute = mixer
            .get("mute")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
            inputs: vec![Port::audio_in("L", 0), Port::audio_in("R", 1)],
            outputs: vec![Port::audio_out("L", 0), Port::audio_out("R", 1)],
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
            outputs: vec![Port::midi_out("MIDI")],
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
            outputs: vec![Port::audio_out("L", 0), Port::audio_out("R", 1)],
            position: [0.0, 0.0],
            latency_samples: 0,
        }
    }

    pub fn master_output() -> Self {
        Self {
            id: NodeId::new(),
            name: "Master".into(),
            kind: NodeKind::MasterOutput,
            inputs: vec![Port::audio_in("L", 0), Port::audio_in("R", 1)],
            outputs: vec![],
            position: [600.0, 100.0],
            latency_samples: 0,
        }
    }

    pub fn sum_mixer(name: impl Into<String>) -> Self {
        let mut inputs = Vec::with_capacity(MIXER_STRIP_COUNT * 2);
        for i in 0..MIXER_STRIP_COUNT {
            let n = i + 1;
            inputs.push(Port::audio_in(format!("In{n} L"), (i * 2) as u32));
            inputs.push(Port::audio_in(format!("In{n} R"), (i * 2 + 1) as u32));
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
            outputs: vec![Port::audio_out("L", 0), Port::audio_out("R", 1)],
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
            inputs: vec![Port::audio_in("L", 0), Port::audio_in("R", 1)],
            outputs: vec![
                Port::audio_out("A L", 0),
                Port::audio_out("A R", 1),
                Port::audio_out("B L", 2),
                Port::audio_out("B R", 3),
            ],
            position: [400.0, 200.0],
            latency_samples: 0,
        }
    }

    pub fn midi_mixer(name: impl Into<String>) -> Self {
        let mut inputs = Vec::with_capacity(MIDI_MIXER_INPUT_COUNT);
        for i in 0..MIDI_MIXER_INPUT_COUNT {
            inputs.push(Port::midi_in(format!("In{}", i + 1)));
        }
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::MidiMixer,
            inputs,
            outputs: vec![Port::midi_out("Out")],
            position: [300.0, 160.0],
            latency_samples: 0,
        }
    }

    pub fn midi_splitter(name: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(),
            name: name.into(),
            kind: NodeKind::MidiSplitter,
            inputs: vec![Port::midi_in("In")],
            outputs: vec![Port::midi_out("A"), Port::midi_out("B")],
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
            inputs: vec![Port::midi_in("MIDI")],
            outputs: vec![Port::audio_out("L", 0), Port::audio_out("R", 1)],
            position: [200.0, 0.0],
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
            inputs: vec![Port::audio_in("L", 0), Port::audio_in("R", 1)],
            outputs: vec![Port::audio_out("L", 0), Port::audio_out("R", 1)],
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

    /// Expand legacy mixer port layouts after load (keeps existing port IDs).
    pub fn migrate_ports(&mut self) {
        match &self.kind {
            NodeKind::SumMixer { .. } => {
                if self.inputs.len() >= MIXER_STRIP_COUNT * 2 {
                    return;
                }
                let mut by_ch: IndexMap<u32, Port> = IndexMap::new();
                for port in self.inputs.drain(..) {
                    if port.port_type == PortType::Audio {
                        by_ch.insert(port.channel, port);
                    }
                }
                let mut inputs = Vec::with_capacity(MIXER_STRIP_COUNT * 2);
                for i in 0..MIXER_STRIP_COUNT {
                    let n = i + 1;
                    let l_ch = (i * 2) as u32;
                    let r_ch = (i * 2 + 1) as u32;
                    let mut l = by_ch
                        .shift_remove(&l_ch)
                        .unwrap_or_else(|| Port::audio_in(format!("In{n} L"), l_ch));
                    l.name = format!("In{n} L");
                    l.channel = l_ch;
                    l.is_input = true;
                    let mut r = by_ch
                        .shift_remove(&r_ch)
                        .unwrap_or_else(|| Port::audio_in(format!("In{n} R"), r_ch));
                    r.name = format!("In{n} R");
                    r.channel = r_ch;
                    r.is_input = true;
                    inputs.push(l);
                    inputs.push(r);
                }
                self.inputs = inputs;
            }
            _ => {}
        }
    }
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioGraph {
    pub nodes: IndexMap<NodeId, GraphNode>,
    pub edges: IndexMap<EdgeId, GraphEdge>,
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
        assert_eq!(node.inputs[0].name, "In1 L");
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
}
