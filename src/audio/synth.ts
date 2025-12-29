import * as Tone from 'tone';
import type { Track, WaveType } from '../types/index.ts';

interface TrackSynth {
  synth: Tone.Synth;
  panner: Tone.Panner;
  reverb: Tone.Reverb;
  delay: Tone.FeedbackDelay;
  filter: Tone.Filter;
  distortion: Tone.Distortion;
  gain: Tone.Gain;
}

const trackSynths = new Map<string, TrackSynth>();

export function createTrackSynth(track: Track): TrackSynth {
  // Create effects chain
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

  // Create synth with ADSR envelope
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

  const trackSynth = { synth, panner, reverb, delay, filter, distortion, gain };
  trackSynths.set(track.id, trackSynth);
  
  return trackSynth;
}

export function getTrackSynth(trackId: string): TrackSynth | undefined {
  return trackSynths.get(trackId);
}

export function updateTrackSynth(track: Track): void {
  let trackSynth = trackSynths.get(track.id);
  
  if (!trackSynth) {
    trackSynth = createTrackSynth(track);
    return;
  }

  // Update synth parameters
  trackSynth.synth.oscillator.type = track.waveType;
  trackSynth.synth.envelope.attack = track.envelope.attack;
  trackSynth.synth.envelope.decay = track.envelope.decay;
  trackSynth.synth.envelope.sustain = track.envelope.sustain;
  trackSynth.synth.envelope.release = track.envelope.release;

  // Update effects
  trackSynth.gain.gain.value = track.muted ? 0 : track.volume;
  trackSynth.panner.pan.value = track.pan;
  trackSynth.reverb.wet.value = track.effects.reverb.wet;
  trackSynth.delay.feedback.value = track.effects.delay.feedback;
  trackSynth.delay.wet.value = track.effects.delay.wet;
  trackSynth.filter.type = track.effects.filter.type;
  trackSynth.filter.frequency.value = track.effects.filter.frequency;
  trackSynth.filter.Q.value = track.effects.filter.Q;
  trackSynth.distortion.distortion = track.effects.distortion.amount;
}

export function disposeTrackSynth(trackId: string): void {
  const trackSynth = trackSynths.get(trackId);
  if (trackSynth) {
    trackSynth.synth.dispose();
    trackSynth.panner.dispose();
    trackSynth.reverb.dispose();
    trackSynth.delay.dispose();
    trackSynth.filter.dispose();
    trackSynth.distortion.dispose();
    trackSynth.gain.dispose();
    trackSynths.delete(trackId);
  }
}

export function disposeAllSynths(): void {
  trackSynths.forEach((_, trackId) => disposeTrackSynth(trackId));
}

export function playNote(trackId: string, frequency: number, duration: number, time: number, velocity: number): void {
  const trackSynth = trackSynths.get(trackId);
  if (trackSynth) {
    trackSynth.synth.triggerAttackRelease(frequency, duration, time, velocity);
  }
}

export function triggerAttack(trackId: string, frequency: number, velocity: number = 0.8): void {
  const trackSynth = trackSynths.get(trackId);
  if (trackSynth) {
    trackSynth.synth.triggerAttack(frequency, undefined, velocity);
  }
}

export function triggerRelease(trackId: string): void {
  const trackSynth = trackSynths.get(trackId);
  if (trackSynth) {
    trackSynth.synth.triggerRelease();
  }
}

