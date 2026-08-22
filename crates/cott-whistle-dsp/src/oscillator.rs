//! Band-limited pulse and staircase-saw, the Pro Soloist divider pair.
//!
//! A high-frequency oscillator feeds a six-stage divider (US3930429 FIG. 6,
//! SM 6.1). The saw is those six squares summed with weights 1/2^n (DAFx 2019).
//! Pulse widths are ROM-4 selects on the same taps: Dynamic, 1/14, 1/9, 1/64,
//! 1/2, 2/11. They are independent; analoguediehard notes they can combine.
//! Board A gate delay was not decoded from the schematic, so combination is
//! an OR of 0-phase pulses (the widest enabled duty). Dynamic pulse is a
//! separate ADSR-driven converter, not PWM of the 1/14 train.

/// Duty cycles stay off the rails so the wave never collapses to silence.
const MIN_PULSE_WIDTH: f32 = 0.015;

/// polyBLEP stops correcting anything useful once a cycle is this short.
const MAX_BLEP_DT: f32 = 0.16;

/// A band-limited edge needs this many samples of room on either side.
const EDGE_GUARD: f32 = 3.0;

/// Divider stages summed into the staircase. Patent FIG. 6: six taps.
const STAIRCASE_STAGES: usize = 6;

/// A full-scale ramp has this RMS; pulses are matched to it.
const SAW_RMS_SCALE: f32 = 0.577_350_3;

/// ROM-4 duties excluding Dynamic. Index matches bits 1..5.
pub const PULSE_DUTIES: [f32; 5] = [1.0 / 14.0, 1.0 / 9.0, 1.0 / 64.0, 0.5, 2.0 / 11.0];

/// One free-running phase accumulator shared by pulse and staircase.
#[derive(Debug, Clone, Copy, Default)]
pub struct Oscillator {
    phase: f32,
}

impl Oscillator {
    pub fn reset(&mut self, phase: f32) {
        self.phase = wrap01(phase);
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Advance by `dt` cycles. Both waves share the same divider phase.
    pub fn next(&mut self, dt: f32, pulse_bits: u8, dyn_width: f32) -> OscSample {
        self.phase = wrap01(self.phase + dt);
        OscSample {
            pulse: combined_pulse(self.phase, dt, pulse_bits, dyn_width),
            saw: staircase_saw(self.phase, dt),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OscSample {
    pub pulse: f32,
    pub saw: f32,
}

/// OR of the enabled ROM-4 pulse selects, DC-removed and levelled.
///
/// Bit 0 is the dynamic converter (`dyn_width`). Bits 1..5 are the named
/// duties in [`PULSE_DUTIES`]. Without decoded Board A delay, OR of 0-phase
/// pulses is the maximum enabled width.
pub fn combined_pulse(phase: f32, dt: f32, bits: u8, dyn_width: f32) -> f32 {
    if bits == 0 {
        return 0.0;
    }
    let mut width = 0.0f32;
    if bits & 1 != 0 {
        width = width.max(dyn_width);
    }
    for i in 0..5 {
        if bits & (1 << (i + 1)) != 0 {
            width = width.max(PULSE_DUTIES[i]);
        }
    }
    if width <= 0.0 {
        0.0
    } else {
        pulse(phase, dt, width)
    }
}

/// Rectangular wave with a `width` duty cycle, DC removed and levelled.
pub fn pulse(phase: f32, dt: f32, width: f32) -> f32 {
    let p = wrap01(phase);
    let dt = dt.clamp(0.0, MAX_BLEP_DT);
    let floor = (EDGE_GUARD * dt).max(MIN_PULSE_WIDTH);
    let w = width.clamp(floor, 1.0 - floor);

    let mut y = if p < w { 1.0 } else { -1.0 };
    y -= 2.0 * w - 1.0;
    y += poly_blep(p, dt);
    y -= poly_blep(wrap01(p - w), dt);

    y * SAW_RMS_SCALE / (2.0 * (w * (1.0 - w)).sqrt())
}

/// Ramp built from six octave squares, weights 1/2^n.
pub fn staircase_saw(phase: f32, dt: f32) -> f32 {
    let p = wrap01(phase);
    let mut sum = 0.0;
    let mut total = 0.0;
    let mut weight = 0.5;
    let mut stride = 1.0f32;

    for _ in 0..STAIRCASE_STAGES {
        let stage_dt = dt * stride;
        if stage_dt > MAX_BLEP_DT {
            break;
        }
        sum -= weight * square(wrap01(p * stride), stage_dt);
        total += weight;
        weight *= 0.5;
        stride *= 2.0;
    }

    if total <= 0.0 {
        return -square(p, dt.min(MAX_BLEP_DT));
    }
    sum / total
}

fn square(p: f32, dt: f32) -> f32 {
    let mut y = if p < 0.5 { 1.0 } else { -1.0 };
    y += poly_blep(p, dt);
    y -= poly_blep(wrap01(p - 0.5), dt);
    y
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

pub fn wrap01(x: f32) -> f32 {
    x - x.floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_is_half_duty() {
        let high = (0..64)
            .filter(|i| pulse(*i as f32 / 64.0, 0.0, 0.5) > 0.0)
            .count();
        assert!((30..=34).contains(&high), "square high samples: {high}");
    }

    #[test]
    fn a_narrow_pulse_spends_most_of_the_cycle_low() {
        let high = (0..128)
            .filter(|i| pulse(*i as f32 / 128.0, 0.0, 1.0 / 14.0) > 0.0)
            .count();
        assert!(high < 20, "1/14 pulse should be a short spike, got {high}");
    }

    #[test]
    fn the_staircase_is_rising() {
        let a = staircase_saw(0.1, 0.0);
        let b = staircase_saw(0.8, 0.0);
        assert!(b > a, "staircase should climb through the cycle");
    }

    #[test]
    fn combined_or_without_delay_is_the_widest_select() {
        let bits_14 = 1 << 1;
        let bits_both = (1 << 1) | (1 << 4);
        let a = combined_pulse(0.2, 0.0, bits_14, 0.5);
        let b = combined_pulse(0.2, 0.0, bits_both, 0.5);
        let half = pulse(0.2, 0.0, 0.5);
        assert!((b - half).abs() < 1e-5, "OR of 1/14 and 1/2 should be 1/2");
        assert!(a != b);
    }

    #[test]
    fn no_selects_is_silence() {
        assert_eq!(combined_pulse(0.3, 0.0, 0, 0.5), 0.0);
    }
}
