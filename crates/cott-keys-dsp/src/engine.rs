//! Polyphonic keys / bells / pluck.

use serde::{Deserialize, Serialize};

use crate::midi_note_to_hz;

const TAU: f32 = std::f32::consts::TAU;

/// Maximum simultaneous voices.
pub const MAX_VOICES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VoiceKind {
    #[default]
    Keys,
    Bells,
    Pluck,
}

impl VoiceKind {
    pub const ALL: [VoiceKind; 3] = [VoiceKind::Keys, VoiceKind::Bells, VoiceKind::Pluck];

    pub fn label(self) -> &'static str {
        match self {
            VoiceKind::Keys => "Keys",
            VoiceKind::Bells => "Bells",
            VoiceKind::Pluck => "Pluck",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeysParams {
    pub kind: VoiceKind,
    /// Amp decay 0..1 (short to long).
    pub decay: f32,
    /// Low-pass. Not hiss.
    pub warmth: f32,
    /// Two short modulated delays.
    pub chorus: f32,
    pub volume: f32,
}

impl Default for KeysParams {
    fn default() -> Self {
        Self {
            kind: VoiceKind::Keys,
            decay: 0.48,
            warmth: 0.22,
            chorus: 0.28,
            volume: 0.70,
        }
    }
}

impl KeysParams {
    pub fn clamped(self) -> Self {
        Self {
            kind: self.kind,
            decay: self.decay.clamp(0.0, 1.0),
            warmth: self.warmth.clamp(0.0, 1.0),
            chorus: self.chorus.clamp(0.0, 1.0),
            volume: self.volume.clamp(0.0, 1.0),
        }
    }

    pub fn decay_seconds(self) -> f32 {
        match self.kind {
            VoiceKind::Keys => 0.18 + self.decay * 1.4,
            VoiceKind::Bells => 0.40 + self.decay * 2.4,
            VoiceKind::Pluck => 0.06 + self.decay * 0.35,
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
    vel: f32,
    freq: f32,
    env: f32,
    tine: f32,
    car_phase: f32,
    mod_phase: f32,
    noise: u32,
    lp: f32,
    releasing: bool,
    age: u64,
}

#[inline]
fn noise_tick(state: &mut u32) -> f32 {
    let mut x = *state;
    if x == 0 {
        x = 0xA341_316C;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as i32 as f32) * (1.0 / 2_147_483_648.0)
}

#[inline]
fn wrap01(phase: &mut f32) {
    if *phase >= 1.0 {
        *phase -= phase.floor();
    }
}

#[derive(Debug, Clone)]
struct Chorus {
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
    lfo: f32,
    sample_rate: f32,
}

impl Chorus {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let n = ((0.030 * sr).ceil() as usize).max(8);
        Self {
            left: vec![0.0; n],
            right: vec![0.0; n],
            write: 0,
            lfo: 0.0,
            sample_rate: sr,
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        *self = Self::new(sample_rate);
    }

    fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.write = 0;
        self.lfo = 0.0;
    }

    fn tick(&mut self, input: f32, amount: f32) -> (f32, f32) {
        let n = self.left.len();
        if n < 8 {
            return (input, input);
        }
        let amount = amount.clamp(0.0, 1.0);
        self.lfo += 0.85 / self.sample_rate;
        if self.lfo >= 1.0 {
            self.lfo -= 1.0;
        }
        let lfo_l = (self.lfo * TAU).sin();
        let lfo_r = ((self.lfo + 0.25) * TAU).sin();
        let base = 0.012 * self.sample_rate;
        let depth = 0.004 * self.sample_rate * amount;
        let delay_l = (base + lfo_l * depth).clamp(2.0, (n - 2) as f32);
        let delay_r = (base + lfo_r * depth).clamp(2.0, (n - 2) as f32);

        let read = |buf: &[f32], write: usize, delay: f32| {
            let pos = write as f32 - delay;
            let pos = if pos < 0.0 { pos + n as f32 } else { pos };
            let i0 = pos.floor() as usize % n;
            let i1 = (i0 + 1) % n;
            let frac = pos.fract();
            buf[i0] + (buf[i1] - buf[i0]) * frac
        };

        let wet_l = read(&self.left, self.write, delay_l);
        let wet_r = read(&self.right, self.write, delay_r);
        self.left[self.write] = input;
        self.right[self.write] = input;
        self.write += 1;
        if self.write >= n {
            self.write = 0;
        }
        let wet = amount * 0.45;
        (
            input * (1.0 - wet * 0.4) + wet_l * wet,
            input * (1.0 - wet * 0.4) + wet_r * wet,
        )
    }
}

/// 16-voice keys engine.
#[derive(Debug, Clone)]
pub struct KeysEngine {
    voices: [Option<Voice>; MAX_VOICES],
    next_age: u64,
    sample_rate: f32,
    chorus: Chorus,
    warmth_l: f32,
    warmth_r: f32,
}

impl Default for KeysEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl KeysEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            voices: std::array::from_fn(|_| None),
            next_age: 1,
            sample_rate: sr,
            chorus: Chorus::new(sr),
            warmth_l: 0.0,
            warmth_r: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.chorus.set_sample_rate(self.sample_rate);
        self.warmth_l = 0.0;
        self.warmth_r = 0.0;
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_some()).count()
    }

    pub fn is_active(&self) -> bool {
        self.active_voices() > 0
    }

    pub fn reset(&mut self) {
        self.voices.fill(None);
        self.next_age = 1;
        self.chorus.reset();
        self.warmth_l = 0.0;
        self.warmth_r = 0.0;
    }

    pub fn all_notes_off(&mut self) {
        for voice in self.voices.iter_mut().flatten() {
            voice.releasing = true;
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        let note = note.min(127);
        let velocity = velocity.min(127);
        if velocity == 0 {
            self.note_off(note, channel);
            return;
        }
        let freq = midi_note_to_hz(note);
        let channel = channel & 0x0f;
        if let Some(voice) = self
            .voices
            .iter_mut()
            .flatten()
            .find(|v| v.note == note && v.channel == channel)
        {
            voice.vel = velocity as f32 / 127.0;
            voice.freq = freq;
            voice.env = 1.0;
            voice.tine = 1.0;
            voice.releasing = false;
            voice.age = self.next_age;
            self.next_age = self.next_age.wrapping_add(1);
            return;
        }

        let idx = self
            .voices
            .iter()
            .position(|v| v.is_none())
            .unwrap_or_else(|| self.steal_voice_index());
        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);
        let seed = 0x51ED_BEEF
            ^ (note as u32).wrapping_mul(0x9E37_79B9)
            ^ (age as u32).wrapping_mul(0x85EB_CA6B);
        self.voices[idx] = Some(Voice {
            note,
            channel,
            vel: velocity as f32 / 127.0,
            freq,
            env: 1.0,
            tine: 1.0,
            car_phase: 0.0,
            mod_phase: 0.0,
            noise: seed | 1,
            lp: 0.0,
            releasing: false,
            age,
        });
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        let note = note.min(127);
        let channel = channel & 0x0f;
        for voice in self.voices.iter_mut().flatten() {
            if voice.note == note && voice.channel == channel {
                voice.releasing = true;
            }
        }
    }

    pub fn process_block(
        &mut self,
        params: &KeysParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);

        let sr = self.sample_rate;
        let decay_s = params.decay_seconds();
        let decay_coeff = (-1.0 / (decay_s * sr).max(1.0)).exp();
        let release_coeff = (-1.0 / ((0.05 + params.decay * 0.22).max(0.04) * sr).max(1.0)).exp();
        let tine_s = match params.kind {
            VoiceKind::Keys => 0.045,
            VoiceKind::Bells => 0.22,
            VoiceKind::Pluck => 0.012,
        };
        let tine_coeff = (-1.0 / (tine_s * sr).max(1.0)).exp();
        let warmth_hz = 14_000.0 - params.warmth * 9_500.0;
        let warmth_coeff = 1.0 - (-TAU * warmth_hz.clamp(400.0, 18_000.0) / sr).exp();

        let mut event_i = 0;
        for frame in 0..frames {
            while event_i < events.len() && events[event_i].sample_offset as usize <= frame {
                let ev = events[event_i];
                if ev.on {
                    self.note_on(ev.note, ev.velocity, ev.channel);
                } else {
                    self.note_off(ev.note, ev.channel);
                }
                event_i += 1;
            }

            let mut mix = 0.0f32;
            for slot in &mut self.voices {
                let Some(voice) = slot else { continue };
                if voice.releasing {
                    voice.env *= release_coeff;
                } else {
                    voice.env *= decay_coeff;
                    // Tiny sustain so chords don't vanish while held, except pluck.
                    if params.kind != VoiceKind::Pluck {
                        voice.env = voice.env.max(0.08);
                    }
                }
                voice.tine *= tine_coeff;
                if voice.env < 1.0e-4 {
                    *slot = None;
                    continue;
                }

                let sample = tick_voice(voice, params.kind, sr);
                mix += sample * voice.env * voice.vel;
            }

            mix *= params.volume;
            let (mut l, mut r) = self.chorus.tick(mix, params.chorus);
            self.warmth_l += warmth_coeff * (l - self.warmth_l);
            self.warmth_r += warmth_coeff * (r - self.warmth_r);
            l = self.warmth_l.tanh();
            r = self.warmth_r.tanh();
            left[frame] = if l.is_finite() { l } else { 0.0 };
            right[frame] = if r.is_finite() { r } else { 0.0 };
        }

        while event_i < events.len() {
            let ev = events[event_i];
            if ev.on {
                self.note_on(ev.note, ev.velocity, ev.channel);
            } else {
                self.note_off(ev.note, ev.channel);
            }
            event_i += 1;
        }
    }

    fn steal_voice_index(&self) -> usize {
        let mut best = 0usize;
        let mut best_age = u64::MAX;
        let mut best_releasing = false;
        for (i, slot) in self.voices.iter().enumerate() {
            let Some(voice) = slot else {
                return i;
            };
            if voice.releasing && !best_releasing {
                best = i;
                best_age = voice.age;
                best_releasing = true;
            } else if voice.releasing == best_releasing && voice.age < best_age {
                best = i;
                best_age = voice.age;
            }
        }
        best
    }
}

fn tick_voice(voice: &mut Voice, kind: VoiceKind, sample_rate: f32) -> f32 {
    match kind {
        VoiceKind::Keys => {
            // DX electric piano: carrier at the note, modulator ~14:1, tine index decays.
            let ratio = 14.0;
            let index = (2.2 + voice.vel * 1.6) * voice.tine + 0.35;
            voice.mod_phase += (voice.freq * ratio) / sample_rate;
            wrap01(&mut voice.mod_phase);
            voice.car_phase += voice.freq / sample_rate;
            wrap01(&mut voice.car_phase);
            let modulator = (voice.mod_phase * TAU).sin();
            (voice.car_phase * TAU + index * modulator).sin()
        }
        VoiceKind::Bells => {
            let ratio = 2.718;
            let index = 1.8 * voice.tine + 0.55;
            voice.mod_phase += (voice.freq * ratio) / sample_rate;
            wrap01(&mut voice.mod_phase);
            voice.car_phase += voice.freq / sample_rate;
            wrap01(&mut voice.car_phase);
            let modulator = (voice.mod_phase * TAU).sin();
            let metal = (voice.car_phase * TAU + index * modulator).sin();
            // Quiet inharmonic sparkle.
            let sparkle = ((voice.mod_phase * 1.414 * TAU).sin()) * voice.tine * 0.18;
            metal + sparkle
        }
        VoiceKind::Pluck => {
            voice.car_phase += voice.freq / sample_rate;
            wrap01(&mut voice.car_phase);
            let body = (voice.car_phase * TAU).sin();
            let n = noise_tick(&mut voice.noise);
            let tick = n * voice.tine * 0.45;
            // Filter closes as the pluck dies.
            let cutoff = 1_200.0 + voice.tine * 4_000.0;
            let c = 1.0 - (-TAU * cutoff.clamp(200.0, 10_000.0) / sample_rate).exp();
            let raw = body * 0.75 + tick;
            voice.lp += c * (raw - voice.lp);
            voice.lp
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

    #[test]
    fn polyphony_allows_chords() {
        let mut eng = KeysEngine::new(48_000.0);
        let params = KeysParams::default();
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        eng.process_block(
            &params,
            &[on(0, 60, 100), on(0, 64, 100), on(0, 67, 100)],
            &mut l,
            &mut r,
        );
        assert_eq!(eng.active_voices(), 3);
        assert!(energy(&l) > 1.0e-3);
    }

    #[test]
    fn voice_stealing_caps_at_max() {
        let mut eng = KeysEngine::new(48_000.0);
        let params = KeysParams::default();
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        let events: Vec<_> = (0..MAX_VOICES + 4)
            .map(|i| on(0, (48 + i) as u8, 100))
            .collect();
        eng.process_block(&params, &events, &mut l, &mut r);
        assert!(eng.active_voices() <= MAX_VOICES);
    }

    #[test]
    fn pluck_decays_without_hold() {
        let mut eng = KeysEngine::new(48_000.0);
        let params = KeysParams {
            kind: VoiceKind::Pluck,
            decay: 0.0,
            chorus: 0.0,
            warmth: 0.0,
            volume: 1.0,
        };
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        eng.process_block(&params, &[on(0, 72, 120)], &mut l, &mut r);
        assert!(eng.active_voices() > 0);
        let mut l = vec![0.0f32; 48_000];
        let mut r = vec![0.0f32; 48_000];
        eng.process_block(&params, &[], &mut l, &mut r);
        assert_eq!(eng.active_voices(), 0);
        let tail = &l[40_000..];
        assert!(energy(tail) < 1.0e-4);
    }

    #[test]
    fn note_off_releases_keys() {
        let mut eng = KeysEngine::new(1_000.0);
        let params = KeysParams {
            kind: VoiceKind::Keys,
            decay: 1.0,
            chorus: 0.0,
            warmth: 0.0,
            volume: 1.0,
        };
        let mut l = vec![0.0f32; 8];
        let mut r = vec![0.0f32; 8];
        eng.process_block(&params, &[on(0, 60, 100)], &mut l, &mut r);
        assert_eq!(eng.active_voices(), 1);
        let mut l = vec![0.0f32; 8_000];
        let mut r = vec![0.0f32; 8_000];
        eng.process_block(&params, &[off(0, 60)], &mut l, &mut r);
        assert_eq!(eng.active_voices(), 0);
    }

    #[test]
    fn bells_make_sound() {
        let mut eng = KeysEngine::new(48_000.0);
        let params = KeysParams {
            kind: VoiceKind::Bells,
            ..KeysParams::default()
        };
        let mut l = vec![0.0f32; 1024];
        let mut r = vec![0.0f32; 1024];
        eng.process_block(&params, &[on(0, 76, 110)], &mut l, &mut r);
        assert!(energy(&l) > 1.0e-3);
        assert!(l.iter().all(|s| s.is_finite()));
    }
}
