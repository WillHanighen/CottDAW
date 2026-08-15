//! Voicing report for each character playing a high-register legato lick.
//!
//! Tuning these voices against the records is done by ear, but the numbers keep
//! the ear honest: they show where the filter is sitting, how loud the voice
//! runs, and how the harmonics fall away above the fundamental.
//!
//! `cargo run -p cott-whistle-dsp --example audition`

use cott_whistle_dsp::{Character, MidiNoteEvent, WhistleEngine, WhistleParams, midi_note_to_hz};

const SR: f32 = 48_000.0;

/// Bars of a lead line. Every note overlaps the one before it, so the voice
/// glides the whole way through.
const LICK: [u8; 5] = [60, 63, 65, 62, 60];

fn main() {
    for character in Character::ALL {
        let params = WhistleParams::for_character(character);
        let mut engine = WhistleEngine::new(SR);

        let step = (SR * 0.28) as usize;
        let mut events = Vec::new();
        for (i, note) in LICK.iter().enumerate() {
            events.push(MidiNoteEvent {
                sample_offset: (i * step) as u32,
                note: *note,
                velocity: 105,
                channel: 0,
                on: true,
            });
            if i > 0 {
                events.push(MidiNoteEvent {
                    sample_offset: (i * step + 400) as u32,
                    note: LICK[i - 1],
                    velocity: 0,
                    channel: 0,
                    on: false,
                });
            }
        }
        events.sort_by_key(|e| e.sample_offset);

        let frames = step * LICK.len() + (SR * 0.5) as usize;
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.process_block(&params, &events, &mut left, &mut right);

        let peak = left.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (left.iter().map(|s| s * s).sum::<f32>() / frames as f32).sqrt();

        // Analyse the sustained middle of the first note.
        let window = &left[(SR * 0.10) as usize..(SR * 0.25) as usize];
        let f0 = midi_note_to_hz(LICK[0]) * 2f32.powi(params.octave);
        let partials: Vec<f32> = (1..=10)
            .map(|h| harmonic_energy(window, f0 * h as f32))
            .collect();
        let strongest = partials.iter().cloned().fold(0.0f32, f32::max).max(1e-9);

        println!("\n{:<12} {}", character.label(), character.blurb());
        println!(
            "  f0 {:>6.0} Hz   cutoff {:>6.0} Hz ({:.0}% key follow)   peak {:.3}  rms {:.3}",
            f0,
            params.cutoff_hz(f0),
            params.key_track() * 100.0,
            peak,
            rms
        );
        print!("  partials ");
        for (i, m) in partials.iter().enumerate() {
            print!("{}:{:>4.0}% ", i + 1, m / strongest * 100.0);
        }
        println!();
    }
}

/// Energy in a narrow band around `hz`.
///
/// A single bin is no good here: the oscillators are detuned, so every harmonic
/// arrives as a pair straddling its nominal frequency.
fn harmonic_energy(samples: &[f32], hz: f32) -> f32 {
    let span = (hz * 0.02).max(12.0);
    (-4..=4)
        .map(|i| magnitude_at(samples, hz + span * i as f32 / 4.0).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn magnitude_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len();
    let w = std::f64::consts::TAU * hz as f64 / SR as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, s) in samples.iter().enumerate() {
        let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
        re += *s as f64 * win * (w * i as f64).cos();
        im += *s as f64 * win * (w * i as f64).sin();
    }
    ((re * re + im * im).sqrt() * 4.0 / n as f64) as f32
}
