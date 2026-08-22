use std::f32::consts::TAU;

use crate::filter::OnePoleLp;
use crate::midi_note_to_hz;
use crate::noise_tick;
use crate::HazeParams;

const BELL_RATIO: f32 = 14.0;
const BODY_DECAY_MS: f32 = 220.0;
const BODY_SUSTAIN: f32 = 0.84;
const BELL_DECAY_MS: f32 = 110.0;
const HAMMER_DECAY_MS: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone)]
struct Adsr {
    stage: EnvStage,
    level: f32,
    samples_left: u32,
    delta: f32,
    target: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
            samples_left: 0,
            delta: 0.0,
            target: 0.0,
        }
    }
}

impl Adsr {
    fn note_on(&mut self, attack_ms: f32, sample_rate: f32) {
        let attack = ms_to_samples(attack_ms, sample_rate);
        if attack == 0 {
            self.level = 1.0;
            self.enter_decay(sample_rate);
        } else {
            self.stage = EnvStage::Attack;
            self.target = 1.0;
            self.samples_left = attack;
            self.delta = (1.0 - self.level) / attack as f32;
        }
    }

    fn note_off(&mut self, release_ms: f32, sample_rate: f32) {
        if matches!(self.stage, EnvStage::Idle) {
            return;
        }
        let release = ms_to_samples(release_ms, sample_rate);
        if release == 0 || self.level <= 0.0 {
            self.idle();
            return;
        }
        self.stage = EnvStage::Release;
        self.target = 0.0;
        self.samples_left = release;
        self.delta = -self.level / release as f32;
    }

    fn next(&mut self, sample_rate: f32) -> f32 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Sustain => {
                self.level = BODY_SUSTAIN;
                self.level
            }
            EnvStage::Attack | EnvStage::Decay | EnvStage::Release => {
                if self.samples_left > 0 {
                    self.level = (self.level + self.delta).clamp(0.0, 1.0);
                    self.samples_left -= 1;
                    if self.samples_left == 0 {
                        self.level = self.target;
                        self.advance(sample_rate);
                    }
                } else {
                    self.advance(sample_rate);
                }
                self.level
            }
        }
    }

    fn advance(&mut self, sample_rate: f32) {
        match self.stage {
            EnvStage::Attack => self.enter_decay(sample_rate),
            EnvStage::Decay => {
                self.stage = EnvStage::Sustain;
                self.level = BODY_SUSTAIN;
                self.samples_left = 0;
                self.delta = 0.0;
            }
            EnvStage::Release => self.idle(),
            EnvStage::Idle | EnvStage::Sustain => {}
        }
    }

    fn enter_decay(&mut self, sample_rate: f32) {
        let decay = ms_to_samples(BODY_DECAY_MS, sample_rate);
        if decay == 0 || (self.level - BODY_SUSTAIN).abs() < 1e-6 {
            self.stage = EnvStage::Sustain;
            self.level = BODY_SUSTAIN;
            self.samples_left = 0;
            self.delta = 0.0;
        } else {
            self.stage = EnvStage::Decay;
            self.target = BODY_SUSTAIN;
            self.samples_left = decay;
            self.delta = (BODY_SUSTAIN - self.level) / decay as f32;
        }
    }

    fn idle(&mut self) {
        self.stage = EnvStage::Idle;
        self.level = 0.0;
        self.samples_left = 0;
        self.delta = 0.0;
    }

    fn is_active(&self) -> bool {
        !matches!(self.stage, EnvStage::Idle)
    }
}

#[derive(Debug, Clone, Copy)]
struct OneShot {
    level: f32,
    coeff: f32,
}

impl Default for OneShot {
    fn default() -> Self {
        Self {
            level: 0.0,
            coeff: 0.0,
        }
    }
}

impl OneShot {
    fn trigger(&mut self, decay_ms: f32, sample_rate: f32) {
        self.level = 1.0;
        let samples = (decay_ms.max(0.1) * 0.001 * sample_rate.max(1.0)).max(1.0);
        self.coeff = (-1.0 / samples).exp();
    }

    #[inline]
    fn next(&mut self) -> f32 {
        let out = self.level;
        self.level *= self.coeff;
        if self.level < 1e-5 {
            self.level = 0.0;
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct Voice {
    pub note: u8,
    pub channel: u8,
    pub age: u64,
    velocity: f32,
    fund_hz: f32,
    body_phase: f32,
    oct_phase: f32,
    bell_phase: f32,
    body: Adsr,
    bell: OneShot,
    hammer: OneShot,
    noise: u32,
    filter: OnePoleLp,
}

impl Voice {
    pub fn start(
        note: u8,
        velocity: u8,
        channel: u8,
        age: u64,
        params: &HazeParams,
        sample_rate: f32,
    ) -> Self {
        let mut voice = Self {
            note: note.min(127),
            channel: channel & 0x0f,
            age,
            velocity: 0.0,
            fund_hz: 0.0,
            body_phase: 0.0,
            oct_phase: 0.0,
            bell_phase: 0.0,
            body: Adsr::default(),
            bell: OneShot::default(),
            hammer: OneShot::default(),
            noise: 0xA341_316C ^ (note as u32).wrapping_mul(0x9E37_79B9),
            filter: OnePoleLp::default(),
        };
        voice.retrigger(velocity, params, sample_rate);
        voice
    }

    pub fn retrigger(&mut self, velocity: u8, params: &HazeParams, sample_rate: f32) {
        let velocity = velocity.min(127);
        self.velocity = velocity as f32 / 127.0;
        self.fund_hz = midi_note_to_hz(self.note);
        self.body_phase = 0.0;
        self.oct_phase = 0.0;
        self.bell_phase = 0.0;
        self.body.note_on(params.attack_ms, sample_rate);
        self.bell.trigger(BELL_DECAY_MS, sample_rate);
        self.hammer.trigger(HAMMER_DECAY_MS, sample_rate);
        self.filter.reset();
        self.set_tone(params.tone, sample_rate);
    }

    pub fn note_off(&mut self, params: &HazeParams, sample_rate: f32) {
        self.body.note_off(params.release_ms, sample_rate);
    }

    pub fn stage(&self) -> EnvStage {
        self.body.stage
    }

    pub fn is_active(&self) -> bool {
        self.body.is_active()
    }

    pub fn tick(&mut self, params: &HazeParams, pitch_ratio: f32, sample_rate: f32) -> f32 {
        self.set_tone(params.tone, sample_rate);
        let env = self.body.next(sample_rate);
        if !self.body.is_active() {
            return 0.0;
        }

        let fund = self.fund_hz * pitch_ratio;
        let nyquist = sample_rate * 0.45;
        let body_inc = (fund / sample_rate).min(0.49);
        let oct_inc = ((fund * 2.0) / sample_rate).min(0.49);
        let bell_hz = (fund * BELL_RATIO).min(nyquist);
        let bell_inc = bell_hz / sample_rate;

        let body = (self.body_phase * TAU).sin() * 0.72 + (self.oct_phase * TAU).sin() * 0.18;
        let vel2 = self.velocity * self.velocity;
        let bell =
            (self.bell_phase * TAU).sin() * self.bell.next() * params.bell * (0.22 + 0.78 * vel2);
        let hammer = noise_tick(&mut self.noise) * self.hammer.next() * 0.14 * self.velocity;
        let raw = (body + bell + hammer) * env * 0.20 * (0.55 + 0.45 * self.velocity);

        self.body_phase = wrap(self.body_phase + body_inc);
        self.oct_phase = wrap(self.oct_phase + oct_inc);
        self.bell_phase = wrap(self.bell_phase + bell_inc);

        self.filter.process(raw)
    }

    fn set_tone(&mut self, tone: f32, sample_rate: f32) {
        let ceiling = 180.0 * 48.0_f32.powf(tone);
        let vel_open = 0.38 + 0.62 * self.velocity;
        self.filter.set_cutoff(ceiling * vel_open, sample_rate);
    }
}

#[inline]
fn wrap(phase: f32) -> f32 {
    phase - phase.floor()
}

#[inline]
fn ms_to_samples(ms: f32, sample_rate: f32) -> u32 {
    (ms.max(0.0) * 0.001 * sample_rate.max(1.0)).round() as u32
}
