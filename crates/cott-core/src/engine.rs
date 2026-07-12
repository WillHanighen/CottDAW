//! Realtime-safe engine messaging and offline renderer.

use crate::clips::ScheduledMidiEvent;
use crate::dsp::{
    AudioBuffer, MeterState, NullPluginHost, PluginAudioHost, ProcessContext, SampleCache,
    process_block,
};
use crate::graph::CompiledPlan;
use crate::ids::{NodeId, TrackId};
use crate::project::Project;
use crate::time::{SamplePos, TempoMap, TransportState};
use indexmap::IndexMap;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering};

#[derive(Debug, Clone)]
pub enum EngineCommand {
    SetPlan(Arc<CompiledPlan>),
    SetTransport(TransportState),
    Seek(SamplePos),
    SetTempo(TempoMap),
    SetLoop {
        enabled: bool,
        start: SamplePos,
        end: SamplePos,
    },
    UpdateClips(Arc<Vec<crate::clips::Clip>>),
    UpdateAutomation(Arc<Vec<crate::automation::AutomationLane>>),
    UpdateSampleCache(Arc<SampleCache>),
    /// One-shot MIDI audition (piano roll click / drag pitch feedback).
    PreviewNote {
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        duration_samples: u32,
    },
}

#[derive(Debug, Clone)]
struct NotePreview {
    track_id: TrackId,
    pitch: u8,
    velocity: u8,
    channel: u8,
    /// Samples remaining until note-off after note-on was sent.
    samples_left: i64,
    sent_on: bool,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Position(SamplePos),
    Meters(IndexMap<NodeId, MeterState>),
    XRun,
    Underrun,
}

/// Shared transport clocks readable from UI without locking the audio thread hard.
pub struct SharedTransport {
    pub playing: AtomicU8, // 0 stopped, 1 playing, 2 paused
    pub position: AtomicI64,
    pub sample_rate: AtomicU8, // unused placeholder; real SR in config
}

impl SharedTransport {
    pub fn new() -> Self {
        Self {
            playing: AtomicU8::new(0),
            position: AtomicI64::new(0),
            sample_rate: AtomicU8::new(0),
        }
    }

    pub fn state(&self) -> TransportState {
        match self.playing.load(Ordering::Relaxed) {
            1 => TransportState::Playing,
            2 => TransportState::Paused,
            _ => TransportState::Stopped,
        }
    }

    pub fn set_state(&self, state: TransportState) {
        let v = match state {
            TransportState::Stopped => 0,
            TransportState::Playing => 1,
            TransportState::Paused => 2,
        };
        self.playing.store(v, Ordering::Relaxed);
    }

    pub fn position(&self) -> SamplePos {
        SamplePos(self.position.load(Ordering::Relaxed))
    }

    pub fn set_position(&self, pos: SamplePos) {
        self.position.store(pos.0, Ordering::Relaxed);
    }
}

impl Default for SharedTransport {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EngineChannels {
    pub cmd_tx: Producer<EngineCommand>,
    pub cmd_rx: Consumer<EngineCommand>,
    pub evt_tx: Producer<EngineEvent>,
    pub evt_rx: Consumer<EngineEvent>,
}

pub fn create_engine_channels(
    capacity: usize,
) -> (
    Producer<EngineCommand>,
    Consumer<EngineCommand>,
    Producer<EngineEvent>,
    Consumer<EngineEvent>,
) {
    let (cmd_tx, cmd_rx) = RingBuffer::new(capacity);
    let (evt_tx, evt_rx) = RingBuffer::new(capacity);
    (cmd_tx, cmd_rx, evt_tx, evt_rx)
}

/// Audio-thread processor state.
pub struct AudioProcessor {
    pub plan: Arc<CompiledPlan>,
    pub tempo: TempoMap,
    pub transport: TransportState,
    pub position: SamplePos,
    pub loop_enabled: bool,
    pub loop_start: SamplePos,
    pub loop_end: SamplePos,
    pub clips: Arc<Vec<crate::clips::Clip>>,
    pub automation: Arc<Vec<crate::automation::AutomationLane>>,
    pub sample_cache: Arc<SampleCache>,
    pub meters: IndexMap<NodeId, MeterState>,
    pub shared: Arc<SharedTransport>,
    pub running: Arc<AtomicBool>,
    previews: Vec<NotePreview>,
    /// Note-offs queued when a preview is replaced/cancelled.
    pending_preview_midi: Vec<(TrackId, ScheduledMidiEvent)>,
    /// Persistent PDC delay rings keyed by node.
    pdc_state: IndexMap<NodeId, Vec<Vec<f32>>>,
}

impl AudioProcessor {
    pub fn new(shared: Arc<SharedTransport>) -> Self {
        Self {
            plan: Arc::new(CompiledPlan::empty()),
            tempo: TempoMap::default(),
            transport: TransportState::Stopped,
            position: SamplePos(0),
            loop_enabled: false,
            loop_start: SamplePos(0),
            loop_end: SamplePos(0),
            clips: Arc::new(Vec::new()),
            automation: Arc::new(Vec::new()),
            sample_cache: Arc::new(SampleCache::default()),
            meters: IndexMap::new(),
            shared,
            running: Arc::new(AtomicBool::new(true)),
            previews: Vec::new(),
            pending_preview_midi: Vec::new(),
            pdc_state: IndexMap::new(),
        }
    }

    pub fn handle_commands(&mut self, rx: &mut Consumer<EngineCommand>) {
        while let Ok(cmd) = rx.pop() {
            match cmd {
                EngineCommand::SetPlan(plan) => {
                    // Drop PDC state for removed nodes; keep rings for survivors.
                    self.pdc_state
                        .retain(|id, _| plan.nodes.contains_key(id));
                    self.plan = plan;
                }
                EngineCommand::SetTransport(state) => {
                    self.transport = state;
                    self.shared.set_state(state);
                    if state == TransportState::Stopped {
                        self.position = SamplePos(0);
                        self.shared.set_position(self.position);
                    }
                }
                EngineCommand::Seek(pos) => {
                    self.position = pos;
                    self.shared.set_position(pos);
                }
                EngineCommand::SetTempo(tempo) => self.tempo = tempo,
                EngineCommand::SetLoop {
                    enabled,
                    start,
                    end,
                } => {
                    self.loop_enabled = enabled;
                    self.loop_start = start;
                    self.loop_end = end;
                }
                EngineCommand::UpdateClips(clips) => self.clips = clips,
                EngineCommand::UpdateAutomation(a) => self.automation = a,
                EngineCommand::UpdateSampleCache(c) => self.sample_cache = c,
                EngineCommand::PreviewNote {
                    track_id,
                    pitch,
                    velocity,
                    duration_samples,
                } => {
                    self.start_preview(track_id, pitch, velocity, duration_samples);
                }
            }
        }
    }

    fn start_preview(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        duration_samples: u32,
    ) {
        // Retrigger: note-off any active previews on this track first.
        let mut i = 0;
        while i < self.previews.len() {
            if self.previews[i].track_id == track_id {
                let old = self.previews.remove(i);
                if old.sent_on {
                    self.pending_preview_midi.push((
                        track_id,
                        ScheduledMidiEvent::note_off(0, old.pitch, old.channel),
                    ));
                }
            } else {
                i += 1;
            }
        }
        self.previews.push(NotePreview {
            track_id,
            pitch: pitch.min(127),
            velocity: velocity.min(127).max(1),
            channel: 0,
            samples_left: duration_samples.max(1) as i64,
            sent_on: false,
        });
    }

    /// Advance preview voices and collect MIDI for this block.
    fn tick_previews(&mut self, block_len: u32) -> Vec<(TrackId, ScheduledMidiEvent)> {
        let mut events = std::mem::take(&mut self.pending_preview_midi);
        let mut i = 0;
        while i < self.previews.len() {
            let p = &mut self.previews[i];
            if !p.sent_on {
                events.push((
                    p.track_id,
                    ScheduledMidiEvent::note_on(0, p.pitch, p.velocity, p.channel),
                ));
                p.sent_on = true;
            }
            if p.samples_left <= block_len as i64 {
                let off_at = p.samples_left.max(0) as u32;
                let off_at = off_at.min(block_len.saturating_sub(1));
                events.push((
                    p.track_id,
                    ScheduledMidiEvent::note_off(off_at, p.pitch, p.channel),
                ));
                self.previews.remove(i);
            } else {
                p.samples_left -= block_len as i64;
                i += 1;
            }
        }
        events.sort_by_key(|(_, e)| e.sample_offset);
        events
    }

    pub fn process(
        &mut self,
        output: &mut [f32],
        channels: usize,
        frames: usize,
        sample_rate: u32,
        plugin_host: &mut dyn PluginAudioHost,
        evt_tx: &mut Producer<EngineEvent>,
    ) {
        if channels == 0 || frames == 0 {
            return;
        }
        let playing = self.transport == TransportState::Playing;
        let had_preview_work =
            !self.previews.is_empty() || !self.pending_preview_midi.is_empty();
        let preview_midi = self.tick_previews(frames as u32);
        let audition = had_preview_work || !preview_midi.is_empty();

        let rendered = if playing || audition {
            let host_ref = plugin_host;
            let mut ctx = ProcessContext {
                sample_rate,
                block_start: self.position,
                block_len: frames as u32,
                tempo: &self.tempo,
                transport: self.transport,
                clips: &self.clips,
                sample_cache: &self.sample_cache,
                automation: &self.automation,
                plugin_audio: host_ref,
                preview_midi: &preview_midi,
                pdc_state: &mut self.pdc_state,
            };
            process_block(&self.plan, &mut ctx, &mut self.meters)
        } else {
            AudioBuffer::silent(2, frames)
        };

        // Interleave into output.
        for i in 0..frames {
            for ch in 0..channels {
                let sample = rendered
                    .channels
                    .get(ch)
                    .and_then(|c| c.get(i))
                    .copied()
                    .unwrap_or(0.0);
                output[i * channels + ch] = sample;
            }
        }

        if playing {
            self.position = self.position.saturating_add(frames as i64);
            if self.loop_enabled && self.loop_end.0 > self.loop_start.0 {
                if self.position.0 >= self.loop_end.0 {
                    self.position = self.loop_start;
                }
            }
            self.shared.set_position(self.position);
            let _ = evt_tx.push(EngineEvent::Position(self.position));
            let _ = evt_tx.push(EngineEvent::Meters(self.meters.clone()));
        } else if audition {
            let _ = evt_tx.push(EngineEvent::Meters(self.meters.clone()));
        }
    }
}

/// Offline bounce of the project to planar stereo at project sample rate.
pub fn render_offline(
    project: &Project,
    sample_cache: &SampleCache,
    plugin_host: &mut dyn PluginAudioHost,
    length_samples: i64,
    block_size: u32,
) -> AudioBuffer {
    let plan = project.compiled_plan();
    let mut meters = IndexMap::new();
    let mut pdc_state = IndexMap::new();
    let mut position = SamplePos(0);
    let mut out = AudioBuffer::silent(2, length_samples.max(0) as usize);
    while position.0 < length_samples {
        let remaining = (length_samples - position.0) as u32;
        let n = remaining.min(block_size);
        let mut ctx = ProcessContext {
            sample_rate: project.tempo.sample_rate,
            block_start: position,
            block_len: n,
            tempo: &project.tempo,
            transport: TransportState::Playing,
            clips: &project.clips,
            sample_cache,
            automation: &project.automation,
            plugin_audio: plugin_host,
            preview_midi: &[],
            pdc_state: &mut pdc_state,
        };
        let block = process_block(&plan, &mut ctx, &mut meters);
        let start = position.0 as usize;
        for ch in 0..2 {
            for i in 0..n as usize {
                if start + i < out.frames() {
                    out.channels[ch][start + i] = block.channels[ch][i];
                }
            }
        }
        position = position.saturating_add(n as i64);
    }
    out
}

/// Convenience: sync project state into engine commands (UI thread).
pub fn push_project_snapshot(
    tx: &mut Producer<EngineCommand>,
    project: &Project,
    sample_cache: Arc<SampleCache>,
) {
    match project.try_compiled_plan() {
        Ok(plan) => {
            if tx.push(EngineCommand::SetPlan(Arc::new(plan))).is_err() {
                tracing::warn!("engine command ring full; dropped SetPlan");
            }
        }
        Err(e) => {
            tracing::warn!("skipping SetPlan; graph compile failed: {e}");
        }
    }
    let cmds = [
        EngineCommand::SetTempo(project.tempo.clone()),
        EngineCommand::UpdateClips(Arc::new(project.clips.clone())),
        EngineCommand::UpdateAutomation(Arc::new(project.automation.clone())),
        EngineCommand::UpdateSampleCache(sample_cache),
        EngineCommand::SetLoop {
            enabled: project.loop_enabled,
            start: project
                .tempo
                .beat_to_sample(crate::time::BeatPos(project.loop_start_beats)),
            end: project
                .tempo
                .beat_to_sample(crate::time::BeatPos(project.loop_end_beats)),
        },
    ];
    for cmd in cmds {
        if tx.push(cmd).is_err() {
            tracing::warn!("engine command ring full; dropped project snapshot update");
            break;
        }
    }
}

pub fn render_with_null_plugins(
    project: &Project,
    sample_cache: &SampleCache,
    length_samples: i64,
) -> AudioBuffer {
    let mut host = NullPluginHost;
    render_offline(project, sample_cache, &mut host, length_samples, 512)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clips::{Clip, MidiNote};

    #[test]
    fn offline_render_produces_audio_for_midi() {
        let mut project = Project::new("t");
        let track = project.add_midi_track("Synth");
        // Attach stub instrument path via gain only — NullPluginHost tones on MIDI
        // through instrument nodes. Add a fake instrument node.
        let _ = project.attach_instrument(track, "stub".into(), "/dev/null".into(), "Stub".into());
        let mut clip = Clip::new_midi(track, "C", 0.0, 2.0);
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(60, 100, 0.0, 1.0));
        project.clips.push(clip);
        let cache = SampleCache::default();
        let out = render_with_null_plugins(&project, &cache, 48_000);
        assert!(out.peak() > 0.0);
    }
}
