// MIDI note utilities
// C1 = 24, C7 = 96 (we use 24-95 for C1 to B6)

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

export const MIN_MIDI_NOTE = 24; // C1
export const MAX_MIDI_NOTE = 95; // B6
export const TOTAL_NOTES = MAX_MIDI_NOTE - MIN_MIDI_NOTE + 1; // 72 notes

export function midiToNoteName(midi: number): string {
  const octave = Math.floor(midi / 12) - 1;
  const noteIndex = midi % 12;
  return `${NOTE_NAMES[noteIndex]}${octave}`;
}

export function midiToFrequency(midi: number): number {
  // A4 = 440Hz = MIDI 69
  return 440 * Math.pow(2, (midi - 69) / 12);
}

export function noteNameToMidi(noteName: string): number {
  const match = noteName.match(/^([A-G]#?)(\d+)$/);
  if (!match) return 60; // Default to C4
  
  const [, note, octaveStr] = match;
  const octave = parseInt(octaveStr, 10);
  const noteIndex = NOTE_NAMES.indexOf(note);
  
  return (octave + 1) * 12 + noteIndex;
}

export function isBlackKey(midi: number): boolean {
  const noteIndex = midi % 12;
  return [1, 3, 6, 8, 10].includes(noteIndex);
}

export function getNoteColor(midi: number): string {
  return isBlackKey(midi) ? '#1a1a2e' : '#2d2d44';
}

