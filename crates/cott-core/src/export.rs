//! Offline mixdown and Ogg Opus / WAV export.

use crate::dsp::{AudioBuffer, SampleCache};
use crate::engine::render_offline;
use crate::project::Project;
use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Command;

pub struct ExportOptions {
    pub bitrate_bps: i32,
    pub block_size: u32,
    /// Extra bars of silence padding at end.
    pub tail_beats: f64,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            bitrate_bps: 128_000,
            block_size: 512,
            tail_beats: 1.0,
        }
    }
}

pub fn project_length_samples(project: &Project, opts: &ExportOptions) -> i64 {
    let end_beat = project
        .clips
        .iter()
        .map(|c| c.end_beats())
        .fold(project.loop_end_beats, f64::max)
        + opts.tail_beats;
    project
        .tempo
        .beat_to_sample(crate::time::BeatPos(end_beat))
        .0
}

pub fn render_project_stereo(
    project: &Project,
    sample_cache: &SampleCache,
    plugin_host: &mut dyn crate::dsp::PluginAudioHost,
    opts: &ExportOptions,
) -> AudioBuffer {
    let len = project_length_samples(project, opts);
    render_offline(project, sample_cache, plugin_host, len, opts.block_size)
}

/// Resample planar buffer to 48 kHz stereo for Opus.
pub fn ensure_48k_stereo(buffer: &AudioBuffer, src_sr: u32) -> Result<AudioBuffer> {
    let stereo = match buffer.channel_count() {
        0 => AudioBuffer::silent(2, 0),
        1 => AudioBuffer {
            channels: vec![buffer.channels[0].clone(), buffer.channels[0].clone()],
        },
        _ => AudioBuffer {
            channels: vec![buffer.channels[0].clone(), buffer.channels[1].clone()],
        },
    };
    if src_sr == 48_000 {
        return Ok(stereo);
    }
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
    let frames = stereo.frames().max(1);
    let mut resampler = SincFixedIn::<f32>::new(48_000.0 / src_sr as f64, 2.0, params, frames, 2)?;
    let waves = [stereo.channels[0].as_slice(), stereo.channels[1].as_slice()];
    let out = resampler.process(&waves, None)?;
    Ok(AudioBuffer { channels: out })
}

pub fn export_opus(
    project: &Project,
    sample_cache: &SampleCache,
    plugin_host: &mut dyn crate::dsp::PluginAudioHost,
    out_path: &Path,
    opts: &ExportOptions,
) -> Result<()> {
    let rendered = render_project_stereo(project, sample_cache, plugin_host, opts);
    let stereo = ensure_48k_stereo(&rendered, project.tempo.sample_rate)?;
    write_opus_via_ffmpeg(&stereo, 48_000, out_path, opts.bitrate_bps)
}

pub fn write_opus_via_ffmpeg(
    stereo: &AudioBuffer,
    sample_rate: u32,
    out_path: &Path,
    bitrate_bps: i32,
) -> Result<()> {
    let tmp_dir = tempfile::tempdir()?;
    let wav_path = tmp_dir.path().join("bounce.wav");
    write_wav_file(stereo, sample_rate, &wav_path)?;

    let bitrate = format!("{}k", (bitrate_bps / 1000).max(16));
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            wav_path.to_str().ok_or_else(|| anyhow!("bad wav path"))?,
            "-c:a",
            "libopus",
            "-b:a",
            &bitrate,
            out_path
                .to_str()
                .ok_or_else(|| anyhow!("bad output path"))?,
        ])
        .status()
        .context("spawn ffmpeg (install ffmpeg for Opus export)")?;
    if !status.success() {
        return Err(anyhow!("ffmpeg failed with {status}"));
    }
    Ok(())
}

pub fn write_wav_file(stereo: &AudioBuffer, sample_rate: u32, out_path: &Path) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(out_path, spec).context("create wav")?;
    let frames = stereo.frames();
    for i in 0..frames {
        let l = (stereo.channels[0][i].clamp(-1.0, 1.0) * 32767.0) as i16;
        let r = stereo
            .channels
            .get(1)
            .map(|c| (c[i].clamp(-1.0, 1.0) * 32767.0) as i16)
            .unwrap_or(l);
        writer.write_sample(l)?;
        writer.write_sample(r)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Convenience for tests that don't need Opus.
pub fn export_wav(
    project: &Project,
    sample_cache: &SampleCache,
    plugin_host: &mut dyn crate::dsp::PluginAudioHost,
    out_path: &Path,
    opts: &ExportOptions,
) -> Result<()> {
    let buf = render_project_stereo(project, sample_cache, plugin_host, opts);
    write_wav_file(&buf, project.tempo.sample_rate, out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::NullPluginHost;
    use tempfile::tempdir;

    #[test]
    fn export_wav_smoke() {
        let project = Project::new("t");
        let cache = SampleCache::default();
        let mut host = NullPluginHost;
        let opts = ExportOptions {
            tail_beats: 0.25,
            ..Default::default()
        };
        let buf = render_project_stereo(&project, &cache, &mut host, &opts);
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.wav");
        write_wav_file(&buf, 48_000, &path).unwrap();
        assert!(path.exists());
    }
}
