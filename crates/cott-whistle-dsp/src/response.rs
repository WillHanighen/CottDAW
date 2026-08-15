//! Analytic magnitude response of the voice's filter section, for the editor.
//!
//! The panel needs to show where the ladder is sitting *and* where the fixed
//! resonators are, because with the bank engaged those peaks are the voice and
//! the cutoff is only trimming them. Everything here is closed form; nothing
//! runs the audio path.

use crate::engine::WhistleParams;
use crate::filter::{BANK_MAKEUP, ResonatorSpec};

/// Lowest frequency the editor plots.
pub const PLOT_MIN_HZ: f32 = 40.0;
/// Highest frequency the editor plots.
pub const PLOT_MAX_HZ: f32 = 18_000.0;

/// Map a 0..1 position across the plot to a frequency, logarithmically.
pub fn plot_hz(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    PLOT_MIN_HZ * (PLOT_MAX_HZ / PLOT_MIN_HZ).powf(t)
}

/// Inverse of [`plot_hz`]: where a frequency lands across the plot.
pub fn plot_position(hz: f32) -> f32 {
    let hz = hz.clamp(PLOT_MIN_HZ, PLOT_MAX_HZ);
    (hz / PLOT_MIN_HZ).ln() / (PLOT_MAX_HZ / PLOT_MIN_HZ).ln()
}

/// Linear magnitude the whole filter section applies at `freq_hz` when the
/// voice is playing `note_hz`.
pub fn voice_magnitude(params: &WhistleParams, note_hz: f32, freq_hz: f32) -> f32 {
    let params = params.clamped();
    let recipe = params.character.recipe();
    let w = freq_hz.max(1.0);

    let pre = one_pole_hp(w, recipe.hp_hz);
    let bank = bank_magnitude(&recipe.resonators, w);

    let body = params.body;
    let direct = pre * (1.0 - body);
    let banked = pre * bank * body;

    let to_vcf = direct + banked * recipe.resonator_to_vcf;
    let bypass = banked * (1.0 - recipe.resonator_to_vcf);

    let cutoff = params.cutoff_hz(note_hz).clamp(60.0, PLOT_MAX_HZ);
    to_vcf * ladder_lp(w, cutoff, params.emphasis) + bypass
}

/// As [`voice_magnitude`], in decibels.
pub fn voice_magnitude_db(params: &WhistleParams, note_hz: f32, freq_hz: f32) -> f32 {
    20.0 * voice_magnitude(params, note_hz, freq_hz).max(1e-6).log10()
}

/// Four-pole ladder with feedback `emphasis`, including the passband makeup the
/// engine applies so the plot matches what is heard.
fn ladder_lp(w: f32, cutoff_hz: f32, emphasis: f32) -> f32 {
    let k = emphasis.clamp(0.0, 1.0) * 3.85;
    let x = w / cutoff_hz.max(1.0);
    let x2 = x * x;
    // (1 + jx)^4 = (1 - 6x^2 + x^4) + j(4x - 4x^3)
    let re = 1.0 - 6.0 * x2 + x2 * x2 + k;
    let im = 4.0 * x - 4.0 * x * x2;
    (1.0 + k * 0.3) / (re * re + im * im).sqrt().max(1e-9)
}

/// Two-pole band-pass with unity peak gain.
fn band_pass(w: f32, freq_hz: f32, q: f32) -> f32 {
    let w0 = freq_hz.max(1.0);
    let q = q.clamp(0.2, 24.0);
    let r = w / w0;
    let num = r / q;
    let den = ((1.0 - r * r).powi(2) + num * num).sqrt();
    num / den.max(1e-9)
}

fn bank_magnitude(specs: &[ResonatorSpec], w: f32) -> f32 {
    let total: f32 = specs.iter().map(|s| s.gain.max(0.0)).sum();
    if total <= 1e-6 {
        return 0.0;
    }
    let sum: f32 = specs
        .iter()
        .map(|s| band_pass(w, s.freq_hz, s.q) * s.gain.max(0.0))
        .sum();
    sum / total * BANK_MAKEUP
}

fn one_pole_hp(w: f32, cutoff_hz: f32) -> f32 {
    let r = w / cutoff_hz.max(1.0);
    r / (1.0 + r * r).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Character;
    use crate::filter::LadderLp;

    /// Level of a steady tone at `hz` after the filter, relative to its input.
    fn measure_ladder(cutoff: f32, emphasis: f32, hz: f32, sample_rate: f32) -> f32 {
        let mut ladder = LadderLp::new(cutoff, emphasis, sample_rate);
        let n = sample_rate as usize;
        let mut out = vec![0.0f32; n];
        // Small enough that the ladder's input saturation cannot colour it.
        let amp = 0.01;
        for (i, o) in out.iter_mut().enumerate() {
            let x = amp * (std::f32::consts::TAU * hz * i as f32 / sample_rate).sin();
            *o = ladder.process(x);
        }

        let tail = &out[n / 2..];
        let m = tail.len();
        let w = std::f64::consts::TAU * hz as f64 / sample_rate as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in tail.iter().enumerate() {
            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / m as f64).cos();
            re += *s as f64 * win * (w * i as f64).cos();
            im += *s as f64 * win * (w * i as f64).sin();
        }
        (((re * re + im * im).sqrt() * 4.0 / m as f64) as f32) / amp
    }

    #[test]
    fn the_plotted_ladder_is_the_ladder_that_runs() {
        // The editor draws a closed-form curve while the audio path runs a
        // discrete filter. If those two drift apart the panel starts lying.
        let sample_rate = 48_000.0;
        for (cutoff, emphasis) in [(4_000.0f32, 0.2f32), (3_000.0, 0.45), (7_000.0, 0.7)] {
            for hz in [200.0f32, 523.0, 1_047.0, 2_094.0, 3_141.0, 4_188.0] {
                let measured = measure_ladder(cutoff, emphasis, hz, sample_rate);
                let plotted = ladder_lp(hz, cutoff, emphasis);
                // A tenth: the discrete cascade sits a little under the analog
                // prototype right on the resonant peak, and nowhere else.
                assert!(
                    (measured - plotted).abs() < 0.10 * plotted.max(0.1),
                    "cutoff {cutoff} emphasis {emphasis} at {hz} Hz: \
                     ran {measured}, plotted {plotted}"
                );
            }
        }
    }

    #[test]
    fn plot_mapping_round_trips() {
        for hz in [50.0, 440.0, 2_000.0, 10_000.0] {
            let back = plot_hz(plot_position(hz));
            assert!((back - hz).abs() / hz < 1e-3, "{hz} -> {back}");
        }
    }

    #[test]
    fn the_ladder_rolls_off_above_its_cutoff() {
        let params = WhistleParams {
            body: 0.0,
            emphasis: 0.2,
            ..WhistleParams::for_character(Character::WestCoast)
        };
        let note = 523.0;
        let cutoff = params.cutoff_hz(note);
        let at = voice_magnitude(&params, note, cutoff);
        let above = voice_magnitude(&params, note, cutoff * 4.0);
        assert!(above < at * 0.2, "no roll-off: {at} then {above}");
    }

    #[test]
    fn the_reed_bank_puts_peaks_where_the_resonators_are() {
        let params = WhistleParams::for_character(Character::Worm);
        let recipe = Character::Worm.recipe();
        let peak_hz = recipe.resonators[0].freq_hz;
        let note = 523.0;

        let at_peak = voice_magnitude(&params, note, peak_hz);
        let below = voice_magnitude(&params, note, peak_hz * 0.35);
        assert!(
            at_peak > below * 1.5,
            "resonator peak did not show: {at_peak} vs {below}"
        );
    }
}
