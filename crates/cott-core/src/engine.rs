//! Realtime-safe engine messaging and offline renderer.

use crate::clips::{midi_panic_events, notes_held_at, ScheduledMidiEvent};
use crate::dsp::{
    AudioBuffer, MeterState, NullPluginHost, PluginAudioHost, ProcessContext, SampleCache,
    process_block,
};
use crate::graph::{CompiledPlan, NodeKind};
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
    /// All-notes/sound-off queued on transport discontinuities (stop/seek/loop).
    pending_panic_midi: Vec<(TrackId, ScheduledMidiEvent)>,
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
            pending_panic_midi: Vec::new(),
            pdc_state: IndexMap::new(),
        }
    }

    /// Silence every instrument voice on a transport discontinuity.
    ///
    /// Arrangement MIDI is streaming — note-offs may never be delivered if
    /// transport stops, seeks, or loops mid-note. Real VST3 plugins also ignore
    /// CC 120/123 (truce-rack drops ControlChange), so we emit **real NoteOff**
    /// for every currently held arrangement/preview note, plus CC panic for
    /// workers that expand CC into note-offs.
    fn queue_midi_panic(&mut self) {
        self.pending_panic_midi.clear();

        // Release active piano-roll previews with real note-offs.
        for p in self.previews.drain(..) {
            if p.sent_on {
                self.pending_panic_midi.push((
                    p.track_id,
                    ScheduledMidiEvent::note_off(0, p.pitch, p.channel),
                ));
            }
        }
        self.pending_preview_midi.clear();

        let mut tracks = Vec::new();
        for node in self.plan.nodes.values() {
            if let NodeKind::MidiClipSource { track_id } = &node.kind {
                if !tracks.contains(track_id) {
                    tracks.push(*track_id);
                }
            }
        }

        // Hold position used for "which notes are latched" — prefer the sample
        // just before a loop wrap so notes ending exactly on the boundary are
        // still considered held if their off was never delivered.
        let hold_pos = SamplePos(self.position.0.saturating_sub(1));

        for track_id in tracks {
            for (pitch, channel) in notes_held_at(&self.clips, track_id, &self.tempo, hold_pos) {
                self.pending_panic_midi.push((
                    track_id,
                    ScheduledMidiEvent::note_off(0, pitch, channel),
                ));
            }
            // Nuclear fallback: note-off every pitch on ch 0. Covers overlapping
            // clip duplicates / stolen voices that notes_held_at may miss.
            // 128 events × tracks still fits comfortably under MAX_MIDI_EVENTS
            // for typical session sizes; we cap tracks if needed below.
            for pitch in 0u8..=127 {
                self.pending_panic_midi.push((
                    track_id,
                    ScheduledMidiEvent::note_off(0, pitch, 0),
                ));
            }
            self.pending_panic_midi
                .extend(midi_panic_events(track_id));
        }

        // Keep SHM MIDI under the 512-event cap (leave room for same-block note-ons).
        const MAX_PANIC_EVENTS: usize = 480;
        if self.pending_panic_midi.len() > MAX_PANIC_EVENTS {
            self.pending_panic_midi.truncate(MAX_PANIC_EVENTS);
        }
    }

    pub fn handle_commands(&mut self, rx: &mut Consumer<EngineCommand>) {
        while let Ok(cmd) = rx.pop() {
            match cmd {
                EngineCommand::SetPlan(plan) => {
                    // Existing delay rings encode the old plan's latency.
                    self.pdc_state.clear();
                    self.plan = plan;
                }
                EngineCommand::SetTransport(state) => {
                    let leaving_play =
                        self.transport == TransportState::Playing && state != TransportState::Playing;
                    if leaving_play || state == TransportState::Stopped {
                        self.queue_midi_panic();
                    }
                    // Entering play: kill any dangling preview voices so they
                    // can't stack with arrangement MIDI (orphan voice hang).
                    if state == TransportState::Playing && !self.previews.is_empty() {
                        for p in self.previews.drain(..) {
                            if p.sent_on {
                                self.pending_panic_midi.push((
                                    p.track_id,
                                    ScheduledMidiEvent::note_off(0, p.pitch, p.channel),
                                ));
                            }
                        }
                        self.pending_preview_midi.clear();
                    }
                    self.transport = state;
                    self.shared.set_state(state);
                    if state == TransportState::Stopped {
                        self.position = SamplePos(0);
                        self.shared.set_position(self.position);
                    }
                }
                EngineCommand::Seek(pos) => {
                    // Seeking orphans any in-flight note-offs from streaming MIDI.
                    self.queue_midi_panic();
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
        // Preview + arrangement on the same pitch creates two note-ons and one
        // note-off → orphan voice that hangs after a partial release.
        if self.transport == TransportState::Playing {
            return;
        }
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
        events.sort_by(|(_, a), (_, b)| {
            a.sample_offset
                .cmp(&b.sample_offset)
                .then_with(|| a.sort_priority().cmp(&b.sort_priority()))
        });
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
        // Drain panic MIDI first so instruments receive CC 120/123 even when
        // transport is already Stopped (graph would otherwise be skipped).
        let mut preview_midi = std::mem::take(&mut self.pending_panic_midi);
        let flush_panic = !preview_midi.is_empty();
        let had_preview_work =
            !self.previews.is_empty() || !self.pending_preview_midi.is_empty();
        preview_midi.extend(self.tick_previews(frames as u32));
        preview_midi.sort_by(|(_, a), (_, b)| {
            a.sample_offset
                .cmp(&b.sample_offset)
                .then_with(|| a.sort_priority().cmp(&b.sort_priority()))
        });
        let audition = had_preview_work || !preview_midi.is_empty() || flush_panic;

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
                    // Loop jump orphans streaming note-offs — panic before next block.
                    self.queue_midi_panic();
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
    use crate::clips::{Clip, MidiNote, ScheduledMidiEvent};
    use crate::ids::PluginInstanceId;

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

    /// Host that records MIDI delivered to instruments (for panic flush tests).
    struct RecordingHost {
        last_midi: Vec<ScheduledMidiEvent>,
    }

    impl PluginAudioHost for RecordingHost {
        fn process_instrument(
            &mut self,
            _instance: PluginInstanceId,
            midi: &[ScheduledMidiEvent],
            output: &mut AudioBuffer,
            _ctx: &crate::dsp::TransportBlockInfo,
        ) -> bool {
            self.last_midi = midi.to_vec();
            output.clear();
            true
        }

        fn process_effect(
            &mut self,
            _instance: PluginInstanceId,
            input: &AudioBuffer,
            output: &mut AudioBuffer,
            _ctx: &crate::dsp::TransportBlockInfo,
        ) -> bool {
            *output = input.clone();
            true
        }
    }

    #[test]
    fn stop_flushes_midi_panic_to_instruments() {
        let mut project = Project::new("panic");
        let track = project.add_midi_track("Synth");
        let _ =
            project.attach_instrument(track, "stub".into(), "/dev/null".into(), "Stub".into());

        let shared = Arc::new(SharedTransport::new());
        let mut proc = AudioProcessor::new(shared);
        proc.plan = Arc::new(project.compiled_plan());
        proc.clips = Arc::new(project.clips.clone());
        proc.transport = TransportState::Playing;

        let (mut cmd_tx, mut cmd_rx) = RingBuffer::new(8);
        let (mut evt_tx, _evt_rx) = RingBuffer::new(16);
        let mut host = RecordingHost {
            last_midi: Vec::new(),
        };
        let mut out = vec![0.0f32; 512 * 2];

        // Stop mid-note: arrangement note-offs are gated off when not Playing,
        // so without panic the instrument would keep sounding.
        let _ = cmd_tx.push(EngineCommand::SetTransport(TransportState::Stopped));
        proc.handle_commands(&mut cmd_rx);
        proc.process(&mut out, 2, 512, 48_000, &mut host, &mut evt_tx);

        assert!(
            host.last_midi
                .iter()
                .any(|e| e.status & 0xf0 == 0xb0 && e.data1 == 123),
            "expected All Notes Off (CC 123) after stop, got {:?}",
            host.last_midi
        );
        assert!(
            host.last_midi
                .iter()
                .any(|e| e.status & 0xf0 == 0xb0 && e.data1 == 120),
            "expected All Sound Off (CC 120) after stop, got {:?}",
            host.last_midi
        );
        // Real VST3 plugins ignore CC — host must also send NoteOff events.
        assert!(
            host.last_midi
                .iter()
                .filter(|e| e.status & 0xf0 == 0x80)
                .count()
                >= 128,
            "expected NoteOff storm for VST3 (got {} note-offs)",
            host.last_midi
                .iter()
                .filter(|e| e.status & 0xf0 == 0x80)
                .count()
        );
    }

    #[test]
    fn seek_queues_midi_panic() {
        let mut project = Project::new("seek");
        let track = project.add_midi_track("Synth");
        let _ =
            project.attach_instrument(track, "stub".into(), "/dev/null".into(), "Stub".into());

        let shared = Arc::new(SharedTransport::new());
        let mut proc = AudioProcessor::new(shared);
        proc.plan = Arc::new(project.compiled_plan());

        let (mut cmd_tx, mut cmd_rx) = RingBuffer::new(8);
        let (mut evt_tx, _evt_rx) = RingBuffer::new(16);
        let mut host = RecordingHost {
            last_midi: Vec::new(),
        };
        let mut out = vec![0.0f32; 256 * 2];

        let _ = cmd_tx.push(EngineCommand::Seek(SamplePos(1000)));
        proc.handle_commands(&mut cmd_rx);
        proc.process(&mut out, 2, 256, 48_000, &mut host, &mut evt_tx);

        assert!(
            host.last_midi
                .iter()
                .any(|e| e.status & 0xf0 == 0xb0 && e.data1 == 123),
            "expected panic MIDI after seek"
        );
    }
}
