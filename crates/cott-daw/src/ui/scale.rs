//! Musical scale helpers for the piano roll: note naming, scale membership,
//! and a "blueprint" of the currently selected key so you can stay in scale.

pub use cott_core::scale::{NOTE_NAMES_FLAT, NOTE_NAMES_SHARP, ScaleMode, ScaleSettings};

/// Full note name with octave, e.g. MIDI 60 -> "C4" (scientific pitch notation).
/// Uses sharp names by default; for scale-aware naming use ScaleSettings::note_name.
pub fn note_name(pitch: u8) -> String {
    let octave = pitch as i16 / 12 - 1;
    format!("{}{}", NOTE_NAMES_SHARP[(pitch % 12) as usize], octave)
}
