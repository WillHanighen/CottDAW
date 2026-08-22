//! CottPluck DSP: a 6-voice Karplus-Strong guitar with mute and dust.

mod engine;
mod filter;
mod voice;

pub use engine::{MidiNoteEvent, PluckEngine, PluckParams, MAX_VOICES};

/// MIDI note number → frequency in Hz (A4 = 440 Hz at note 69).
#[inline]
pub fn midi_note_to_hz(note: u8) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
}

#[inline]
fn noise_tick(state: &mut u32) -> f32 {
    let mut x = *state;
    if x == 0 {
        x = 0xA341_316C;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as i32 as f32) * (1.0 / 2_147_483_648.0)
}
