//! MIDI and audio clips on the arrangement timeline.

use crate::ids::{AssetId, ClipId, NoteId, TrackId};
use crate::time::{BeatPos, SamplePos};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiNote {
    pub id: NoteId,
    pub pitch: u8,
    pub velocity: u8,
    /// Start relative to clip start, in beats.
    pub start_beats: f64,
    pub length_beats: f64,
    pub channel: u8,
}

impl MidiNote {
    pub fn new(pitch: u8, velocity: u8, start_beats: f64, length_beats: f64) -> Self {
        Self {
            id: NoteId::new(),
            pitch,
            velocity: velocity.min(127),
            start_beats,
            length_beats: length_beats.max(1.0 / 64.0),
            channel: 0,
        }
    }

    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipContent {
    Midi {
        notes: Vec<MidiNote>,
    },
    Audio {
        asset_id: AssetId,
        /// Offset into the source file in samples (at project sample rate after cache).
        source_offset_samples: i64,
        /// Gain applied to this clip.
        gain_db: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub track_id: TrackId,
    pub name: String,
    /// Timeline start in beats.
    pub start_beats: f64,
    /// Length in beats.
    pub length_beats: f64,
    pub content: ClipContent,
    pub color: [u8; 3],
}

impl Clip {
    pub fn new_midi(track_id: TrackId, name: impl Into<String>, start_beats: f64, length_beats: f64) -> Self {
        Self {
            id: ClipId::new(),
            track_id,
            name: name.into(),
            start_beats,
            length_beats,
            content: ClipContent::Midi { notes: Vec::new() },
            color: [80, 160, 220],
        }
    }

    pub fn new_audio(
        track_id: TrackId,
        name: impl Into<String>,
        start_beats: f64,
        length_beats: f64,
        asset_id: AssetId,
    ) -> Self {
        Self {
            id: ClipId::new(),
            track_id,
            name: name.into(),
            start_beats,
            length_beats,
            content: ClipContent::Audio {
                asset_id,
                source_offset_samples: 0,
                gain_db: 0.0,
            },
            color: [80, 200, 140],
        }
    }

    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }

    pub fn contains_beat(&self, beat: f64) -> bool {
        beat >= self.start_beats && beat < self.end_beats()
    }

    pub fn notes_mut(&mut self) -> Option<&mut Vec<MidiNote>> {
        match &mut self.content {
            ClipContent::Midi { notes } => Some(notes),
            _ => None,
        }
    }

    pub fn notes(&self) -> Option<&[MidiNote]> {
        match &self.content {
            ClipContent::Midi { notes } => Some(notes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Midi,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub kind: TrackKind,
    pub color: [u8; 3],
    pub height: f32,
    pub armed: bool,
    /// Node IDs belonging to this track's default chain.
    pub midi_source_node: Option<crate::ids::NodeId>,
    pub audio_source_node: Option<crate::ids::NodeId>,
    pub instrument_node: Option<crate::ids::NodeId>,
    pub gain_node: Option<crate::ids::NodeId>,
}

impl Track {
    pub fn new_midi(name: impl Into<String>) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            kind: TrackKind::Midi,
            color: [80, 160, 220],
            height: 72.0,
            armed: false,
            midi_source_node: None,
            audio_source_node: None,
            instrument_node: None,
            gain_node: None,
        }
    }

    pub fn new_audio(name: impl Into<String>) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            kind: TrackKind::Audio,
            color: [80, 200, 140],
            height: 72.0,
            armed: false,
            midi_source_node: None,
            audio_source_node: None,
            instrument_node: None,
            gain_node: None,
        }
    }
}

/// Scheduled MIDI event for the audio engine.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledMidiEvent {
    pub sample_offset: u32,
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl ScheduledMidiEvent {
    pub fn note_on(sample_offset: u32, pitch: u8, velocity: u8, channel: u8) -> Self {
        Self {
            sample_offset,
            status: 0x90 | (channel & 0x0f),
            data1: pitch,
            data2: velocity,
        }
    }

    pub fn note_off(sample_offset: u32, pitch: u8, channel: u8) -> Self {
        Self {
            sample_offset,
            status: 0x80 | (channel & 0x0f),
            data1: pitch,
            data2: 0,
        }
    }
}

/// Collect MIDI events from clips overlapping `[block_start, block_start + block_len)`.
pub fn schedule_midi_for_block(
    clips: &[Clip],
    track_id: TrackId,
    tempo: &crate::time::TempoMap,
    block_start: SamplePos,
    block_len: u32,
) -> Vec<ScheduledMidiEvent> {
    let block_end = block_start.saturating_add(block_len as i64);
    let start_beat = tempo.sample_to_beat(block_start).0;
    let end_beat = tempo.sample_to_beat(block_end).0;
    let mut events = Vec::new();

    for clip in clips.iter().filter(|c| c.track_id == track_id) {
        let Some(notes) = clip.notes() else { continue };
        if clip.end_beats() <= start_beat || clip.start_beats >= end_beat {
            continue;
        }
        for note in notes {
            let abs_on = clip.start_beats + note.start_beats;
            let abs_off = abs_on + note.length_beats;
            let on_sample = tempo.beat_to_sample(BeatPos(abs_on));
            let off_sample = tempo.beat_to_sample(BeatPos(abs_off));
            if on_sample.0 >= block_start.0 && on_sample.0 < block_end.0 {
                let offset = (on_sample.0 - block_start.0) as u32;
                events.push(ScheduledMidiEvent::note_on(
                    offset,
                    note.pitch,
                    note.velocity,
                    note.channel,
                ));
            }
            if off_sample.0 >= block_start.0 && off_sample.0 < block_end.0 {
                let offset = (off_sample.0 - block_start.0) as u32;
                events.push(ScheduledMidiEvent::note_off(
                    offset,
                    note.pitch,
                    note.channel,
                ));
            }
        }
    }
    events.sort_by_key(|e| e.sample_offset);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::TempoMap;

    #[test]
    fn schedules_note_on_off_in_block() {
        let track = TrackId::new();
        let mut clip = Clip::new_midi(track, "Clip", 0.0, 4.0);
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(60, 100, 0.0, 1.0));
        let tempo = TempoMap::default();
        // At 120bpm / 48kHz, 1 beat = 24000 samples. Note spans [0, 24000).
        let events = schedule_midi_for_block(&[clip], track, &tempo, SamplePos(0), 48_000);
        assert!(events.iter().any(|e| e.status & 0xf0 == 0x90 && e.data1 == 60));
        assert!(events.iter().any(|e| e.status & 0xf0 == 0x80 && e.data1 == 60));
    }

    /// Streaming MIDI: note-on and note-off may land in different blocks; middle
    /// blocks emit nothing while the instrument holds the voice.
    #[test]
    fn note_spanning_blocks_emits_on_then_off() {
        let track = TrackId::new();
        let mut clip = Clip::new_midi(track, "Clip", 0.0, 16.0);
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(60, 100, 0.0, 2.0));
        let tempo = TempoMap::default();
        let b0 = schedule_midi_for_block(&[clip.clone()], track, &tempo, SamplePos(0), 16_000);
        let b1 = schedule_midi_for_block(&[clip.clone()], track, &tempo, SamplePos(16_000), 16_000);
        let b3 = schedule_midi_for_block(&[clip], track, &tempo, SamplePos(48_000), 16_000);
        let is_on = |e: &ScheduledMidiEvent| e.status & 0xf0 == 0x90;
        let is_off = |e: &ScheduledMidiEvent| e.status & 0xf0 == 0x80;
        assert_eq!(b0.iter().filter(|e| is_on(e)).count(), 1);
        assert_eq!(b0.iter().filter(|e| is_off(e)).count(), 0);
        assert!(b1.is_empty());
        assert_eq!(b3.iter().filter(|e| is_on(e)).count(), 0);
        assert_eq!(b3.iter().filter(|e| is_off(e)).count(), 1);
    }
}
