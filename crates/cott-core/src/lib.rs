//! CottDAW core library: project model, graph, DSP, and engine messaging.

pub mod automation;
pub mod clips;
pub mod commands;
pub mod dsp;
pub mod engine;
pub mod export;
pub mod graph;
pub mod ids;
pub mod import;
pub mod project;
pub mod time;

pub use ids::*;
pub use project::Project;
pub use time::{BeatPos, SamplePos, TempoMap, TransportState};
