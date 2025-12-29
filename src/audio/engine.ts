import * as Tone from 'tone';
import type { Track, Note, LoopRegion } from '../types/index.ts';
import { midiToFrequency } from '../utils/noteHelpers.ts';
import { beatsToSeconds } from '../utils/timeHelpers.ts';
import { createTrackSynth, updateTrackSynth, disposeTrackSynth, getTrackSynth } from './synth.ts';

let isInitialized = false;
let scheduledEvents: number[] = [];
let metronomeLoop: Tone.Loop | null = null;
let metronomeSynth: Tone.MembraneSynth | null = null;

// Initialize audio context (must be called from user interaction)
export async function initAudio(): Promise<void> {
  if (isInitialized) return;
  
  await Tone.start();
  isInitialized = true;
  console.log('Audio context initialized');
}

export function isAudioInitialized(): boolean {
  return isInitialized;
}

// Set transport BPM
export function setBPM(bpm: number): void {
  Tone.getTransport().bpm.value = bpm;
}

// Schedule all notes for playback
export function scheduleNotes(
  tracks: Track[],
  loop: LoopRegion,
  bpm: number,
  onBeatUpdate: (beat: number) => void
): void {
  clearScheduledNotes();
  setBPM(bpm);

  const transport = Tone.getTransport();

  // Check if any track has solo enabled
  const hasSolo = tracks.some((t) => t.solo);

  tracks.forEach((track) => {
    // Skip muted tracks or non-solo tracks when solo is active
    if (track.muted) return;
    if (hasSolo && !track.solo) return;

    // Ensure synth exists
    let trackSynth = getTrackSynth(track.id);
    if (!trackSynth) {
      trackSynth = createTrackSynth(track);
    }
    updateTrackSynth(track);

    // Schedule each note
    track.notes.forEach((note) => {
      const frequency = midiToFrequency(note.pitch);
      const startTime = beatsToSeconds(note.start, bpm);
      const duration = beatsToSeconds(note.duration, bpm);

      const eventId = transport.schedule((time) => {
        const synth = getTrackSynth(track.id);
        if (synth) {
          synth.synth.triggerAttackRelease(frequency, duration, time, note.velocity);
        }
      }, startTime);

      scheduledEvents.push(eventId as unknown as number);
    });
  });

  // Set up loop if enabled
  if (loop.enabled) {
    transport.loop = true;
    transport.loopStart = beatsToSeconds(loop.start, bpm);
    transport.loopEnd = beatsToSeconds(loop.end, bpm);
  } else {
    transport.loop = false;
  }

  // Schedule beat updates
  const beatSchedule = transport.scheduleRepeat((time) => {
    Tone.getDraw().schedule(() => {
      const position = transport.position;
      if (typeof position === 'string') {
        const [bars, beats, sixteenths] = position.split(':').map(Number);
        const totalBeats = bars * 4 + beats + sixteenths / 4;
        onBeatUpdate(totalBeats);
      }
    }, time);
  }, '16n');

  scheduledEvents.push(beatSchedule as unknown as number);
}

export function clearScheduledNotes(): void {
  const transport = Tone.getTransport();
  scheduledEvents.forEach((id) => transport.clear(id));
  scheduledEvents = [];
}

export function startPlayback(): void {
  Tone.getTransport().start();
}

export function pausePlayback(): void {
  Tone.getTransport().pause();
}

export function stopPlayback(): void {
  Tone.getTransport().stop();
  Tone.getTransport().position = 0;
}

export function seekTo(beat: number, bpm: number): void {
  const time = beatsToSeconds(beat, bpm);
  Tone.getTransport().position = time;
}

// Metronome
export function setupMetronome(beatsPerBar: number, volume: number): void {
  if (metronomeSynth) {
    metronomeSynth.dispose();
  }
  if (metronomeLoop) {
    metronomeLoop.dispose();
  }

  metronomeSynth = new Tone.MembraneSynth({
    pitchDecay: 0.008,
    octaves: 2,
    envelope: {
      attack: 0.001,
      decay: 0.3,
      sustain: 0,
      release: 0.1,
    },
  }).toDestination();

  metronomeSynth.volume.value = Tone.gainToDb(volume);

  let beatCount = 0;

  metronomeLoop = new Tone.Loop((time) => {
    const isDownbeat = beatCount % beatsPerBar === 0;
    const pitch = isDownbeat ? 'C5' : 'G4';
    metronomeSynth?.triggerAttackRelease(pitch, '32n', time);
    beatCount++;
  }, '4n');
}

export function startMetronome(): void {
  metronomeLoop?.start(0);
}

export function stopMetronome(): void {
  metronomeLoop?.stop();
}

export function setMetronomeVolume(volume: number): void {
  if (metronomeSynth) {
    metronomeSynth.volume.value = Tone.gainToDb(volume);
  }
}

// Preview note (when clicking piano keys)
export function previewNote(trackId: string, midiNote: number): void {
  const synth = getTrackSynth(trackId);
  if (synth) {
    const frequency = midiToFrequency(midiNote);
    synth.synth.triggerAttackRelease(frequency, '8n');
  }
}

// Cleanup
export function cleanup(): void {
  stopPlayback();
  clearScheduledNotes();
  stopMetronome();
  if (metronomeSynth) {
    metronomeSynth.dispose();
    metronomeSynth = null;
  }
  if (metronomeLoop) {
    metronomeLoop.dispose();
    metronomeLoop = null;
  }
}

