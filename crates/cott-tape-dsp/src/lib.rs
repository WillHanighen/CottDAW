//! CottTape DSP: a stereo tape delay with wow, dark repeats, and drive.

mod engine;
mod filter;

pub use engine::{TapeEngine, TapeParams};
