//! Musical scale helpers for the piano roll: note naming, scale membership,
//! and a "blueprint" of the currently selected key so you can stay in scale.

/// Sharp-based note names for the 12 pitch classes.
pub const NOTE_NAMES_SHARP: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Flat-based note names for the 12 pitch classes.
pub const NOTE_NAMES_FLAT: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// Available scale modes. Intervals are semitone offsets from the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Whether this mode prefers flat spellings (true) or sharp spellings (false).
    /// This follows standard music theory conventions for enharmonic spelling.
    pub fn prefers_flats(self) -> bool {
        match self {
            // Major modes that typically use flats: Dorian, Mixolydian on flat roots
            // but we simplify: modes with b3, b6, b7 tend toward flats
            ScaleMode::Minor
            | ScaleMode::Dorian
            | ScaleMode::Phrygian
            | ScaleMode::Locrian
            | ScaleMode::HarmonicMinor
            | ScaleMode::MelodicMinor
            | ScaleMode::MinorPentatonic
            | ScaleMode::Blues => true,
            // Major-based modes tend toward sharps, except Mixolydian which is mixed
            ScaleMode::Major | ScaleMode::Lydian | ScaleMode::MajorPentatonic => false,
            // Mixolydian has a b7, so it often uses flats
            ScaleMode::Mixolydian => true,
            // Chromatic is neutral, default to sharps
            ScaleMode::Chromatic => false,
        }
    }
}

/// Editor-only scale settings that drive piano-roll highlighting.
#[derive(Debug, Clone, Copy)]
pub struct ScaleSettings {
    /// When false, the piano roll draws with the classic all-chromatic look.
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
    /// True if the given MIDI pitch's pitch class is a degree of the scale.
    pub fn contains(self, pitch: u8) -> bool {
        let pc = (pitch as i16 - self.root as i16).rem_euclid(12) as u8;
        self.mode.intervals().contains(&pc)
    }

    /// True if the pitch is the tonic (root) of the scale.
    pub fn is_root(self, pitch: u8) -> bool {
        pitch % 12 == self.root
    }

    pub fn root_name(self) -> &'static str {
        self.note_name(self.root)
    }

    /// Get the appropriate note name for a pitch class based on scale context.
    pub fn note_name(self, pitch_class: u8) -> &'static str {
        let pc = (pitch_class % 12) as usize;
        if self.mode.prefers_flats() {
            NOTE_NAMES_FLAT[pc]
        } else {
            NOTE_NAMES_SHARP[pc]
        }
    }

    /// "C D E F G A B" — the notes of the scale in pitch order.
    pub fn blueprint(self) -> String {
        self.mode
            .intervals()
            .iter()
            .map(|iv| {
                let pc = ((self.root as u16 + *iv as u16) % 12) as u8;
                self.note_name(pc)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Full note name with octave, e.g. MIDI 60 -> "C4" (scientific pitch notation).
/// Uses sharp names by default; for scale-aware naming use ScaleSettings::note_name.
pub fn note_name(pitch: u8) -> String {
    let octave = pitch as i16 / 12 - 1;
    format!("{}{}", NOTE_NAMES_SHARP[(pitch % 12) as usize], octave)
}
