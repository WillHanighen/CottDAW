export type WaveType = 'sine' | 'square' | 'sawtooth' | 'triangle';
export type FilterType = 'lowpass' | 'highpass';
export type Tool = 'draw' | 'eraser' | 'select';
export type GridSnap = '1/4' | '1/8' | '1/16' | '1/32';

export interface Envelope {
  attack: number;
  decay: number;
  sustain: number;
  release: number;
}

export interface ReverbEffect {
  wet: number;
}

export interface DelayEffect {
  time: string;
  feedback: number;
  wet: number;
}

export interface FilterEffect {
  type: FilterType;
  frequency: number;
  Q: number;
}

export interface DistortionEffect {
  amount: number;
}

export interface TrackEffects {
  reverb: ReverbEffect;
  delay: DelayEffect;
  filter: FilterEffect;
  distortion: DistortionEffect;
}

export interface Note {
  id: string;
  pitch: number;       // MIDI note number (24-95 for C1-C7)
  start: number;       // Start time in beats
  duration: number;    // Duration in beats
  velocity: number;    // 0-1
}

export interface Track {
  id: string;
  name: string;
  color: string;
  waveType: WaveType;
  volume: number;      // 0-1
  pan: number;         // -1 to 1
  muted: boolean;
  solo: boolean;
  envelope: Envelope;
  effects: TrackEffects;
  notes: Note[];
}

export interface Project {
  name: string;
  bpm: number;
  timeSignature: [number, number]; // [beats, noteValue]
  tracks: Track[];
}

export interface LoopRegion {
  start: number;  // in beats
  end: number;    // in beats
  enabled: boolean;
}

export interface Selection {
  noteIds: string[];
  trackId: string | null;
}

