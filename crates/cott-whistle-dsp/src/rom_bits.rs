//! 2701 ROM truth tables, SM pages 8-9.
//!
//! 32 columns. Bit 0 is FUZZ GUITAR I, bit 15 and 31 are unused OFF slots.
//! A `1` in the source string is a logic-1 on that pin, leftmost = bit 0.

use crate::envelope::AdsrParams;
use crate::filter::hp_from_mask;
use crate::recipe::{Recipe, ResonatorSlot};
use crate::voice::{PulseWidth, Voice};

/// ROM column for a factory voice. OFF slots are 15 and 31.
pub fn column(voice: Voice) -> u8 {
    match voice {
        Voice::FuzzGuitar1 => 0,
        Voice::Banjo => 1,
        Voice::Piano => 2,
        Voice::Bass => 3,
        Voice::Violin => 4,
        Voice::Cello => 5,
        Voice::Trumpet => 6,
        Voice::FrenchHorn => 7,
        Voice::Trombone => 8,
        Voice::Tuba => 9,
        Voice::Flute => 10,
        Voice::Clarinet => 11,
        Voice::Oboe => 12,
        Voice::EnglishHorn => 13,
        Voice::Bassoon => 14,
        Voice::FuzzGuitar2 => 16,
        Voice::CountryGuitar => 17,
        Voice::SteelDrum => 18,
        Voice::SpaceBass => 19,
        Voice::Harpsichord => 20,
        Voice::SteelGuitar => 21,
        Voice::MuteTrumpet => 22,
        Voice::ComicWow => 23,
        Voice::Pulsar => 24,
        Voice::Noze => 25,
        Voice::SongWhistle => 26,
        Voice::Telstar => 27,
        Voice::SpaceReed => 28,
        Voice::Sax => 29,
        Voice::BuzzBassoon => 30,
    }
}

fn bit(row: u32, col: u8) -> bool {
    (row >> col) & 1 == 1
}

// Board A Z15, SM p.8
const Z15_DYN: u32 = bits("10010000000000011000000000000101");
const Z15_1_14: u32 = bits("00000000000011111101100001001011");
const Z15_1_9: u32 = bits("01100000000000111000010010000101");
const Z15_1_64: u32 = bits("00000000000000011000001001000011");
const Z15_1_2: u32 = bits("00000000000100010010000000110001");
const Z15_2_11: u32 = bits("00001100000000010000000101000001");
const Z15_DOWN1: u32 = bits("11110011010101010111111110010111");
const Z15_DOWN2: u32 = bits("00010100110000111101000001000011");

// Board C Z7, SM p.9
const Z7_EHORN_N2: u32 = bits("11111111111110111110011111111001"); // logic 0 enable
const Z7_PULSE_10DB: u32 = bits("01110000000011010001011000000001");
const Z7_PULSE_HPF: u32 = bits("11001100000100111110101101111111");
const Z7_SAW_HPF: u32 = bits("10000011111000010000000011001011");
const Z7_HPF_D: u32 = bits("00000000000000011001000000110011");
const Z7_HPF_C: u32 = bits("10000001010000010010000000000001");
const Z7_HPF_B: u32 = bits("10000011010100111100100111001111");
const Z7_HPF_A: u32 = bits("00001100000100110000101111001001");

// Board C Z8, SM p.9
const Z8_RES3_VCF: u32 = bits("01110000000110011111111010001111");
const Z8_RES3_VCA: u32 = bits("10001100000000011000000010001011");
const Z8_OBOE: u32 = bits("10000000000110011100101000000001");
const Z8_EBASS: u32 = bits("10010011111001110100000000000011");
const Z8_EPIANO: u32 = bits("10100000000000010101010010001001");
const Z8_VIO1: u32 = bits("01001000000000011010000000000111");
const Z8_VIO23: u32 = bits("11110111111111110111111110110101"); // logic 0 enable
/// Pin 2 did not survive OCR. Logic 0 enables cello 1/2/3. Cello is column 5.
const Z8_CELLO123: u32 = bits("01111011111111110111110111111111"); // logic 0 enable

// Board C Z6 resonance / track, SM p.9
const Z6_RES_MAX: u32 = bits("10110111111100011111111010001111");
const Z6_RES_MED: u32 = bits("00110110101100011101001000000001");
const Z6_RES_NONE: u32 = bits("00111100010000010101100000000001");
const Z6_NO_TRACK: u32 = bits("01111111101110111111111101110111"); // logic 0
const Z6_TRACK_5: u32 = bits("11111111111110011000111101110011"); // logic 0
const Z6_TRACK_2: u32 = bits("10000000010000010111111000000001"); // logic 0

// Board D Z1 growl / vibrato, SM p.9
const D_GROWL_B: u32 = bits("00000000010000010000000000000001");
const D_GROWL_A: u32 = bits("00000010110010010000001000000001");
const D_VIB_C: u32 = bits("00000000001110010000000010010001");
const D_VIB_B: u32 = bits("10000000101011110100010110010101");
const D_VIB_A: u32 = bits("10001100100000010000010010010001");
const D_VIB_NOT_TREM: u32 = bits("11001111100111111001111101000111");

// Board B envelopes, SM p.8. Attack/decay/release bits select parallel R.
const B_ADSR_ATK_A: u32 = bits("11110000000100011111110000001000");
const B_ADSR_ATK_B: u32 = bits("11010010100110000101000001000110");
const B_ADSR_ATK_C: u32 = bits("11000110101110000101001000100000");
const B_ADSR_DEC_A: u32 = bits("00101000000000010100010111100001");
const B_ADSR_DEC_B: u32 = bits("00001100000000011100010110000001");
const B_ADSR_REL_A: u32 = bits("10110000101010011110110011011001");
const B_ADSR_REL_B: u32 = bits("10110010000100111011011101100001");
const B_ADSR_REL_C: u32 = bits("11010101000010010110011011010001");
const B_ADSR_SUS_A: u32 = bits("00111101001010010000011000111101");
const B_ADSR_SUS_B: u32 = bits("00011111111100100001000101101111");
const B_ADSR_TO_VCF: u32 = bits("00110011111100011111111110111110");
const B_ADSR_TO_VCA: u32 = bits("01111111001100001111111111111110");
const B_AR_TO_VCF: u32 = bits("11001100000000010010000000000010");
const B_AR_TO_VCA: u32 = bits("10000000110011101100001101000000");
const B_AR_ATK_A: u32 = bits("11000000001100000100110000000000");
const B_AR_ATK_B: u32 = bits("10001110111111011101111010001110");
const B_AR_REL_A: u32 = bits("00000100011000010100110010001011");

const fn bits(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut v = 0u32;
    let mut i = 0;
    let mut bit_i = 0;
    while i < bytes.len() {
        if bytes[i] == b'0' || bytes[i] == b'1' {
            if bytes[i] == b'1' {
                v |= 1 << bit_i;
            }
            bit_i += 1;
        }
        i += 1;
    }
    v
}

pub fn recipe_from_rom(voice: Voice) -> Recipe {
    let c = column(voice);
    let dyn_on = bit(Z15_DYN, c);
    let mut pulse_bits = 0u8;
    if dyn_on {
        pulse_bits |= 1 << 0;
    }
    if bit(Z15_1_14, c) {
        pulse_bits |= 1 << 1;
    }
    if bit(Z15_1_9, c) {
        pulse_bits |= 1 << 2;
    }
    if bit(Z15_1_64, c) {
        pulse_bits |= 1 << 3;
    }
    if bit(Z15_1_2, c) {
        pulse_bits |= 1 << 4;
    }
    if bit(Z15_2_11, c) {
        pulse_bits |= 1 << 5;
    }

    let pulse = primary_pulse(pulse_bits);
    let mut hp_mask = 0u8;
    if bit(Z7_HPF_A, c) {
        hp_mask |= 1 << 0;
    }
    if bit(Z7_HPF_B, c) {
        hp_mask |= 1 << 1;
    }
    if bit(Z7_HPF_C, c) {
        hp_mask |= 1 << 2;
    }
    if bit(Z7_HPF_D, c) {
        hp_mask |= 1 << 3;
    }

    let saw_hpf = bit(Z7_SAW_HPF, c);
    let pulse_hpf = bit(Z7_PULSE_HPF, c);
    let pulse_10db = bit(Z7_PULSE_10DB, c);

    let res3_vcf = bit(Z8_RES3_VCF, c);
    let res3_vca = bit(Z8_RES3_VCA, c);
    let ehorn = !bit(Z7_EHORN_N2, c);
    let cello_nets = !bit(Z8_CELLO123, c);
    let vio23 = !bit(Z8_VIO23, c);

    let mut slots = [ResonatorSlot::OFF; 5];
    let mut n = 0usize;
    let mut push = |curve: u8, to_vcf: f32, to_vca: f32| {
        if n < 5 {
            slots[n] = ResonatorSlot {
                enabled: true,
                curve,
                to_vcf,
                to_vca,
            };
            n += 1;
        }
    };

    // Banks 1 and 2 to VCA only (SM 6.3).
    if cello_nets {
        push(0, 0.0, 1.0); // Cello 2
        push(3, 0.0, 1.0); // Cello 1
        if res3_vcf || res3_vca {
            push(6, res3_vcf as u8 as f32, res3_vca as u8 as f32); // Cello 3
        }
    }
    if vio23 {
        push(1, 0.0, 1.0); // Violin 2
        push(4, 0.0, 1.0); // Violin 3
    }
    if ehorn {
        push(2, 0.0, 1.0);
    }
    if res3_vcf || res3_vca {
        let vcf = if res3_vcf { 1.0 } else { 0.0 };
        let vca = if res3_vca { 1.0 } else { 0.0 };
        if bit(Z8_VIO1, c) {
            push(5, vcf, vca);
        }
        if bit(Z8_EPIANO, c) {
            push(7, vcf, vca);
        }
        if bit(Z8_EBASS, c) {
            push(8, vcf, vca);
        }
        if bit(Z8_OBOE, c) {
            push(9, vcf, vca);
        }
    }

    let vcf_enable = saw_hpf || pulse_hpf || res3_vcf;
    let pulse_level = if pulse_hpf {
        if pulse_10db { 1.0 } else { 0.7 }
    } else {
        0.0
    };
    let saw_level = if saw_hpf { 1.0 } else { 0.0 };

    let growl = if bit(D_GROWL_B, c) {
        0.7
    } else if bit(D_GROWL_A, c) {
        0.4
    } else {
        0.0
    };

    let (lfo_fm, lfo_to_vca) = if bit(D_VIB_NOT_TREM, c) {
        let depth = if bit(D_VIB_A, c) {
            0.18
        } else if bit(D_VIB_B, c) {
            0.12
        } else if bit(D_VIB_C, c) {
            0.06
        } else {
            0.0
        };
        (depth, 0.0)
    } else {
        (0.0, 0.35)
    };
    // SM 6.4: upper (second ROM page) delayed, lower undelayed.
    let lfo_delay_ms = if c >= 16 && lfo_fm > 0.0 { 260.0 } else { 0.0 };

    let vcf_resonance = if bit(Z6_RES_MAX, c) {
        0.42
    } else if bit(Z6_RES_MED, c) {
        0.22
    } else if bit(Z6_RES_NONE, c) {
        0.06
    } else {
        0.16
    };

    let vcf_keytrack = if !bit(Z6_NO_TRACK, c) {
        0.0
    } else if !bit(Z6_TRACK_5, c) {
        1.0
    } else if !bit(Z6_TRACK_2, c) {
        0.5
    } else {
        0.25
    };

    Recipe {
        pulse,
        pulse_bits,
        pulse_level,
        saw_level,
        resonator_mix: 0.0,
        hp_hz: hp_from_mask(hp_mask),
        hp_mask,
        vcf_enable,
        vcf_cutoff: 0.48,
        vcf_resonance,
        vcf_keytrack,
        adsr_to_vcf: if bit(B_ADSR_TO_VCF, c) { 0.62 } else { 0.0 },
        ar_to_vcf: if bit(B_AR_TO_VCF, c) { 0.35 } else { 0.0 },
        growl,
        adsr: envelope_adsr(c),
        ar_attack_ms: ar_attack(c),
        ar_release_ms: ar_release(c),
        adsr_to_vca: if bit(B_ADSR_TO_VCA, c) { 1.0 } else { 0.0 },
        ar_to_vca: if bit(B_AR_TO_VCA, c) { 1.0 } else { 0.0 },
        adsr_pwm: if dyn_on { 0.72 } else { 0.0 },
        ar_pwm: 0.0,
        lfo_pwm: 0.0,
        lfo_fm,
        lfo_delay_ms,
        lfo_to_vca,
        resonators: slots,
        octave: 0,
        rom_octave: -(bit(Z15_DOWN1, c) as i32) - 2 * (bit(Z15_DOWN2, c) as i32),
    }
}

fn primary_pulse(bits: u8) -> PulseWidth {
    if bits & (1 << 1) != 0 {
        PulseWidth::One14
    } else if bits & (1 << 2) != 0 {
        PulseWidth::One9
    } else if bits & (1 << 5) != 0 {
        PulseWidth::Two11
    } else if bits & (1 << 3) != 0 {
        PulseWidth::One64
    } else if bits & (1 << 4) != 0 {
        PulseWidth::Half
    } else {
        PulseWidth::One14
    }
}

fn parallel_r(g: f32) -> f32 {
    if g <= 0.0 { 1_000_000.0 } else { 1.0 / g }
}

fn add_g(g: &mut f32, on: bool, r: f32) {
    if on {
        *g += 1.0 / r;
    }
}

fn envelope_adsr(c: u8) -> AdsrParams {
    // Board B: tau_ms = R_kOhm * C_uF with C3 taken as 1 uF (notes/2701.md).
    let mut atk = 0.0;
    add_g(&mut atk, bit(B_ADSR_ATK_A, c), 22_000.0);
    add_g(&mut atk, bit(B_ADSR_ATK_B, c), 100_000.0);
    add_g(&mut atk, bit(B_ADSR_ATK_C, c), 470_000.0);
    let mut dec = 0.0;
    add_g(&mut dec, bit(B_ADSR_DEC_A, c), 100_000.0);
    add_g(&mut dec, bit(B_ADSR_DEC_B, c), 560_000.0);
    let mut rel = 0.0;
    add_g(&mut rel, bit(B_ADSR_REL_A, c), 100_000.0);
    add_g(&mut rel, bit(B_ADSR_REL_B, c), 220_000.0);
    add_g(&mut rel, bit(B_ADSR_REL_C, c), 680_000.0);
    let sustain = match (bit(B_ADSR_SUS_A, c), bit(B_ADSR_SUS_B, c)) {
        (true, true) => 0.85,
        (true, false) => 0.55,
        (false, true) => 0.70,
        (false, false) => 0.40,
    };
    AdsrParams {
        attack_ms: (parallel_r(atk) * 0.001).clamp(1.0, 800.0),
        decay_ms: (parallel_r(dec) * 0.001).clamp(4.0, 1_200.0),
        sustain,
        release_ms: (parallel_r(rel) * 0.001).clamp(8.0, 1_500.0),
    }
}

fn ar_attack(c: u8) -> f32 {
    let mut g = 0.0;
    add_g(&mut g, bit(B_AR_ATK_A, c), 120_000.0);
    add_g(&mut g, bit(B_AR_ATK_B, c), 1_000_000.0);
    (parallel_r(g) * 0.001).clamp(2.0, 800.0)
}

fn ar_release(c: u8) -> f32 {
    let r: f32 = if bit(B_AR_REL_A, c) {
        560_000.0
    } else {
        1_000_000.0
    };
    (r * 0.001).clamp(8.0, 1_200.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::CURVES;

    #[test]
    fn bits_are_thirty_two_wide() {
        assert_eq!(Z15_DYN.count_ones() + (!Z15_DYN).count_ones(), 32);
    }

    #[test]
    fn oboe_column_is_twelve() {
        assert_eq!(column(Voice::Oboe), 12);
        assert!(bit(Z15_1_14, 12));
        assert!(bit(Z8_OBOE, 12));
        assert!(bit(Z8_RES3_VCF, 12));
        assert!(!bit(Z8_RES3_VCA, 12));
    }

    #[test]
    fn tuba_is_saw_down_three() {
        let c = column(Voice::Tuba);
        assert!(bit(Z7_SAW_HPF, c));
        assert!(!bit(Z7_PULSE_HPF, c));
        assert!(bit(Z15_DOWN1, c) && bit(Z15_DOWN2, c));
        assert!(bit(Z7_HPF_C, c));
    }

    #[test]
    fn flute_has_no_pulse_select() {
        let c = column(Voice::Flute);
        assert!(!bit(Z15_DYN, c));
        assert!(!bit(Z15_1_14, c));
        assert!(!bit(Z15_1_9, c));
        assert!(!bit(Z15_1_64, c));
        assert!(!bit(Z15_1_2, c));
        assert!(!bit(Z15_2_11, c));
        assert!(bit(Z7_SAW_HPF, c));
        assert!(bit(B_ADSR_TO_VCA, c));
    }

    #[test]
    fn cello_enables_banks_one_and_two() {
        let c = column(Voice::Cello);
        assert!(!bit(Z8_CELLO123, c));
        assert!(bit(Z15_2_11, c));
    }

    #[test]
    fn clarinet_is_half() {
        assert!(bit(Z15_1_2, column(Voice::Clarinet)));
    }

    #[test]
    fn fuzz1_is_dynamic() {
        assert!(bit(Z15_DYN, column(Voice::FuzzGuitar1)));
    }

    #[test]
    fn named_curve_count() {
        assert_eq!(CURVES.len(), 10);
    }

    #[test]
    fn factory_never_sends_saw_into_the_banks() {
        for v in Voice::ALL {
            assert_eq!(v.recipe().resonator_mix, 0.0, "{}", v.label());
        }
    }
}
