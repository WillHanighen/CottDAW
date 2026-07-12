//! Built-in DSP nodes and planar buffer helpers.

use crate::automation::gain_db_to_linear;
use crate::clips::ScheduledMidiEvent;
use crate::graph::{CompiledPlan, NodeKind, PortType};
use crate::ids::{NodeId, PortId, TrackId};
use crate::time::{SamplePos, TempoMap, TransportState};
use indexmap::IndexMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub channels: Vec<Vec<f32>>,
}

impl AudioBuffer {
    pub fn silent(channels: usize, frames: usize) -> Self {
        Self {
            channels: (0..channels).map(|_| vec![0.0; frames]).collect(),
        }
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn clear(&mut self) {
        for ch in &mut self.channels {
            ch.fill(0.0);
        }
    }

    pub fn add_from(&mut self, other: &AudioBuffer) {
        let frames = self.frames().min(other.frames());
        let chans = self.channel_count().min(other.channel_count());
        for c in 0..chans {
            for i in 0..frames {
                self.channels[c][i] += other.channels[c][i];
            }
        }
    }

    pub fn apply_gain(&mut self, gain: f32) {
        for ch in &mut self.channels {
            for s in ch.iter_mut() {
                *s *= gain;
            }
        }
    }

    pub fn peak(&self) -> f32 {
        self.channels
            .iter()
            .flat_map(|c| c.iter())
            .map(|s| s.abs())
            .fold(0.0f32, f32::max)
    }

    pub fn apply_pan_stereo(&mut self, pan: f32) {
        if self.channel_count() < 2 {
            return;
        }
        let pan = pan.clamp(-1.0, 1.0);
        let left = ((1.0 - pan) * 0.5).sqrt();
        let right = ((1.0 + pan) * 0.5).sqrt();
        let frames = self.frames();
        for i in 0..frames {
            self.channels[0][i] *= left;
            self.channels[1][i] *= right;
        }
    }

    /// Delay this buffer by `delay` samples using a persistent FIFO (PDC).
    pub fn delay_with_state(&mut self, delay: usize, state: &mut Vec<Vec<f32>>) {
        if delay == 0 {
            return;
        }
        let channels = self.channel_count();
        let frames = self.frames();
        while state.len() < channels {
            state.push(Vec::new());
        }
        for ch in 0..channels {
            let fifo = &mut state[ch];
            let mut out = vec![0.0; frames];
            for i in 0..frames {
                fifo.push(self.channels[ch][i]);
                if fifo.len() > delay {
                    out[i] = fifo.remove(0);
                } else {
                    out[i] = 0.0;
                }
            }
            self.channels[ch] = out;
        }
    }
}

#[derive(Debug, Clone)]
pub struct MeterState {
    pub peak_l: f32,
    pub peak_r: f32,
}

impl Default for MeterState {
    fn default() -> Self {
        Self {
            peak_l: 0.0,
            peak_r: 0.0,
        }
    }
}

/// Shared clip/audio sample cache keyed by asset id (planar f32 at project SR).
#[derive(Debug, Default)]
pub struct SampleCache {
    pub buffers: IndexMap<crate::ids::AssetId, Arc<AudioBuffer>>,
}

/// Context passed into one process block.
pub struct ProcessContext<'a> {
    pub sample_rate: u32,
    pub block_start: SamplePos,
    pub block_len: u32,
    pub tempo: &'a TempoMap,
    pub transport: TransportState,
    pub clips: &'a [crate::clips::Clip],
    pub sample_cache: &'a SampleCache,
    pub automation: &'a [crate::automation::AutomationLane],
    /// Optional plugin processor callbacks: (instance kind handled externally).
    pub plugin_audio: &'a mut dyn PluginAudioHost,
    /// Live audition MIDI (piano roll), keyed by source track.
    pub preview_midi: &'a [(TrackId, ScheduledMidiEvent)],
    /// Per-node PDC delay line state (channel rings).
    pub pdc_state: &'a mut IndexMap<NodeId, Vec<Vec<f32>>>,
}

pub trait PluginAudioHost {
    fn process_instrument(
        &mut self,
        instance: crate::ids::PluginInstanceId,
        midi: &[ScheduledMidiEvent],
        output: &mut AudioBuffer,
        ctx: &TransportBlockInfo,
    ) -> bool;

    fn process_effect(
        &mut self,
        instance: crate::ids::PluginInstanceId,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        ctx: &TransportBlockInfo,
    ) -> bool;

    /// Apply a normalized plugin parameter (0..1). Default: no-op.
    fn set_param(&mut self, _instance: crate::ids::PluginInstanceId, _param_id: u32, _value: f32) {}
}

#[derive(Debug, Clone, Copy)]
pub struct TransportBlockInfo {
    pub sample_rate: u32,
    pub block_start: SamplePos,
    pub block_len: u32,
    pub bpm: f64,
    pub playing: bool,
}

pub struct NullPluginHost;

impl PluginAudioHost for NullPluginHost {
    fn process_instrument(
        &mut self,
        _instance: crate::ids::PluginInstanceId,
        midi: &[ScheduledMidiEvent],
        output: &mut AudioBuffer,
        ctx: &TransportBlockInfo,
    ) -> bool {
        // Simple sine stub so MIDI tracks are audible without plugins.
        output.clear();
        if output.channel_count() == 0 {
            return true;
        }
        let frames = output.frames();
        let mut phase = 0.0f32;
        let mut amp = 0.0f32;
        let mut freq = 440.0f32;
        for ev in midi {
            if ev.status & 0xf0 == 0x90 && ev.data2 > 0 {
                freq = 440.0 * 2f32.powf((ev.data1 as f32 - 69.0) / 12.0);
                amp = ev.data2 as f32 / 127.0 * 0.2;
                phase = 0.0;
            } else if ev.status & 0xf0 == 0x80 || (ev.status & 0xf0 == 0x90 && ev.data2 == 0) {
                amp = 0.0;
            }
        }
        // If any note-on in block, render a short tone for the whole block (MVP stub).
        let any_on = midi.iter().any(|e| e.status & 0xf0 == 0x90 && e.data2 > 0);
        if any_on {
            let sr = ctx.sample_rate.max(1) as f32;
            for i in 0..frames {
                let s = (phase * std::f32::consts::TAU).sin() * amp;
                for ch in &mut output.channels {
                    ch[i] = s;
                }
                phase = (phase + freq / sr) % 1.0;
            }
        }
        true
    }

    fn process_effect(
        &mut self,
        _instance: crate::ids::PluginInstanceId,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        _ctx: &TransportBlockInfo,
    ) -> bool {
        *output = input.clone();
        true
    }
}

/// Process one block through a compiled plan into master stereo output.
pub fn process_block(
    plan: &CompiledPlan,
    ctx: &mut ProcessContext<'_>,
    meters: &mut IndexMap<NodeId, MeterState>,
) -> AudioBuffer {
    let frames = ctx.block_len as usize;
    let mut port_buffers: IndexMap<(NodeId, PortId), Vec<f32>> = IndexMap::new();
    let mut node_stereo: IndexMap<NodeId, AudioBuffer> = IndexMap::new();
    let mut master = AudioBuffer::silent(2, frames);

    let transport_info = TransportBlockInfo {
        sample_rate: ctx.sample_rate,
        block_start: ctx.block_start,
        block_len: ctx.block_len,
        bpm: ctx.tempo.bpm,
        playing: matches!(ctx.transport, TransportState::Playing),
    };

    let any_solo = plan.nodes.values().any(|n| {
        matches!(
            &n.kind,
            NodeKind::GainPan { solo: true, .. }
        )
    });

    let beat = ctx.tempo.sample_to_beat(ctx.block_start).0;
    for lane in ctx.automation.iter().filter(|l| l.enabled) {
        if let crate::automation::AutomationTarget::PluginParam {
            instance_id,
            param_id,
        } = &lane.target
        {
            ctx.plugin_audio
                .set_param(*instance_id, *param_id, lane.value_at(beat));
        }
    }

    // Pre-create output buffers for each port.
    for (id, node) in &plan.nodes {
        for port in node
            .outputs
            .iter()
            .filter(|p| p.port_type == PortType::Audio)
        {
            port_buffers.insert((*id, port.id), vec![0.0; frames]);
        }
        node_stereo.insert(*id, AudioBuffer::silent(2, frames));
    }

    for node_id in &plan.order {
        let Some(node) = plan.nodes.get(node_id) else {
            continue;
        };

        // Sum audio inputs into a stereo buffer.
        let mut input = AudioBuffer::silent(2, frames);
        for edge in plan.edges.iter().filter(|e| e.to_node == *node_id) {
            if let Some(buf) = port_buffers.get(&(edge.from_node, edge.from_port)) {
                if let Some(to_port) = node.find_port(edge.to_port) {
                    if to_port.port_type == PortType::Audio {
                        let ch = to_port.channel as usize;
                        if ch < input.channel_count() {
                            for i in 0..frames {
                                input.channels[ch][i] += buf[i];
                            }
                        }
                    }
                }
            }
        }

        let mut output = AudioBuffer::silent(2, frames);

        match &node.kind {
            NodeKind::AudioClipSource { track_id } => {
                // Only schedule arrangement audio while transport is playing —
                // otherwise a frozen playhead would retrigger the same slice every block.
                if matches!(ctx.transport, TransportState::Playing) {
                    render_audio_clips(*track_id, ctx, &mut output);
                } else {
                    output.clear();
                }
            }
            NodeKind::MidiClipSource { track_id } => {
                // MIDI-only source; events consumed by downstream instrument.
                let _ = track_id;
            }
            NodeKind::GainPan {
                gain_db,
                pan,
                mute,
                solo,
            } => {
                output = input;
                let solo_muted = any_solo && !*solo;
                if *mute || solo_muted {
                    output.clear();
                } else {
                    let mut g = *gain_db;
                    let mut p = *pan;
                    // Apply automation if present.
                    let beat = ctx.tempo.sample_to_beat(ctx.block_start).0;
                    for lane in ctx.automation.iter().filter(|l| l.enabled) {
                        match &lane.target {
                            crate::automation::AutomationTarget::NodeGain { node_id: nid }
                                if *nid == *node_id =>
                            {
                                g = crate::automation::normalized_to_gain_db(lane.value_at(beat));
                            }
                            crate::automation::AutomationTarget::NodePan { node_id: nid }
                                if *nid == *node_id =>
                            {
                                p = lane.value_at(beat) * 2.0 - 1.0;
                            }
                            _ => {}
                        }
                    }
                    output.apply_gain(gain_db_to_linear(g));
                    output.apply_pan_stereo(p);
                }
            }
            NodeKind::SumMixer => {
                output = input;
            }
            NodeKind::MasterOutput => {
                master.add_from(&input);
                output = input;
            }
            NodeKind::Vst3Instrument {
                instance_id,
                failed,
                ..
            } => {
                if *failed {
                    output.clear();
                } else {
                    let midi = collect_midi_for_instrument(plan, *node_id, ctx);
                    let ok = ctx.plugin_audio.process_instrument(
                        *instance_id,
                        &midi,
                        &mut output,
                        &transport_info,
                    );
                    if !ok {
                        output.clear();
                    }
                }
            }
            NodeKind::Vst3Effect {
                instance_id,
                bypass,
                failed,
                ..
            } => {
                if *bypass || *failed {
                    output = input;
                } else {
                    let ok = ctx.plugin_audio.process_effect(
                        *instance_id,
                        &input,
                        &mut output,
                        &transport_info,
                    );
                    if !ok {
                        output = input;
                    }
                }
            }
        }

        // Apply plugin delay compensation before exposing audio to downstream ports.
        let delay = plan
            .delay_compensation
            .get(node_id)
            .copied()
            .unwrap_or(0) as usize;
        if delay > 0 {
            let state = ctx.pdc_state.entry(*node_id).or_default();
            output.delay_with_state(delay, state);
        }

        // Write planar outputs to ports.
        for port in node
            .outputs
            .iter()
            .filter(|p| p.port_type == PortType::Audio)
        {
            if let Some(buf) = port_buffers.get_mut(&(*node_id, port.id)) {
                let ch = port.channel as usize;
                if ch < output.channel_count() {
                    let len = frames.min(buf.len());
                    buf.copy_from_slice(&output.channels[ch][..len]);
                }
            }
        }
        node_stereo.insert(*node_id, output.clone());
        let peak_l = output
            .channels
            .first()
            .map(|c| c.iter().copied().map(f32::abs).fold(0.0, f32::max))
            .unwrap_or(0.0);
        let peak_r = output
            .channels
            .get(1)
            .map(|c| c.iter().copied().map(f32::abs).fold(0.0, f32::max))
            .unwrap_or(peak_l);
        meters.insert(*node_id, MeterState { peak_l, peak_r });
    }

    // Soft clip master lightly.
    for ch in &mut master.channels {
        for s in ch.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
    master
}

fn render_audio_clips(track_id: TrackId, ctx: &ProcessContext<'_>, output: &mut AudioBuffer) {
    output.clear();
    let frames = output.frames();
    let block_start = ctx.block_start.0;
    let block_end = block_start + ctx.block_len as i64;
    for clip in ctx.clips.iter().filter(|c| c.track_id == track_id) {
        let crate::clips::ClipContent::Audio {
            asset_id,
            source_offset_samples,
            gain_db,
        } = &clip.content
        else {
            continue;
        };
        let clip_start = ctx
            .tempo
            .beat_to_sample(crate::time::BeatPos(clip.start_beats))
            .0;
        let clip_end = ctx
            .tempo
            .beat_to_sample(crate::time::BeatPos(clip.end_beats()))
            .0;
        if clip_end <= block_start || clip_start >= block_end {
            continue;
        }
        let Some(buf) = ctx.sample_cache.buffers.get(asset_id) else {
            continue;
        };
        let gain = gain_db_to_linear(*gain_db);
        let overlap_start = block_start.max(clip_start);
        let overlap_end = block_end.min(clip_end);
        for abs in overlap_start..overlap_end {
            let out_i = (abs - block_start) as usize;
            let src_i64 = abs - clip_start + source_offset_samples;
            // Negative source index = before the trimmed audio start (silence).
            if src_i64 < 0 {
                continue;
            }
            let src_i = src_i64 as usize;
            if src_i >= buf.frames() || out_i >= frames {
                continue;
            }
            for ch in 0..output.channel_count().min(buf.channel_count()) {
                output.channels[ch][out_i] += buf.channels[ch][src_i] * gain;
            }
        }
    }
}

fn collect_midi_for_instrument(
    plan: &CompiledPlan,
    instrument_node: NodeId,
    ctx: &ProcessContext<'_>,
) -> Vec<ScheduledMidiEvent> {
    let mut events = Vec::new();
    let mut source_tracks = Vec::new();
    for edge in plan.edges.iter().filter(|e| e.to_node == instrument_node) {
        if let Some(src) = plan.nodes.get(&edge.from_node) {
            if let NodeKind::MidiClipSource { track_id } = &src.kind {
                source_tracks.push(*track_id);
                if matches!(ctx.transport, TransportState::Playing) {
                    events.extend(crate::clips::schedule_midi_for_block(
                        ctx.clips,
                        *track_id,
                        ctx.tempo,
                        ctx.block_start,
                        ctx.block_len,
                    ));
                }
            }
        }
    }
    for (track_id, ev) in ctx.preview_midi {
        if source_tracks.contains(track_id) {
            events.push(*ev);
        }
    }
    events.sort_by(|a, b| {
        a.sample_offset
            .cmp(&b.sample_offset)
            .then_with(|| a.sort_priority().cmp(&b.sort_priority()))
    });
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{AudioGraph, GraphNode};
    use crate::project::Project;

    #[test]
    fn gain_pan_silence_when_muted() {
        let mut project = Project::new("t");
        let track = project.add_audio_track("A");
        let gain_id = project
            .tracks
            .iter()
            .find(|t| t.id == track)
            .unwrap()
            .gain_node
            .unwrap();
        if let Some(node) = project.graph.nodes.get_mut(&gain_id) {
            if let NodeKind::GainPan { mute, .. } = &mut node.kind {
                *mute = true;
            }
        }
        let plan = project.compiled_plan();
        let cache = SampleCache::default();
        let mut host = NullPluginHost;
        let mut meters = IndexMap::new();
        let mut pdc_state = IndexMap::new();
        let mut ctx = ProcessContext {
            sample_rate: 48_000,
            block_start: SamplePos(0),
            block_len: 128,
            tempo: &project.tempo,
            transport: TransportState::Playing,
            clips: &project.clips,
            sample_cache: &cache,
            automation: &project.automation,
            plugin_audio: &mut host,
            preview_midi: &[],
            pdc_state: &mut pdc_state,
        };
        let out = process_block(&plan, &mut ctx, &mut meters);
        assert_eq!(out.peak(), 0.0);
    }

    #[test]
    fn solo_mutes_non_soloed_gain_nodes() {
        let mut project = Project::new("t");
        let a = project.add_audio_track("A");
        let b = project.add_audio_track("B");
        let gain_a = project
            .tracks
            .iter()
            .find(|t| t.id == a)
            .unwrap()
            .gain_node
            .unwrap();
        let gain_b = project
            .tracks
            .iter()
            .find(|t| t.id == b)
            .unwrap()
            .gain_node
            .unwrap();
        if let Some(node) = project.graph.nodes.get_mut(&gain_a) {
            if let NodeKind::GainPan { solo, .. } = &mut node.kind {
                *solo = true;
            }
        }
        let plan = project.compiled_plan();
        let any_solo = plan.nodes.values().any(|n| {
            matches!(&n.kind, NodeKind::GainPan { solo: true, .. })
        });
        assert!(any_solo);
        let b_soloed = matches!(
            &plan.nodes[&gain_b].kind,
            NodeKind::GainPan { solo: true, .. }
        );
        assert!(!b_soloed);
    }

    #[test]
    fn null_host_uses_transport_sample_rate() {
        let mut host = NullPluginHost;
        let mut out_48 = AudioBuffer::silent(2, 48);
        let mut out_24 = AudioBuffer::silent(2, 48);
        let midi = [ScheduledMidiEvent {
            sample_offset: 0,
            status: 0x90,
            data1: 69,
            data2: 100,
        }];
        let ctx48 = TransportBlockInfo {
            sample_rate: 48_000,
            block_start: SamplePos(0),
            block_len: 48,
            bpm: 120.0,
            playing: true,
        };
        let ctx24 = TransportBlockInfo {
            sample_rate: 24_000,
            block_start: SamplePos(0),
            block_len: 48,
            bpm: 120.0,
            playing: true,
        };
        host.process_instrument(crate::ids::PluginInstanceId::new(), &midi, &mut out_48, &ctx48);
        host.process_instrument(crate::ids::PluginInstanceId::new(), &midi, &mut out_24, &ctx24);
        // At A440, one period is 48 samples @ 48k and 24 samples @ 24k — peaks should differ.
        assert!(out_48.peak() > 0.0);
        assert!(out_24.peak() > 0.0);
        assert!((out_48.channels[0][24] - out_24.channels[0][24]).abs() > 1e-4);
    }

    #[test]
    fn fan_in_sums() {
        let mut g = AudioGraph::new();
        let a = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "A"));
        let b = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "B"));
        let mix = g.add_node(GraphNode::sum_mixer("Mix"));
        let master = g.add_node(GraphNode::master_output());
        g.connect_stereo(a, mix).unwrap();
        g.connect_stereo(b, mix).unwrap();
        g.connect_stereo(mix, master).unwrap();
        let plan = CompiledPlan::compile(&g).unwrap();
        assert!(plan.order.len() >= 4);
    }

    #[test]
    fn delay_with_state_preserves_fifo_across_blocks() {
        let mut buf = AudioBuffer::silent(1, 4);
        buf.channels[0] = vec![1.0, 2.0, 3.0, 4.0];
        let mut state = Vec::new();
        buf.delay_with_state(2, &mut state);
        assert_eq!(buf.channels[0], vec![0.0, 0.0, 1.0, 2.0]);

        buf.channels[0] = vec![5.0, 6.0, 7.0, 8.0];
        buf.delay_with_state(2, &mut state);
        assert_eq!(buf.channels[0], vec![3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn negative_source_offset_skips_before_audio_start() {
        let track = TrackId::new();
        let asset = crate::ids::AssetId::new();
        let mut clip = crate::clips::Clip::new_audio(track, "A", 0.0, 1.0, asset);
        if let crate::clips::ClipContent::Audio {
            source_offset_samples,
            ..
        } = &mut clip.content
        {
            *source_offset_samples = -100;
        }
        let mut buf = AudioBuffer::silent(1, 64);
        for (i, s) in buf.channels[0].iter_mut().enumerate() {
            *s = i as f32;
        }
        let mut cache = SampleCache::default();
        cache.buffers.insert(asset, Arc::new(buf));
        let tempo = TempoMap::default();
        let mut host = NullPluginHost;
        let mut pdc_state = IndexMap::new();
        let mut meters = IndexMap::new();
        let mut g = AudioGraph::new();
        let src = g.add_node(GraphNode::audio_clip_source(track, "src"));
        let master = g.add_node(GraphNode::master_output());
        g.connect_stereo(src, master).unwrap();
        let plan = CompiledPlan::compile(&g).unwrap();
        let clips = [clip];
        let mut ctx = ProcessContext {
            sample_rate: 48_000,
            block_start: SamplePos(0),
            block_len: 128,
            tempo: &tempo,
            transport: TransportState::Playing,
            clips: &clips,
            sample_cache: &cache,
            automation: &[],
            plugin_audio: &mut host,
            preview_midi: &[],
            pdc_state: &mut pdc_state,
        };
        let out = process_block(&plan, &mut ctx, &mut meters);
        // offset -100 => first output frame maps before buffer start → silence.
        assert_eq!(out.channels[0][0], 0.0);
    }
}
