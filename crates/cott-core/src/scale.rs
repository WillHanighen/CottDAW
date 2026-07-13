//! Musical scale settings shared by the project model and piano-roll UI.

use serde::{Deserialize, Serialize};

/// Sharp-based note names for the 12 pitch classes.
pub const NOTE_NAMES_SHARP: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Flat-based note names for the 12 pitch classes.
pub const NOTE_NAMES_FLAT: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// Available scale modes. Intervals are semitone offsets from the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleMode {
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    HarmonicMinor,
    MelodicMinor,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Chromatic,
}

impl ScaleMode {
    /// Every mode, in the order shown in the picker.
    pub const ALL: [ScaleMode; 13] = [
        ScaleMode::Major,
        ScaleMode::Minor,
        ScaleMode::Dorian,
        ScaleMode::Phrygian,
        ScaleMode::Lydian,
        ScaleMode::Mixolydian,
        ScaleMode::Locrian,
        ScaleMode::HarmonicMinor,
        ScaleMode::MelodicMinor,
        ScaleMode::MajorPentatonic,
        ScaleMode::MinorPentatonic,
        ScaleMode::Blues,
        ScaleMode::Chromatic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ScaleMode::Major => "Major",
            ScaleMode::Minor => "Minor",
            ScaleMode::Dorian => "Dorian",
            ScaleMode::Phrygian => "Phrygian",
            ScaleMode::Lydian => "Lydian",
            ScaleMode::Mixolydian => "Mixolydian",
            ScaleMode::Locrian => "Locrian",
            ScaleMode::HarmonicMinor => "Harmonic Minor",
            ScaleMode::MelodicMinor => "Melodic Minor",
            ScaleMode::MajorPentatonic => "Major Pentatonic",
            ScaleMode::MinorPentatonic => "Minor Pentatonic",
            ScaleMode::Blues => "Blues",
            ScaleMode::Chromatic => "Chromatic",
        }
    }

    /// Semitone offsets from the root that belong to the scale.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            ScaleMode::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleMode::Minor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleMode::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleMode::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleMode::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleMode::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleMode::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            ScaleMode::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleMode::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            ScaleMode::MajorPentatonic => &[0, 2, 4, 7, 9],
            ScaleMode::MinorPentatonic => &[0, 3, 5, 7, 10],
            ScaleMode::Blues => &[0, 3, 5, 6, 7, 10],
            ScaleMode::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    /// Whether this mode conventionally favors flat note names.
    pub fn prefers_flats(self) -> bool {
        matches!(
            self,
            ScaleMode::Minor
                | ScaleMode::Dorian
                | ScaleMode::Phrygian
                | ScaleMode::Mixolydian
                | ScaleMode::Locrian
                | ScaleMode::HarmonicMinor
                | ScaleMode::MelodicMinor
                | ScaleMode::MinorPentatonic
                | ScaleMode::Blues
        )
    }
}

/// Scale guide saved with each MIDI clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScaleSettings {
    /// When false, the piano roll uses the all-chromatic appearance.
    pub highlight: bool,
    /// Root pitch class, 0 = C .. 11 = B.
    pub root: u8,
    pub mode: ScaleMode,
}

impl Default for ScaleSettings {
    fn default() -> Self {
        Self {
            highlight: true,
            root: 0,
            mode: ScaleMode::Major,
        }
    }
}

impl ScaleSettings {
    pub fn contains(self, pitch: u8) -> bool {
        let pc = (pitch as i16 - self.root as i16).rem_euclid(12) as u8;
        self.mode.intervals().contains(&pc)
    }

    pub fn is_root(self, pitch: u8) -> bool {
        pitch % 12 == self.root % 12
    }

    pub fn root_name(self) -> &'static str {
        self.note_name(self.root)
    }

    pub fn note_name(self, pitch_class: u8) -> &'static str {
        let pc = (pitch_class % 12) as usize;
        if self.mode.prefers_flats() {
            NOTE_NAMES_FLAT[pc]
        } else {
            NOTE_NAMES_SHARP[pc]
        }
    }

    pub fn blueprint(self) -> String {
        self.mode
            .intervals()
            .iter()
            .map(|interval| {
                let pc = ((self.root as u16 + *interval as u16) % 12) as u8;
                self.note_name(pc)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}
