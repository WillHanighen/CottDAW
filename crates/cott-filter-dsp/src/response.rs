//! Frequency response of the current filter setting, for the plugin display.

use crate::biquad::{design_biquad, BiquadCoeffs};
use crate::FilterParams;

/// Evaluates |H(e^jw)| for a filter setting, including the dry/wet mix.
#[derive(Debug, Clone, Copy)]
pub struct ResponseProbe {
    coeffs: BiquadCoeffs,
    mix: f32,
    sample_rate: f32,
}

impl ResponseProbe {
    pub fn new(params: &FilterParams, sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let cutoff = params.cutoff_hz.clamp(20.0, sample_rate * 0.49);
        let q = params.q.clamp(0.1, 20.0);
        Self {
            coeffs: design_biquad(params.mode, cutoff, q, sample_rate),
            mix: params.mix.clamp(0.0, 1.0),
            sample_rate,
        }
    }

    /// Linear magnitude at `freq_hz`, with the dry path mixed back in.
    pub fn magnitude(&self, freq_hz: f32) -> f32 {
        let w = std::f32::consts::TAU * freq_hz.clamp(0.0, self.sample_rate * 0.5)
            / self.sample_rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let c = &self.coeffs;

        // Numerator and denominator of the transfer function on the unit circle.
        let num_re = c.b0 + c.b1 * c1 + c.b2 * c2;
        let num_im = -(c.b1 * s1 + c.b2 * s2);
        let den_re = 1.0 + c.a1 * c1 + c.a2 * c2;
        let den_im = -(c.a1 * s1 + c.a2 * s2);

        let den_mag2 = den_re * den_re + den_im * den_im;
        if den_mag2 <= f32::MIN_POSITIVE {
            return 0.0;
        }
        let h_re = (num_re * den_re + num_im * den_im) / den_mag2;
        let h_im = (num_im * den_re - num_re * den_im) / den_mag2;

        // Dry and wet sum as complex signals, not magnitudes.
        let total_re = (1.0 - self.mix) + self.mix * h_re;
        let total_im = self.mix * h_im;
        (total_re * total_re + total_im * total_im).sqrt()
    }

    pub fn magnitude_db(&self, freq_hz: f32) -> f32 {
        20.0 * self.magnitude(freq_hz).max(1e-6).log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilterMode;

    #[test]
    fn lowpass_passes_lows_and_stops_highs() {
        let probe = ResponseProbe::new(
            &FilterParams {
                mode: FilterMode::LowPass,
                cutoff_hz: 1_000.0,
                q: 0.707,
                mix: 1.0,
            },
            48_000.0,
        );
        assert!(probe.magnitude_db(20.0).abs() < 0.5);
        assert!(probe.magnitude_db(1_000.0) < -2.0);
        assert!(probe.magnitude_db(10_000.0) < -30.0);
    }

    #[test]
    fn highpass_is_the_mirror() {
        let probe = ResponseProbe::new(
            &FilterParams {
                mode: FilterMode::HighPass,
                cutoff_hz: 1_000.0,
                q: 0.707,
                mix: 1.0,
            },
            48_000.0,
        );
        assert!(probe.magnitude_db(20.0) < -30.0);
        assert!(probe.magnitude_db(15_000.0).abs() < 1.0);
    }

    #[test]
    fn resonance_lifts_the_corner() {
        let peaky = ResponseProbe::new(
            &FilterParams {
                mode: FilterMode::LowPass,
                cutoff_hz: 1_000.0,
                q: 8.0,
                mix: 1.0,
            },
            48_000.0,
        );
        assert!(probe_peak_db(&peaky) > 12.0);
    }

    fn probe_peak_db(probe: &ResponseProbe) -> f32 {
        (1..2_000)
            .map(|i| probe.magnitude_db(i as f32 * 10.0))
            .fold(f32::NEG_INFINITY, f32::max)
    }

    #[test]
    fn dry_mix_is_flat() {
        let probe = ResponseProbe::new(
            &FilterParams {
                mode: FilterMode::LowPass,
                cutoff_hz: 500.0,
                q: 4.0,
                mix: 0.0,
            },
            48_000.0,
        );
        for f in [20.0, 500.0, 5_000.0, 18_000.0] {
            assert!(probe.magnitude_db(f).abs() < 1e-3, "{f} Hz should be flat");
        }
    }
}
