import type { GridSnap } from '../types/index.ts';

// Convert grid snap value to beats
export function gridSnapToBeats(snap: GridSnap): number {
  switch (snap) {
    case '1/4': return 1;
    case '1/8': return 0.5;
    case '1/16': return 0.25;
    case '1/32': return 0.125;
    default: return 1;
  }
}

// Snap a beat position to the nearest grid position
export function snapToGrid(beat: number, snap: GridSnap): number {
  const snapValue = gridSnapToBeats(snap);
  return Math.round(beat / snapValue) * snapValue;
}

// Convert beats to time in seconds
export function beatsToSeconds(beats: number, bpm: number): number {
  return (beats / bpm) * 60;
}

// Convert seconds to beats
export function secondsToBeats(seconds: number, bpm: number): number {
  return (seconds / 60) * bpm;
}

// Format time as bars:beats:sixteenths
export function formatTime(beats: number, beatsPerBar: number): string {
  const bars = Math.floor(beats / beatsPerBar) + 1;
  const beat = Math.floor(beats % beatsPerBar) + 1;
  const sixteenth = Math.floor((beats % 1) * 4) + 1;
  return `${bars}:${beat}:${sixteenth}`;
}

// Get total beats for a given number of bars
export function barsToBeats(bars: number, beatsPerBar: number): number {
  return bars * beatsPerBar;
}

// Calculate pixels per beat based on zoom level
export function getPixelsPerBeat(zoom: number): number {
  const BASE_PIXELS_PER_BEAT = 40;
  return BASE_PIXELS_PER_BEAT * zoom;
}

