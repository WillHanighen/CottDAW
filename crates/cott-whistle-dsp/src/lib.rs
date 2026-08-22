//! CottWhistle DSP: one Pro Soloist circuit, thirty factory paddles.
//!
//! The engine does not branch on voice name. Each paddle is a [`Recipe`] of
//! switch bits and resistor values stamped into [`WhistleParams`].

mod engine;
mod envelope;
mod filter;
mod oscillator;
mod recipe;
mod rom;
mod rom_bits;
mod voice;

pub use engine::{MidiNoteEvent, OVERSAMPLE, WhistleEngine};
pub use envelope::{AdsrParams, AdsrState, ArState, Stage};
pub use filter::{BANK_SIZE, CURVE_NAMES, CURVES, HPF_HZ, hp_from_index, hp_from_mask};
pub use oscillator::{Oscillator, combined_pulse, pulse, staircase_saw};
pub use recipe::{Recipe, ResonatorSlot, WhistleParams};
pub use voice::{PADDLES, Paddle, PaddleThrow, PulseWidth, Voice};

/// Growl LFO. Separate from the panel vibrato LFO, ~32 Hz.
pub const GROWL_HZ: f32 = 32.0;

/// MIDI note number → frequency in Hz (A4 = 440 Hz at note 69).
#[inline]
pub fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn render_voice(voice: Voice, note: u8, n: usize) -> Vec<f32> {
        let params = WhistleParams::from_voice(voice);
        let mut eng = WhistleEngine::new(48_000.0);
        let events = [MidiNoteEvent {
            sample_offset: 0,
            note,
            velocity: 100,
            channel: 0,
            on: true,
        }];
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        eng.process_block(&params, &events, &mut left, &mut right);
        left
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn write_wav(name: &str, samples: &[f32]) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dumps");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        let mut data = Vec::new();
        let n = samples.len() as u32;
        let sr = 48_000u32;
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&(36u32 + n * 2).to_le_bytes());
        data.extend_from_slice(b"WAVE");
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&sr.to_le_bytes());
        data.extend_from_slice(&(sr * 2).to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes());
        data.extend_from_slice(b"data");
        data.extend_from_slice(&(n * 2).to_le_bytes());
        for &x in samples {
            let s = (x.clamp(-1.0, 1.0) * 32767.0) as i16;
            data.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, data).expect("wav dump");
    }

    #[test]
    fn a4_is_four_forty() {
        assert!((midi_note_to_hz(69) - 440.0).abs() < 1e-4);
    }

    #[test]
    fn default_is_oboe() {
        assert_eq!(WhistleParams::default().voice, Voice::Oboe);
        assert_eq!(WhistleParams::default().pulse, PulseWidth::One14);
    }

    #[test]
    fn rom_routing_matches_the_flow_charts() {
        let oboe = Voice::Oboe.recipe();
        assert_eq!(oboe.pulse_bits & (1 << 1), 1 << 1, "oboe 1/14");
        assert!(
            oboe.resonators
                .iter()
                .any(|s| s.enabled && s.curve == 9 && s.to_vcf > 0.0)
        );
        assert!(oboe.resonators.iter().all(|s| s.to_vca == 0.0));
        assert!(oboe.vcf_enable);
        assert_eq!(oboe.resonator_mix, 0.0);
        assert!(oboe.rom_octave > -3);

        let tuba = Voice::Tuba.recipe();
        assert_eq!(tuba.pulse_bits, 0, "tuba is saw, no pulse selects");
        assert!(tuba.saw_level > 0.0);
        assert!(tuba.pulse_level == 0.0);
        assert_eq!(tuba.rom_octave, -3);
        assert!(tuba.growl > 0.0);
        assert!(
            tuba.resonators
                .iter()
                .all(|s| !s.enabled || (s.to_vcf == 0.0 && s.to_vca == 0.0)),
            "tuba flow chart: no live resonators"
        );

        let flute = Voice::Flute.recipe();
        assert_eq!(flute.pulse_bits, 0);
        assert!(flute.saw_level > 0.0);

        let fuzz = Voice::FuzzGuitar1.recipe();
        assert_eq!(fuzz.pulse_bits & 1, 1, "fuzz I is dynamic pulse");
        assert!(fuzz.adsr_pwm > 0.0);

        let cello = Voice::Cello.recipe();
        assert!(cello.pulse_bits & (1 << 5) != 0, "cello 2/11");
        assert!(
            cello.resonators.iter().any(|s| s.enabled && s.to_vca > 0.0),
            "cello has a VCA resonator path, slots={:?}",
            cello.resonators
        );
        assert_eq!(cello.resonator_mix, 0.0);

        let clarinet = Voice::Clarinet.recipe();
        assert!(clarinet.pulse_bits & (1 << 4) != 0, "clarinet 1/2");
    }

    #[test]
    fn factory_voices_speak() {
        for v in [
            Voice::Oboe,
            Voice::Tuba,
            Voice::Cello,
            Voice::Flute,
            Voice::FuzzGuitar1,
        ] {
            let buf = render_voice(v, 60, 8_000);
            assert!(
                rms(&buf[2_000..]) > 1e-4,
                "{} was silent (rms={})",
                v.label(),
                rms(&buf[2_000..])
            );
        }
    }

    #[test]
    fn dump_listen_wavs() {
        for (v, note, name) in [
            (Voice::Oboe, 69u8, "oboe.wav"),
            (Voice::Tuba, 48, "tuba.wav"),
            (Voice::Cello, 57, "cello.wav"),
            (Voice::Flute, 72, "flute.wav"),
            (Voice::FuzzGuitar1, 52, "fuzz1.wav"),
        ] {
            let buf = render_voice(v, note, 48_000);
            write_wav(name, &buf);
        }
    }
}
