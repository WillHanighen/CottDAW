//! CottBass DSP: a mono sub-bass with a folded body and glide.

mod engine;
mod filter;

pub use engine::{BassEngine, BassParams, MidiNoteEvent};

/// MIDI note number → frequency in Hz (A4 = 440 Hz at note 69).
#[inline]
pub fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}
