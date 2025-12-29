import React from 'react';
import { MIN_MIDI_NOTE, MAX_MIDI_NOTE, midiToNoteName, isBlackKey } from '../utils/noteHelpers.ts';

interface PianoKeysProps {
  noteHeight: number;
  onNoteClick?: (midi: number) => void;
}

export default function PianoKeys({ noteHeight, onNoteClick }: PianoKeysProps) {
  const notes = [];
  for (let midi = MAX_MIDI_NOTE; midi >= MIN_MIDI_NOTE; midi--) {
    notes.push(midi);
  }

  return (
    <div className="flex flex-col w-16 flex-shrink-0 border-r border-[var(--color-border)]">
      {notes.map((midi) => {
        const isBlack = isBlackKey(midi);
        const noteName = midiToNoteName(midi);
        const isC = midi % 12 === 0;

        return (
          <div
            key={midi}
            onClick={() => onNoteClick?.(midi)}
            className={`
              flex items-center justify-end px-2 border-b cursor-pointer select-none
              ${isBlack 
                ? 'bg-[#1a1a2e] text-[var(--color-text-muted)] hover:bg-[#252540]' 
                : 'bg-[#2d2d44] text-[var(--color-text-secondary)] hover:bg-[#3a3a55]'}
              ${isC ? 'border-b-[var(--color-border-light)]' : 'border-b-[var(--color-border)]'}
            `}
            style={{ height: noteHeight }}
          >
            <span className={`text-xs font-mono ${isC ? 'font-bold' : ''}`}>
              {noteName}
            </span>
          </div>
        );
      })}
    </div>
  );
}

