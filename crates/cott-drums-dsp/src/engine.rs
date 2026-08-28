//! Drum machine engine: MIDI → pads → room → warmth.

use serde::{Deserialize, Serialize};

use crate::voice::Voice;

/// Pads in the kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Pad {
    Kick = 0,
    Rim = 1,
    Snare = 2,
    Clap = 3,
    Tom = 4,
    ClosedHat = 5,
    OpenHat = 6,
}

pub const PAD_COUNT: usize = 7;

impl Pad {
    pub const ALL: [Pad; PAD_COUNT] = [
        Pad::Kick,
        Pad::Rim,
        Pad::Snare,
        Pad::Clap,
        Pad::Tom,
        Pad::ClosedHat,
        Pad::OpenHat,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Pad::Kick => "Kick",
            Pad::Rim => "Rim",
            Pad::Snare => "Snare",
            Pad::Clap => "Clap",
            Pad::Tom => "Tom",
            Pad::ClosedHat => "CH",
            Pad::OpenHat => "OH",
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    /// General MIDI percussion notes. Extra toms / snare aliases fold onto these pads.
    pub fn from_note(note: u8) -> Option<Self> {
        match note {
            35 | 36 => Some(Pad::Kick),
            37 => Some(Pad::Rim),
            38 | 40 => Some(Pad::Snare),
            39 => Some(Pad::Clap),
            41 | 43 | 45 | 47 | 48 | 50 => Some(Pad::Tom),
            42 | 44 => Some(Pad::ClosedHat),
            46 => Some(Pad::OpenHat),
            _ => None,
        }
    }

    /// Canonical GM note used when the pad is tapped from the editor.
    pub fn midi_note(self) -> u8 {
        match self {
            Pad::Kick => 36,
            Pad::Rim => 37,
            Pad::Snare => 38,
            Pad::Clap => 39,
            Pad::Tom => 41,
            Pad::ClosedHat => 42,
            Pad::OpenHat => 46,
        }
    }
}

/// Per-pad knobs. Pitch is semitones, the rest are 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PadParams {
    pub pitch: f32,
    pub decay: f32,
    pub tone: f32,
    pub level: f32,
}

impl PadParams {
    pub fn clamped(self) -> Self {
        Self {
            pitch: self.pitch.clamp(-12.0, 12.0),
            decay: self.decay.clamp(0.0, 1.0),
            tone: self.tone.clamp(0.0, 1.0),
            level: self.level.clamp(0.0, 1.0),
        }
    }
}

impl Default for PadParams {
    fn default() -> Self {
        Self {
            pitch: 0.0,
            decay: 0.5,
            tone: 0.5,
            level: 0.8,
        }
    }
}

/// Global kit plus one param block per pad.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DrumParams {
    /// 0 = electronic box, 1 = bedroom kit.
    pub kit: f32,
    /// Short stereo wash, not a hall.
    pub room: f32,
    /// Gentle low-pass. Not vinyl hiss.
    pub warmth: f32,
    pub volume: f32,
    pub pads: [PadParams; PAD_COUNT],
}

impl Default for DrumParams {
    fn default() -> Self {
        let mut pads = [PadParams::default(); PAD_COUNT];
        pads[Pad::Kick as usize] = PadParams {
            pitch: 0.0,
            decay: 0.55,
            tone: 0.40,
            level: 0.92,
        };
        pads[Pad::Rim as usize] = PadParams {
            pitch: 0.0,
            decay: 0.18,
            tone: 0.78,
            level: 0.55,
        };
        pads[Pad::Snare as usize] = PadParams {
            pitch: 0.0,
            decay: 0.42,
            tone: 0.55,
            level: 0.82,
        };
        pads[Pad::Clap as usize] = PadParams {
            pitch: 0.0,
            decay: 0.35,
            tone: 0.32,
            level: 0.70,
        };
        pads[Pad::Tom as usize] = PadParams {
            pitch: 0.0,
            decay: 0.50,
            tone: 0.40,
            level: 0.72,
        };
        pads[Pad::ClosedHat as usize] = PadParams {
            pitch: 0.0,
            decay: 0.28,
            tone: 0.70,
            level: 0.48,
        };
        pads[Pad::OpenHat as usize] = PadParams {
            pitch: 0.0,
            decay: 0.72,
            tone: 0.62,
            level: 0.42,
        };
        Self {
            kit: 0.45,
            room: 0.22,
            warmth: 0.18,
            volume: 0.85,
            pads,
        }
    }
}

impl DrumParams {
    pub fn clamped(self) -> Self {
        Self {
            kit: self.kit.clamp(0.0, 1.0),
            room: self.room.clamp(0.0, 1.0),
            warmth: self.warmth.clamp(0.0, 1.0),
            volume: self.volume.clamp(0.0, 1.0),
            pads: self.pads.map(PadParams::clamped),
        }
    }

    pub fn pad(&self, pad: Pad) -> PadParams {
        self.pads[pad as usize]
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
struct Room {
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
    delay_l: usize,
    delay_r: usize,
}

impl Room {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let delay_l = ((0.017 * sr).round() as usize).max(2);
        let delay_r = ((0.023 * sr).round() as usize).max(3);
        let n = delay_r + 8;
        Self {
            left: vec![0.0; n],
            right: vec![0.0; n],
            write: 0,
            delay_l,
            delay_r,
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        *self = Self::new(sample_rate);
    }

    fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.write = 0;
    }

    fn tick(&mut self, input: f32, amount: f32) -> (f32, f32) {
        let n = self.left.len();
        if n < 4 {
            return (input, input);
        }
        let amount = amount.clamp(0.0, 1.0);
        let read_l = (self.write + n - self.delay_l.min(n - 1)) % n;
        let read_r = (self.write + n - self.delay_r.min(n - 1)) % n;
        let echo_l = self.left[read_l];
        let echo_r = self.right[read_r];
        let fb = 0.22 * amount;
        self.left[self.write] = input + echo_r * fb;
        self.right[self.write] = input + echo_l * fb;
        self.write += 1;
        if self.write >= n {
            self.write = 0;
        }
        let wet = amount * 0.42;
        (
            input * (1.0 - wet * 0.35) + echo_l * wet,
            input * (1.0 - wet * 0.35) + echo_r * wet,
        )
    }
}

/// Analog drum machine. One voice per pad.
#[derive(Debug, Clone)]
pub struct DrumEngine {
    voices: [Voice; PAD_COUNT],
    sample_rate: f32,
    room: Room,
    warmth_l: f32,
    warmth_r: f32,
    /// Peak envelope per pad, for the UI lamps. Audio thread writes, UI reads.
    pad_level: [f32; PAD_COUNT],
}

impl Default for DrumEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl DrumEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            voices: std::array::from_fn(|i| {
                let pad = Pad::from_index(i).unwrap_or(Pad::Kick);
                Voice::new(pad, 0xC0FF_EE00 ^ (i as u32).wrapping_mul(0x9E37_79B9))
            }),
            sample_rate: sr,
            room: Room::new(sr),
            warmth_l: 0.0,
            warmth_r: 0.0,
            pad_level: [0.0; PAD_COUNT],
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.room.set_sample_rate(self.sample_rate);
        self.warmth_l = 0.0;
        self.warmth_r = 0.0;
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn reset(&mut self) {
        for (i, voice) in self.voices.iter_mut().enumerate() {
            let pad = Pad::from_index(i).unwrap_or(Pad::Kick);
            *voice = Voice::new(pad, 0xC0FF_EE00 ^ (i as u32).wrapping_mul(0x9E37_79B9));
        }
        self.room.reset();
        self.warmth_l = 0.0;
        self.warmth_r = 0.0;
        self.pad_level = [0.0; PAD_COUNT];
    }

    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            voice.choke();
        }
    }

    pub fn is_active(&self) -> bool {
        self.voices.iter().any(Voice::is_active)
    }

    pub fn pad_level(&self, pad: Pad) -> f32 {
        self.pad_level[pad as usize]
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let velocity = velocity.min(127);
        if velocity == 0 {
            self.note_off(note);
            return;
        }
        let Some(pad) = Pad::from_note(note) else {
            return;
        };
        if pad == Pad::ClosedHat {
            self.voices[Pad::OpenHat as usize].choke();
        }
        self.voices[pad as usize].trigger(velocity as f32 / 127.0);
    }

    pub fn note_off(&mut self, note: u8) {
        if Pad::from_note(note) == Some(Pad::OpenHat) {
            self.voices[Pad::OpenHat as usize].choke();
        }
    }

    pub fn process_block(
        &mut self,
        params: &DrumParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);

        let warmth_hz = 16_000.0 - params.warmth * 11_500.0;
        let warmth_coeff =
            1.0 - (-std::f32::consts::TAU * warmth_hz.clamp(400.0, 20_000.0) / self.sample_rate)
                .exp();

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

            let mut mix = 0.0f32;
            for (i, voice) in self.voices.iter_mut().enumerate() {
                let pad = Pad::from_index(i).unwrap_or(Pad::Kick);
                let sample = voice.tick(params.kit, &params.pads[i], self.sample_rate);
                mix += sample;
                let level = voice.env.max(voice.click) * voice.vel;
                self.pad_level[i] = self.pad_level[i].max(level);
                let _ = pad;
            }

            mix *= params.volume;
            let (mut l, mut r) = self.room.tick(mix, params.room);
            self.warmth_l += warmth_coeff * (l - self.warmth_l);
            self.warmth_r += warmth_coeff * (r - self.warmth_r);
            l = self.warmth_l;
            r = self.warmth_r;
            l = l.tanh();
            r = r.tanh();
            left[frame] = if l.is_finite() { l } else { 0.0 };
            right[frame] = if r.is_finite() { r } else { 0.0 };
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

        for level in &mut self.pad_level {
            *level *= 0.92;
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
    fn kick_makes_sound() {
        let mut eng = DrumEngine::new(48_000.0);
        let params = DrumParams::default();
        let mut l = vec![0.0f32; 2048];
        let mut r = vec![0.0f32; 2048];
        eng.process_block(&params, &[on(0, 36, 120)], &mut l, &mut r);
        assert!(energy(&l) > 1.0e-3, "kick should produce energy");
        assert!(l.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn unused_notes_stay_quiet() {
        let mut eng = DrumEngine::new(48_000.0);
        let params = DrumParams::default();
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        eng.process_block(&params, &[on(0, 60, 127)], &mut l, &mut r);
        assert!(
            energy(&l) < 1.0e-8,
            "middle C is not a pad, got {}",
            energy(&l)
        );
    }

    #[test]
    fn hat_choke_kills_open_hat() {
        let mut eng = DrumEngine::new(48_000.0);
        let mut params = DrumParams::default();
        params.room = 0.0;
        params.warmth = 0.0;
        // Let the open hat ring, then choke it.
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        eng.process_block(&params, &[on(0, 46, 120)], &mut l, &mut r);
        assert!(eng.voices[Pad::OpenHat as usize].is_active());
        let mut l = vec![0.0f32; 2048];
        let mut r = vec![0.0f32; 2048];
        eng.process_block(&params, &[on(0, 42, 100)], &mut l, &mut r);
        // After ~40 ms the open hat should be gone; closed hat may still tick.
        let mut tail = vec![0.0f32; 4096];
        let mut rtail = vec![0.0f32; 4096];
        eng.process_block(&params, &[], &mut tail, &mut rtail);
        assert!(
            !eng.voices[Pad::OpenHat as usize].is_active(),
            "closed hat should choke the open hat"
        );
    }

    #[test]
    fn open_hat_note_off_chokes() {
        let mut eng = DrumEngine::new(48_000.0);
        let params = DrumParams::default();
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        eng.process_block(&params, &[on(0, 46, 120)], &mut l, &mut r);
        assert!(eng.voices[Pad::OpenHat as usize].is_active());
        let mut l = vec![0.0f32; 4096];
        let mut r = vec![0.0f32; 4096];
        eng.process_block(&params, &[off(0, 46)], &mut l, &mut r);
        assert!(!eng.voices[Pad::OpenHat as usize].is_active());
    }

    #[test]
    fn decay_reaches_silence() {
        let mut eng = DrumEngine::new(48_000.0);
        let mut params = DrumParams::default();
        params.room = 0.0;
        params.pads[Pad::Rim as usize].decay = 0.05;
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        eng.process_block(&params, &[on(0, 37, 100)], &mut l, &mut r);
        let mut l = vec![0.0f32; 48_000];
        let mut r = vec![0.0f32; 48_000];
        eng.process_block(&params, &[], &mut l, &mut r);
        let tail = &l[40_000..];
        assert!(energy(tail) < 1.0e-4, "rim should be done, energy {}", energy(tail));
        assert!(!eng.is_active());
    }

    #[test]
    fn gm_aliases_hit_kick() {
        let mut a = DrumEngine::new(48_000.0);
        let mut b = DrumEngine::new(48_000.0);
        let params = DrumParams::default();
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        a.process_block(&params, &[on(0, 35, 100)], &mut l, &mut r);
        let ea = energy(&l);
        b.process_block(&params, &[on(0, 36, 100)], &mut l, &mut r);
        assert!(ea > 1.0e-4);
        assert!(energy(&l) > 1.0e-4);
    }
}
