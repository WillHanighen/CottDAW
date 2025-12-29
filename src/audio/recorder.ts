import * as Tone from 'tone';
import type { Track, LoopRegion } from '../types/index.ts';
import { midiToFrequency } from '../utils/noteHelpers.ts';
import { beatsToSeconds } from '../utils/timeHelpers.ts';

interface RenderOptions {
  tracks: Track[];
  bpm: number;
  duration: number; // in beats
  sampleRate?: number;
}

export async function renderToWav({
  tracks,
  bpm,
  duration,
  sampleRate = 44100,
}: RenderOptions): Promise<Blob> {
  const durationSeconds = beatsToSeconds(duration, bpm) + 2; // Add 2 seconds for release tails

  // Check if any track has solo enabled
  const hasSolo = tracks.some((t) => t.solo);

  const audioBuffer = await Tone.Offline(({ transport }) => {
    transport.bpm.value = bpm;

    tracks.forEach((track) => {
      // Skip muted tracks or non-solo tracks when solo is active
      if (track.muted) return;
      if (hasSolo && !track.solo) return;

      // Create effects chain for offline rendering
      const gain = new Tone.Gain(track.volume).toDestination();
      const panner = new Tone.Panner(track.pan).connect(gain);
      const distortion = new Tone.Distortion(track.effects.distortion.amount).connect(panner);
      const filter = new Tone.Filter({
        type: track.effects.filter.type,
        frequency: track.effects.filter.frequency,
        Q: track.effects.filter.Q,
      }).connect(distortion);
      const delay = new Tone.FeedbackDelay({
        delayTime: track.effects.delay.time,
        feedback: track.effects.delay.feedback,
        wet: track.effects.delay.wet,
      }).connect(filter);
      const reverb = new Tone.Reverb({
        decay: 2.5,
        wet: track.effects.reverb.wet,
      }).connect(delay);

      const synth = new Tone.Synth({
        oscillator: {
          type: track.waveType,
        },
        envelope: {
          attack: track.envelope.attack,
          decay: track.envelope.decay,
          sustain: track.envelope.sustain,
          release: track.envelope.release,
        },
      }).connect(reverb);

      // Schedule each note
      track.notes.forEach((note) => {
        const frequency = midiToFrequency(note.pitch);
        const startTime = beatsToSeconds(note.start, bpm);
        const noteDuration = beatsToSeconds(note.duration, bpm);

        transport.schedule((time) => {
          synth.triggerAttackRelease(frequency, noteDuration, time, note.velocity);
        }, startTime);
      });
    });

    transport.start();
  }, durationSeconds, 2, sampleRate);

  // Convert to WAV
  return audioBufferToWav(audioBuffer);
}

function audioBufferToWav(audioBuffer: Tone.ToneAudioBuffer): Blob {
  const buffer = audioBuffer.get();
  if (!buffer) {
    throw new Error('Failed to get audio buffer');
  }

  const numChannels = buffer.numberOfChannels;
  const sampleRate = buffer.sampleRate;
  const length = buffer.length;

  // Create interleaved buffer
  const interleaved = new Float32Array(length * numChannels);
  for (let channel = 0; channel < numChannels; channel++) {
    const channelData = buffer.getChannelData(channel);
    for (let i = 0; i < length; i++) {
      interleaved[i * numChannels + channel] = channelData[i];
    }
  }

  // Convert to 16-bit PCM
  const pcm = new Int16Array(interleaved.length);
  for (let i = 0; i < interleaved.length; i++) {
    const s = Math.max(-1, Math.min(1, interleaved[i]));
    pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }

  // Create WAV header
  const wavHeader = createWavHeader(pcm.length * 2, numChannels, sampleRate);
  
  // Combine header and data
  const wav = new Uint8Array(wavHeader.length + pcm.length * 2);
  wav.set(wavHeader, 0);
  wav.set(new Uint8Array(pcm.buffer), wavHeader.length);

  return new Blob([wav], { type: 'audio/wav' });
}

function createWavHeader(dataLength: number, numChannels: number, sampleRate: number): Uint8Array {
  const header = new ArrayBuffer(44);
  const view = new DataView(header);

  // RIFF chunk descriptor
  writeString(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataLength, true);
  writeString(view, 8, 'WAVE');

  // fmt sub-chunk
  writeString(view, 12, 'fmt ');
  view.setUint32(16, 16, true); // Subchunk1Size
  view.setUint16(20, 1, true); // AudioFormat (PCM)
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * numChannels * 2, true); // ByteRate
  view.setUint16(32, numChannels * 2, true); // BlockAlign
  view.setUint16(34, 16, true); // BitsPerSample

  // data sub-chunk
  writeString(view, 36, 'data');
  view.setUint32(40, dataLength, true);

  return new Uint8Array(header);
}

function writeString(view: DataView, offset: number, str: string): void {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i));
  }
}

export async function exportProject(
  tracks: Track[],
  bpm: number,
  projectName: string
): Promise<void> {
  // Calculate total duration from notes
  let maxEnd = 0;
  tracks.forEach((track) => {
    track.notes.forEach((note) => {
      const end = note.start + note.duration;
      if (end > maxEnd) maxEnd = end;
    });
  });

  // Default to 16 beats if no notes
  const duration = Math.max(16, Math.ceil(maxEnd));

  const blob = await renderToWav({ tracks, bpm, duration });

  // Download
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `${projectName.replace(/[^a-z0-9]/gi, '_').toLowerCase()}.wav`;
  a.click();
  URL.revokeObjectURL(url);
}

