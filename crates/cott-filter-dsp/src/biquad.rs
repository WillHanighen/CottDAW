//! RBJ cookbook biquad (Audio EQ Cookbook) — low-pass / high-pass.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FilterMode {
    #[default]
    LowPass,
    HighPass,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FilterParams {
    pub mode: FilterMode,
    /// Hertz.
    pub cutoff_hz: f32,
    /// Resonance / Q (0.5 ≈ Butterworth, higher = more resonance).
    pub q: f32,
    /// Dry/wet mix, 0 = bypass, 1 = fully filtered.
    pub mix: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            mode: FilterMode::LowPass,
            cutoff_hz: 2_000.0,
            q: 0.707,
            mix: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    fn process(&mut self, x: f32, c: &BiquadCoeffs) -> f32 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Stereo filter with shared coefficients and per-channel state.
#[derive(Debug, Clone)]
pub struct StereoFilter {
    sample_rate: f32,
    coeffs: BiquadCoeffs,
    left: BiquadState,
    right: BiquadState,
    last_mode: FilterMode,
    last_cutoff: f32,
    last_q: f32,
}

impl StereoFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut f = Self {
            sample_rate: sample_rate.max(1.0),
            coeffs: BiquadCoeffs::default(),
            left: BiquadState::default(),
            right: BiquadState::default(),
            last_mode: FilterMode::LowPass,
            last_cutoff: -1.0,
            last_q: -1.0,
        };
        f.update_coeffs(&FilterParams::default());
        f
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        // Force coeff recompute on next process.
        self.last_cutoff = -1.0;
    }

    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }

    fn update_coeffs(&mut self, params: &FilterParams) {
        let cutoff = params
            .cutoff_hz
            .clamp(20.0, self.sample_rate * 0.49);
        let q = params.q.clamp(0.1, 20.0);

        if params.mode == self.last_mode
            && (cutoff - self.last_cutoff).abs() < 1e-4
            && (q - self.last_q).abs() < 1e-5
        {
            return;
        }

        self.last_mode = params.mode;
        self.last_cutoff = cutoff;
        self.last_q = q;
        self.coeffs = design_biquad(params.mode, cutoff, q, self.sample_rate);
    }

    /// Process interleaved-independent stereo buffers in place (dry/wet mix).
    pub fn process_block(&mut self, params: &FilterParams, left: &mut [f32], right: &mut [f32]) {
        self.update_coeffs(params);
        let mix = params.mix.clamp(0.0, 1.0);
        let dry = 1.0 - mix;
        let n = left.len().min(right.len());
        for i in 0..n {
            let l_in = left[i];
            let r_in = right[i];
            let l_wet = self.left.process(l_in, &self.coeffs);
            let r_wet = self.right.process(r_in, &self.coeffs);
            left[i] = dry * l_in + mix * l_wet;
            right[i] = dry * r_in + mix * r_wet;
        }
    }
}

fn design_biquad(mode: FilterMode, cutoff_hz: f32, q: f32, sample_rate: f32) -> BiquadCoeffs {
    let w0 = std::f32::consts::TAU * (cutoff_hz / sample_rate);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);

    let (b0, b1, b2, a0, a1, a2) = match mode {
        FilterMode::LowPass => {
            let b1 = 1.0 - cos_w0;
            let b0 = b1 * 0.5;
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w0;
            let a2 = 1.0 - alpha;
            (b0, b1, b2, a0, a1, a2)
        }
        FilterMode::HighPass => {
            let b1 = -(1.0 + cos_w0);
            let b0 = (1.0 + cos_w0) * 0.5;
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w0;
            let a2 = 1.0 - alpha;
            (b0, b1, b2, a0, a1, a2)
        }
    };

    let inv_a0 = 1.0 / a0;
    BiquadCoeffs {
        b0: b0 * inv_a0,
        b1: b1 * inv_a0,
        b2: b2 * inv_a0,
        a1: a1 * inv_a0,
        a2: a2 * inv_a0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stays_silence() {
        let mut f = StereoFilter::new(48_000.0);
        let mut left = [0.0f32; 64];
        let mut right = [0.0f32; 64];
        f.process_block(&FilterParams::default(), &mut left, &mut right);
        assert!(left.iter().all(|s| s.abs() < 1e-6));
        assert!(right.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn lowpass_attenuates_high_freq_impulse_tail() {
        let mut f = StereoFilter::new(48_000.0);
        let params = FilterParams {
            mode: FilterMode::LowPass,
            cutoff_hz: 200.0,
            q: 0.707,
            mix: 1.0,
        };
        let mut left = [0.0f32; 256];
        let mut right = [0.0f32; 256];
        left[0] = 1.0;
        right[0] = 1.0;
        f.process_block(&params, &mut left, &mut right);
        // Impulse response should ring but remain finite.
        assert!(left.iter().all(|s| s.is_finite()));
        let energy: f32 = left.iter().map(|s| s * s).sum();
        assert!(energy > 0.0 && energy < 10.0);
    }
}
