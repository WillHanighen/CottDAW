import { create } from 'zustand';
import { v4 as uuidv4 } from 'uuid';
import type { Track, Note, Project, WaveType, Envelope, TrackEffects } from '../types/index.ts';
import { getNextTrackColor, resetColorIndex, setColorIndex } from '../utils/colorPalette.ts';
import { useHistoryStore } from './historyStore.ts';

const STORAGE_KEY = 'cottdaw-project';

// Load initial state from localStorage
function loadFromStorage(): Partial<{
  projectName: string;
  bpm: number;
  timeSignature: [number, number];
  tracks: Track[];
  selectedTrackId: string | null;
}> {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved);
      // Restore color index based on saved tracks
      if (parsed.tracks) {
        setColorIndex(parsed.tracks.length);
      }
      return parsed;
    }
  } catch (e) {
    console.warn('Failed to load project from localStorage:', e);
  }
  return {};
}

// Save state to localStorage
function saveToStorage(state: {
  projectName: string;
  bpm: number;
  timeSignature: [number, number];
  tracks: Track[];
  selectedTrackId: string | null;
}) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      projectName: state.projectName,
      bpm: state.bpm,
      timeSignature: state.timeSignature,
      tracks: state.tracks,
      selectedTrackId: state.selectedTrackId,
    }));
  } catch (e) {
    console.warn('Failed to save project to localStorage:', e);
  }
}

const DEFAULT_ENVELOPE: Envelope = {
  attack: 0.01,
  decay: 0.1,
  sustain: 0.7,
  release: 0.3,
};

const DEFAULT_EFFECTS: TrackEffects = {
  reverb: { wet: 0 },
  delay: { time: '8n', feedback: 0.3, wet: 0 },
  filter: { type: 'lowpass', frequency: 20000, Q: 1 },
  distortion: { amount: 0 },
};

function createDefaultTrack(name: string): Track {
  return {
    id: uuidv4(),
    name,
    color: getNextTrackColor(),
    waveType: 'sine',
    volume: 0.7,
    pan: 0,
    muted: false,
    solo: false,
    envelope: { ...DEFAULT_ENVELOPE },
    effects: JSON.parse(JSON.stringify(DEFAULT_EFFECTS)),
    notes: [],
  };
}

interface ProjectState {
  // Project metadata
  projectName: string;
  bpm: number;
  timeSignature: [number, number];
  
  // Tracks
  tracks: Track[];
  selectedTrackId: string | null;
  
  // Actions - Project
  setProjectName: (name: string) => void;
  setBpm: (bpm: number) => void;
  setTimeSignature: (sig: [number, number]) => void;
  
  // Actions - Tracks
  addTrack: (name?: string) => void;
  removeTrack: (id: string) => void;
  selectTrack: (id: string) => void;
  updateTrack: (id: string, updates: Partial<Track>) => void;
  setTrackWaveType: (id: string, waveType: WaveType) => void;
  setTrackVolume: (id: string, volume: number) => void;
  setTrackPan: (id: string, pan: number) => void;
  toggleTrackMute: (id: string) => void;
  toggleTrackSolo: (id: string) => void;
  setTrackEnvelope: (id: string, envelope: Partial<Envelope>) => void;
  setTrackEffect: <K extends keyof TrackEffects>(id: string, effectName: K, value: Partial<TrackEffects[K]>) => void;
  
  // Actions - Notes
  addNote: (trackId: string, note: Omit<Note, 'id'>) => void;
  removeNote: (trackId: string, noteId: string) => void;
  updateNote: (trackId: string, noteId: string, updates: Partial<Note>) => void;
  updateNotes: (trackId: string, noteUpdates: { noteId: string; updates: Partial<Note> }[]) => void;
  removeNotes: (trackId: string, noteIds: string[]) => void;
  duplicateNotes: (trackId: string, noteIds: string[]) => string[];
  
  // Actions - History
  saveToHistory: () => void;
  undo: () => void;
  redo: () => void;
  
  // Actions - Import/Export
  exportProject: () => Project;
  importProject: (project: Project) => void;
  resetProject: () => void;
}

const initialState = loadFromStorage();

export const useProjectStore = create<ProjectState>((set, get) => ({
  projectName: initialState.projectName ?? 'Untitled Project',
  bpm: initialState.bpm ?? 120,
  timeSignature: initialState.timeSignature ?? [4, 4],
  tracks: initialState.tracks ?? [],
  selectedTrackId: initialState.selectedTrackId ?? null,

  setProjectName: (name) => set({ projectName: name }),
  
  setBpm: (bpm) => set({ bpm: Math.max(40, Math.min(240, bpm)) }),
  
  setTimeSignature: (sig) => set({ timeSignature: sig }),

  addTrack: (name) => {
    const trackCount = get().tracks.length;
    const newTrack = createDefaultTrack(name || `Track ${trackCount + 1}`);
    set((state) => ({
      tracks: [...state.tracks, newTrack],
      selectedTrackId: newTrack.id,
    }));
  },

  removeTrack: (id) => {
    set((state) => {
      const newTracks = state.tracks.filter((t) => t.id !== id);
      const newSelectedId = state.selectedTrackId === id
        ? (newTracks.length > 0 ? newTracks[0].id : null)
        : state.selectedTrackId;
      return { tracks: newTracks, selectedTrackId: newSelectedId };
    });
  },

  selectTrack: (id) => set({ selectedTrackId: id }),

  updateTrack: (id, updates) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === id ? { ...t, ...updates } : t
      ),
    }));
  },

  setTrackWaveType: (id, waveType) => {
    get().updateTrack(id, { waveType });
  },

  setTrackVolume: (id, volume) => {
    get().updateTrack(id, { volume: Math.max(0, Math.min(1, volume)) });
  },

  setTrackPan: (id, pan) => {
    get().updateTrack(id, { pan: Math.max(-1, Math.min(1, pan)) });
  },

  toggleTrackMute: (id) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === id ? { ...t, muted: !t.muted } : t
      ),
    }));
  },

  toggleTrackSolo: (id) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === id ? { ...t, solo: !t.solo } : t
      ),
    }));
  },

  setTrackEnvelope: (id, envelope) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === id ? { ...t, envelope: { ...t.envelope, ...envelope } } : t
      ),
    }));
  },

  setTrackEffect: (id, effectName, value) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === id
          ? {
              ...t,
              effects: {
                ...t.effects,
                [effectName]: { ...t.effects[effectName], ...value },
              },
            }
          : t
      ),
    }));
  },

  addNote: (trackId, note) => {
    const newNote: Note = { ...note, id: uuidv4() };
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === trackId ? { ...t, notes: [...t.notes, newNote] } : t
      ),
    }));
  },

  removeNote: (trackId, noteId) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === trackId
          ? { ...t, notes: t.notes.filter((n) => n.id !== noteId) }
          : t
      ),
    }));
  },

  updateNote: (trackId, noteId, updates) => {
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === trackId
          ? {
              ...t,
              notes: t.notes.map((n) =>
                n.id === noteId ? { ...n, ...updates } : n
              ),
            }
          : t
      ),
    }));
  },

  updateNotes: (trackId, noteUpdates) => {
    const updateMap = new Map(noteUpdates.map(u => [u.noteId, u.updates]));
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === trackId
          ? {
              ...t,
              notes: t.notes.map((n) => {
                const updates = updateMap.get(n.id);
                return updates ? { ...n, ...updates } : n;
              }),
            }
          : t
      ),
    }));
  },

  removeNotes: (trackId, noteIds) => {
    const noteIdSet = new Set(noteIds);
    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === trackId
          ? { ...t, notes: t.notes.filter((n) => !noteIdSet.has(n.id)) }
          : t
      ),
    }));
  },

  duplicateNotes: (trackId, noteIds) => {
    const state = get();
    const track = state.tracks.find(t => t.id === trackId);
    if (!track) return [];

    const noteIdSet = new Set(noteIds);
    const notesToDuplicate = track.notes.filter(n => noteIdSet.has(n.id));
    
    // Create new notes with new IDs (same position - user will drag them)
    const newNotes: Note[] = notesToDuplicate.map(n => ({
      ...n,
      id: uuidv4(),
    }));

    set((state) => ({
      tracks: state.tracks.map((t) =>
        t.id === trackId
          ? { ...t, notes: [...t.notes, ...newNotes] }
          : t
      ),
    }));

    return newNotes.map(n => n.id);
  },

  // History actions
  saveToHistory: () => {
    const state = get();
    useHistoryStore.getState().pushState(state.tracks);
  },

  undo: () => {
    const state = get();
    const historyState = useHistoryStore.getState();
    
    if (!historyState.canUndo()) return;
    
    // Create snapshot of current state to save to future
    const currentSnapshot: Map<string, Note[]> = new Map();
    state.tracks.forEach(track => {
      currentSnapshot.set(track.id, JSON.parse(JSON.stringify(track.notes)));
    });
    
    // Get the previous state (pass current state so it can be saved to future)
    const previousSnapshot = historyState.undo(currentSnapshot);
    if (!previousSnapshot) return;
    
    // Apply the previous state
    set((state) => ({
      tracks: state.tracks.map(track => {
        const previousNotes = previousSnapshot.get(track.id);
        if (previousNotes !== undefined) {
          return { ...track, notes: previousNotes };
        }
        return track;
      }),
    }));
  },

  redo: () => {
    const state = get();
    const historyState = useHistoryStore.getState();
    
    if (!historyState.canRedo()) return;
    
    // Create snapshot of current state to save to past
    const currentSnapshot: Map<string, Note[]> = new Map();
    state.tracks.forEach(track => {
      currentSnapshot.set(track.id, JSON.parse(JSON.stringify(track.notes)));
    });
    
    // Get the next state (pass current state so it can be saved to past)
    const nextSnapshot = historyState.redo(currentSnapshot);
    if (!nextSnapshot) return;
    
    // Apply the next state
    set((state) => ({
      tracks: state.tracks.map(track => {
        const nextNotes = nextSnapshot.get(track.id);
        if (nextNotes !== undefined) {
          return { ...track, notes: nextNotes };
        }
        return track;
      }),
    }));
  },

  exportProject: () => {
    const state = get();
    return {
      name: state.projectName,
      bpm: state.bpm,
      timeSignature: state.timeSignature,
      tracks: state.tracks,
    };
  },

  importProject: (project) => {
    resetColorIndex();
    setColorIndex(project.tracks.length);
    set({
      projectName: project.name,
      bpm: project.bpm,
      timeSignature: project.timeSignature,
      tracks: project.tracks,
      selectedTrackId: project.tracks.length > 0 ? project.tracks[0].id : null,
    });
  },

  resetProject: () => {
    resetColorIndex();
    set({
      projectName: 'Untitled Project',
      bpm: 120,
      timeSignature: [4, 4],
      tracks: [],
      selectedTrackId: null,
    });
  },
}));

// Subscribe to state changes and save to localStorage
useProjectStore.subscribe((state) => {
  saveToStorage({
    projectName: state.projectName,
    bpm: state.bpm,
    timeSignature: state.timeSignature,
    tracks: state.tracks,
    selectedTrackId: state.selectedTrackId,
  });
});

