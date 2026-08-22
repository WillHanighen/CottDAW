use crate::filter::OnePoleLp;
use crate::noise_tick;
use crate::voice::Voice;

pub const MAX_VOICES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluckParams {
    pub mute: f32,
    pub body: f32,
    pub tone: f32,
    pub dust: f32,
    pub level: f32,
}

impl Default for PluckParams {
    fn default() -> Self {
        Self {
            mute: 0.28,
            body: 0.42,
            tone: 0.48,
            dust: 0.06,
            level: 0.46,
        }
    }
}

impl PluckParams {
    pub fn clamped(self) -> Self {
        Self {
            mute: self.mute.clamp(0.0, 1.0),
            body: self.body.clamp(0.0, 1.0),
            tone: self.tone.clamp(0.0, 1.0),
            dust: self.dust.clamp(0.0, 1.0),
            level: self.level.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MidiNoteEvent {
    pub sample_offset: u32,
    pub note: u8,
    pub velocity: u8,
    pub channel: u8,
    pub on: bool,
}

#[derive(Debug, Clone)]
pub struct PluckEngine {
    voices: [Option<Voice>; MAX_VOICES],
    next_age: u64,
    sample_rate: f32,
    rng: u32,
    dust_lp: OnePoleLp,
}

impl Default for PluckEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl PluckEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut dust_lp = OnePoleLp::default();
        dust_lp.set_cutoff(2_400.0, sr);
        Self {
            voices: std::array::from_fn(|_| None),
            next_age: 1,
            sample_rate: sr,
            rng: 0xC0FF_EE42,
            dust_lp,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.dust_lp.set_cutoff(2_400.0, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.voices.fill(None);
        self.next_age = 1;
        self.dust_lp.reset();
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_some()).count()
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8, params: &PluckParams) {
        let note = note.min(127);
        let velocity = velocity.min(127);
        if velocity == 0 {
            return;
        }
        let params = params.clamped();
        let channel = channel & 0x0f;
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
            &mut self.rng,
        ));
    }

    pub fn process_block(
        &mut self,
        params: &PluckParams,
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
                }
                event_i += 1;
            }

            let mut mix = 0.0f32;
            for slot in &mut self.voices {
                let Some(voice) = slot else { continue };
                let sample = voice.tick(&params);
                if !voice.is_active() {
                    *slot = None;
                    continue;
                }
                mix += sample;
            }

            if params.dust > 0.0 {
                let n = noise_tick(&mut self.rng);
                mix += self.dust_lp.process(n) * params.dust * 0.035;
            }

            let out = (mix * params.level).clamp(-1.0, 1.0);
            left[frame] = out;
            right[frame] = out * 0.96;
        }

        while event_i < events.len() {
            let ev = events[event_i];
            if ev.on {
                self.note_on(ev.note, ev.velocity, ev.channel, &params);
            }
            event_i += 1;
        }
    }

    fn steal_voice_index(&self) -> usize {
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.as_ref().map(|v| v.age).unwrap_or(u64::MAX))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(note: u8) -> MidiNoteEvent {
        MidiNoteEvent {
            sample_offset: 0,
            note,
            velocity: 110,
            channel: 0,
            on: true,
        }
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

    #[test]
    fn silence_with_no_notes() {
        let mut engine = PluckEngine::new(48_000.0);
        let params = PluckParams {
            dust: 0.0,
            ..PluckParams::default()
        };
        let mut l = vec![0.0f32; 1024];
        let mut r = vec![0.0f32; 1024];
        engine.process_block(&params, &[], &mut l, &mut r);
        assert!(peak(&l) < 1e-6);
    }

    #[test]
    fn pluck_makes_sound() {
        let mut engine = PluckEngine::new(48_000.0);
        let mut l = vec![0.0f32; 4096];
        let mut r = vec![0.0f32; 4096];
        engine.process_block(&PluckParams::default(), &[on(64)], &mut l, &mut r);
        assert!(peak(&l) > 0.02);
        assert!(peak(&l) < 0.99);
        assert_eq!(engine.active_voices(), 1);
    }

    #[test]
    fn mute_dies_faster() {
        let open = PluckParams {
            mute: 0.0,
            dust: 0.0,
            ..PluckParams::default()
        };
        let muted = PluckParams {
            mute: 1.0,
            dust: 0.0,
            ..PluckParams::default()
        };
        let mut a = PluckEngine::new(48_000.0);
        let mut b = PluckEngine::new(48_000.0);
        let mut l = vec![0.0f32; 16_384];
        let mut r = vec![0.0f32; 16_384];
        a.process_block(&open, &[on(60)], &mut l, &mut r);
        let open_tail = rms(&l[10_000..]);
        b.process_block(&muted, &[on(60)], &mut l, &mut r);
        let mute_tail = rms(&l[10_000..]);
        assert!(mute_tail < open_tail, "open={open_tail} mute={mute_tail}");
    }

    #[test]
    fn dust_hisses() {
        let mut engine = PluckEngine::new(48_000.0);
        let params = PluckParams {
            dust: 1.0,
            ..PluckParams::default()
        };
        let mut l = vec![0.0f32; 8192];
        let mut r = vec![0.0f32; 8192];
        engine.process_block(&params, &[], &mut l, &mut r);
        assert!(rms(&l) > 1e-4);
    }
}
