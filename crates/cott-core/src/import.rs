//! MIDI and audio file import.

use crate::clips::{Clip, MidiNote};
use crate::dsp::AudioBuffer;
use crate::ids::AssetId;
use crate::ids::TrackId;
use crate::project::{Asset, AssetKind, Project};
use crate::time::TempoMap;
use anyhow::{Context, Result, anyhow};
use midly::{MetaMessage, Smf, Timing, TrackEventKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct ImportedAudio {
    pub buffer: AudioBuffer,
    pub sample_rate: u32,
    pub name: String,
}

pub fn import_audio_file(path: &Path, target_sr: u32) -> Result<ImportedAudio> {
    let file =
        std::fs::File::open(path).with_context(|| format!("open audio {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("probe audio")?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("no default audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("create decoder")?;
    let src_sr = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("unknown sample rate"))?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);

    let mut planar: Vec<Vec<f32>> = (0..channels).map(|_| Vec::new()).collect();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::ResetRequired) => continue,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if sample_buf.is_none() {
            sample_buf = Some(SampleBuffer::new(
                decoded.capacity() as u64,
                *decoded.spec(),
            ));
        }
        if let Some(buf) = sample_buf.as_mut() {
            buf.copy_interleaved_ref(decoded);
            let samples = buf.samples();
            let ch = channels;
            for (i, s) in samples.iter().enumerate() {
                planar[i % ch].push(*s);
            }
        }
    }

    let buffer = if src_sr != target_sr {
        resample_planar(&planar, src_sr, target_sr)?
    } else {
        AudioBuffer { channels: planar }
    };

    Ok(ImportedAudio {
        buffer,
        sample_rate: target_sr,
        name: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio")
            .to_string(),
    })
}

fn resample_planar(planar: &[Vec<f32>], src_sr: u32, dst_sr: u32) -> Result<AudioBuffer> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 16,
        window: WindowFunction::BlackmanHarris2,
    };
    let channels = planar.len();
    let mut resampler = SincFixedIn::<f32>::new(
        dst_sr as f64 / src_sr as f64,
        2.0,
        params,
        planar.first().map(|c| c.len()).unwrap_or(0).max(1),
        channels,
    )?;
    let waves: Vec<&[f32]> = planar.iter().map(|c| c.as_slice()).collect();
    let out = resampler.process(&waves, None)?;
    Ok(AudioBuffer { channels: out })
}

pub fn import_midi_file(path: &Path, tempo: &TempoMap) -> Result<Vec<MidiNote>> {
    let data = std::fs::read(path)?;
    let smf = Smf::parse(&data).map_err(|e| anyhow!("midi parse: {e}"))?;
    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as f64,
        Timing::Timecode(fps, subframe) => {
            // Tick length is 1/(fps*subframe) seconds; convert to beats via project tempo.
            let ticks_per_second = fps.as_f32() as f64 * (subframe.max(1) as f64);
            let beats_per_second = tempo.bpm / 60.0;
            (ticks_per_second / beats_per_second.max(1e-9)).max(1.0)
        }
    };

    let mut notes = Vec::new();
    // Track active note-ons: (channel, pitch) -> (start_ticks, velocity)
    for track in &smf.tracks {
        let mut abs_ticks: u64 = 0;
        let mut active: std::collections::HashMap<(u8, u8), (u64, u8)> =
            std::collections::HashMap::new();
        for ev in track {
            abs_ticks += ev.delta.as_int() as u64;
            match ev.kind {
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        midly::MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            active.insert((ch, key.as_int()), (abs_ticks, vel.as_int()));
                        }
                        midly::MidiMessage::NoteOn { key, vel: _ }
                        | midly::MidiMessage::NoteOff { key, vel: _ } => {
                            if let Some((start, velocity)) = active.remove(&(ch, key.as_int())) {
                                let start_beats = start as f64 / ticks_per_beat;
                                let end_beats = abs_ticks as f64 / ticks_per_beat;
                                let mut note = MidiNote::new(
                                    key.as_int(),
                                    velocity,
                                    start_beats,
                                    (end_beats - start_beats).max(1.0 / 64.0),
                                );
                                note.channel = ch;
                                notes.push(note);
                            }
                        }
                        _ => {}
                    }
                }
                TrackEventKind::Meta(MetaMessage::Tempo(_)) => {
                    // Project tempo map stays authoritative for MVP.
                    let _ = tempo;
                }
                _ => {}
            }
        }
    }
    Ok(notes)
}

/// Copy a file into the project assets folder and register it.
pub fn add_audio_asset_to_project(
    project: &mut Project,
    src_path: &Path,
    decoded: &ImportedAudio,
) -> Result<AssetId> {
    let root = project
        .root_dir
        .clone()
        .ok_or_else(|| anyhow!("project has no root directory; save first"))?;
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets)?;
    let file_name = src_path
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}.wav", decoded.name)));
    let dest = assets.join(&file_name);
    if src_path != dest {
        std::fs::copy(src_path, &dest)?;
    }
    let id = AssetId::new();
    let asset = Asset {
        id,
        name: decoded.name.clone(),
        relative_path: PathBuf::from("assets").join(file_name),
        kind: AssetKind::Audio,
        sample_rate: decoded.sample_rate,
        channels: decoded.buffer.channel_count() as u16,
        length_samples: decoded.buffer.frames() as u64,
        missing: false,
    };
    project.assets.insert(id, asset);
    project.touch();
    Ok(id)
}

pub fn create_audio_clip_from_import(
    project: &mut Project,
    track_id: TrackId,
    asset_id: AssetId,
    start_beats: f64,
) -> Option<Clip> {
    let asset = project.assets.get(&asset_id)?;
    let length_beats = project
        .tempo
        .samples_to_beats(asset.length_samples as i64)
        .max(0.25);
    Some(Clip::new_audio(
        track_id,
        asset.name.clone(),
        start_beats,
        length_beats,
        asset_id,
    ))
}

pub fn create_midi_clip_from_notes(
    track_id: TrackId,
    name: impl Into<String>,
    notes: Vec<MidiNote>,
    start_beats: f64,
) -> Clip {
    let end = notes.iter().map(|n| n.end_beats()).fold(4.0f64, f64::max);
    let mut clip = Clip::new_midi(track_id, name, start_beats, end.max(1.0));
    if let Some(n) = clip.notes_mut() {
        *n = notes;
    }
    clip
}

pub fn cache_put(cache: &mut crate::dsp::SampleCache, asset_id: AssetId, buffer: AudioBuffer) {
    cache.buffers.insert(asset_id, Arc::new(buffer));
}

/// Rebuild sample cache by decoding all non-missing audio assets from disk.
pub fn rebuild_sample_cache(
    project: &Project,
    target_sr: u32,
) -> Result<crate::dsp::SampleCache> {
    let mut cache = crate::dsp::SampleCache::default();
    let Some(root) = project.root_dir.as_ref() else {
        return Ok(cache);
    };
    for asset in project.assets.values() {
        if asset.kind != AssetKind::Audio || asset.missing {
            continue;
        }
        let path = root.join(&asset.relative_path);
        if !path.exists() {
            continue;
        }
        match import_audio_file(&path, target_sr) {
            Ok(decoded) => {
                cache_put(&mut cache, asset.id, decoded.buffer);
            }
            Err(e) => {
                tracing::warn!("failed to decode asset {}: {e:#}", asset.name);
            }
        }
    }
    Ok(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use midly::{Format, Header, TrackEvent};

    #[test]
    fn parse_simple_midi_bytes() {
        // Minimal valid SMF with one note is awkward to craft by hand;
        // ensure empty SMF parse path is exercised via midly roundtrip.
        let smf = Smf {
            header: Header::new(Format::SingleTrack, Timing::Metrical(480.into())),
            tracks: vec![vec![TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            }]],
        };
        let mut buf = Vec::new();
        smf.write(&mut buf).unwrap();
        let notes = import_midi_file_from_bytes(&buf, &TempoMap::default()).unwrap();
        assert!(notes.is_empty());
    }

    fn import_midi_file_from_bytes(data: &[u8], tempo: &TempoMap) -> Result<Vec<MidiNote>> {
        let path = {
            let dir = tempfile::tempdir()?;
            let p = dir.path().join("t.mid");
            std::fs::write(&p, data)?;
            // Keep dir alive by leaking for test simplicity — write to OS temp instead.
            let p2 = std::env::temp_dir().join(format!("cott-midi-{}.mid", std::process::id()));
            std::fs::write(&p2, data)?;
            p2
        };
        let notes = import_midi_file(&path, tempo);
        let _ = std::fs::remove_file(&path);
        notes
    }
}
