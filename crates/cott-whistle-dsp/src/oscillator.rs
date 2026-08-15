//! Band-limited pulse and staircase-saw generators.
//!
//! Every source in this instrument is a rectangular wave or something summed
//! out of rectangular waves, because that is what the records were made on.
//! The ARP Pro Soloist ran a high-frequency oscillator into a divider chain and
//! offered pulses at 1/14, 1/9, 2/11, 1/64 and square; its "sawtooth" was five
//! of those pulses added into a staircase rather than a real ramp generator.
//! The Minimoog leads that replaced it stacked a saw against a detuned square.
//! There is no sine oscillator here on purpose.

use serde::{Deserialize, Serialize};

/// The Pro Soloist's narrow pulse — the reed voices, and the worm, come from
/// this width.
pub const ARP_PULSE_WIDTH: f32 = 1.0 / 14.0;

/// Duty cycles are kept off the rails so the wave never collapses to silence.
const MIN_PULSE_WIDTH: f32 = 0.015;

/// polyBLEP stops correcting anything useful once a cycle is this short.
const MAX_BLEP_DT: f32 = 0.16;

/// A band-limited edge needs this many samples of room on either side. Two
/// edges closer together than twice this cannot both be corrected, which is why
/// narrow pulses have to open up as the pitch climbs.
const EDGE_GUARD: f32 = 3.0;

/// Divider stages summed into the staircase saw. The ARP used five.
const STAIRCASE_STAGES: usize = 6;

/// Which generator a slot in the mixer is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    /// Rectangular wave at the voice's programmed duty cycle.
    Pulse,
    /// Band-limited analog ramp — the Minimoog side of the family.
    Saw,
    /// Divider staircase standing in for a ramp — the ARP side.
    Staircase,
}

/// A single free-running phase accumulator.
#[derive(Debug, Clone, Copy, Default)]
pub struct Oscillator {
    phase: f32,
}

impl Oscillator {
    pub fn new(phase: f32) -> Self {
        Self {
            phase: wrap01(phase),
        }
    }

    pub fn reset(&mut self, phase: f32) {
        self.phase = wrap01(phase);
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Advance by `dt` cycles and return the wave at the new phase.
    pub fn next(&mut self, dt: f32, shape: Shape, pulse_width: f32) -> f32 {
        self.phase = wrap01(self.phase + dt);
        match shape {
            Shape::Pulse => pulse(self.phase, dt, pulse_width),
            Shape::Saw => analog_saw(self.phase, dt),
            Shape::Staircase => staircase_saw(self.phase, dt),
        }
    }
}

/// Rectangular wave with a `width` duty cycle, DC removed and levelled.
///
/// Both the level and the DC matter: a 1/14 pulse sits at its low rail for 93%
/// of every cycle, so left alone it would push a large offset into the filter
/// and be far quieter than a square. Centring it and dividing by its own RMS
/// means the duty cycle changes the timbre without changing the loudness.
///
/// High up the keyboard the duty is forced open far enough for both edges to be
/// band-limited. A real pulse generator runs into the same wall from the other
/// direction, as its rise time eats the narrow half of the cycle.
pub fn pulse(phase: f32, dt: f32, width: f32) -> f32 {
    let p = wrap01(phase);
    let dt = dt.clamp(0.0, MAX_BLEP_DT);
    let floor = (EDGE_GUARD * dt).max(MIN_PULSE_WIDTH);
    let w = width.clamp(floor, 1.0 - floor);

    let mut y = if p < w { 1.0 } else { -1.0 };
    y -= 2.0 * w - 1.0;
    y += poly_blep(p, dt);
    y -= poly_blep(wrap01(p - w), dt);

    // RMS of a centred rectangle is 2*sqrt(w(1-w)); a square lands on 1.
    y * SAW_RMS_SCALE / (2.0 * (w * (1.0 - w)).sqrt())
}

/// Band-limited rising ramp. This is what the Minimoog actually put out; the
/// ARP's "saw" was a staircase of pulses and lives in [`staircase_saw`].
pub fn analog_saw(phase: f32, dt: f32) -> f32 {
    let p = wrap01(phase);
    let dt = dt.clamp(0.0, MAX_BLEP_DT);
    let mut y = 2.0 * p - 1.0;
    y -= poly_blep(p, dt);
    y
}

/// A ramp built the ARP way: binary divider stages summed with halving weights.
///
/// Stage `k` is a square at `2^k` times the pitch carrying weight `2^-(k+1)`,
/// which converges on a rising ramp while leaving the staircase edges that make
/// the real thing grittier than a textbook sawtooth. Stages whose own pitch has
/// run past the point where band-limiting can hold them are dropped, so the top
/// of the keyboard thins out instead of folding back as aliasing.
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
        // Past the divider's useful range only the fundamental survives anyway.
        return -square(p, dt.min(MAX_BLEP_DT));
    }
    sum / total
}

/// Bipolar 50% rectangle, band-limited at both edges.
fn square(p: f32, dt: f32) -> f32 {
    let mut y = if p < 0.5 { 1.0 } else { -1.0 };
    y += poly_blep(p, dt);
    y -= poly_blep(wrap01(p - 0.5), dt);
    y
}

/// A full-scale ramp has this RMS; pulses are matched to it so the mixer's
/// blend control is a timbre change and not a volume change.
const SAW_RMS_SCALE: f32 = 0.577_350_3;

/// Polynomial band-limited step. Returns the residual to subtract from a
/// downward unit-times-two jump (add it for an upward one).
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

fn wrap01(x: f32) -> f32 {
    x - x.floor()
}

/// One cycle of the mixer's output for the editor's shape preview, scaled to
/// fill the well.
///
/// Drawn, not heard, so the band-limiting is left out.
pub fn preview_wave(phase: f32, blend: f32, pulse_width: f32, staircase: bool) -> f32 {
    let blend = blend.clamp(0.0, 1.0);
    let w = pulse_width.clamp(MIN_PULSE_WIDTH, 1.0 - MIN_PULSE_WIDTH);
    let saw = if staircase {
        staircase_saw(phase, 0.0)
    } else {
        analog_saw(phase, 0.0)
    };
    let raw = pulse(phase, 0.0, w) * (1.0 - blend) + saw * blend;
    // A narrow pulse spikes far above a ramp; normalise by the peak this
    // particular mix can reach so the preview always reads full height.
    let pulse_peak = (2.0 - 2.0 * w) * SAW_RMS_SCALE / (2.0 * (w * (1.0 - w)).sqrt());
    let peak = (pulse_peak * (1.0 - blend) + blend).max(1e-6);
    (raw / peak).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycle(f: impl Fn(f32) -> f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| f(i as f32 / n as f32)).collect()
    }

    fn mean(v: &[f32]) -> f32 {
        v.iter().sum::<f32>() / v.len() as f32
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn pulse_has_no_dc_at_any_width() {
        for width in [ARP_PULSE_WIDTH, 0.1, 0.25, 0.5] {
            let wave = cycle(|p| pulse(p, 0.0, width), 4_096);
            assert!(
                mean(&wave).abs() < 0.02,
                "width {width} left {} of DC behind",
                mean(&wave)
            );
        }
    }

    #[test]
    fn narrow_and_square_pulses_match_in_level() {
        let narrow = rms(&cycle(|p| pulse(p, 0.0, ARP_PULSE_WIDTH), 4_096));
        let square = rms(&cycle(|p| pulse(p, 0.0, 0.5), 4_096));
        assert!(
            (narrow - square).abs() / square < 0.05,
            "narrow {narrow} vs square {square}"
        );
    }

    #[test]
    fn analog_saw_climbs_from_bottom_to_top() {
        let wave = cycle(|p| analog_saw(p, 0.0), 512);
        assert!(wave[8] < -0.85, "should start at the bottom: {}", wave[8]);
        assert!(wave[503] > 0.85, "should end at the top: {}", wave[503]);
        let coarse = cycle(|p| analog_saw(p, 0.0), 64);
        for pair in coarse.windows(2) {
            assert!(pair[1] >= pair[0] - 1e-4, "ramp reversed: {pair:?}");
        }
    }

    #[test]
    fn staircase_saw_climbs_from_bottom_to_top() {
        let wave = cycle(|p| staircase_saw(p, 0.0), 512);
        assert!(wave[8] < -0.85, "should start at the bottom: {}", wave[8]);
        assert!(wave[503] > 0.85, "should end at the top: {}", wave[503]);
        // Sampled coarser than the finest divider step, it is monotonic.
        let coarse = cycle(|p| staircase_saw(p, 0.0), 64);
        for pair in coarse.windows(2) {
            assert!(pair[1] >= pair[0] - 1e-4, "ramp reversed: {pair:?}");
        }
    }

    #[test]
    fn staircase_saw_is_grittier_than_a_plain_ramp() {
        // The divider edges are the point: measure the deviation from an ideal
        // ramp and make sure it is real but small.
        let n = 4_096;
        let error: f32 = (0..n)
            .map(|i| {
                let p = i as f32 / n as f32;
                (staircase_saw(p, 0.0) - (2.0 * p - 1.0)).abs()
            })
            .sum::<f32>()
            / n as f32;
        assert!(error > 0.005, "staircase is indistinguishable from a ramp");
        assert!(
            error < 0.2,
            "staircase strayed too far from a ramp: {error}"
        );
    }

    #[test]
    fn high_notes_drop_divider_stages_instead_of_folding_over() {
        // dt = 0.15 leaves room for one stage only.
        let wave = cycle(|p| staircase_saw(p, 0.15), 256);
        assert!(wave.iter().all(|s| s.is_finite() && s.abs() <= 2.0));
    }
}
