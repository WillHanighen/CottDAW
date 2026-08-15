//! The four voices this instrument ships with, and the circuit each one wires
//! up behind the panel.
//!
//! A [`Character`] is not a preset in the usual sense. It sets the parts of the
//! signal path that were never on the front of the original machines — the
//! pulse width burned into the Pro Soloist's voice ROM, where its resonators
//! sat, whether their output went on into the VCF or straight to the VCA, how
//! far the VCF can be opened. The panel controls then move within that circuit.
//! Selecting a character in the editor also stamps its calibrated settings onto
//! those controls, which is the part that gets you the record.

use serde::{Deserialize, Serialize};

use crate::filter::{ResonatorSpec, BANK_SIZE};
use crate::oscillator::ARP_PULSE_WIDTH;

/// Which lead the voice is wired for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Character {
    /// ARP Pro Soloist "Oboe": a 1/14 pulse through the reed resonators and on
    /// into the VCF, with the portamento wound right up. This is the Funky Worm
    /// solo, and everything the 90s sampled from it.
    #[default]
    Worm,
    /// The Minimoog recreation Dre reached for once the sample had been worn
    /// out: a 2' saw against a slightly detuned square, cutoff well down,
    /// moderate emphasis, short glide.
    WestCoast,
    /// The rounder, further-back version of the same idea heard on the mid-90s
    /// crossover records — saw-led, gentler emphasis, a little body under it.
    Silk,
    /// Tighter and brighter, with the fast articulate glide of the later
    /// game-score take on G-funk.
    SanAndreas,
}

impl Character {
    pub const ALL: [Character; 4] = [
        Character::Worm,
        Character::WestCoast,
        Character::Silk,
        Character::SanAndreas,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Character::Worm => "Worm",
            Character::WestCoast => "West Coast",
            Character::Silk => "Silk",
            Character::SanAndreas => "San Andreas",
        }
    }

    /// Short blurb for the editor's readout.
    pub fn blurb(self) -> &'static str {
        match self {
            Character::Worm => "1/14 PULSE - REED BANK - VCF",
            Character::WestCoast => "SAW + SQUARE - LADDER",
            Character::Silk => "SAW LED - SOFT BODY",
            Character::SanAndreas => "NARROW PULSE - TIGHT GLIDE",
        }
    }

    pub fn recipe(self) -> Recipe {
        match self {
            // "1/14 pulse -> Sharpen III -> VCF -> VCA", with the resonator
            // sitting on an oboe's first three formants, and the portamento
            // slider at the top of its travel.
            Character::Worm => Recipe {
                pulse_width: ARP_PULSE_WIDTH,
                staircase: true,
                hp_hz: 420.0,
                cutoff_octaves: (1.4, 3.8),
                key_track: 0.55,
                resonators: [
                    ResonatorSpec::new(1_150.0, 4.2, 1.0),
                    ResonatorSpec::new(1_950.0, 5.0, 0.7),
                    ResonatorSpec::new(2_950.0, 6.0, 0.45),
                ],
                resonator_to_vcf: 1.0,
                defaults: Settings {
                    glide_ms: 300.0,
                    octave: 2,
                    blend: 0.0,
                    detune_cents: 0.0,
                    brilliance: 0.62,
                    emphasis: 0.22,
                    body: 0.78,
                    vibrato_hz: 5.2,
                    vibrato_cents: 18.0,
                    vibrato_delay_ms: 260.0,
                    attack_ms: 18.0,
                    release_ms: 220.0,
                    drive: 0.08,
                    gain: 0.5,
                },
            },
            // Osc 1 saw at 2', osc 2 square at 2' detuned a touch. Cutoff sits
            // just above the note so the ladder's resonant peak *is* the
            // whistle, not a dark mid-range lead two octaves under it.
            Character::WestCoast => Recipe {
                pulse_width: 0.5,
                staircase: false,
                hp_hz: 220.0,
                cutoff_octaves: (0.25, 2.0),
                key_track: 0.72,
                resonators: [
                    ResonatorSpec::new(760.0, 1.3, 1.0),
                    ResonatorSpec::new(1_500.0, 1.8, 0.55),
                    ResonatorSpec::new(2_600.0, 2.4, 0.3),
                ],
                resonator_to_vcf: 0.7,
                defaults: Settings {
                    glide_ms: 110.0,
                    octave: 2,
                    blend: 0.55,
                    detune_cents: 7.0,
                    brilliance: 0.52,
                    emphasis: 0.62,
                    body: 0.0,
                    vibrato_hz: 5.0,
                    vibrato_cents: 8.0,
                    vibrato_delay_ms: 400.0,
                    attack_ms: 4.0,
                    release_ms: 180.0,
                    drive: 0.10,
                    gain: 0.5,
                },
            },
            Character::Silk => Recipe {
                pulse_width: 0.28,
                staircase: false,
                hp_hz: 260.0,
                cutoff_octaves: (0.35, 2.2),
                key_track: 0.68,
                resonators: [
                    ResonatorSpec::new(820.0, 1.6, 1.0),
                    ResonatorSpec::new(1_620.0, 2.2, 0.5),
                    ResonatorSpec::new(2_600.0, 3.0, 0.25),
                ],
                resonator_to_vcf: 0.6,
                defaults: Settings {
                    glide_ms: 170.0,
                    octave: 2,
                    blend: 0.70,
                    detune_cents: 4.0,
                    brilliance: 0.56,
                    emphasis: 0.28,
                    body: 0.18,
                    vibrato_hz: 4.6,
                    vibrato_cents: 10.0,
                    vibrato_delay_ms: 320.0,
                    attack_ms: 10.0,
                    release_ms: 280.0,
                    drive: 0.06,
                    gain: 0.5,
                },
            },
            Character::SanAndreas => Recipe {
                pulse_width: 0.16,
                staircase: false,
                hp_hz: 320.0,
                cutoff_octaves: (0.5, 2.6),
                key_track: 0.78,
                resonators: [
                    ResonatorSpec::new(1_300.0, 3.0, 1.0),
                    ResonatorSpec::new(2_400.0, 3.6, 0.6),
                    ResonatorSpec::new(3_400.0, 4.2, 0.35),
                ],
                resonator_to_vcf: 0.8,
                defaults: Settings {
                    glide_ms: 70.0,
                    octave: 2,
                    blend: 0.30,
                    detune_cents: 8.0,
                    brilliance: 0.58,
                    emphasis: 0.50,
                    body: 0.14,
                    vibrato_hz: 5.8,
                    vibrato_cents: 9.0,
                    vibrato_delay_ms: 180.0,
                    attack_ms: 3.0,
                    release_ms: 140.0,
                    drive: 0.08,
                    gain: 0.5,
                },
            },
        }
    }
}

/// The wiring behind a character: everything the panel does not reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    /// Duty cycle of the rectangular oscillator.
    pub pulse_width: f32,
    /// Whether the saw slot is the ARP divider staircase. The Minimoog voices
    /// leave this off and run a real ramp instead.
    pub staircase: bool,
    /// Fixed high-pass ahead of the VCF.
    pub hp_hz: f32,
    /// Octaves above C5 the VCF spans, from Brilliance 0 to 1.
    pub cutoff_octaves: (f32, f32),
    /// How far the VCF follows the keyboard, 0 for fixed and 1 for one to one.
    pub key_track: f32,
    /// Where this voice's formants sit.
    pub resonators: [ResonatorSpec; BANK_SIZE],
    /// How much of the bank goes on through the VCF; the rest reaches the VCA
    /// directly, the way the hardware routed most of its resonator voices.
    pub resonator_to_vcf: f32,
    /// Panel settings the editor stamps in when this character is chosen.
    pub defaults: Settings,
}

/// The panel-side half of a character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub glide_ms: f32,
    pub octave: i32,
    pub blend: f32,
    pub detune_cents: f32,
    pub brilliance: f32,
    pub emphasis: f32,
    pub body: f32,
    pub vibrato_hz: f32,
    pub vibrato_cents: f32,
    pub vibrato_delay_ms: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub drive: f32,
    pub gain: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worm_is_wired_to_the_arp_pulse_and_reed_bank() {
        let recipe = Character::Worm.recipe();
        assert!((recipe.pulse_width - 1.0 / 14.0).abs() < 1e-6);
        // Pulse only: the Pro Soloist's Oboe never touched the saw mixer.
        assert_eq!(recipe.defaults.blend, 0.0);
        // "Sharpen III -> VCF": the whole bank continues into the filter.
        assert_eq!(recipe.resonator_to_vcf, 1.0);
        assert!(recipe.defaults.body > 0.5, "the bank should dominate");
        assert!(
            recipe.defaults.glide_ms > 250.0,
            "portamento wound right up"
        );
        assert!(recipe.defaults.octave >= 2, "the worm lives at 2'");
    }

    #[test]
    fn every_character_is_wired_sanely() {
        for character in Character::ALL {
            let r = character.recipe();
            assert!(r.pulse_width > 0.0 && r.pulse_width <= 0.5);
            assert!(r.cutoff_octaves.0 < r.cutoff_octaves.1);
            assert!((0.0..=1.0).contains(&r.resonator_to_vcf));
            assert!((0.0..=1.0).contains(&r.key_track));
            assert!(
                r.defaults.octave >= 2,
                "{character:?} is not in the 2' register"
            );
            assert!(r.resonators.iter().all(|s| s.freq_hz > 100.0 && s.q > 0.2));
            assert!(!character.label().is_empty());
        }
    }

    #[test]
    fn characters_do_not_all_share_one_setting() {
        let widths: Vec<f32> = Character::ALL
            .iter()
            .map(|c| c.recipe().pulse_width)
            .collect();
        let glides: Vec<f32> = Character::ALL
            .iter()
            .map(|c| c.recipe().defaults.glide_ms)
            .collect();
        assert!(widths.windows(2).any(|w| w[0] != w[1]));
        assert!(glides.windows(2).any(|w| w[0] != w[1]));
    }
}
