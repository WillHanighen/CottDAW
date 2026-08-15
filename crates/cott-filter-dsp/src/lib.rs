//! Stereo RBJ biquad low-pass / high-pass filter.

mod biquad;
mod response;

pub use biquad::{FilterMode, FilterParams, StereoFilter};
pub use response::ResponseProbe;
