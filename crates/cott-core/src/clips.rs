//! MIDI and audio clips on the arrangement timeline.

use crate::ids::{AssetId, ClipId, NoteId, TrackId};
use crate::scale::ScaleSettings;
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
        /// Piano-roll scale guide for this clip.
        #[serde(default)]
        scale: ScaleSettings,
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
    pub fn new_midi(
        track_id: TrackId,
        name: impl Into<String>,
        start_beats: f64,
        length_beats: f64,
    ) -> Self {
        Self {
            id: ClipId::new(),
            track_id,
            name: name.into(),
            start_beats,
            length_beats,
            content: ClipContent::Midi {
                notes: Vec::new(),
                scale: ScaleSettings::default(),
            },
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
            ClipContent::Midi { notes, .. } => Some(notes),
            _ => None,
        }
    }

    pub fn notes(&self) -> Option<&[MidiNote]> {
        match &self.content {
            ClipContent::Midi { notes, .. } => Some(notes),
            _ => None,
        }
    }

    pub fn scale(&self) -> Option<ScaleSettings> {
        match &self.content {
            ClipContent::Midi { scale, .. } => Some(*scale),
            _ => None,
        }
    }

    pub fn scale_mut(&mut self) -> Option<&mut ScaleSettings> {
        match &mut self.content {
            ClipContent::Midi { scale, .. } => Some(scale),
            _ => None,
        }
    }

    /// Clone this clip onto `track_id` at `start_beats` with fresh IDs.
    ///
    /// MIDI notes get new [`NoteId`]s; audio clips keep the same asset reference.
    pub fn duplicate_for_paste(&self, track_id: TrackId, start_beats: f64) -> Self {
        let content = match &self.content {
            ClipContent::Midi { notes, scale } => ClipContent::Midi {
                notes: notes
                    .iter()
                    .map(|note| MidiNote {
                        id: NoteId::new(),
                        pitch: note.pitch,
                        velocity: note.velocity,
                        start_beats: note.start_beats,
                        length_beats: note.length_beats,
                        channel: note.channel,
                    })
                    .collect(),
                scale: *scale,
            },
            ClipContent::Audio {
                asset_id,
                source_offset_samples,
                gain_db,
            } => ClipContent::Audio {
                asset_id: *asset_id,
                source_offset_samples: *source_offset_samples,
                gain_db: *gain_db,
            },
        };
        Self {
            id: ClipId::new(),
            track_id,
            name: self.name.clone(),
            start_beats: start_beats.max(0.0),
            length_beats: self.length_beats,
            content,
            color: self.color,
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

    /// MIDI CC 123 — All Notes Off (channel mode message).
    pub fn all_notes_off(sample_offset: u32, channel: u8) -> Self {
        Self {
            sample_offset,
            status: 0xB0 | (channel & 0x0f),
            data1: 123,
            data2: 0,
        }
    }

    /// MIDI CC 120 — All Sound Off (immediate silence, including hanging notes).
    pub fn all_sound_off(sample_offset: u32, channel: u8) -> Self {
        Self {
            sample_offset,
            status: 0xB0 | (channel & 0x0f),
            data1: 120,
            data2: 0,
        }
    }

    /// Sort key so panic / note-off land before note-on at the same sample offset.
    pub fn sort_priority(self) -> u8 {
        match self.status & 0xf0 {
            0xb0 if self.data1 == 120 || self.data1 == 123 => 0,
            0x80 => 1,
            0x90 if self.data2 == 0 => 1,
            0x90 => 3,
            _ => 2,
        }
    }
}

/// Queue all-notes-off + all-sound-off on every MIDI channel for a track.
pub fn midi_panic_events(track_id: TrackId) -> Vec<(TrackId, ScheduledMidiEvent)> {
    let mut events = Vec::with_capacity(32);
    for ch in 0..16u8 {
        events.push((track_id, ScheduledMidiEvent::all_notes_off(0, ch)));
        events.push((track_id, ScheduledMidiEvent::all_sound_off(0, ch)));
    }
    events
}

/// Notes whose note-on has already occurred and note-off has not, at `pos`.
///
/// Used to inject real note-offs on transport discontinuities — VST3 hosts
/// drop MIDI CC panic, so we must release voices with NoteOff events.
pub fn notes_held_at(
    clips: &[Clip],
    track_id: TrackId,
    tempo: &crate::time::TempoMap,
    pos: SamplePos,
) -> Vec<(u8, u8)> {
    let mut held = Vec::new();
    for clip in clips.iter().filter(|c| c.track_id == track_id) {
        let Some(notes) = clip.notes() else { continue };
        for note in notes {
            let abs_on = clip.start_beats + note.start_beats;
            let abs_off = abs_on + note.length_beats;
            let on_sample = tempo.beat_to_sample(BeatPos(abs_on));
            let off_sample = tempo.beat_to_sample(BeatPos(abs_off));
            if on_sample.0 <= pos.0 && pos.0 < off_sample.0 {
                let key = (note.pitch.min(127), note.channel & 0x0f);
                if !held.contains(&key) {
                    held.push(key);
                }
            }
        }
    }
    held
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
        // Keep the block starting exactly at the clip end: a note that reaches
        // the clip boundary emits its note-off at offset zero in that block.
        if clip.end_beats() < start_beat || clip.start_beats >= end_beat {
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
    events.sort_by(|a, b| {
        a.sample_offset
            .cmp(&b.sample_offset)
            .then_with(|| a.sort_priority().cmp(&b.sort_priority()))
    });
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::ScaleMode;
    use crate::time::TempoMap;

    #[test]
    fn midi_clip_scale_roundtrips_and_defaults_for_old_projects() {
        let track = TrackId::new();
        let mut clip = Clip::new_midi(track, "Scale", 0.0, 4.0);
        *clip.scale_mut().unwrap() = ScaleSettings {
            highlight: true,
            root: 9,
            mode: ScaleMode::Minor,
        };

        let json = serde_json::to_value(&clip).unwrap();
        let roundtrip: Clip = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(roundtrip.scale(), clip.scale());

        let mut legacy = json;
        legacy["content"]["Midi"]
            .as_object_mut()
            .unwrap()
            .remove("scale");
        let loaded_legacy: Clip = serde_json::from_value(legacy).unwrap();
        assert_eq!(loaded_legacy.scale(), Some(ScaleSettings::default()));
    }

    #[test]
    fn duplicate_for_paste_regenerates_ids() {
        let track = TrackId::new();
        let other = TrackId::new();
        let mut clip = Clip::new_midi(track, "Chord", 2.0, 4.0);
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(60, 100, 0.5, 1.0));
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(64, 90, 0.5, 1.0));

        let pasted = clip.duplicate_for_paste(other, 8.0);
        assert_ne!(pasted.id, clip.id);
        assert_eq!(pasted.track_id, other);
        assert_eq!(pasted.start_beats, 8.0);
        assert_eq!(pasted.name, "Chord");
        assert_eq!(pasted.notes().unwrap().len(), 2);
        assert_ne!(pasted.notes().unwrap()[0].id, clip.notes().unwrap()[0].id);
        assert_eq!(pasted.notes().unwrap()[0].pitch, 60);
        assert_eq!(pasted.notes().unwrap()[0].start_beats, 0.5);
    }

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
        assert!(
            events
                .iter()
                .any(|e| e.status & 0xf0 == 0x90 && e.data1 == 60)
        );
        assert!(
            events
                .iter()
                .any(|e| e.status & 0xf0 == 0x80 && e.data1 == 60)
        );
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

    #[test]
    fn note_ending_at_clip_boundary_emits_off_in_next_block() {
        let track = TrackId::new();
        let mut clip = Clip::new_midi(track, "Clip", 0.0, 4.0);
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(60, 100, 3.0, 1.0));
        let tempo = TempoMap::default();
        let clip_end = tempo.beat_to_sample(BeatPos(4.0));

        let events = schedule_midi_for_block(&[clip], track, &tempo, clip_end, 256);
        assert!(
            events
                .iter()
                .any(|e| e.status & 0xf0 == 0x80 && e.data1 == 60)
        );
    }

    #[test]
    fn midi_panic_emits_all_channels() {
        let track = TrackId::new();
        let events = midi_panic_events(track);
        assert_eq!(events.len(), 32);
        assert!(events.iter().all(|(t, _)| *t == track));
        assert!(
            events
                .iter()
                .any(|(_, e)| e.status == 0xB0 && e.data1 == 123)
        );
        assert!(
            events
                .iter()
                .any(|(_, e)| e.status == 0xBF && e.data1 == 120)
        );
    }

    #[test]
    fn sort_priority_puts_panic_before_note_on() {
        let on = ScheduledMidiEvent::note_on(0, 60, 100, 0);
        let off = ScheduledMidiEvent::note_off(0, 60, 0);
        let panic = ScheduledMidiEvent::all_notes_off(0, 0);
        assert!(panic.sort_priority() < off.sort_priority());
        assert!(off.sort_priority() < on.sort_priority());
    }

    #[test]
    fn notes_held_at_detects_sounding_note() {
        let track = TrackId::new();
        let mut clip = Clip::new_midi(track, "Clip", 0.0, 8.0);
        clip.notes_mut()
            .unwrap()
            .push(MidiNote::new(60, 100, 0.0, 2.0));
        let tempo = TempoMap::default();
        // Mid-note (1 beat in): held.
        let mid = tempo.beat_to_sample(BeatPos(1.0));
        let held = notes_held_at(&[clip.clone()], track, &tempo, mid);
        assert_eq!(held, vec![(60, 0)]);
        // After note end: not held.
        let after = tempo.beat_to_sample(BeatPos(3.0));
        assert!(notes_held_at(&[clip], track, &tempo, after).is_empty());
    }
}
