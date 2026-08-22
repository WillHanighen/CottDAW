//! CottKit DSP: kick, snare, clap, and hats with a dirt bus.

mod engine;
mod filter;

pub use engine::{KitEngine, KitParams, MidiNoteEvent};

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
