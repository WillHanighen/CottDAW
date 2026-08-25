use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// Oscillator shape selection.
///
/// `Super` is last so older patches that stored a waveform index keep mapping
/// Noise to Noise.
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
    /// Seven detuned saws (JP-8000 Super Saw model).
    Super,
}

impl Waveform {
    pub const ALL: [Waveform; 7] = [
        Waveform::Sine,
        Waveform::Saw,
        Waveform::Square,
        Waveform::Triangle,
        Waveform::Pulse,
        Waveform::Noise,
        Waveform::Super,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Triangle => "Triangle",
            Waveform::Pulse => "Pulse",
            Waveform::Noise => "Noise",
            Waveform::Super => "Super",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&w| w == self).unwrap_or(0)
    }
}

/// Szabo Super Saw side-oscillator frequency offsets (center is 0).
const SUPER_OFFSETS: [f32; 7] = [
    -0.11002313,
    -0.06288439,
    -0.01952356,
    0.0,
    0.01991221,
    0.06216538,
    0.10745242,
];
const SUPER_CENTER: usize = 3;

/// Stateful band-limited oscillator. Super Saw keeps seven phases and a
/// pitch-tracked one-pole high-pass; other shapes use `phases[0]`.
#[derive(Debug, Clone)]
pub struct Oscillator {
    phases: [f32; 7],
    hpf_lp: f32,
    noise: u32,
}

impl Oscillator {
    pub fn new(seed: u32) -> Self {
        let mut osc = Self {
            phases: [0.0; 7],
            hpf_lp: 0.0,
            noise: if seed == 0 { 0xA341_316C } else { seed },
        };
        osc.randomize_phases();
        osc
    }

    pub fn randomize_phases(&mut self) {
        for phase in &mut self.phases {
            *phase = xorshift_unit(&mut self.noise);
        }
        self.hpf_lp = 0.0;
    }

    /// Advance one sample at `freq` Hz.
    pub fn tick(
        &mut self,
        waveform: Waveform,
        freq: f32,
        sample_rate: f32,
        pulse_width: f32,
        super_detune: f32,
        super_mix: f32,
    ) -> f32 {
        let sr = sample_rate.max(1.0);
        let freq = freq.max(0.0);
        let dt = (freq / sr).clamp(0.0, 0.499);
        match waveform {
            Waveform::Sine => {
                let sample = (self.phases[0] * TAU).sin();
                advance_phase(&mut self.phases[0], dt);
                sample
            }
            Waveform::Saw => {
                let sample = poly_blep_saw(self.phases[0], dt);
                advance_phase(&mut self.phases[0], dt);
                sample
            }
            Waveform::Square => {
                let sample = poly_blep_pulse(self.phases[0], dt, 0.5);
                advance_phase(&mut self.phases[0], dt);
                sample
            }
            Waveform::Triangle => {
                let sample = poly_blamp_triangle(self.phases[0], dt);
                advance_phase(&mut self.phases[0], dt);
                sample
            }
            Waveform::Pulse => {
                let sample = poly_blep_pulse(self.phases[0], dt, pulse_width);
                advance_phase(&mut self.phases[0], dt);
                sample
            }
            Waveform::Noise => next_white(&mut self.noise),
            Waveform::Super => self.tick_super(freq, sr, dt, super_detune, super_mix),
        }
    }

    fn tick_super(
        &mut self,
        freq: f32,
        sr: f32,
        dt: f32,
        super_detune: f32,
        super_mix: f32,
    ) -> f32 {
        let amount = super_detune_amount(super_detune.clamp(0.0, 1.0));
        let center_g = super_center_gain(super_mix.clamp(0.0, 1.0));
        let side_g = super_side_gain(super_mix.clamp(0.0, 1.0));
        let mut mix = 0.0f32;
        for (i, phase) in self.phases.iter_mut().enumerate() {
            let ratio = 1.0 + SUPER_OFFSETS[i] * amount;
            let saw_dt = (dt * ratio).clamp(0.0, 0.499);
            let saw = poly_blep_saw(*phase, saw_dt);
            mix += saw * if i == SUPER_CENTER { center_g } else { side_g };
            advance_phase(phase, saw_dt);
        }
        one_pole_hp(&mut self.hpf_lp, mix, freq, sr)
    }
}

/// Preview one sample of `waveform` at normalized phase `[0, 1)`.
///
/// `dt` is the dummy phase increment so PolyBLEP/PolyBLAMP rounding shows on
/// the editor plate. `noise_state` is mutated for [`Waveform::Noise`].
#[inline]
pub fn sample_waveform(
    waveform: Waveform,
    phase: f32,
    dt: f32,
    pulse_width: f32,
    super_detune: f32,
    super_mix: f32,
    noise_state: &mut u32,
) -> f32 {
    let phase = wrap01(phase);
    let dt = dt.clamp(1.0e-6, 0.499);
    match waveform {
        Waveform::Sine => (phase * TAU).sin(),
        Waveform::Saw => poly_blep_saw(phase, dt),
        Waveform::Square => poly_blep_pulse(phase, dt, 0.5),
        Waveform::Triangle => poly_blamp_triangle(phase, dt),
        Waveform::Pulse => poly_blep_pulse(phase, dt, pulse_width),
        Waveform::Noise => next_white(noise_state),
        Waveform::Super => preview_super(phase, dt, super_detune, super_mix),
    }
}

fn preview_super(phase: f32, dt: f32, super_detune: f32, super_mix: f32) -> f32 {
    let amount = super_detune_amount(super_detune.clamp(0.0, 1.0));
    let center_g = super_center_gain(super_mix.clamp(0.0, 1.0));
    let side_g = super_side_gain(super_mix.clamp(0.0, 1.0));
    let mut mix = 0.0f32;
    for (i, offset) in SUPER_OFFSETS.iter().enumerate() {
        let ratio = (1.0 + offset * amount).max(0.001);
        let saw = poly_blep_saw(wrap01(phase * ratio), (dt * ratio).clamp(1.0e-6, 0.499));
        mix += saw * if i == SUPER_CENTER { center_g } else { side_g };
    }
    mix
}

/// 2-point PolyBLEP residual for a unit step at phase 0.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// 2-point PolyBLAMP residual for a unit slope discontinuity at phase 0.
#[inline]
fn poly_blamp(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt - 1.0;
        -x * x * x * dt * (1.0 / 3.0)
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt + 1.0;
        x * x * x * dt * (1.0 / 3.0)
    } else {
        0.0
    }
}

#[inline]
fn poly_blep_saw(phase: f32, dt: f32) -> f32 {
    (2.0 * phase - 1.0) - poly_blep(phase, dt)
}

#[inline]
fn poly_blep_pulse(phase: f32, dt: f32, pulse_width: f32) -> f32 {
    let width = clamp_pulse_width(pulse_width, dt);
    let naive = if phase < width { 1.0 } else { -1.0 };
    naive + poly_blep(phase, dt) - poly_blep(wrap01(phase - width), dt) - (2.0 * width - 1.0)
}

/// Sine-phase triangle (0 at origin) with PolyBLAMP at the two corners.
#[inline]
fn poly_blamp_triangle(phase: f32, dt: f32) -> f32 {
    let naive = if phase < 0.25 {
        phase * 4.0
    } else if phase < 0.75 {
        2.0 - phase * 4.0
    } else {
        phase * 4.0 - 4.0
    };
    // Slope is ±4. Peak at 0.25 jumps -8; trough at 0.75 jumps +8.
    naive - 8.0 * poly_blamp(wrap01(phase - 0.25), dt) + 8.0 * poly_blamp(wrap01(phase - 0.75), dt)
}

fn clamp_pulse_width(width: f32, dt: f32) -> f32 {
    let min_w = dt.clamp(0.05, 0.45);
    width.clamp(min_w, 1.0 - min_w)
}

/// Adam Szabo's 11th-order JP-8000 detune curve. `x` in `[0, 1]`.
fn super_detune_amount(x: f32) -> f32 {
    let x = x as f64;
    let y = 10028.7312891634 * x.powi(11)
        - 50818.8652045924 * x.powi(10)
        + 111363.4808729368 * x.powi(9)
        - 138150.6761080548 * x.powi(8)
        + 106649.6679158292 * x.powi(7)
        - 53046.9642751875 * x.powi(6)
        + 17019.9518580080 * x.powi(5)
        - 3425.0836591318 * x.powi(4)
        + 404.2703938388 * x.powi(3)
        - 24.1878824391 * x.powi(2)
        + 0.6717417634 * x
        + 0.0030115596;
    y as f32
}

fn super_center_gain(x: f32) -> f32 {
    -0.55366 * x + 0.99785
}

fn super_side_gain(x: f32) -> f32 {
    -0.73764 * x * x + 1.2841 * x + 0.044372
}

fn one_pole_hp(lp: &mut f32, x: f32, cutoff_hz: f32, sample_rate: f32) -> f32 {
    let coeff = 1.0 - (-TAU * cutoff_hz.max(1.0) / sample_rate).exp();
    *lp += coeff * (x - *lp);
    x - *lp
}

#[inline]
fn wrap01(phase: f32) -> f32 {
    phase.fract().rem_euclid(1.0)
}

#[inline]
fn advance_phase(phase: &mut f32, dt: f32) {
    *phase += dt;
    if *phase >= 1.0 {
        *phase -= phase.floor();
    }
}

fn next_white(state: &mut u32) -> f32 {
    (xorshift(state) as i32 as f32) * (1.0 / 2_147_483_648.0)
}

fn xorshift_unit(state: &mut u32) -> f32 {
    (xorshift(state) as f32) * (1.0 / 4_294_967_296.0)
}

fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    if x == 0 {
        x = 0xA341_316C;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_zero_at_origin() {
        let mut noise = 1u32;
        let s = sample_waveform(Waveform::Sine, 0.0, 1.0 / 64.0, 0.5, 0.5, 0.75, &mut noise);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn square_is_bipolar() {
        let mut noise = 1u32;
        let dt = 1.0 / 256.0;
        let hi = sample_waveform(Waveform::Square, 0.1, dt, 0.5, 0.5, 0.75, &mut noise);
        let lo = sample_waveform(Waveform::Square, 0.6, dt, 0.5, 0.5, 0.75, &mut noise);
        assert!(hi > 0.5, "square high was {hi}");
        assert!(lo < -0.5, "square low was {lo}");
    }

    #[test]
    fn pulse_respects_width() {
        let mut noise = 1u32;
        let dt = 1.0 / 256.0;
        let hi = sample_waveform(Waveform::Pulse, 0.15, dt, 0.3, 0.5, 0.75, &mut noise);
        let lo = sample_waveform(Waveform::Pulse, 0.5, dt, 0.3, 0.5, 0.75, &mut noise);
        assert!(hi > 0.5, "pulse high was {hi}");
        assert!(lo < -0.5, "pulse low was {lo}");
    }

    #[test]
    fn pulse_dc_near_zero() {
        let mut noise = 1u32;
        let dt = 1.0 / 512.0;
        for width in [0.1, 0.25, 0.5, 0.8] {
            let mut sum = 0.0f32;
            let n = 512;
            for i in 0..n {
                sum += sample_waveform(
                    Waveform::Pulse,
                    i as f32 / n as f32,
                    dt,
                    width,
                    0.5,
                    0.75,
                    &mut noise,
                );
            }
            let dc = sum / n as f32;
            assert!(dc.abs() < 0.05, "width {width} dc {dc}");
        }
    }

    #[test]
    fn noise_changes() {
        let mut noise = 42u32;
        let a = sample_waveform(Waveform::Noise, 0.0, 0.01, 0.5, 0.5, 0.75, &mut noise);
        let b = sample_waveform(Waveform::Noise, 0.0, 0.01, 0.5, 0.5, 0.75, &mut noise);
        assert_ne!(a, b);
    }

    #[test]
    fn super_mix_zero_is_mostly_center() {
        let mut noise = 1u32;
        let dt = 1.0 / 256.0;
        let mut super_sum = 0.0f32;
        let mut saw_sum = 0.0f32;
        let n = 256;
        for i in 0..n {
            let t = i as f32 / n as f32;
            super_sum += sample_waveform(Waveform::Super, t, dt, 0.5, 0.5, 0.0, &mut noise).abs();
            saw_sum += sample_waveform(Waveform::Saw, t, dt, 0.5, 0.5, 0.0, &mut noise).abs();
        }
        let ratio = super_sum / saw_sum.max(1.0e-6);
        assert!(
            (0.7..2.5).contains(&ratio),
            "mix 0 Super/Saw mean-abs ratio {ratio}"
        );
    }

    #[test]
    fn super_mix_raises_side_energy() {
        let n = 512;
        let dt = 1.0 / n as f32;
        let mut quiet = 0.0f32;
        let mut loud = 0.0f32;
        let mut noise = 1u32;
        for i in 0..n {
            let t = i as f32 / n as f32;
            quiet += sample_waveform(Waveform::Super, t, dt, 0.5, 1.0, 0.0, &mut noise).abs();
            loud += sample_waveform(Waveform::Super, t, dt, 0.5, 1.0, 1.0, &mut noise).abs();
        }
        assert!(loud > quiet * 1.15, "mix 1 {loud} vs mix 0 {quiet}");
    }

    #[test]
    fn super_is_last_waveform() {
        assert_eq!(Waveform::Noise.index(), 5);
        assert_eq!(Waveform::Super.index(), 6);
        assert_eq!(Waveform::from_index(5), Waveform::Noise);
        assert_eq!(Waveform::from_index(6), Waveform::Super);
    }

    #[test]
    fn oscillator_ticks_without_nan() {
        let mut osc = Oscillator::new(0xC0FF_EE42);
        for wave in Waveform::ALL {
            let s = osc.tick(wave, 440.0, 48_000.0, 0.25, 0.5, 0.75);
            assert!(s.is_finite(), "{wave:?} produced {s}");
        }
    }
}
