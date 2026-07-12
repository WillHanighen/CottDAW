//! Offline mixdown and Ogg Opus / WAV / Gonio MP4 export.

use crate::dsp::{AudioBuffer, SampleCache};
use crate::engine::render_offline;
use crate::project::Project;
use crate::visualizers::{GonioOptions, GonioRenderer};
use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    Wav,
    #[default]
    Opus,
    GonioMp4,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Opus => "opus",
            Self::GonioMp4 => "mp4",
        }
    }

    pub fn filter_name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Opus => "Ogg Opus",
            Self::GonioMp4 => "Gonio MP4",
        }
    }

    pub fn default_file_name(self) -> &'static str {
        match self {
            Self::Wav => "mixdown.wav",
            Self::Opus => "mixdown.opus",
            Self::GonioMp4 => "gonio.mp4",
        }
    }
}

pub struct ExportOptions {
    pub format: ExportFormat,
    pub bitrate_bps: i32,
    pub block_size: u32,
    /// Extra bars of silence padding at end.
    pub tail_beats: f64,
    pub gonio: GonioOptions,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Opus,
            bitrate_bps: 128_000,
            block_size: 512,
            tail_beats: 1.0,
            gonio: GonioOptions::default(),
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

/// Bounce mix + animate a goniometer, muxed to H.264/AAC MP4 via ffmpeg.
pub fn export_gonio_mp4(
    project: &Project,
    sample_cache: &SampleCache,
    plugin_host: &mut dyn crate::dsp::PluginAudioHost,
    out_path: &Path,
    opts: &ExportOptions,
) -> Result<()> {
    let rendered = render_project_stereo(project, sample_cache, plugin_host, opts);
    write_gonio_mp4(&rendered, project.tempo.sample_rate, out_path, &opts.gonio)
}

pub fn write_gonio_mp4(
    stereo: &AudioBuffer,
    sample_rate: u32,
    out_path: &Path,
    gonio: &GonioOptions,
) -> Result<()> {
    let gonio = gonio.clone().clamp();
    let left = stereo
        .channels
        .first()
        .map(|c| c.as_slice())
        .unwrap_or(&[]);
    let right = stereo
        .channels
        .get(1)
        .map(|c| c.as_slice())
        .unwrap_or(left);
    let frames = left.len().min(right.len());
    if frames == 0 {
        return Err(anyhow!("nothing to export — mix is empty"));
    }

    let tmp_dir = tempfile::tempdir()?;
    let wav_path = tmp_dir.path().join("bounce.wav");
    write_wav_file(stereo, sample_rate, &wav_path)?;

    let samples_per_frame = ((sample_rate as f64) / (gonio.fps as f64))
        .round()
        .max(1.0) as usize;
    let size = format!("{}x{}", gonio.width, gonio.height);
    let fps = gonio.fps.to_string();
    let crf = gonio.crf.to_string();
    let wav_str = wav_path
        .to_str()
        .ok_or_else(|| anyhow!("bad wav path"))?
        .to_string();
    let out_str = out_path
        .to_str()
        .ok_or_else(|| anyhow!("bad output path"))?
        .to_string();

    let mut child = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
            &size,
            "-r",
            &fps,
            "-i",
            "-",
            "-i",
            &wav_str,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            &crf,
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-movflags",
            "+faststart",
            &out_str,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn ffmpeg (install ffmpeg for Gonio MP4 export)")?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("ffmpeg stdin unavailable"))?;

    let mut renderer = GonioRenderer::new(gonio);
    let mut start = 0usize;
    while start < frames {
        let end = (start + samples_per_frame).min(frames);
        renderer.render_frame(&left[start..end], &right[start..end]);
        stdin
            .write_all(renderer.pixels())
            .context("write gonio frame to ffmpeg")?;
        start = end;
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .context("wait for ffmpeg gonio encode")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let tail = err
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(anyhow!("ffmpeg gonio encode failed: {tail}"));
    }
    Ok(())
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

    #[test]
    fn gonio_frame_pipeline_smoke() {
        // Synthetic stereo chirp — no ffmpeg required.
        let n = 2048;
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / n as f32;
            left.push((t * std::f32::consts::TAU * 4.0).sin() * 0.5);
            right.push((t * std::f32::consts::TAU * 4.0).cos() * 0.4);
        }
        let buf = AudioBuffer {
            channels: vec![left, right],
        };
        let mut g = GonioRenderer::new(GonioOptions {
            width: 320,
            height: 320,
            fps: 30,
            ..Default::default()
        });
        let spf = 512;
        let mut start = 0;
        while start < buf.frames() {
            let end = (start + spf).min(buf.frames());
            g.render_frame(&buf.channels[0][start..end], &buf.channels[1][start..end]);
            assert_eq!(g.pixels().len(), 320 * 320 * 3);
            start = end;
        }
    }

    #[test]
    fn gonio_mp4_ffmpeg_smoke() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("skip: ffmpeg not installed");
            return;
        }
        let sr = 48_000u32;
        let n = sr as usize / 4; // 0.25s
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / sr as f32;
            left.push((t * 440.0 * std::f32::consts::TAU).sin() * 0.4);
            right.push((t * 440.0 * std::f32::consts::TAU).sin() * 0.35);
        }
        let buf = AudioBuffer {
            channels: vec![left, right],
        };
        let dir = tempdir().unwrap();
        let path = dir.path().join("gonio.mp4");
        write_gonio_mp4(
            &buf,
            sr,
            &path,
            &GonioOptions {
                width: 320,
                height: 320,
                fps: 15,
                crf: 28,
                draw_mode: crate::visualizers::GonioDrawMode::Line,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(path.exists());
        assert!(path.metadata().unwrap().len() > 1000);
    }
}
