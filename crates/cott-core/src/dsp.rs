//! Built-in DSP nodes and planar buffer helpers.

use crate::automation::gain_db_to_linear;
use crate::clips::ScheduledMidiEvent;
use crate::graph::{CompiledPlan, MIXER_STRIP_COUNT, NodeKind, PortType};
use crate::ids::{NodeId, PortId, TrackId};
use crate::time::{SamplePos, TempoMap, TransportState};
use cott_synth_dsp::{MidiNoteEvent, PolySynth};
use indexmap::IndexMap;
use std::collections::HashSet;
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
    /// Persistent CottSynth voice state keyed by graph node.
    pub builtin_synth_state: &'a mut IndexMap<NodeId, PolySynth>,
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
    pub time_sig_numerator: u32,
    pub time_sig_denominator: u32,
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
        time_sig_numerator: ctx.tempo.beats_per_bar,
        time_sig_denominator: ctx.tempo.beat_unit,
        playing: matches!(ctx.transport, TransportState::Playing),
    };

    let any_solo = plan
        .nodes
        .values()
        .any(|n| matches!(&n.kind, NodeKind::GainPan { solo: true, .. }));

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

        // Sum audio inputs; channel count follows the node's highest input channel.
        let in_channels = node
            .inputs
            .iter()
            .filter(|p| p.port_type == PortType::Audio)
            .map(|p| p.channel as usize + 1)
            .max()
            .unwrap_or(2)
            .max(2);
        let mut input = AudioBuffer::silent(in_channels, frames);
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
            NodeKind::MidiMixer | NodeKind::MidiSplitter => {
                // MIDI routers — audio silent; events collected via graph walk.
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
            NodeKind::SumMixer {
                strips,
                master_gain_db,
                master_pan,
                mute,
            } => {
                if *mute {
                    output.clear();
                } else {
                    let mut mixed = AudioBuffer::silent(2, frames);
                    for (i, strip) in strips.iter().take(MIXER_STRIP_COUNT).enumerate() {
                        if strip.mute {
                            continue;
                        }
                        let mut strip_buf = AudioBuffer::silent(2, frames);
                        let l = i * 2;
                        let r = i * 2 + 1;
                        if l < input.channel_count() {
                            let len = frames.min(input.channels[l].len());
                            strip_buf.channels[0][..len]
                                .copy_from_slice(&input.channels[l][..len]);
                        }
                        if r < input.channel_count() {
                            let len = frames.min(input.channels[r].len());
                            strip_buf.channels[1][..len]
                                .copy_from_slice(&input.channels[r][..len]);
                        }
                        strip_buf.apply_gain(gain_db_to_linear(strip.gain_db));
                        strip_buf.apply_pan_stereo(strip.pan);
                        mixed.add_from(&strip_buf);
                    }
                    mixed.apply_gain(gain_db_to_linear(*master_gain_db));
                    mixed.apply_pan_stereo(*master_pan);
                    output = mixed;
                }
            }
            NodeKind::StereoSplitter { a, b } => {
                let mut branch_a = AudioBuffer::silent(2, frames);
                let mut branch_b = AudioBuffer::silent(2, frames);
                for ch in 0..2 {
                    if ch < input.channel_count() {
                        let len = frames.min(input.channels[ch].len());
                        branch_a.channels[ch][..len].copy_from_slice(&input.channels[ch][..len]);
                        branch_b.channels[ch][..len].copy_from_slice(&input.channels[ch][..len]);
                    }
                }
                branch_a.apply_gain(gain_db_to_linear(a.gain_db));
                branch_a.apply_pan_stereo(a.pan);
                branch_b.apply_gain(gain_db_to_linear(b.gain_db));
                branch_b.apply_pan_stereo(b.pan);
                output = AudioBuffer::silent(4, frames);
                let len = frames;
                output.channels[0][..len].copy_from_slice(&branch_a.channels[0][..len]);
                output.channels[1][..len].copy_from_slice(&branch_a.channels[1][..len]);
                output.channels[2][..len].copy_from_slice(&branch_b.channels[0][..len]);
                output.channels[3][..len].copy_from_slice(&branch_b.channels[1][..len]);
            }
            NodeKind::MasterOutput => {
                // Master is always stereo — take the first two channels.
                let mut stereo = AudioBuffer::silent(2, frames);
                for ch in 0..2 {
                    if ch < input.channel_count() {
                        let len = frames.min(input.channels[ch].len());
                        stereo.channels[ch][..len].copy_from_slice(&input.channels[ch][..len]);
                    }
                }
                master.add_from(&stereo);
                output = stereo;
            }
            NodeKind::BuiltinSynth { params } => {
                let midi = collect_midi_for_instrument(plan, *node_id, ctx);
                let events: Vec<MidiNoteEvent> = midi
                    .iter()
                    .filter_map(|ev| {
                        let status = ev.status & 0xf0;
                        let channel = ev.status & 0x0f;
                        if status == 0x90 && ev.data2 > 0 {
                            Some(MidiNoteEvent {
                                sample_offset: ev.sample_offset,
                                note: ev.data1,
                                velocity: ev.data2,
                                channel,
                                on: true,
                            })
                        } else if status == 0x80 || (status == 0x90 && ev.data2 == 0) {
                            Some(MidiNoteEvent {
                                sample_offset: ev.sample_offset,
                                note: ev.data1,
                                velocity: 0,
                                channel,
                                on: false,
                            })
                        } else if status == 0xb0 && (ev.data1 == 123 || ev.data1 == 120) {
                            // All Notes Off / All Sound Off → release every voice.
                            None
                        } else {
                            None
                        }
                    })
                    .collect();
                let panic = midi.iter().any(|ev| {
                    let status = ev.status & 0xf0;
                    status == 0xb0 && (ev.data1 == 123 || ev.data1 == 120)
                });
                let synth = ctx
                    .builtin_synth_state
                    .entry(*node_id)
                    .or_insert_with(|| PolySynth::new(ctx.sample_rate as f32));
                if (synth.sample_rate() - ctx.sample_rate as f32).abs() > 0.5 {
                    synth.set_sample_rate(ctx.sample_rate as f32);
                }
                if panic {
                    synth.all_notes_off(params);
                }
                // Ensure stereo buffers exist.
                if output.channel_count() < 2 {
                    output = AudioBuffer::silent(2, frames);
                }
                let (left, right) = output.channels.split_at_mut(1);
                synth.process_block(params, &events, &mut left[0], &mut right[0]);
            }
            NodeKind::PluginInstrument {
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
            NodeKind::PluginEffect {
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
        let delay = plan.delay_compensation.get(node_id).copied().unwrap_or(0) as usize;
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
    let mut source_tracks = HashSet::new();
    let mut visited = HashSet::new();
    collect_midi_source_tracks(plan, instrument_node, &mut visited, &mut source_tracks);

    let mut events = Vec::new();
    if matches!(ctx.transport, TransportState::Playing) {
        for track_id in &source_tracks {
            events.extend(crate::clips::schedule_midi_for_block(
                ctx.clips,
                *track_id,
                ctx.tempo,
                ctx.block_start,
                ctx.block_len,
            ));
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

/// Walk upstream over MIDI edges through mixers/splitters to find clip sources.
fn collect_midi_source_tracks(
    plan: &CompiledPlan,
    node_id: NodeId,
    visited: &mut HashSet<NodeId>,
    tracks: &mut HashSet<TrackId>,
) {
    if !visited.insert(node_id) {
        return;
    }
    for edge in plan.edges.iter().filter(|e| e.to_node == node_id) {
        let Some(from_node) = plan.nodes.get(&edge.from_node) else {
            continue;
        };
        let Some(from_port) = from_node.find_port(edge.from_port) else {
            continue;
        };
        if from_port.port_type != PortType::Midi {
            continue;
        }
        match &from_node.kind {
            NodeKind::MidiClipSource { track_id } => {
                tracks.insert(*track_id);
            }
            NodeKind::MidiMixer | NodeKind::MidiSplitter => {
                collect_midi_source_tracks(plan, edge.from_node, visited, tracks);
            }
            _ => {}
        }
    }
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
        let mut builtin_synth_state = IndexMap::new();
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
            builtin_synth_state: &mut builtin_synth_state,
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
        let any_solo = plan
            .nodes
            .values()
            .any(|n| matches!(&n.kind, NodeKind::GainPan { solo: true, .. }));
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
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            playing: true,
        };
        let ctx24 = TransportBlockInfo {
            sample_rate: 24_000,
            block_start: SamplePos(0),
            block_len: 48,
            bpm: 120.0,
            time_sig_numerator: 4,
            time_sig_denominator: 4,
            playing: true,
        };
        host.process_instrument(
            crate::ids::PluginInstanceId::new(),
            &midi,
            &mut out_48,
            &ctx48,
        );
        host.process_instrument(
            crate::ids::PluginInstanceId::new(),
            &midi,
            &mut out_24,
            &ctx24,
        );
        // At A440, one period is 48 samples @ 48k and 24 samples @ 24k — peaks should differ.
        assert!(out_48.peak() > 0.0);
        assert!(out_24.peak() > 0.0);
        assert!((out_48.channels[0][24] - out_24.channels[0][24]).abs() > 1e-4);
    }

    #[test]
    fn instrument_effect_chain_reaches_master() {
        struct ChainHost {
            instrument_calls: usize,
            effect_calls: usize,
        }

        impl PluginAudioHost for ChainHost {
            fn process_instrument(
                &mut self,
                _instance: crate::ids::PluginInstanceId,
                _midi: &[ScheduledMidiEvent],
                output: &mut AudioBuffer,
                _ctx: &TransportBlockInfo,
            ) -> bool {
                self.instrument_calls += 1;
                for channel in &mut output.channels {
                    channel.fill(0.25);
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
                self.effect_calls += 1;
                *output = input.clone();
                output.apply_gain(2.0);
                true
            }
        }

        let mut project = Project::new("chain");
        let track = project.add_midi_track("MIDI");
        let (instrument, _) = project
            .attach_instrument(
                track,
                "instrument".into(),
                "/instrument.vst3".into(),
                "Instrument".into(),
            )
            .unwrap();
        let gain = project
            .tracks
            .iter()
            .find(|candidate| candidate.id == track)
            .and_then(|track| track.gain_node)
            .unwrap();
        let effect = project.add_effect(
            "effect".into(),
            "/effect.vst3".into(),
            "Effect".into(),
            [0.0, 0.0],
        );
        project
            .graph
            .edges
            .retain(|_, edge| !(edge.from_node == instrument && edge.to_node == gain));
        project.graph.connect_stereo(instrument, effect).unwrap();
        project.graph.connect_stereo(effect, gain).unwrap();

        let plan = project.compiled_plan();
        let cache = SampleCache::default();
        let mut host = ChainHost {
            instrument_calls: 0,
            effect_calls: 0,
        };
        let mut meters = IndexMap::new();
        let mut pdc_state = IndexMap::new();
        let mut builtin_synth_state = IndexMap::new();
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
            builtin_synth_state: &mut builtin_synth_state,
        };

        let out = process_block(&plan, &mut ctx, &mut meters);

        assert_eq!(host.instrument_calls, 1);
        assert_eq!(host.effect_calls, 1);
        assert!(out.peak() > 0.3);
    }

    #[test]
    fn mixer_accepts_multiple_stereo_inputs() {
        let mut g = AudioGraph::new();
        let a = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "A"));
        let b = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "B"));
        let mix = g.add_node(GraphNode::sum_mixer("Mix"));
        let master = g.add_node(GraphNode::master_output());
        // Strip 1 (In1 L/R) and strip 2 (In2 L/R).
        let mix_in1_l = g.nodes[&mix].inputs[0].id;
        let mix_in1_r = g.nodes[&mix].inputs[1].id;
        let mix_in2_l = g.nodes[&mix].inputs[2].id;
        let mix_in2_r = g.nodes[&mix].inputs[3].id;
        let a_l = g.nodes[&a].outputs[0].id;
        let a_r = g.nodes[&a].outputs[1].id;
        let b_l = g.nodes[&b].outputs[0].id;
        let b_r = g.nodes[&b].outputs[1].id;
        g.connect(a, a_l, mix, mix_in1_l).unwrap();
        g.connect(a, a_r, mix, mix_in1_r).unwrap();
        g.connect(b, b_l, mix, mix_in2_l).unwrap();
        g.connect(b, b_r, mix, mix_in2_r).unwrap();
        g.connect_stereo(mix, master).unwrap();
        let plan = CompiledPlan::compile(&g).unwrap();
        assert!(plan.order.len() >= 4);
        assert_eq!(g.nodes[&mix].inputs.len(), MIXER_STRIP_COUNT * 2);
    }

    #[test]
    fn midi_mixer_and_splitter_route_to_instrument() {
        let mut g = AudioGraph::new();
        let track_a = TrackId::new();
        let track_b = TrackId::new();
        let src_a = g.add_node(GraphNode::midi_clip_source(track_a, "A"));
        let src_b = g.add_node(GraphNode::midi_clip_source(track_b, "B"));
        let mix = g.add_node(GraphNode::midi_mixer("MIDI Mix"));
        let split = g.add_node(GraphNode::midi_splitter("MIDI Split"));
        let synth = g.add_node(GraphNode::builtin_synth("Synth"));
        let master = g.add_node(GraphNode::master_output());

        let mix_in1 = g.nodes[&mix].inputs[0].id;
        let mix_in2 = g.nodes[&mix].inputs[1].id;
        let mix_out = g.nodes[&mix].outputs[0].id;
        let a_out = g.nodes[&src_a].outputs[0].id;
        let b_out = g.nodes[&src_b].outputs[0].id;
        g.connect(src_a, a_out, mix, mix_in1).unwrap();
        g.connect(src_b, b_out, mix, mix_in2).unwrap();
        let split_in = g.nodes[&split].inputs[0].id;
        g.connect(mix, mix_out, split, split_in).unwrap();
        let split_a = g.nodes[&split].outputs[0].id;
        let synth_in = g.nodes[&synth].inputs[0].id;
        g.connect(split, split_a, synth, synth_in).unwrap();
        g.connect_stereo(synth, master).unwrap();

        let plan = CompiledPlan::compile(&g).unwrap();
        assert!(plan.order.contains(&mix));
        assert!(plan.order.contains(&split));
        assert!(plan.order.contains(&synth));

        let mut visited = HashSet::new();
        let mut tracks = HashSet::new();
        collect_midi_source_tracks(&plan, synth, &mut visited, &mut tracks);
        assert!(tracks.contains(&track_a));
        assert!(tracks.contains(&track_b));
    }

    #[test]
    fn splitter_has_two_stereo_branches() {
        let mut g = AudioGraph::new();
        let src = g.add_node(GraphNode::audio_clip_source(TrackId::new(), "Src"));
        let split = g.add_node(GraphNode::stereo_splitter("Split"));
        let g_a = g.add_node(GraphNode::stereo_gain_pan("A"));
        let g_b = g.add_node(GraphNode::stereo_gain_pan("B"));
        let master = g.add_node(GraphNode::master_output());
        g.connect_stereo(src, split).unwrap();
        let a_l = g.nodes[&split].outputs[0].id;
        let a_r = g.nodes[&split].outputs[1].id;
        let b_l = g.nodes[&split].outputs[2].id;
        let b_r = g.nodes[&split].outputs[3].id;
        let ga_l = g.nodes[&g_a].inputs[0].id;
        let ga_r = g.nodes[&g_a].inputs[1].id;
        let gb_l = g.nodes[&g_b].inputs[0].id;
        let gb_r = g.nodes[&g_b].inputs[1].id;
        g.connect(split, a_l, g_a, ga_l).unwrap();
        g.connect(split, a_r, g_a, ga_r).unwrap();
        g.connect(split, b_l, g_b, gb_l).unwrap();
        g.connect(split, b_r, g_b, gb_r).unwrap();
        g.connect_stereo(g_a, master).unwrap();
        g.connect_stereo(g_b, master).unwrap();
        let plan = CompiledPlan::compile(&g).unwrap();
        assert!(plan.order.contains(&split));
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
        let mut builtin_synth_state = IndexMap::new();
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
            builtin_synth_state: &mut builtin_synth_state,
        };
        let out = process_block(&plan, &mut ctx, &mut meters);
        // offset -100 => first output frame maps before buffer start → silence.
        assert_eq!(out.channels[0][0], 0.0);
    }
}
