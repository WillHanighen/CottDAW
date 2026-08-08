use serde::{Deserialize, Serialize};

/// ADSR times in milliseconds + sustain level in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AdsrParams {
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack_ms: 10.0,
            decay_ms: 100.0,
            sustain: 0.7,
            release_ms: 200.0,
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
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone)]
pub struct AdsrState {
    stage: AdsrStage,
    level: f32,
    /// Samples remaining in the current linear segment (attack/decay/release).
    samples_left: u32,
    /// Level delta per sample for the current segment.
    delta: f32,
    /// Target level at the end of the current segment.
    target: f32,
}

impl Default for AdsrState {
    fn default() -> Self {
        Self {
            stage: AdsrStage::Idle,
            level: 0.0,
            samples_left: 0,
            delta: 0.0,
            target: 0.0,
        }
    }
}

impl AdsrState {
    pub fn stage(&self) -> AdsrStage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.stage, AdsrStage::Idle)
    }

    pub fn note_on(&mut self, params: &AdsrParams, sample_rate: f32) {
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        let attack_samples = ms_to_samples(params.attack_ms, sr);
        if attack_samples == 0 {
            self.level = 1.0;
            self.enter_decay(&params, sr);
        } else {
            self.stage = AdsrStage::Attack;
            self.target = 1.0;
            self.samples_left = attack_samples;
            self.delta = (1.0 - self.level) / attack_samples as f32;
        }
    }

    pub fn note_off(&mut self, params: &AdsrParams, sample_rate: f32) {
        if matches!(self.stage, AdsrStage::Idle) {
            return;
        }
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        let release_samples = ms_to_samples(params.release_ms, sr);
        if release_samples == 0 || self.level <= 0.0 {
            self.level = 0.0;
            self.stage = AdsrStage::Idle;
            self.samples_left = 0;
            self.delta = 0.0;
            return;
        }
        self.stage = AdsrStage::Release;
        self.target = 0.0;
        self.samples_left = release_samples;
        self.delta = -self.level / release_samples as f32;
    }

    /// Advance one sample; returns the current envelope level after the step.
    pub fn next_sample(&mut self, params: &AdsrParams, sample_rate: f32) -> f32 {
        match self.stage {
            AdsrStage::Idle => 0.0,
            AdsrStage::Sustain => {
                self.level = params.clamped().sustain;
                self.level
            }
            AdsrStage::Attack | AdsrStage::Decay | AdsrStage::Release => {
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
            AdsrStage::Attack => self.enter_decay(&params, sr),
            AdsrStage::Decay => {
                self.stage = AdsrStage::Sustain;
                self.level = params.sustain;
                self.samples_left = 0;
                self.delta = 0.0;
            }
            AdsrStage::Release => {
                self.stage = AdsrStage::Idle;
                self.level = 0.0;
                self.samples_left = 0;
                self.delta = 0.0;
            }
            AdsrStage::Idle | AdsrStage::Sustain => {}
        }
    }

    fn enter_decay(&mut self, params: &AdsrParams, sample_rate: f32) {
        let decay_samples = ms_to_samples(params.decay_ms, sample_rate);
        let sustain = params.sustain;
        if decay_samples == 0 || (self.level - sustain).abs() < 1e-6 {
            self.stage = AdsrStage::Sustain;
            self.level = sustain;
            self.samples_left = 0;
            self.delta = 0.0;
        } else {
            self.stage = AdsrStage::Decay;
            self.target = sustain;
            self.samples_left = decay_samples;
            self.delta = (sustain - self.level) / decay_samples as f32;
        }
    }
}

#[inline]
fn ms_to_samples(ms: f32, sample_rate: f32) -> u32 {
    ((ms.max(0.0) * 0.001 * sample_rate).round() as u32).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_reaches_peak() {
        let params = AdsrParams {
            attack_ms: 10.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 10.0,
        };
        let mut env = AdsrState::default();
        env.note_on(&params, 1000.0); // 10 samples attack
        let mut peak = 0.0f32;
        for _ in 0..20 {
            peak = peak.max(env.next_sample(&params, 1000.0));
        }
        assert!((peak - 1.0).abs() < 1e-3);
        assert_eq!(env.stage(), AdsrStage::Sustain);
    }

    #[test]
    fn release_returns_to_idle() {
        let params = AdsrParams {
            attack_ms: 0.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 5.0,
        };
        let mut env = AdsrState::default();
        env.note_on(&params, 1000.0);
        let _ = env.next_sample(&params, 1000.0);
        env.note_off(&params, 1000.0);
        for _ in 0..10 {
            let _ = env.next_sample(&params, 1000.0);
        }
        assert_eq!(env.stage(), AdsrStage::Idle);
        assert!(env.level() < 1e-4);
    }
}
