//! Thirty factory paddles. Hardware is 15 rockers plus a bank/shift.
//!
//! Voice codes are five bits (US3930429). Thirty of the 32 slots are used; the
//! two unused codes are skipped here. Names follow the service-manual Voice
//! Code Truth Table, not Cherry's later Super Wave extras.

/// Discrete pulse widths from ROM-4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseWidth {
    /// 1/64 — Song Whistle, some percussion.
    One64,
    /// 1/14 — the reed family (Oboe, Bassoon, English Horn).
    One14,
    /// 1/9 — strings, some brass.
    One9,
    /// 2/11 — most brass.
    Two11,
    /// Square. Clarinet lives here.
    Half,
}

impl PulseWidth {
    pub const ALL: [PulseWidth; 5] = [
        PulseWidth::One64,
        PulseWidth::One14,
        PulseWidth::One9,
        PulseWidth::Two11,
        PulseWidth::Half,
    ];

    pub fn ratio(self) -> f32 {
        match self {
            PulseWidth::One64 => 1.0 / 64.0,
            PulseWidth::One14 => 1.0 / 14.0,
            PulseWidth::One9 => 1.0 / 9.0,
            PulseWidth::Two11 => 2.0 / 11.0,
            PulseWidth::Half => 0.5,
        }
    }

    /// ROM-4 select bit for this named width. Dynamic is bit 0, elsewhere.
    pub fn select_bit(self) -> u8 {
        match self {
            PulseWidth::One14 => 1 << 1,
            PulseWidth::One9 => 1 << 2,
            PulseWidth::One64 => 1 << 3,
            PulseWidth::Half => 1 << 4,
            PulseWidth::Two11 => 1 << 5,
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&w| w == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i.min(4)]
    }

    pub fn label(self) -> &'static str {
        match self {
            PulseWidth::One64 => "1/64",
            PulseWidth::One14 => "1/14",
            PulseWidth::One9 => "1/9",
            PulseWidth::Two11 => "2/11",
            PulseWidth::Half => "1/2",
        }
    }
}

/// One of the 30 factory voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Voice {
    Bassoon,
    EnglishHorn,
    #[default]
    Oboe,
    BuzzBassoon,
    Sax,
    SpaceReed,
    Clarinet,
    Flute,
    Telstar,
    SongWhistle,
    Tuba,
    Trombone,
    FrenchHorn,
    Trumpet,
    Noze,
    Pulsar,
    ComicWow,
    MuteTrumpet,
    Cello,
    SteelGuitar,
    Violin,
    Harpsichord,
    Bass,
    Piano,
    Banjo,
    SpaceBass,
    SteelDrum,
    CountryGuitar,
    FuzzGuitar1,
    FuzzGuitar2,
}

impl Voice {
    pub const ALL: [Voice; 30] = [
        Voice::Bassoon,
        Voice::EnglishHorn,
        Voice::Oboe,
        Voice::BuzzBassoon,
        Voice::Sax,
        Voice::SpaceReed,
        Voice::Clarinet,
        Voice::Flute,
        Voice::Telstar,
        Voice::SongWhistle,
        Voice::Tuba,
        Voice::Trombone,
        Voice::FrenchHorn,
        Voice::Trumpet,
        Voice::Noze,
        Voice::Pulsar,
        Voice::ComicWow,
        Voice::MuteTrumpet,
        Voice::Cello,
        Voice::SteelGuitar,
        Voice::Violin,
        Voice::Harpsichord,
        Voice::Bass,
        Voice::Piano,
        Voice::Banjo,
        Voice::SpaceBass,
        Voice::SteelDrum,
        Voice::CountryGuitar,
        Voice::FuzzGuitar1,
        Voice::FuzzGuitar2,
    ];

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i % 30]
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&v| v == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Voice::Bassoon => "Bassoon",
            Voice::EnglishHorn => "English Horn",
            Voice::Oboe => "Oboe",
            Voice::BuzzBassoon => "Buzz Bassoon",
            Voice::Sax => "Sax",
            Voice::SpaceReed => "Space Reed",
            Voice::Clarinet => "Clarinet",
            Voice::Flute => "Flute",
            Voice::Telstar => "Telstar",
            Voice::SongWhistle => "Song Whistle",
            Voice::Tuba => "Tuba",
            Voice::Trombone => "Trombone",
            Voice::FrenchHorn => "French Horn",
            Voice::Trumpet => "Trumpet",
            Voice::Noze => "Noze",
            Voice::Pulsar => "Pulsar",
            Voice::ComicWow => "Comic Wow",
            Voice::MuteTrumpet => "Mute Trumpet",
            Voice::Cello => "Cello",
            Voice::SteelGuitar => "Steel Guitar",
            Voice::Violin => "Violin",
            Voice::Harpsichord => "Harpsichord",
            Voice::Bass => "Bass",
            Voice::Piano => "Piano",
            Voice::Banjo => "Banjo",
            Voice::SpaceBass => "Space Bass",
            Voice::SteelDrum => "Steel Drum",
            Voice::CountryGuitar => "Country Guitar",
            Voice::FuzzGuitar1 => "Fuzz Guitar 1",
            Voice::FuzzGuitar2 => "Fuzz Guitar 2",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Voice::Bassoon
            | Voice::EnglishHorn
            | Voice::Oboe
            | Voice::BuzzBassoon
            | Voice::Sax
            | Voice::SpaceReed => "Reeds",
            Voice::Clarinet | Voice::Flute | Voice::Telstar | Voice::SongWhistle => "Woodwinds",
            Voice::Tuba
            | Voice::Trombone
            | Voice::FrenchHorn
            | Voice::Trumpet
            | Voice::Noze
            | Voice::Pulsar
            | Voice::ComicWow
            | Voice::MuteTrumpet => "Brass",
            Voice::Cello | Voice::SteelGuitar | Voice::Violin | Voice::Harpsichord => "Strings",
            Voice::Bass
            | Voice::Piano
            | Voice::Banjo
            | Voice::SpaceBass
            | Voice::SteelDrum
            | Voice::CountryGuitar => "Percussion",
            Voice::FuzzGuitar1 | Voice::FuzzGuitar2 => "Fuzz",
        }
    }

    /// Five-bit voice code as listed in the patent (0..31, two unused).
    pub fn rom_code(self) -> u8 {
        // Sequential assignment matching the SM Voice Code Truth Table order
        // of the 30 used slots. Unused codes 30 and 31 are not represented.
        self.index() as u8
    }
}

/// One hardware paddle: up voice / down voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paddle {
    pub up: Voice,
    pub down: Voice,
}

/// Fifteen rockers as they sit on the panel (Gordon Reid, Music Technology
/// Aug 1991: 15 paddles plus a bank/shift). Cherry hid the bank and made each
/// paddle click between the two labels; we draw both names on the cap.
pub const PADDLES: [Paddle; 15] = [
    Paddle {
        up: Voice::Bassoon,
        down: Voice::BuzzBassoon,
    },
    Paddle {
        up: Voice::EnglishHorn,
        down: Voice::Sax,
    },
    Paddle {
        up: Voice::Oboe,
        down: Voice::SpaceReed,
    },
    Paddle {
        up: Voice::Clarinet,
        down: Voice::Telstar,
    },
    Paddle {
        up: Voice::Flute,
        down: Voice::SongWhistle,
    },
    Paddle {
        up: Voice::Tuba,
        down: Voice::Noze,
    },
    Paddle {
        up: Voice::Trombone,
        down: Voice::Pulsar,
    },
    Paddle {
        up: Voice::FrenchHorn,
        down: Voice::ComicWow,
    },
    Paddle {
        up: Voice::Trumpet,
        down: Voice::MuteTrumpet,
    },
    Paddle {
        up: Voice::Cello,
        down: Voice::SteelGuitar,
    },
    Paddle {
        up: Voice::Violin,
        down: Voice::Harpsichord,
    },
    Paddle {
        up: Voice::Bass,
        down: Voice::SpaceBass,
    },
    Paddle {
        up: Voice::Piano,
        down: Voice::SteelDrum,
    },
    Paddle {
        up: Voice::Banjo,
        down: Voice::CountryGuitar,
    },
    Paddle {
        up: Voice::FuzzGuitar1,
        down: Voice::FuzzGuitar2,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddleThrow {
    Off,
    Up,
    Down,
}

impl Paddle {
    pub fn voice(self, throw: PaddleThrow) -> Option<Voice> {
        match throw {
            PaddleThrow::Off => None,
            PaddleThrow::Up => Some(self.up),
            PaddleThrow::Down => Some(self.down),
        }
    }

    pub fn throw_for(self, voice: Voice) -> PaddleThrow {
        if voice == self.up {
            PaddleThrow::Up
        } else if voice == self.down {
            PaddleThrow::Down
        } else {
            PaddleThrow::Off
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_unique_voices() {
        let mut seen = std::collections::HashSet::new();
        for v in Voice::ALL {
            assert!(seen.insert(v));
            assert!(!v.label().is_empty());
        }
        assert_eq!(seen.len(), 30);
    }

    #[test]
    fn paddles_cover_every_voice_once() {
        let mut seen = std::collections::HashSet::new();
        for p in PADDLES {
            assert!(seen.insert(p.up));
            assert!(seen.insert(p.down));
            assert_ne!(p.up, p.down);
        }
        assert_eq!(seen.len(), 30);
    }
}
