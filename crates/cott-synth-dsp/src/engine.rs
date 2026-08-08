use crate::adsr::{AdsrParams, AdsrState};
use crate::midi_note_to_hz;
use crate::oscillator::{Waveform, sample_waveform};
use serde::{Deserialize, Serialize};

/// Maximum simultaneous voices per synth instance.
pub const MAX_VOICES: usize = 16;

/// User-facing synth parameters (persisted on the graph node / plugin state).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SynthParams {
    pub waveform: Waveform,
    pub adsr: AdsrParams,
    /// Pulse duty cycle in `[0, 1]` (only for [`Waveform::Pulse`]).
    pub pulse_width: f32,
    /// Linear output gain.
    pub gain: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Sine,
            adsr: AdsrParams::default(),
            pulse_width: 0.25,
            gain: 0.25,
        }
    }
}

impl SynthParams {
    pub fn clamped(self) -> Self {
        Self {
            waveform: self.waveform,
            adsr: self.adsr.clamped(),
            pulse_width: self.pulse_width.clamp(0.05, 0.95),
            gain: self.gain.clamp(0.0, 1.0),
        }
    }
}

/// Sample-accurate MIDI note event relative to the current block.
#[derive(Debug, Clone, Copy)]
pub struct MidiNoteEvent {
    pub sample_offset: u32,
    pub note: u8,
    pub velocity: u8,
    pub channel: u8,
    pub on: bool,
}

#[derive(Debug, Clone)]
struct Voice {
    note: u8,
    channel: u8,
    velocity: f32,
    phase: f32,
    phase_inc: f32,
    envelope: AdsrState,
    noise_state: u32,
    /// Monotonic age for voice stealing (lower = older).
    age: u64,
}

/// Polyphonic CottSynth engine — one instance per graph/plugin node.
#[derive(Debug, Clone)]
pub struct PolySynth {
    voices: [Option<Voice>; MAX_VOICES],
    next_age: u64,
    sample_rate: f32,
}

impl Default for PolySynth {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl PolySynth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: std::array::from_fn(|_| None),
            next_age: 1,
            sample_rate: sample_rate.max(1.0),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_some()).count()
    }

    pub fn reset(&mut self) {
        self.voices.fill(None);
        self.next_age = 1;
    }

    pub fn all_notes_off(&mut self, params: &SynthParams) {
        let params = params.clamped();
        for slot in &mut self.voices {
            if let Some(voice) = slot {
                voice.envelope.note_off(&params.adsr, self.sample_rate);
            }
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8, params: &SynthParams) {
        let note = note.min(127);
        let velocity = velocity.min(127);
        if velocity == 0 {
            self.note_off(note, channel, params);
            return;
        }
        let params = params.clamped();

        // Retrigger same note/channel.
        if let Some(slot) = self
            .voices
            .iter_mut()
            .find(|v| v.as_ref().is_some_and(|voice| voice.note == note && voice.channel == channel))
        {
            if let Some(voice) = slot {
                voice.velocity = velocity as f32 / 127.0;
                voice.phase = 0.0;
                voice.phase_inc = midi_note_to_hz(note) / self.sample_rate;
                voice.envelope.note_on(&params.adsr, self.sample_rate);
                voice.age = self.next_age;
                self.next_age = self.next_age.wrapping_add(1);
                return;
            }
        }

        let idx = self
            .voices
            .iter()
            .position(|v| v.is_none())
            .unwrap_or_else(|| self.steal_voice_index());

        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);
        let mut envelope = AdsrState::default();
        envelope.note_on(&params.adsr, self.sample_rate);
        self.voices[idx] = Some(Voice {
            note,
            channel: channel & 0x0f,
            velocity: velocity as f32 / 127.0,
            phase: 0.0,
            phase_inc: midi_note_to_hz(note) / self.sample_rate,
            envelope,
            noise_state: 0xA341_316C ^ (note as u32).wrapping_mul(0x9E37_79B9),
            age,
        });
    }

    pub fn note_off(&mut self, note: u8, channel: u8, params: &SynthParams) {
        let params = params.clamped();
        let note = note.min(127);
        let channel = channel & 0x0f;
        for slot in &mut self.voices {
            if let Some(voice) = slot
                && voice.note == note
                && voice.channel == channel
            {
                voice.envelope.note_off(&params.adsr, self.sample_rate);
            }
        }
    }

    /// Handle a block of sample-accurate note events and render stereo (identical L/R).
    pub fn process_block(
        &mut self,
        params: &SynthParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);

        let mut event_i = 0;
        for frame in 0..frames {
            while event_i < events.len() && events[event_i].sample_offset as usize <= frame {
                let ev = events[event_i];
                if ev.on {
                    self.note_on(ev.note, ev.velocity, ev.channel, &params);
                } else {
                    self.note_off(ev.note, ev.channel, &params);
                }
                event_i += 1;
            }

            let mut mix = 0.0f32;
            for slot in &mut self.voices {
                let Some(voice) = slot else { continue };
                let env = voice.envelope.next_sample(&params.adsr, self.sample_rate);
                if !voice.envelope.is_active() {
                    *slot = None;
                    continue;
                }
                let sample = sample_waveform(
                    params.waveform,
                    voice.phase,
                    params.pulse_width,
                    &mut voice.noise_state,
                );
                mix += sample * env * voice.velocity;
                voice.phase += voice.phase_inc;
                if voice.phase >= 1.0 {
                    voice.phase -= voice.phase.floor();
                }
            }

            let out = (mix * params.gain).clamp(-1.0, 1.0);
            left[frame] = out;
            right[frame] = out;
        }

        // Drain any remaining events past the block (still apply note state).
        while event_i < events.len() {
            let ev = events[event_i];
            if ev.on {
                self.note_on(ev.note, ev.velocity, ev.channel, &params);
            } else {
                self.note_off(ev.note, ev.channel, &params);
            }
            event_i += 1;
        }
    }

    fn steal_voice_index(&self) -> usize {
        // Prefer a voice already in release; otherwise steal the oldest.
        let mut best = 0usize;
        let mut best_age = u64::MAX;
        let mut best_releasing = false;
        for (i, slot) in self.voices.iter().enumerate() {
            let Some(voice) = slot else {
                return i;
            };
            let releasing = matches!(
                voice.envelope.stage(),
                crate::adsr::AdsrStage::Release
            );
            if releasing && !best_releasing {
                best = i;
                best_age = voice.age;
                best_releasing = true;
            } else if releasing == best_releasing && voice.age < best_age {
                best = i;
                best_age = voice.age;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(offset: u32, note: u8, vel: u8) -> MidiNoteEvent {
        MidiNoteEvent {
            sample_offset: offset,
            note,
            velocity: vel,
            channel: 0,
            on: true,
        }
    }

    fn off(offset: u32, note: u8) -> MidiNoteEvent {
        MidiNoteEvent {
            sample_offset: offset,
            note,
            velocity: 0,
            channel: 0,
            on: false,
        }
    }

    #[test]
    fn polyphony_allows_multiple_notes() {
        let mut synth = PolySynth::new(48_000.0);
        let params = SynthParams::default();
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        synth.process_block(
            &params,
            &[on(0, 60, 100), on(0, 64, 100), on(0, 67, 100)],
            &mut l,
            &mut r,
        );
        assert_eq!(synth.active_voices(), 3);
        assert!(l.iter().any(|s| s.abs() > 1e-4));
    }

    #[test]
    fn voice_stealing_caps_at_max() {
        let mut synth = PolySynth::new(48_000.0);
        let params = SynthParams::default();
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        let events: Vec<_> = (0..MAX_VOICES + 4)
            .map(|i| on(0, (48 + i) as u8, 100))
            .collect();
        synth.process_block(&params, &events, &mut l, &mut r);
        assert!(synth.active_voices() <= MAX_VOICES);
    }

    #[test]
    fn note_off_releases() {
        let mut synth = PolySynth::new(1_000.0);
        let params = SynthParams {
            adsr: AdsrParams {
                attack_ms: 0.0,
                decay_ms: 0.0,
                sustain: 1.0,
                release_ms: 5.0,
            },
            ..SynthParams::default()
        };
        let mut l = vec![0.0f32; 8];
        let mut r = vec![0.0f32; 8];
        synth.process_block(&params, &[on(0, 60, 100)], &mut l, &mut r);
        assert_eq!(synth.active_voices(), 1);
        // Long enough for release to finish.
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        synth.process_block(&params, &[off(0, 60)], &mut l, &mut r);
        assert_eq!(synth.active_voices(), 0);
    }
}
