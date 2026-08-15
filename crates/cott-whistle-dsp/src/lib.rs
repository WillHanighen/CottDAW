//! CottWhistle DSP — the whistle synth that runs through 90s hip-hop.
//!
//! The sound starts with Junie Morrison playing the ARP Pro Soloist's Oboe
//! voice on the Ohio Players' "Funky Worm" in 1973: a 1/14 pulse wave squeezed
//! through a bank of fixed reed resonators, with the portamento slider pushed
//! all the way up. N.W.A sampled it, and once the sample had been worn out Dre
//! rebuilt it on a Minimoog — a saw against a slightly detuned square, cutoff
//! well down, emphasis up, played monophonically so every overlapping note
//! slides into the next.
//!
//! Both halves of that lineage are rectangular waves through resonant filters.
//! There is no sine oscillator anywhere in this crate, and adding one would
//! miss the sound; see [`oscillator`] for the generators and [`character`] for
//! the four voices the instrument wires up.

pub mod character;
pub mod engine;
pub mod filter;
pub mod oscillator;
pub mod response;

pub use character::{Character, Recipe, Settings};
pub use engine::{MAX_HELD_NOTES, MidiNoteEvent, WhistleEngine, WhistleParams};
pub use filter::{
    BANK_SIZE, DcBlocker, LadderLp, OnePoleHp, Resonator, ResonatorBank, ResonatorSpec,
};
pub use oscillator::{ARP_PULSE_WIDTH, Oscillator, Shape, preview_wave};
pub use response::{plot_hz, plot_position, voice_magnitude, voice_magnitude_db};

/// Equal-tempered MIDI note to frequency (A4 = 440 Hz).
pub fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440() {
        assert!((midi_note_to_hz(69) - 440.0).abs() < 1e-3);
        assert!((midi_note_to_hz(81) - 880.0).abs() < 1e-3);
    }
}
