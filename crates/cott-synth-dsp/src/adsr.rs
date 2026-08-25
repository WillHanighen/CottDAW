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

/// Analog-style one-pole ADSR. Attack charges toward 1.5 and flips at 1.0;
/// decay/release undershoot the target so they finish in finite time.
#[derive(Debug, Clone)]
pub struct AdsrState {
    stage: AdsrStage,
    level: f32,
    coeff: f32,
    target: f32,
}

impl Default for AdsrState {
    fn default() -> Self {
        Self {
            stage: AdsrStage::Idle,
            level: 0.0,
            coeff: 1.0,
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
        if params.attack_ms <= 0.0 {
            self.level = 1.0;
            self.enter_decay(&params, sr);
        } else {
            self.stage = AdsrStage::Attack;
            self.target = 1.5;
            self.coeff = analog_coeff(params.attack_ms, sr);
        }
    }

    pub fn note_off(&mut self, params: &AdsrParams, sample_rate: f32) {
        if matches!(self.stage, AdsrStage::Idle) {
            return;
        }
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        if params.release_ms <= 0.0 || self.level <= 0.0 {
            self.level = 0.0;
            self.stage = AdsrStage::Idle;
            self.coeff = 1.0;
            self.target = 0.0;
            return;
        }
        self.stage = AdsrStage::Release;
        self.target = -0.5 * self.level.max(1.0e-4);
        self.coeff = analog_coeff(params.release_ms, sr);
    }

    /// Advance one sample; returns the current envelope level after the step.
    pub fn next_sample(&mut self, params: &AdsrParams, sample_rate: f32) -> f32 {
        let params = params.clamped();
        let sr = sample_rate.max(1.0);
        match self.stage {
            AdsrStage::Idle => 0.0,
            AdsrStage::Sustain => {
                self.level = params.sustain;
                self.level
            }
            AdsrStage::Attack => {
                self.coeff = analog_coeff(params.attack_ms, sr);
                self.level += (self.target - self.level) * self.coeff;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.enter_decay(&params, sr);
                }
                self.level
            }
            AdsrStage::Decay => {
                self.coeff = analog_coeff(params.decay_ms, sr);
                self.target = decay_target(params.sustain);
                self.level += (self.target - self.level) * self.coeff;
                if self.level <= params.sustain {
                    self.level = params.sustain;
                    self.stage = AdsrStage::Sustain;
                }
                self.level
            }
            AdsrStage::Release => {
                self.coeff = analog_coeff(params.release_ms, sr);
                self.level += (self.target - self.level) * self.coeff;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
                self.level
            }
        }
    }

    fn enter_decay(&mut self, params: &AdsrParams, sample_rate: f32) {
        if params.decay_ms <= 0.0
            || self.level <= params.sustain
            || (self.level - params.sustain).abs() < 1e-6
        {
            self.stage = AdsrStage::Sustain;
            self.level = params.sustain;
            self.coeff = 1.0;
            self.target = params.sustain;
        } else {
            self.stage = AdsrStage::Decay;
            self.target = decay_target(params.sustain);
            self.coeff = analog_coeff(params.decay_ms, sample_rate);
        }
    }
}

fn decay_target(sustain: f32) -> f32 {
    sustain - 0.5 * (1.0 - sustain).max(1.0e-4)
}

/// One-pole coefficient that crosses the analog threshold in `ms` milliseconds.
/// `ln(3)` matches the 1.5× overshoot / 50% undershoot used by the stages.
fn analog_coeff(ms: f32, sample_rate: f32) -> f32 {
    let samples = ms.max(0.0) * 0.001 * sample_rate;
    if samples <= 0.5 {
        1.0
    } else {
        1.0 - (-LN_3 / samples).exp()
    }
}

const LN_3: f32 = 1.098_612_3;

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
        env.note_on(&params, 1000.0);
        let mut peak = 0.0f32;
        for _ in 0..20 {
            peak = peak.max(env.next_sample(&params, 1000.0));
        }
        assert!((peak - 1.0).abs() < 1e-3, "peak {peak}");
        assert_eq!(env.stage(), AdsrStage::Sustain);
    }

    #[test]
    fn zero_ms_attack_is_instant() {
        let params = AdsrParams {
            attack_ms: 0.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 10.0,
        };
        let mut env = AdsrState::default();
        env.note_on(&params, 1000.0);
        let s = env.next_sample(&params, 1000.0);
        assert!((s - 1.0).abs() < 1e-5, "level {s}");
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
        for _ in 0..16 {
            let _ = env.next_sample(&params, 1000.0);
        }
        assert_eq!(env.stage(), AdsrStage::Idle);
        assert!(env.level() < 1e-4);
    }
}
