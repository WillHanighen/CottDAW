//! Dual envelopes: ADSR and AR (hold-at-peak). ROM-1/2/3 pick resistor times.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdsrParams {
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack_ms: 8.0,
            decay_ms: 80.0,
            sustain: 0.7,
            release_ms: 120.0,
        }
    }
}

impl AdsrParams {
    pub fn clamped(self) -> Self {
        Self {
            attack_ms: self.attack_ms.clamp(0.0, 10_000.0),
            decay_ms: self.decay_ms.clamp(0.0, 10_000.0),
            sustain: self.sustain.clamp(0.0, 1.0),
            release_ms: self.release_ms.clamp(0.0, 10_000.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone)]
pub struct AdsrState {
    stage: Stage,
    level: f32,
    samples_left: u32,
    delta: f32,
    target: f32,
}

impl Default for AdsrState {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            samples_left: 0,
            delta: 0.0,
            target: 0.0,
        }
    }
}

impl AdsrState {
    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.stage, Stage::Idle)
    }

    pub fn note_on(&mut self, params: &AdsrParams, sample_rate: f32) {
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        let attack_samples = ms_to_samples(params.attack_ms, sr);
        if attack_samples == 0 {
            self.level = 1.0;
            self.enter_decay(&params, sr);
        } else {
            self.stage = Stage::Attack;
            self.target = 1.0;
            self.samples_left = attack_samples;
            self.delta = (1.0 - self.level) / attack_samples as f32;
        }
    }

    pub fn note_off(&mut self, params: &AdsrParams, sample_rate: f32) {
        if matches!(self.stage, Stage::Idle) {
            return;
        }
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        let release_samples = ms_to_samples(params.release_ms, sr);
        if release_samples == 0 || self.level <= 0.0 {
            self.level = 0.0;
            self.stage = Stage::Idle;
            self.samples_left = 0;
            self.delta = 0.0;
            return;
        }
        self.stage = Stage::Release;
        self.target = 0.0;
        self.samples_left = release_samples;
        self.delta = -self.level / release_samples as f32;
    }

    pub fn next_sample(&mut self, params: &AdsrParams, sample_rate: f32) -> f32 {
        match self.stage {
            Stage::Idle => 0.0,
            Stage::Sustain => {
                self.level = params.clamped().sustain;
                self.level
            }
            Stage::Attack | Stage::Decay | Stage::Release => {
                if self.samples_left > 0 {
                    self.level = (self.level + self.delta).clamp(0.0, 1.0);
                    self.samples_left -= 1;
                    if self.samples_left == 0 {
                        self.level = self.target;
                        self.advance_stage(params, sample_rate);
                    }
                } else {
                    self.advance_stage(params, sample_rate);
                }
                self.level
            }
        }
    }

    fn advance_stage(&mut self, params: &AdsrParams, sample_rate: f32) {
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        match self.stage {
            Stage::Attack => self.enter_decay(&params, sr),
            Stage::Decay => {
                self.stage = Stage::Sustain;
                self.level = params.sustain;
                self.samples_left = 0;
                self.delta = 0.0;
            }
            Stage::Release => {
                self.stage = Stage::Idle;
                self.level = 0.0;
                self.samples_left = 0;
                self.delta = 0.0;
            }
            Stage::Idle | Stage::Sustain => {}
        }
    }

    fn enter_decay(&mut self, params: &AdsrParams, sample_rate: f32) {
        let decay_samples = ms_to_samples(params.decay_ms, sample_rate);
        let sustain = params.sustain;
        if decay_samples == 0 || (self.level - sustain).abs() < 1e-6 {
            self.stage = Stage::Sustain;
            self.level = sustain;
            self.samples_left = 0;
            self.delta = 0.0;
        } else {
            self.stage = Stage::Decay;
            self.target = sustain;
            self.samples_left = decay_samples;
            self.delta = (sustain - self.level) / decay_samples as f32;
        }
    }
}

/// Attack-sustain-release. Holds at 1 while the gate is high.
#[derive(Debug, Clone)]
pub struct ArState {
    inner: AdsrState,
}

impl Default for ArState {
    fn default() -> Self {
        Self {
            inner: AdsrState::default(),
        }
    }
}

impl ArState {
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    pub fn note_on(&mut self, attack_ms: f32, release_ms: f32, sample_rate: f32) {
        let params = AdsrParams {
            attack_ms,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms,
        };
        self.inner.note_on(&params, sample_rate);
    }

    pub fn note_off(&mut self, attack_ms: f32, release_ms: f32, sample_rate: f32) {
        let params = AdsrParams {
            attack_ms,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms,
        };
        self.inner.note_off(&params, sample_rate);
    }

    pub fn next_sample(&mut self, attack_ms: f32, release_ms: f32, sample_rate: f32) -> f32 {
        let params = AdsrParams {
            attack_ms,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms,
        };
        self.inner.next_sample(&params, sample_rate)
    }
}

fn ms_to_samples(ms: f32, sample_rate: f32) -> u32 {
    (ms.max(0.0) * 0.001 * sample_rate).round() as u32
}
