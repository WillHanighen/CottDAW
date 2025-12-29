import React, { useRef, useState, useCallback } from 'react';
import type { Note } from '../types/index.ts';
import { useUIStore } from '../stores/uiStore.ts';
import { useProjectStore } from '../stores/projectStore.ts';
import { snapToGrid, gridSnapToBeats } from '../utils/timeHelpers.ts';
import { midiToFrequency } from '../utils/noteHelpers.ts';
import { triggerAttack, triggerRelease, getTrackSynth, createTrackSynth } from '../audio/synth.ts';
import * as Tone from 'tone';
import type { Track } from '../types/index.ts';

interface NoteBlockProps {
  note: Note;
  trackId: string;
  trackColor: string;
  track: Track;
  pixelsPerBeat: number;
  noteHeight: number;
  minMidi: number;
  maxMidi: number;
}

export default function NoteBlock({
  note,
  trackId,
  trackColor,
  track,
  pixelsPerBeat,
  noteHeight,
  minMidi,
  maxMidi,
}: NoteBlockProps) {
  const noteRef = useRef<HTMLDivElement>(null);
  const [isResizing, setIsResizing] = useState(false);
  const [isDragging, setIsDragging] = useState(false);

  const currentTool = useUIStore((s) => s.currentTool);
  const gridSnap = useUIStore((s) => s.gridSnap);
  const selection = useUIStore((s) => s.selection);
  const addToSelection = useUIStore((s) => s.addToSelection);
  const setSelection = useUIStore((s) => s.setSelection);
  const clearSelection = useUIStore((s) => s.clearSelection);

  const updateNote = useProjectStore((s) => s.updateNote);
  const removeNote = useProjectStore((s) => s.removeNote);
  
  const snapBeats = gridSnapToBeats(gridSnap);

  // Play preview note when dragging to a new pitch
  const playPreviewNote = useCallback(async (pitch: number) => {
    if (Tone.getContext().state !== 'running') {
      await Tone.start();
    }
    
    if (!getTrackSynth(trackId)) {
      createTrackSynth(track);
    }
    
    const frequency = midiToFrequency(pitch);
    triggerAttack(trackId, frequency, 0.8);
    
    setTimeout(() => {
      triggerRelease(trackId);
    }, 150);
  }, [trackId, track]);

  const isSelected = selection.noteIds.includes(note.id);

  const left = note.start * pixelsPerBeat;
  const width = note.duration * pixelsPerBeat;
  const top = (maxMidi - note.pitch) * noteHeight;

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();

    // Middle-click to delete
    if (e.button === 1) {
      e.preventDefault();
      removeNote(trackId, note.id);
      return;
    }

    // Only process left-click for other operations
    if (e.button !== 0) return;

    if (currentTool === 'eraser') {
      removeNote(trackId, note.id);
      return;
    }

    // Allow dragging in both select and draw modes
    if (currentTool === 'select' || currentTool === 'draw') {
      // Check if clicking resize handle
      const rect = noteRef.current?.getBoundingClientRect();
      if (rect) {
        const isResizeHandle = e.clientX > rect.right - 10;
        
        if (isResizeHandle) {
          setIsResizing(true);
          const startX = e.clientX;
          const startDuration = note.duration;

          const handleMouseMove = (e: MouseEvent) => {
            const deltaX = e.clientX - startX;
            const deltaDuration = deltaX / pixelsPerBeat;
            // Snap duration to grid
            const rawDuration = startDuration + deltaDuration;
            const snappedDuration = Math.max(snapBeats, Math.round(rawDuration / snapBeats) * snapBeats);
            updateNote(trackId, note.id, { duration: snappedDuration });
          };

          const handleMouseUp = () => {
            setIsResizing(false);
            document.removeEventListener('mousemove', handleMouseMove);
            document.removeEventListener('mouseup', handleMouseUp);
          };

          document.addEventListener('mousemove', handleMouseMove);
          document.addEventListener('mouseup', handleMouseUp);
          return;
        }
      }

      // Select logic (only in select mode)
      if (currentTool === 'select') {
        if (e.ctrlKey || e.metaKey) {
          // Ctrl+click to toggle selection
          if (isSelected) {
            // Remove from selection - use setSelection with filtered noteIds
            const newNoteIds = selection.noteIds.filter(id => id !== note.id);
            if (newNoteIds.length > 0) {
              setSelection({ noteIds: newNoteIds, trackId });
            } else {
              clearSelection();
            }
          } else {
            addToSelection(note.id, trackId);
          }
        } else if (!isSelected) {
          setSelection({ noteIds: [note.id], trackId });
        }
      }

      // Start dragging
      setIsDragging(true);
      const startX = e.clientX;
      const startY = e.clientY;
      const startBeat = note.start;
      const startPitch = note.pitch;
      let lastPitch = startPitch;

      const handleMouseMove = (e: MouseEvent) => {
        const deltaX = e.clientX - startX;
        const deltaY = e.clientY - startY;
        
        const deltaBeat = deltaX / pixelsPerBeat;
        const deltaPitch = -Math.round(deltaY / noteHeight);

        // Snap to grid
        const rawStart = startBeat + deltaBeat;
        const snappedStart = Math.max(0, snapToGrid(rawStart, gridSnap));
        const newPitch = Math.max(minMidi, Math.min(maxMidi, startPitch + deltaPitch));

        // Play preview note if pitch changed
        if (newPitch !== lastPitch) {
          playPreviewNote(newPitch);
          lastPitch = newPitch;
        }

        updateNote(trackId, note.id, { start: snappedStart, pitch: newPitch });
      };

      const handleMouseUp = () => {
        setIsDragging(false);
        document.removeEventListener('mousemove', handleMouseMove);
        document.removeEventListener('mouseup', handleMouseUp);
      };

      document.addEventListener('mousemove', handleMouseMove);
      document.addEventListener('mouseup', handleMouseUp);
    }
  }, [currentTool, note, trackId, isSelected, pixelsPerBeat, noteHeight, minMidi, maxMidi, updateNote, removeNote, addToSelection, setSelection, clearSelection, selection, gridSnap, snapBeats, playPreviewNote]);

  // Handle auxiliary click (middle mouse button)
  const handleAuxClick = useCallback((e: React.MouseEvent) => {
    if (e.button === 1) {
      e.preventDefault();
      e.stopPropagation();
      removeNote(trackId, note.id);
    }
  }, [trackId, note.id, removeNote]);

  // Calculate velocity-based opacity (0.4 to 1)
  const opacity = 0.4 + note.velocity * 0.6;

  return (
    <div
      ref={noteRef}
      className={`
        absolute rounded-md cursor-pointer transition-shadow
        ${isSelected ? 'ring-2 ring-white ring-offset-1 ring-offset-[var(--color-bg-primary)]' : ''}
        ${isDragging || isResizing ? 'z-10' : ''}
      `}
      style={{
        left,
        top,
        width: Math.max(width, 4),
        height: noteHeight - 2,
        backgroundColor: trackColor,
        opacity,
        boxShadow: `0 0 ${isSelected ? '12px' : '6px'} ${trackColor}80`,
      }}
      onMouseDown={handleMouseDown}
      onAuxClick={handleAuxClick}
    >
      {/* Resize handle */}
      <div
        className="absolute right-0 top-0 bottom-0 w-2 cursor-ew-resize hover:bg-white/20 rounded-r"
      />
      
      {/* Velocity indicator */}
      <div
        className="absolute bottom-0 left-0 right-0 bg-black/30 rounded-b"
        style={{ height: `${(1 - note.velocity) * 100}%` }}
      />
    </div>
  );
}

