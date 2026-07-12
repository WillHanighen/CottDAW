//! Tempo, transport, and musical-time conversions.

use serde::{Deserialize, Serialize};

/// Position in musical beats (quarter notes).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Default)]
pub struct BeatPos(pub f64);

impl BeatPos {
    pub fn new(beats: f64) -> Self {
        Self(beats)
    }

    pub fn beats(self) -> f64 {
        self.0
    }

    pub fn bars(self, beats_per_bar: f64) -> f64 {
        self.0 / beats_per_bar
    }
}

/// Absolute sample position on the timeline.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct SamplePos(pub i64);

impl SamplePos {
    pub fn new(samples: i64) -> Self {
        Self(samples)
    }

    pub fn samples(self) -> i64 {
        self.0
    }

    pub fn saturating_add(self, delta: i64) -> Self {
        Self(self.0.saturating_add(delta))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TransportState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Simple constant-tempo map for MVP (extensible later).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoMap {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub beat_unit: u32,
    pub sample_rate: u32,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            beats_per_bar: 4,
            beat_unit: 4,
            sample_rate: 48_000,
        }
    }
}

impl TempoMap {
    pub fn samples_per_beat(&self) -> f64 {
        (self.sample_rate as f64) * 60.0 / self.bpm
    }

    /// Beats (quarter-note units) in one bar for the current time signature.
    pub fn bar_length_beats(&self) -> f64 {
        self.beats_per_bar.max(1) as f64
    }

    pub fn beat_to_sample(&self, beat: BeatPos) -> SamplePos {
        SamplePos((beat.0 * self.samples_per_beat()).round() as i64)
    }

    pub fn sample_to_beat(&self, sample: SamplePos) -> BeatPos {
        BeatPos(sample.0 as f64 / self.samples_per_beat())
    }

    pub fn beats_to_samples(&self, beats: f64) -> i64 {
        (beats * self.samples_per_beat()).round() as i64
    }

    pub fn samples_to_beats(&self, samples: i64) -> f64 {
        samples as f64 / self.samples_per_beat()
    }

    pub fn bar_beat_tick(&self, sample: SamplePos) -> (u32, f64) {
        let beat = self.sample_to_beat(sample).0;
        let bpb = self.bar_length_beats();
        let bar = (beat / bpb).floor() as u32 + 1;
        let beat_in_bar = beat % bpb;
        (bar, beat_in_bar + 1.0)
    }

    /// Bar / beat display from a beat position (1-based bar and beat-in-bar).
    pub fn bar_beat_from_beats(&self, beats: f64) -> (u32, f64) {
        let bpb = self.bar_length_beats();
        let bar = (beats / bpb).floor() as u32 + 1;
        let beat_in_bar = beats.rem_euclid(bpb) + 1.0;
        (bar, beat_in_bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_sample_roundtrip() {
        let map = TempoMap::default();
        let beat = BeatPos(4.0);
        let sample = map.beat_to_sample(beat);
        let back = map.sample_to_beat(sample);
        assert!((back.0 - beat.0).abs() < 1e-6);
    }

    #[test]
    fn samples_per_beat_at_120_bpm() {
        let map = TempoMap {
            bpm: 120.0,
            sample_rate: 48_000,
            ..Default::default()
        };
        assert!((map.samples_per_beat() - 24_000.0).abs() < 1e-9);
    }

    #[test]
    fn bar_length_follows_time_signature() {
        let mut map = TempoMap::default();
        assert!((map.bar_length_beats() - 4.0).abs() < 1e-9);
        map.beats_per_bar = 3;
        map.beat_unit = 4;
        assert!((map.bar_length_beats() - 3.0).abs() < 1e-9);
        let (bar, beat) = map.bar_beat_from_beats(3.0);
        assert_eq!(bar, 2);
        assert!((beat - 1.0).abs() < 1e-9);
    }
}
