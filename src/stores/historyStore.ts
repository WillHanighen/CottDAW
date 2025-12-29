import { create } from 'zustand';
import type { Note } from '../types/index.ts';

// Snapshot of all notes across all tracks
type NotesSnapshot = Map<string, Note[]>; // trackId -> notes

interface HistoryState {
  past: NotesSnapshot[];
  future: NotesSnapshot[];
  maxHistory: number;
  
  // Actions
  pushState: (tracks: { id: string; notes: Note[] }[]) => void;
  undo: (currentState?: NotesSnapshot) => NotesSnapshot | null;
  redo: (currentState?: NotesSnapshot) => NotesSnapshot | null;
  canUndo: () => boolean;
  canRedo: () => boolean;
  clear: () => void;
}

const MAX_HISTORY = 50;

export const useHistoryStore = create<HistoryState>((set, get) => ({
  past: [],
  future: [],
  maxHistory: MAX_HISTORY,

  pushState: (tracks) => {
    const snapshot: NotesSnapshot = new Map();
    tracks.forEach(track => {
      // Deep clone the notes array
      snapshot.set(track.id, JSON.parse(JSON.stringify(track.notes)));
    });

    set((state) => {
      const newPast = [...state.past, snapshot];
      // Limit history size
      if (newPast.length > state.maxHistory) {
        newPast.shift();
      }
      return {
        past: newPast,
        future: [], // Clear future when new action is performed
      };
    });
  },

  undo: (currentState?: NotesSnapshot) => {
    const state = get();
    if (state.past.length === 0) return null;

    const newPast = [...state.past];
    const previousState = newPast.pop()!;
    
    // If current state is provided, save it to future for redo
    const newFuture = currentState 
      ? [currentState, ...state.future]
      : state.future;

    set({ past: newPast, future: newFuture });
    
    // Return the state we're restoring to
    return previousState;
  },

  redo: (currentState?: NotesSnapshot) => {
    const state = get();
    if (state.future.length === 0) return null;

    const newFuture = [...state.future];
    const nextState = newFuture.shift()!;
    
    // If current state is provided, save it to past for undo
    const newPast = currentState 
      ? [...state.past, currentState]
      : state.past;

    set({ future: newFuture, past: newPast });
    return nextState;
  },

  canUndo: () => get().past.length > 0,
  canRedo: () => get().future.length > 0,

  clear: () => set({ past: [], future: [] }),
}));


