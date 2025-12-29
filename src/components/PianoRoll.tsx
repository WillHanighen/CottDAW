import React, { useRef, useCallback, useEffect, useState } from 'react';
import { v4 as uuidv4 } from 'uuid';
import PianoKeys from './PianoKeys.tsx';
import NoteBlock from './NoteBlock.tsx';
import { useUIStore } from '../stores/uiStore.ts';
import { useProjectStore } from '../stores/projectStore.ts';
import { useTransportStore } from '../stores/transportStore.ts';
import { MIN_MIDI_NOTE, MAX_MIDI_NOTE, TOTAL_NOTES, isBlackKey, midiToFrequency } from '../utils/noteHelpers.ts';
import { getPixelsPerBeat, snapToGrid, gridSnapToBeats } from '../utils/timeHelpers.ts';
import { triggerAttack, triggerRelease, getTrackSynth, createTrackSynth } from '../audio/synth.ts';
import * as Tone from 'tone';
import type { Track } from '../types/index.ts';

interface PianoRollProps {
  track: Track;
}

const BASE_NOTE_HEIGHT = 20;
const MIN_BEATS_VISIBLE = 64; // Minimum 16 bars in 4/4
const EXPANSION_THRESHOLD = 0.95; // Expand when note is in last 5%
const EXPANSION_AMOUNT = 16; // Add 4 bars (16 beats) when expanding

export default function PianoRoll({ track }: PianoRollProps) {
  const gridRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [isDrawing, setIsDrawing] = useState(false);
  const [drawStart, setDrawStart] = useState<{ beat: number; pitch: number } | null>(null);
  
  // Selection box state for rubber band selection
  const [isSelecting, setIsSelecting] = useState(false);
  const [selectionBox, setSelectionBox] = useState<{ startX: number; startY: number; endX: number; endY: number } | null>(null);
  
  // Extra beats for preview expansion (when drawing near end)
  const [extraBeats, setExtraBeats] = useState(0);

  const horizontalZoom = useUIStore((s) => s.horizontalZoom);
  const verticalZoom = useUIStore((s) => s.verticalZoom);
  const currentTool = useUIStore((s) => s.currentTool);
  const gridSnap = useUIStore((s) => s.gridSnap);
  const clearSelection = useUIStore((s) => s.clearSelection);
  const selection = useUIStore((s) => s.selection);
  const setSelection = useUIStore((s) => s.setSelection);
  const addToSelection = useUIStore((s) => s.addToSelection);

  const addNote = useProjectStore((s) => s.addNote);
  const removeNote = useProjectStore((s) => s.removeNote);
  const removeNotes = useProjectStore((s) => s.removeNotes);
  const duplicateNotes = useProjectStore((s) => s.duplicateNotes);
  const saveToHistory = useProjectStore((s) => s.saveToHistory);
  const undo = useProjectStore((s) => s.undo);
  const redo = useProjectStore((s) => s.redo);
  const bpm = useProjectStore((s) => s.bpm);
  const timeSignature = useProjectStore((s) => s.timeSignature);

  const currentBeat = useTransportStore((s) => s.currentBeat);
  const isPlaying = useTransportStore((s) => s.isPlaying);
  const loop = useTransportStore((s) => s.loop);

  // Calculate dynamic grid length based on notes
  const maxNoteEnd = Math.max(0, ...track.notes.map(n => n.start + n.duration));
  // Round up to next expansion increment and ensure minimum, plus any extra preview beats
  const beatsVisible = Math.max(
    MIN_BEATS_VISIBLE,
    Math.ceil(maxNoteEnd / EXPANSION_AMOUNT) * EXPANSION_AMOUNT + EXPANSION_AMOUNT
  ) + extraBeats;

  const pixelsPerBeat = getPixelsPerBeat(horizontalZoom);
  const noteHeight = BASE_NOTE_HEIGHT * verticalZoom;
  const gridWidth = beatsVisible * pixelsPerBeat;
  const gridHeight = TOTAL_NOTES * noteHeight;
  const beatsPerBar = timeSignature[0];
  const snapBeats = gridSnapToBeats(gridSnap);

  // Calculate position from mouse event
  const getPositionFromEvent = useCallback((e: React.MouseEvent) => {
    if (!gridRef.current) return null;
    
    const rect = gridRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left + gridRef.current.scrollLeft;
    const y = e.clientY - rect.top + gridRef.current.scrollTop;
    
    const beat = x / pixelsPerBeat;
    const pitch = MAX_MIDI_NOTE - Math.floor(y / noteHeight);
    
    return { beat: Math.max(0, beat), pitch: Math.max(MIN_MIDI_NOTE, Math.min(MAX_MIDI_NOTE, pitch)) };
  }, [pixelsPerBeat, noteHeight]);

  // Play a preview note
  const playPreviewNote = useCallback(async (pitch: number) => {
    // Ensure audio context is started
    if (Tone.getContext().state !== 'running') {
      await Tone.start();
    }
    
    // Ensure track synth exists
    if (!getTrackSynth(track.id)) {
      createTrackSynth(track);
    }
    
    const frequency = midiToFrequency(pitch);
    triggerAttack(track.id, frequency, 0.8);
    
    // Auto-release after a short time
    setTimeout(() => {
      triggerRelease(track.id);
    }, 200);
  }, [track]);

  // Get raw pixel position from mouse event (for selection box)
  const getRawPositionFromEvent = useCallback((e: React.MouseEvent | MouseEvent) => {
    if (!gridRef.current) return null;
    
    const rect = gridRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left + gridRef.current.scrollLeft;
    const y = e.clientY - rect.top + gridRef.current.scrollTop;
    
    return { x, y };
  }, []);

  // Handle grid mouse down
  const handleGridMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return; // Only left click
    
    const pos = getPositionFromEvent(e);
    if (!pos) return;

    const snappedBeat = snapToGrid(pos.beat, gridSnap);

    if (currentTool === 'draw') {
      setIsDrawing(true);
      setDrawStart({ beat: snappedBeat, pitch: pos.pitch });
      
      // Expand grid if clicking in last 5%
      if (snappedBeat >= beatsVisible * EXPANSION_THRESHOLD) {
        setExtraBeats(prev => prev + EXPANSION_AMOUNT);
      }
      
      // Play preview note
      playPreviewNote(pos.pitch);
    } else if (currentTool === 'eraser') {
      // Eraser - handled by note clicks
    } else if (currentTool === 'select') {
      // Start rubber band selection
      const rawPos = getRawPositionFromEvent(e);
      if (!rawPos) return;
      
      // If not Ctrl-clicking, clear existing selection
      if (!e.ctrlKey && !e.metaKey) {
        clearSelection();
      }
      
      setIsSelecting(true);
      setSelectionBox({ startX: rawPos.x, startY: rawPos.y, endX: rawPos.x, endY: rawPos.y });
    }
  }, [currentTool, gridSnap, getPositionFromEvent, getRawPositionFromEvent, clearSelection, playPreviewNote, beatsVisible]);

  // Handle grid mouse move
  const handleGridMouseMove = useCallback((e: React.MouseEvent) => {
    // Handle drawing preview
    if (isDrawing && drawStart) {
      // Preview handled by CSS
      return;
    }
    
    // Handle selection box
    if (isSelecting && selectionBox) {
      const rawPos = getRawPositionFromEvent(e);
      if (!rawPos) return;
      
      setSelectionBox(prev => prev ? { ...prev, endX: rawPos.x, endY: rawPos.y } : null);
    }
  }, [isDrawing, drawStart, isSelecting, selectionBox, getRawPositionFromEvent]);

  // Handle grid mouse up
  const handleGridMouseUp = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return; // Only left click
    
    // Handle selection box completion
    if (isSelecting && selectionBox) {
      // Calculate the selection bounds in beats/pitch
      const minX = Math.min(selectionBox.startX, selectionBox.endX);
      const maxX = Math.max(selectionBox.startX, selectionBox.endX);
      const minY = Math.min(selectionBox.startY, selectionBox.endY);
      const maxY = Math.max(selectionBox.startY, selectionBox.endY);
      
      const minBeat = minX / pixelsPerBeat;
      const maxBeat = maxX / pixelsPerBeat;
      const maxPitch = MAX_MIDI_NOTE - Math.floor(minY / noteHeight);
      const minPitch = MAX_MIDI_NOTE - Math.floor(maxY / noteHeight);
      
      // Find all notes that intersect with the selection box
      const selectedNoteIds: string[] = [];
      track.notes.forEach(note => {
        const noteStart = note.start;
        const noteEnd = note.start + note.duration;
        const notePitch = note.pitch;
        
        // Check if note intersects with selection box
        if (noteEnd > minBeat && noteStart < maxBeat && notePitch >= minPitch && notePitch <= maxPitch) {
          selectedNoteIds.push(note.id);
        }
      });
      
      // If Ctrl/Meta is held, add to existing selection; otherwise replace
      if (e.ctrlKey || e.metaKey) {
        selectedNoteIds.forEach(id => addToSelection(id, track.id));
      } else if (selectedNoteIds.length > 0) {
        setSelection({ noteIds: selectedNoteIds, trackId: track.id });
      }
      
      setIsSelecting(false);
      setSelectionBox(null);
      return;
    }
    
    // Handle drawing completion
    if (!isDrawing || !drawStart) return;

    const pos = getPositionFromEvent(e);
    if (!pos) {
      setIsDrawing(false);
      setDrawStart(null);
      return;
    }

    const snappedEnd = snapToGrid(pos.beat, gridSnap);
    const startBeat = Math.min(drawStart.beat, snappedEnd);
    const endBeat = Math.max(drawStart.beat, snappedEnd);
    const duration = Math.max(snapBeats, endBeat - startBeat + snapBeats);

    saveToHistory();
    addNote(track.id, {
      pitch: drawStart.pitch,
      start: startBeat,
      duration,
      velocity: 0.8,
    });

    setIsDrawing(false);
    setDrawStart(null);
  }, [isDrawing, drawStart, isSelecting, selectionBox, gridSnap, snapBeats, pixelsPerBeat, noteHeight, getPositionFromEvent, addNote, addToSelection, setSelection, track, saveToHistory]);

  // Keyboard shortcuts for notes (Delete, Duplicate, Undo, Redo)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Skip if typing in an input
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      
      // Undo (Ctrl+Z)
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !e.shiftKey) {
        e.preventDefault();
        undo();
        return;
      }
      
      // Redo (Ctrl+Y or Ctrl+Shift+Z)
      if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === 'y' || (e.key.toLowerCase() === 'z' && e.shiftKey))) {
        e.preventDefault();
        redo();
        return;
      }
      
      // Delete selected notes
      if (e.key === 'Delete' || e.key === 'Backspace') {
        if (selection.noteIds.length > 0 && selection.trackId === track.id) {
          e.preventDefault();
          saveToHistory();
          removeNotes(track.id, selection.noteIds);
          clearSelection();
        }
      }
      
      // Duplicate selected notes (Ctrl+D)
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'd') {
        if (selection.noteIds.length > 0 && selection.trackId === track.id) {
          e.preventDefault();
          saveToHistory();
          const newNoteIds = duplicateNotes(track.id, selection.noteIds);
          // Select the new notes so they're ready to be dragged
          setSelection({ noteIds: newNoteIds, trackId: track.id });
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selection, track.id, removeNotes, duplicateNotes, clearSelection, setSelection, saveToHistory, undo, redo]);

  // Reset extra beats when notes are added (the grid will recalculate based on actual notes)
  useEffect(() => {
    setExtraBeats(0);
  }, [track.notes.length]);

  // Scroll to middle octave on mount
  useEffect(() => {
    if (containerRef.current) {
      const middleNote = 60; // C4
      const scrollY = (MAX_MIDI_NOTE - middleNote) * noteHeight - containerRef.current.clientHeight / 2;
      containerRef.current.scrollTop = Math.max(0, scrollY);
    }
  }, [noteHeight]);

  // Auto-scroll during playback
  useEffect(() => {
    if (isPlaying && containerRef.current) {
      const playheadX = currentBeat * pixelsPerBeat;
      const containerWidth = containerRef.current.clientWidth - 64; // Account for piano keys
      const scrollX = containerRef.current.scrollLeft;
      
      if (playheadX > scrollX + containerWidth - 100 || playheadX < scrollX) {
        containerRef.current.scrollLeft = playheadX - 100;
      }
    }
  }, [currentBeat, isPlaying, pixelsPerBeat]);

  // Render grid lines
  const renderGridLines = () => {
    const lines = [];
    
    // Vertical lines (beats/bars)
    for (let beat = 0; beat <= beatsVisible; beat += snapBeats) {
      const x = beat * pixelsPerBeat;
      const isBar = beat % beatsPerBar === 0;
      const isBeat = beat % 1 === 0;
      
      lines.push(
        <line
          key={`v-${beat}`}
          x1={x}
          y1={0}
          x2={x}
          y2={gridHeight}
          stroke={isBar ? 'var(--color-grid-bar)' : 'var(--color-grid-line)'}
          strokeWidth={isBar ? 2 : 1}
          opacity={isBeat ? 0.8 : 0.4}
        />
      );
    }
    
    // Horizontal lines (notes)
    for (let i = 0; i <= TOTAL_NOTES; i++) {
      const y = i * noteHeight;
      const midi = MAX_MIDI_NOTE - i;
      const isC = midi % 12 === 0;
      
      lines.push(
        <line
          key={`h-${i}`}
          x1={0}
          y1={y}
          x2={gridWidth}
          y2={y}
          stroke={isC ? 'var(--color-border-light)' : 'var(--color-grid-line)'}
          strokeWidth={isC ? 1.5 : 1}
        />
      );
    }
    
    return lines;
  };

  // Render row backgrounds
  const renderRowBackgrounds = () => {
    const rows = [];
    for (let i = 0; i < TOTAL_NOTES; i++) {
      const midi = MAX_MIDI_NOTE - i;
      const isBlack = isBlackKey(midi);
      rows.push(
        <rect
          key={`row-${i}`}
          x={0}
          y={i * noteHeight}
          width={gridWidth}
          height={noteHeight}
          fill={isBlack ? '#12121a' : '#1a1a24'}
        />
      );
    }
    return rows;
  };

  return (
    <div className="flex h-full overflow-hidden bg-[var(--color-bg-primary)]">
      {/* Piano Keys - Fixed left column */}
      <div 
        className="flex-shrink-0 overflow-hidden"
        style={{ height: gridHeight }}
      >
        <div 
          ref={containerRef}
          className="h-full overflow-y-auto overflow-x-hidden"
          onScroll={(e) => {
            // Sync scroll with grid
            if (gridRef.current) {
              gridRef.current.scrollTop = (e.target as HTMLDivElement).scrollTop;
            }
          }}
        >
          <PianoKeys noteHeight={noteHeight} />
        </div>
      </div>

      {/* Grid Area */}
      <div
        ref={gridRef}
        className="flex-1 overflow-auto relative no-select"
        onMouseDown={handleGridMouseDown}
        onMouseMove={handleGridMouseMove}
        onMouseUp={handleGridMouseUp}
        onMouseLeave={() => {
          if (isDrawing) {
            setIsDrawing(false);
            setDrawStart(null);
          }
          if (isSelecting) {
            setIsSelecting(false);
            setSelectionBox(null);
          }
        }}
        onScroll={(e) => {
          // Sync scroll with piano keys
          if (containerRef.current) {
            containerRef.current.scrollTop = (e.target as HTMLDivElement).scrollTop;
          }
        }}
      >
        <div
          className="relative"
          style={{ width: gridWidth, height: gridHeight, minWidth: gridWidth }}
        >
          {/* SVG Grid */}
          <svg
            className="absolute inset-0 pointer-events-none"
            width={gridWidth}
            height={gridHeight}
          >
            {renderRowBackgrounds()}
            {renderGridLines()}
          </svg>

          {/* Loop Region */}
          {loop.enabled && (
            <div
              className="absolute top-0 bottom-0 bg-[var(--color-accent)] opacity-10 pointer-events-none"
              style={{
                left: loop.start * pixelsPerBeat,
                width: (loop.end - loop.start) * pixelsPerBeat,
              }}
            />
          )}

          {/* Notes */}
          {track.notes.map((note) => (
            <NoteBlock
              key={note.id}
              note={note}
              trackId={track.id}
              trackColor={track.color}
              track={track}
              pixelsPerBeat={pixelsPerBeat}
              noteHeight={noteHeight}
              minMidi={MIN_MIDI_NOTE}
              maxMidi={MAX_MIDI_NOTE}
            />
          ))}

          {/* Drawing Preview */}
          {isDrawing && drawStart && (
            <div
              className="absolute rounded-md pointer-events-none opacity-60"
              style={{
                left: drawStart.beat * pixelsPerBeat,
                top: (MAX_MIDI_NOTE - drawStart.pitch) * noteHeight,
                width: snapBeats * pixelsPerBeat,
                height: noteHeight - 2,
                backgroundColor: track.color,
                boxShadow: `0 0 8px ${track.color}80`,
              }}
            />
          )}

          {/* Selection Box (Rubber Band) */}
          {isSelecting && selectionBox && (
            <div
              className="absolute pointer-events-none border-2 border-white/60 bg-white/10 rounded z-30"
              style={{
                left: Math.min(selectionBox.startX, selectionBox.endX),
                top: Math.min(selectionBox.startY, selectionBox.endY),
                width: Math.abs(selectionBox.endX - selectionBox.startX),
                height: Math.abs(selectionBox.endY - selectionBox.startY),
              }}
            />
          )}

          {/* Playhead */}
          <div
            className="absolute top-0 bottom-0 w-0.5 bg-white shadow-lg pointer-events-none z-20"
            style={{
              left: currentBeat * pixelsPerBeat,
              boxShadow: '0 0 10px rgba(255, 255, 255, 0.5)',
            }}
          />
        </div>
      </div>
    </div>
  );
}

