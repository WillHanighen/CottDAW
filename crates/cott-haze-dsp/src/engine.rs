use crate::dust::Dust;
use crate::smear::Smear;
use crate::tape::Tape;
use crate::voice::{EnvStage, Voice};

pub const MAX_VOICES: usize = 12;

/// Nine knobs. Decay and sustain stay inside the voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazeParams {
    pub tone: f32,
    pub bell: f32,
    pub flutter: f32,
    pub warmth: f32,
    pub smear: f32,
    pub dust: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub level: f32,
}

impl Default for HazeParams {
    fn default() -> Self {
        Self {
            tone: 0.42,
            bell: 0.38,
            flutter: 0.20,
            warmth: 0.22,
            smear: 0.35,
            dust: 0.04,
            attack_ms: 12.0,
            release_ms: 420.0,
            level: 0.38,
        }
    }
}

impl HazeParams {
    pub fn clamped(self) -> Self {
        Self {
            tone: self.tone.clamp(0.0, 1.0),
            bell: self.bell.clamp(0.0, 1.0),
            flutter: self.flutter.clamp(0.0, 1.0),
            warmth: self.warmth.clamp(0.0, 1.0),
            smear: self.smear.clamp(0.0, 1.0),
            dust: self.dust.clamp(0.0, 1.0),
            attack_ms: self.attack_ms.clamp(0.0, 5_000.0),
            release_ms: self.release_ms.clamp(0.0, 8_000.0),
            level: self.level.clamp(0.0, 1.0),
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

/// Polyphonic CottHaze engine.
#[derive(Debug, Clone)]
pub struct HazeEngine {
    voices: [Option<Voice>; MAX_VOICES],
    next_age: u64,
    sample_rate: f32,
    tape: Tape,
    smear: Smear,
    dust: Dust,
}

impl Default for HazeEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl HazeEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            voices: std::array::from_fn(|_| None),
            next_age: 1,
            sample_rate: sr,
            tape: Tape::new(sr),
            smear: Smear::new(sr),
            dust: Dust::new(sr),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.sample_rate = sr;
        self.tape.set_sample_rate(sr);
        self.smear.set_sample_rate(sr);
        self.dust.set_sample_rate(sr);
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
        self.tape.reset();
        self.smear.reset();
        self.dust.reset();
    }

    pub fn all_notes_off(&mut self, params: &HazeParams) {
        let params = params.clamped();
        for voice in self.voices.iter_mut().flatten() {
            voice.note_off(&params, self.sample_rate);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8, params: &HazeParams) {
        let note = note.min(127);
        let velocity = velocity.min(127);
        if velocity == 0 {
            self.note_off(note, channel, params);
            return;
        }
        let params = params.clamped();
        let channel = channel & 0x0f;

        if let Some(voice) = self
            .voices
            .iter_mut()
            .flatten()
            .find(|voice| voice.note == note && voice.channel == channel)
        {
            voice.age = self.next_age;
            self.next_age = self.next_age.wrapping_add(1);
            voice.retrigger(velocity, &params, self.sample_rate);
            return;
        }

        let idx = self
            .voices
            .iter()
            .position(|v| v.is_none())
            .unwrap_or_else(|| self.steal_voice_index());

        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);
        self.voices[idx] = Some(Voice::start(
            note,
            velocity,
            channel,
            age,
            &params,
            self.sample_rate,
        ));
    }

    pub fn note_off(&mut self, note: u8, channel: u8, params: &HazeParams) {
        let params = params.clamped();
        let note = note.min(127);
        let channel = channel & 0x0f;
        for slot in &mut self.voices {
            if let Some(voice) = slot
                && voice.note == note
                && voice.channel == channel
            {
                voice.note_off(&params, self.sample_rate);
            }
        }
    }

    pub fn process_block(
        &mut self,
        params: &HazeParams,
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

            let pitch = self.tape.pitch_ratio(params.flutter);
            let mut mix = 0.0f32;
            for slot in &mut self.voices {
                let Some(voice) = slot else { continue };
                let sample = voice.tick(&params, pitch, self.sample_rate);
                if !voice.is_active() {
                    *slot = None;
                    continue;
                }
                mix += sample;
            }

            let (l, r) = self.tape.process(mix, mix, params.flutter, params.warmth);
            let (l, r) = self.smear.process(l, r, params.smear);
            let (l, r) = self.dust.process(l, r, params.dust);
            left[frame] = (l * params.level).clamp(-1.0, 1.0);
            right[frame] = (r * params.level).clamp(-1.0, 1.0);
        }

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
        let mut best = 0usize;
        let mut best_age = u64::MAX;
        let mut best_releasing = false;
        for (i, slot) in self.voices.iter().enumerate() {
            let Some(voice) = slot else {
                return i;
            };
            let releasing = matches!(voice.stage(), EnvStage::Release);
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

    fn render(
        engine: &mut HazeEngine,
        params: &HazeParams,
        events: &[MidiNoteEvent],
        n: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        engine.process_block(params, events, &mut l, &mut r);
        (l, r)
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn zero_crossings(buf: &[f32]) -> usize {
        buf.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn silence_with_no_notes() {
        let mut engine = HazeEngine::new(48_000.0);
        let params = HazeParams {
            dust: 0.0,
            flutter: 0.0,
            smear: 0.0,
            ..HazeParams::default()
        };
        let (l, r) = render(&mut engine, &params, &[], 2048);
        assert!(peak(&l) < 1e-6);
        assert!(peak(&r) < 1e-6);
        assert_eq!(engine.active_voices(), 0);
    }

    #[test]
    fn chord_does_not_clip_at_default_level() {
        let mut engine = HazeEngine::new(48_000.0);
        let params = HazeParams {
            smear: 0.0,
            ..HazeParams::default()
        };
        let (l, r) = render(
            &mut engine,
            &params,
            &[
                on(0, 60, 100),
                on(0, 64, 100),
                on(0, 67, 100),
                on(0, 71, 100),
            ],
            4096,
        );
        assert!(peak(&l) < 0.99, "left peak {}", peak(&l));
        assert!(peak(&r) < 0.99, "right peak {}", peak(&r));
        assert!(peak(&l) > 1e-3);
        assert_eq!(engine.active_voices(), 4);
    }

    #[test]
    fn dust_at_zero_is_quiet() {
        let mut engine = HazeEngine::new(48_000.0);
        let quiet = HazeParams {
            dust: 0.0,
            ..HazeParams::default()
        };
        let (l, _) = render(&mut engine, &quiet, &[], 8192);
        assert!(rms(&l) < 1e-6);

        engine.reset();
        let loud = HazeParams {
            dust: 1.0,
            ..HazeParams::default()
        };
        let (l, _) = render(&mut engine, &loud, &[], 8192);
        assert!(rms(&l) > 1e-4, "dust=1 should hiss, rms={}", rms(&l));
    }

    #[test]
    fn flutter_at_zero_does_not_walk_pitch() {
        let mut engine = HazeEngine::new(48_000.0);
        let params = HazeParams {
            flutter: 0.0,
            dust: 0.0,
            smear: 0.0,
            attack_ms: 0.0,
            warmth: 0.0,
            ..HazeParams::default()
        };
        let (l, _) = render(&mut engine, &params, &[on(0, 60, 110)], 48_000);
        // Bell and body decay are done by here; both windows sit on sustain.
        let a = zero_crossings(&l[20_000..32_000]);
        let b = zero_crossings(&l[32_000..44_000]);
        assert!(a > 20, "expected a tone, crossings={a}");
        assert!((a as i32 - b as i32).abs() <= 1, "pitch walked: {a} vs {b}");
    }

    #[test]
    fn note_off_releases() {
        let mut engine = HazeEngine::new(1_000.0);
        let params = HazeParams {
            attack_ms: 0.0,
            release_ms: 5.0,
            dust: 0.0,
            flutter: 0.0,
            smear: 0.0,
            ..HazeParams::default()
        };
        let _ = render(&mut engine, &params, &[on(0, 60, 100)], 8);
        assert_eq!(engine.active_voices(), 1);
        let _ = render(&mut engine, &params, &[off(0, 60)], 32);
        assert_eq!(engine.active_voices(), 0);
    }

    #[test]
    fn smear_zero_is_dry_after_release() {
        let sr = 48_000.0;
        let mut engine = HazeEngine::new(sr);
        let params = HazeParams {
            smear: 0.0,
            dust: 0.0,
            flutter: 0.0,
            warmth: 0.0,
            attack_ms: 0.0,
            release_ms: 8.0,
            ..HazeParams::default()
        };
        let _ = render(
            &mut engine,
            &params,
            &[on(0, 60, 110), on(0, 64, 110), on(0, 67, 110)],
            2_048,
        );
        let (l, _) = render(
            &mut engine,
            &params,
            &[off(0, 60), off(0, 64), off(0, 67)],
            8_192,
        );
        assert_eq!(engine.active_voices(), 0);
        assert!(
            rms(&l[4_096..]) < 1e-5,
            "smear=0 should not leave a wash"
        );
    }

    #[test]
    fn smear_leaves_energy_after_note_off() {
        let sr = 48_000.0;
        let mut engine = HazeEngine::new(sr);
        let params = HazeParams {
            smear: 1.0,
            dust: 0.0,
            flutter: 0.0,
            warmth: 0.0,
            attack_ms: 0.0,
            release_ms: 8.0,
            ..HazeParams::default()
        };
        let _ = render(
            &mut engine,
            &params,
            &[on(0, 60, 110), on(0, 64, 110), on(0, 67, 110)],
            2_048,
        );
        let (l, _) = render(&mut engine, &params, &[off(0, 60), off(0, 64), off(0, 67)], 8_192);
        assert_eq!(engine.active_voices(), 0);
        let tail = rms(&l[4_096..]);
        assert!(tail > 1e-4, "smear should leave a wash, rms={tail}");
    }
}
