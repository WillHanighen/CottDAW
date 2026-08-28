//! Monophonic sine sub. Last note wins, held notes stack for hammer-ons.

use serde::{Deserialize, Serialize};

use crate::midi_note_to_hz;

const TAU: f32 = std::f32::consts::TAU;
const DC_R: f32 = 0.995;

/// Knobs. All 0..1 except where noted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BassParams {
    /// Pitch drop on a fresh note, in 0..1 (maps to about 0–7 semitones).
    pub punch: f32,
    /// Portamento time, 0..1 (maps to 0–150 ms).
    pub slide: f32,
    /// Amp release while the note is up. 0..1 (about 80 ms–2.5 s).
    pub length: f32,
    /// Harmonics so the sub still reads on a laptop. Keep this low.
    pub growl: f32,
    pub volume: f32,
}

impl Default for BassParams {
    fn default() -> Self {
        Self {
            punch: 0.28,
            slide: 0.12,
            length: 0.55,
            growl: 0.18,
            volume: 0.85,
        }
    }
}

impl BassParams {
    pub fn clamped(self) -> Self {
        Self {
            punch: self.punch.clamp(0.0, 1.0),
            slide: self.slide.clamp(0.0, 1.0),
            length: self.length.clamp(0.0, 1.0),
            growl: self.growl.clamp(0.0, 1.0),
            volume: self.volume.clamp(0.0, 1.0),
        }
    }

    pub fn slide_seconds(self) -> f32 {
        self.slide * 0.150
    }

    pub fn length_seconds(self) -> f32 {
        0.04 + self.length * 2.46
    }

    pub fn punch_semitones(self) -> f32 {
        self.punch * 7.0
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

#[derive(Debug, Clone, Copy)]
struct Held {
    note: u8,
    vel: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AmpStage {
    Idle,
    Attack,
    Hold,
    Release,
}

/// One-voice sub engine.
#[derive(Debug, Clone)]
pub struct BassEngine {
    sample_rate: f32,
    held: Vec<Held>,
    amp: f32,
    amp_stage: AmpStage,
    vel: f32,
    log_hz: f32,
    target_hz: f32,
    phase: f32,
    punch_env: f32,
    dc_x1: f32,
    dc_y1: f32,
    hp_lp: f32,
    /// Exposed for tests: last instantaneous frequency after punch.
    last_freq: f32,
}

// Keep the sub mono and DC-free. Harmonics ride on the same mono path.

impl Default for BassEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl BassEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            held: Vec::with_capacity(16),
            amp: 0.0,
            amp_stage: AmpStage::Idle,
            vel: 0.0,
            log_hz: midi_note_to_hz(36).ln(),
            target_hz: midi_note_to_hz(36),
            phase: 0.0,
            punch_env: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            hp_lp: 0.0,
            last_freq: midi_note_to_hz(36),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn reset(&mut self) {
        self.held.clear();
        self.amp = 0.0;
        self.amp_stage = AmpStage::Idle;
        self.vel = 0.0;
        self.phase = 0.0;
        self.punch_env = 0.0;
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.hp_lp = 0.0;
    }

    pub fn is_active(&self) -> bool {
        self.amp > 1.0e-4 || !self.held.is_empty()
    }

    pub fn last_freq(&self) -> f32 {
        self.last_freq
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let note = note.min(127);
        let velocity = velocity.min(127);
        if velocity == 0 {
            self.note_off(note);
            return;
        }
        self.held.retain(|h| h.note != note);
        let from_silence = self.held.is_empty() && self.amp < 1.0e-3;
        self.held.push(Held {
            note,
            vel: velocity as f32 / 127.0,
        });
        self.retarget(from_silence);
    }

    pub fn note_off(&mut self, note: u8) {
        let note = note.min(127);
        self.held.retain(|h| h.note != note);
        if self.held.is_empty() {
            if self.amp_stage != AmpStage::Idle {
                self.amp_stage = AmpStage::Release;
            }
        } else {
            self.retarget(false);
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held.clear();
        self.amp_stage = AmpStage::Release;
    }

    fn retarget(&mut self, from_silence: bool) {
        let Some(held) = self.held.last() else {
            return;
        };
        self.target_hz = midi_note_to_hz(held.note).clamp(20.0, 400.0);
        self.vel = held.vel;
        if from_silence {
            self.log_hz = self.target_hz.ln();
            self.punch_env = 1.0;
            self.amp_stage = AmpStage::Attack;
        } else {
            // Legato: keep the amp, no new punch.
            if self.amp_stage == AmpStage::Idle || self.amp_stage == AmpStage::Release {
                self.amp_stage = AmpStage::Attack;
            }
        }
    }

    pub fn process_block(
        &mut self,
        params: &BassParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);

        let sr = self.sample_rate;
        let slide_s = params.slide_seconds();
        // Divide by 3 so `slide` is roughly "time to arrive", not a 1/e time constant.
        let glide_coeff = if slide_s < 0.0005 {
            1.0
        } else {
            1.0 - (-3.0 / (slide_s * sr).max(1.0)).exp()
        };
        let punch_coeff = (-1.0 / (0.025 * sr).max(1.0)).exp();
        let attack_coeff = 1.0 - (-1.0 / (0.004 * sr).max(1.0)).exp();
        let release_coeff = (-1.0 / (params.length_seconds() * sr).max(1.0)).exp();
        let hp_coeff = 1.0 - (-TAU * 20.0 / sr).exp();

        let mut event_i = 0;
        for frame in 0..frames {
            while event_i < events.len() && events[event_i].sample_offset as usize <= frame {
                let ev = events[event_i];
                if ev.on {
                    self.note_on(ev.note, ev.velocity);
                } else {
                    self.note_off(ev.note);
                }
                event_i += 1;
            }

            let target_log = self.target_hz.max(20.0).ln();
            self.log_hz += (target_log - self.log_hz) * glide_coeff;
            let base_hz = self.log_hz.exp();
            self.punch_env *= punch_coeff;
            let freq = base_hz * 2f32.powf(params.punch_semitones() * self.punch_env / 12.0);
            self.last_freq = freq;

            self.phase += freq / sr;
            if self.phase >= 1.0 {
                self.phase -= self.phase.floor();
            }
            let sine = (self.phase * TAU).sin();
            // Cheap triangle from the same phase, then a soft fold for growl.
            let tri = 4.0 * (self.phase - 0.5).abs() - 1.0;
            let growl = params.growl;
            let raw = sine * (1.0 - growl * 0.55) + tri * growl * 0.45;
            let driven = (raw * (1.15 + growl * 0.9)).tanh();

            match self.amp_stage {
                AmpStage::Idle => self.amp = 0.0,
                AmpStage::Attack => {
                    self.amp += (1.0 - self.amp) * attack_coeff;
                    if self.amp > 0.995 {
                        self.amp = 1.0;
                        self.amp_stage = AmpStage::Hold;
                    }
                }
                AmpStage::Hold => self.amp = 1.0,
                AmpStage::Release => {
                    self.amp *= release_coeff;
                    if self.amp < 1.0e-4 {
                        self.amp = 0.0;
                        self.amp_stage = AmpStage::Idle;
                    }
                }
            }

            let mut sample = driven * self.amp * self.vel * params.volume;

            // 20 Hz high-pass so DC does not eat headroom.
            self.hp_lp += hp_coeff * (sample - self.hp_lp);
            sample -= self.hp_lp * 0.15;

            // DC blocker.
            let y = sample - self.dc_x1 + DC_R * self.dc_y1;
            self.dc_x1 = sample;
            self.dc_y1 = y;
            sample = y;

            let out = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            left[frame] = out;
            right[frame] = out;
        }

        while event_i < events.len() {
            let ev = events[event_i];
            if ev.on {
                self.note_on(ev.note, ev.velocity);
            } else {
                self.note_off(ev.note);
            }
            event_i += 1;
        }
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

    fn energy(buf: &[f32]) -> f32 {
        buf.iter().map(|s| s * s).sum()
    }

    fn highpass_energy(buf: &[f32], sr: f32, cutoff: f32) -> f32 {
        let c = 1.0 - (-TAU * cutoff / sr).exp();
        let mut lp = 0.0f32;
        let mut e = 0.0f32;
        for &x in buf {
            lp += c * (x - lp);
            let y = x - lp;
            e += y * y;
        }
        e
    }

    #[test]
    fn note_makes_sound() {
        let mut eng = BassEngine::new(48_000.0);
        let params = BassParams::default();
        let mut l = vec![0.0f32; 2048];
        let mut r = vec![0.0f32; 2048];
        eng.process_block(&params, &[on(0, 36, 120)], &mut l, &mut r);
        assert!(energy(&l) > 1.0e-2);
        assert_eq!(l, r, "sub should be identical L/R");
    }

    #[test]
    fn energy_lives_in_the_low_band() {
        let mut eng = BassEngine::new(48_000.0);
        let mut params = BassParams::default();
        params.growl = 0.0;
        params.punch = 0.0;
        params.slide = 0.0;
        let mut l = vec![0.0f32; 8192];
        let mut r = vec![0.0f32; 8192];
        eng.process_block(&params, &[on(0, 24, 127)], &mut l, &mut r);
        let total = energy(&l);
        let high = highpass_energy(&l, 48_000.0, 200.0);
        assert!(total > 0.0);
        assert!(
            high / total < 0.25,
            "too much energy above 200 Hz: high {high} total {total}"
        );
    }

    #[test]
    fn slide_moves_pitch() {
        let mut eng = BassEngine::new(48_000.0);
        let mut params = BassParams::default();
        params.slide = 1.0;
        params.punch = 0.0;
        params.length = 1.0;
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        eng.process_block(&params, &[on(0, 36, 120)], &mut l, &mut r);
        let start = eng.last_freq();
        // Legato up a fifth while still holding the first note.
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        eng.process_block(&params, &[on(0, 43, 120)], &mut l, &mut r);
        let mid = eng.last_freq();
        let mut l = vec![0.0f32; 16_000];
        let mut r = vec![0.0f32; 16_000];
        eng.process_block(&params, &[], &mut l, &mut r);
        let end = eng.last_freq();
        let target = midi_note_to_hz(43);
        assert!(
            mid > start,
            "glide should have started moving, start {start} mid {mid}"
        );
        assert!(
            (end - target).abs() < 2.0,
            "should arrive near {target} Hz, got {end}"
        );
    }

    #[test]
    fn release_goes_quiet() {
        let mut eng = BassEngine::new(48_000.0);
        let mut params = BassParams::default();
        params.length = 0.0;
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        eng.process_block(&params, &[on(0, 36, 120)], &mut l, &mut r);
        let mut l = vec![0.0f32; 48_000];
        let mut r = vec![0.0f32; 48_000];
        eng.process_block(&params, &[off(0, 36)], &mut l, &mut r);
        let tail = &l[40_000..];
        assert!(energy(tail) < 1.0e-4, "tail energy {}", energy(tail));
        assert!(!eng.is_active());
    }

    #[test]
    fn hammer_on_returns_to_held_note() {
        let mut eng = BassEngine::new(48_000.0);
        let mut params = BassParams::default();
        params.slide = 0.0;
        params.punch = 0.0;
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        eng.process_block(&params, &[on(0, 36, 100), on(0, 40, 100)], &mut l, &mut r);
        assert!((eng.last_freq() - midi_note_to_hz(40)).abs() < 1.0);
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        eng.process_block(&params, &[off(0, 40)], &mut l, &mut r);
        assert!((eng.last_freq() - midi_note_to_hz(36)).abs() < 1.0);
    }
}
