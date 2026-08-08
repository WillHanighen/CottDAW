use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// Oscillator shape selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    #[default]
    Sine,
    Saw,
    Square,
    Triangle,
    /// Variable pulse (duty cycle from `pulse_width`).
    Pulse,
    Noise,
}

impl Waveform {
    pub const ALL: [Waveform; 6] = [
        Waveform::Sine,
        Waveform::Saw,
        Waveform::Square,
        Waveform::Triangle,
        Waveform::Pulse,
        Waveform::Noise,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Triangle => "Triangle",
            Waveform::Pulse => "Pulse",
            Waveform::Noise => "Noise",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&w| w == self).unwrap_or(0)
    }
}

/// Sample one period of `waveform` at normalized phase `[0, 1)`.
///
/// `pulse_width` is the high-duty fraction for [`Waveform::Pulse`] (clamped to a
/// usable range so the pulse never collapses to silence).
/// `noise_state` is a simple LCG seed mutated on each noise sample.
#[inline]
pub fn sample_waveform(
    waveform: Waveform,
    phase: f32,
    pulse_width: f32,
    noise_state: &mut u32,
) -> f32 {
    let phase = phase.fract().rem_euclid(1.0);
    match waveform {
        Waveform::Sine => (phase * TAU).sin(),
        Waveform::Saw => 2.0 * phase - 1.0,
        Waveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Triangle => {
            // 0..0.25 → 0..1, 0.25..0.75 → 1..-1, 0.75..1 → -1..0
            if phase < 0.25 {
                phase * 4.0
            } else if phase < 0.75 {
                2.0 - phase * 4.0
            } else {
                phase * 4.0 - 4.0
            }
        }
        Waveform::Pulse => {
            let width = pulse_width.clamp(0.05, 0.95);
            if phase < width { 1.0 } else { -1.0 }
        }
        Waveform::Noise => {
            // xorshift32 — cheap, deterministic, non-allocating.
            let mut x = *noise_state;
            if x == 0 {
                x = 0xA341_316C;
            }
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *noise_state = x;
            (x as i32 as f32) * (1.0 / 2_147_483_648.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_zero_at_origin() {
        let mut noise = 1u32;
        let s = sample_waveform(Waveform::Sine, 0.0, 0.5, &mut noise);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn square_is_bipolar() {
        let mut noise = 1u32;
        assert!((sample_waveform(Waveform::Square, 0.0, 0.5, &mut noise) - 1.0).abs() < 1e-6);
        assert!((sample_waveform(Waveform::Square, 0.6, 0.5, &mut noise) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn pulse_respects_width() {
        let mut noise = 1u32;
        assert_eq!(sample_waveform(Waveform::Pulse, 0.2, 0.3, &mut noise), 1.0);
        assert_eq!(sample_waveform(Waveform::Pulse, 0.4, 0.3, &mut noise), -1.0);
    }

    #[test]
    fn noise_changes() {
        let mut noise = 42u32;
        let a = sample_waveform(Waveform::Noise, 0.0, 0.5, &mut noise);
        let b = sample_waveform(Waveform::Noise, 0.0, 0.5, &mut noise);
        assert_ne!(a, b);
    }
}
