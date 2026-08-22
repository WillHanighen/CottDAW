use crate::filter::{OnePoleHp, OnePoleLp};
use crate::noise_tick;

const KICK: u8 = 36;
const KICK_ALT: u8 = 35;
const SNARE: u8 = 38;
const SNARE_ALT: u8 = 40;
const CLAP: u8 = 39;
const HAT_CLOSED: u8 = 42;
const HAT_OPEN: u8 = 46;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KitParams {
    pub kick: f32,
    pub snare: f32,
    pub hats: f32,
    pub dirt: f32,
    pub tune: f32,
    pub level: f32,
}

impl Default for KitParams {
    fn default() -> Self {
        Self {
            kick: 0.78,
            snare: 0.62,
            hats: 0.48,
            dirt: 0.10,
            tune: 0.50,
            level: 0.55,
        }
    }
}

impl KitParams {
    pub fn clamped(self) -> Self {
        Self {
            kick: self.kick.clamp(0.0, 1.0),
            snare: self.snare.clamp(0.0, 1.0),
            hats: self.hats.clamp(0.0, 1.0),
            dirt: self.dirt.clamp(0.0, 1.0),
            tune: self.tune.clamp(0.0, 1.0),
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
pub struct KitEngine {
    sample_rate: f32,
    rng: u32,
    kick_env: f32,
    kick_hz: f32,
    kick_phase: f32,
    kick_vel: f32,
    snare_env: f32,
    snare_body: f32,
    snare_phase: f32,
    snare_vel: f32,
    clap_env: f32,
    clap_wait: [u32; 3],
    clap_vel: f32,
    hat_env: f32,
    hat_open: bool,
    hat_vel: f32,
    hat_hp: OnePoleHp,
    dirt_lp: OnePoleLp,
    dirt_hp: OnePoleHp,
}

impl Default for KitEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl KitEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut hat_hp = OnePoleHp::default();
        hat_hp.set_cutoff(6_800.0, sr);
        let mut dirt_lp = OnePoleLp::default();
        dirt_lp.set_cutoff(2_800.0, sr);
        let mut dirt_hp = OnePoleHp::default();
        dirt_hp.set_cutoff(700.0, sr);
        Self {
            sample_rate: sr,
            rng: 0x0BAD_F00D,
            kick_env: 0.0,
            kick_hz: 55.0,
            kick_phase: 0.0,
            kick_vel: 0.0,
            snare_env: 0.0,
            snare_body: 0.0,
            snare_phase: 0.0,
            snare_vel: 0.0,
            clap_env: 0.0,
            clap_wait: [0, 0, 0],
            clap_vel: 0.0,
            hat_env: 0.0,
            hat_open: false,
            hat_vel: 0.0,
            hat_hp,
            dirt_lp,
            dirt_hp,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.hat_hp.set_cutoff(6_800.0, self.sample_rate);
        self.dirt_lp.set_cutoff(2_800.0, self.sample_rate);
        self.dirt_hp.set_cutoff(700.0, self.sample_rate);
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.sample_rate);
    }

    fn trig_kick(&mut self, vel: f32, tune: f32) {
        let start = 90.0 * 2f32.powf((tune - 0.5) * 1.2);
        self.kick_hz = start;
        self.kick_env = 1.0;
        self.kick_phase = 0.0;
        self.kick_vel = vel;
    }

    fn trig_snare(&mut self, vel: f32) {
        self.snare_env = 1.0;
        self.snare_body = 1.0;
        self.snare_phase = 0.0;
        self.snare_vel = vel;
    }

    fn trig_clap(&mut self, vel: f32) {
        self.clap_env = 1.0;
        self.clap_vel = vel;
        let gap = (0.012 * self.sample_rate) as u32;
        self.clap_wait = [gap, gap * 2, gap * 3];
    }

    fn trig_hat(&mut self, vel: f32, open: bool) {
        self.hat_env = 1.0;
        self.hat_open = open;
        self.hat_vel = vel;
    }

    fn note_on(&mut self, note: u8, velocity: u8, params: &KitParams) {
        let vel = (velocity.min(127) as f32) / 127.0;
        match note {
            KICK | KICK_ALT => self.trig_kick(vel, params.tune),
            SNARE | SNARE_ALT => self.trig_snare(vel),
            CLAP => self.trig_clap(vel),
            HAT_CLOSED => self.trig_hat(vel, false),
            HAT_OPEN => self.trig_hat(vel, true),
            _ => {}
        }
    }

    pub fn process_block(
        &mut self,
        params: &KitParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);

        let kick_decay = (-1.0 / (0.22 * self.sample_rate)).exp();
        let kick_pitch = (-1.0 / (0.045 * self.sample_rate)).exp();
        let snare_decay = (-1.0 / (0.12 * self.sample_rate)).exp();
        let body_decay = (-1.0 / (0.08 * self.sample_rate)).exp();
        let clap_decay = (-1.0 / (0.09 * self.sample_rate)).exp();
        let hat_closed = (-1.0 / (0.045 * self.sample_rate)).exp();
        let hat_open = (-1.0 / (0.28 * self.sample_rate)).exp();
        let end_hz = 42.0 * 2f32.powf((params.tune - 0.5) * 1.2);

        let mut event_i = 0;
        for frame in 0..frames {
            while event_i < events.len() && events[event_i].sample_offset as usize <= frame {
                let ev = events[event_i];
                if ev.on {
                    self.note_on(ev.note, ev.velocity, &params);
                }
                event_i += 1;
            }

            let mut mix = 0.0f32;

            if self.kick_env > 1e-4 {
                self.kick_phase = wrap(self.kick_phase + self.kick_hz / self.sample_rate);
                let sine = (self.kick_phase * std::f32::consts::TAU).sin();
                mix += sine * self.kick_env * self.kick_vel * params.kick * 0.85;
                self.kick_env *= kick_decay;
                self.kick_hz = end_hz + (self.kick_hz - end_hz) * kick_pitch;
            }

            if self.snare_env > 1e-4 || self.snare_body > 1e-4 {
                let n = noise_tick(&mut self.rng);
                self.snare_phase = wrap(self.snare_phase + 190.0 / self.sample_rate);
                let body = (self.snare_phase * std::f32::consts::TAU).sin() * self.snare_body;
                mix +=
                    (n * self.snare_env * 0.7 + body * 0.35) * self.snare_vel * params.snare * 0.7;
                self.snare_env *= snare_decay;
                self.snare_body *= body_decay;
            }

            if self.clap_env > 1e-4 {
                let mut burst = self.clap_env;
                for wait in &mut self.clap_wait {
                    if *wait == 0 {
                        burst += 0.65;
                    } else {
                        *wait -= 1;
                    }
                }
                let n = noise_tick(&mut self.rng);
                mix += n * burst.min(1.6) * self.clap_vel * params.snare * 0.45;
                self.clap_env *= clap_decay;
            }

            if self.hat_env > 1e-4 {
                let n = noise_tick(&mut self.rng);
                let hat = self.hat_hp.process(n) * self.hat_env * self.hat_vel * params.hats * 0.38;
                mix += hat;
                self.hat_env *= if self.hat_open { hat_open } else { hat_closed };
            }

            if params.dirt > 0.0 {
                let n = noise_tick(&mut self.rng);
                mix += self.dirt_hp.process(self.dirt_lp.process(n)) * params.dirt * 0.04;
            }

            let out = (mix * params.level).clamp(-1.0, 1.0);
            left[frame] = out;
            right[frame] = out * 0.92;
        }

        while event_i < events.len() {
            let ev = events[event_i];
            if ev.on {
                self.note_on(ev.note, ev.velocity, &params);
            }
            event_i += 1;
        }
    }
}

fn wrap(phase: f32) -> f32 {
    let p = phase.fract();
    if p < 0.0 {
        p + 1.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(note: u8) -> MidiNoteEvent {
        MidiNoteEvent {
            sample_offset: 0,
            note,
            velocity: 120,
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
    fn silence_without_dirt() {
        let mut engine = KitEngine::new(48_000.0);
        let params = KitParams {
            dirt: 0.0,
            ..KitParams::default()
        };
        let mut l = vec![0.0f32; 1024];
        let mut r = vec![0.0f32; 1024];
        engine.process_block(&params, &[], &mut l, &mut r);
        assert!(peak(&l) < 1e-6);
    }

    #[test]
    fn kick_hits() {
        let mut engine = KitEngine::new(48_000.0);
        let mut l = vec![0.0f32; 4096];
        let mut r = vec![0.0f32; 4096];
        engine.process_block(&KitParams::default(), &[on(36)], &mut l, &mut r);
        assert!(peak(&l) > 0.05);
        assert!(peak(&l) < 0.99);
    }

    #[test]
    fn snare_hits() {
        let mut engine = KitEngine::new(48_000.0);
        let mut l = vec![0.0f32; 4096];
        let mut r = vec![0.0f32; 4096];
        engine.process_block(&KitParams::default(), &[on(38)], &mut l, &mut r);
        assert!(peak(&l) > 0.03);
    }

    #[test]
    fn hat_is_quieter_than_kick() {
        let params = KitParams {
            dirt: 0.0,
            ..KitParams::default()
        };
        let mut kick = KitEngine::new(48_000.0);
        let mut hat = KitEngine::new(48_000.0);
        let mut l = vec![0.0f32; 2048];
        let mut r = vec![0.0f32; 2048];
        kick.process_block(&params, &[on(36)], &mut l, &mut r);
        let kick_p = peak(&l);
        hat.process_block(&params, &[on(42)], &mut l, &mut r);
        let hat_p = peak(&l);
        assert!(hat_p < kick_p);
    }

    #[test]
    fn dirt_hisses() {
        let mut engine = KitEngine::new(48_000.0);
        let params = KitParams {
            dirt: 1.0,
            ..KitParams::default()
        };
        let mut l = vec![0.0f32; 8192];
        let mut r = vec![0.0f32; 8192];
        engine.process_block(&params, &[], &mut l, &mut r);
        assert!(rms(&l) > 1e-4);
    }
}
