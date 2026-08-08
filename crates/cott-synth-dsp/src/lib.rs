//! CottSynth DSP: waveforms, ADSR, and polyphonic voice management.
//!
//! Shared by the built-in DAW instrument node and the redistributable VST3.

mod adsr;
mod engine;
mod oscillator;

pub use adsr::{AdsrParams, AdsrStage, AdsrState};
pub use engine::{MidiNoteEvent, PolySynth, SynthParams, MAX_VOICES};
pub use oscillator::{Waveform, sample_waveform};

/// MIDI note number → frequency in Hz (A4 = 440 Hz at note 69).
#[inline]
pub fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}
