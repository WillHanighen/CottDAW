import { create } from 'zustand';
import type { LoopRegion } from '../types/index.ts';

const STORAGE_KEY = 'cottdaw-transport';

// Load initial state from localStorage
function loadFromStorage(): Partial<{
  loop: LoopRegion;
  metronomeEnabled: boolean;
  metronomeVolume: number;
}> {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      return JSON.parse(saved);
    }
  } catch (e) {
    console.warn('Failed to load transport from localStorage:', e);
  }
  return {};
}

// Save state to localStorage
function saveToStorage(state: {
  loop: LoopRegion;
  metronomeEnabled: boolean;
  metronomeVolume: number;
}) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch (e) {
    console.warn('Failed to save transport to localStorage:', e);
  }
}

const initialState = loadFromStorage();

// Callback to sync with audio engine (set by useAudioEngine)
let seekCallback: ((beat: number) => void) | null = null;

export function setSeekCallback(callback: ((beat: number) => void) | null) {
  seekCallback = callback;
}

interface TransportState {
  // Playback state
  isPlaying: boolean;
  isPaused: boolean;
  currentBeat: number;
  
  // Loop
  loop: LoopRegion;
  
  // Metronome
  metronomeEnabled: boolean;
  metronomeVolume: number;
  
  // Actions
  play: () => void;
  pause: () => void;
  stop: () => void;
  togglePlay: () => void;
  setCurrentBeat: (beat: number) => void;
  seek: (beat: number) => void;
  
  // Loop actions
  setLoopEnabled: (enabled: boolean) => void;
  setLoopRegion: (start: number, end: number) => void;
  toggleLoop: () => void;
  
  // Metronome actions
  toggleMetronome: () => void;
  setMetronomeVolume: (volume: number) => void;
}

export const useTransportStore = create<TransportState>((set, get) => ({
  isPlaying: false,
  isPaused: false,
  currentBeat: 0,
  
  loop: initialState.loop ?? {
    start: 0,
    end: 16,
    enabled: false,
  },
  
  metronomeEnabled: initialState.metronomeEnabled ?? false,
  metronomeVolume: initialState.metronomeVolume ?? 0.5,

  play: () => {
    set({ isPlaying: true, isPaused: false });
  },

  pause: () => {
    set({ isPlaying: false, isPaused: true });
  },

  stop: () => {
    set({ isPlaying: false, isPaused: false, currentBeat: 0 });
  },

  togglePlay: () => {
    const { isPlaying, isPaused } = get();
    if (isPlaying) {
      get().pause();
    } else {
      get().play();
    }
  },

  setCurrentBeat: (beat) => {
    const { loop } = get();
    if (loop.enabled && beat >= loop.end) {
      set({ currentBeat: loop.start });
    } else {
      set({ currentBeat: beat });
    }
  },

  seek: (beat) => {
    const clampedBeat = Math.max(0, beat);
    set({ currentBeat: clampedBeat });
    // Sync with audio engine
    if (seekCallback) {
      seekCallback(clampedBeat);
    }
  },

  setLoopEnabled: (enabled) => {
    set((state) => ({
      loop: { ...state.loop, enabled },
    }));
  },

  setLoopRegion: (start, end) => {
    set((state) => ({
      loop: { ...state.loop, start: Math.min(start, end), end: Math.max(start, end) },
    }));
  },

  toggleLoop: () => {
    set((state) => ({
      loop: { ...state.loop, enabled: !state.loop.enabled },
    }));
  },

  toggleMetronome: () => {
    set((state) => ({ metronomeEnabled: !state.metronomeEnabled }));
  },

  setMetronomeVolume: (volume) => {
    set({ metronomeVolume: Math.max(0, Math.min(1, volume)) });
  },
}));

// Subscribe to state changes and save to localStorage
useTransportStore.subscribe((state) => {
  saveToStorage({
    loop: state.loop,
    metronomeEnabled: state.metronomeEnabled,
    metronomeVolume: state.metronomeVolume,
  });
});

