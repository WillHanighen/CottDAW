import { create } from 'zustand';
import type { Tool, GridSnap, Selection } from '../types/index.ts';

const STORAGE_KEY = 'cottdaw-ui';

// Load initial state from localStorage
function loadFromStorage(): Partial<{
  currentTool: Tool;
  gridSnap: GridSnap;
  horizontalZoom: number;
  verticalZoom: number;
}> {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      return JSON.parse(saved);
    }
  } catch (e) {
    console.warn('Failed to load UI settings from localStorage:', e);
  }
  return {};
}

// Save state to localStorage
function saveToStorage(state: {
  currentTool: Tool;
  gridSnap: GridSnap;
  horizontalZoom: number;
  verticalZoom: number;
}) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch (e) {
    console.warn('Failed to save UI settings to localStorage:', e);
  }
}

const initialState = loadFromStorage();

interface UIState {
  // Tool selection
  currentTool: Tool;
  
  // Grid
  gridSnap: GridSnap;
  
  // Zoom levels (1 = 100%)
  horizontalZoom: number;
  verticalZoom: number;
  
  // Selection
  selection: Selection;
  
  // Scroll position (for syncing between components)
  scrollX: number;
  scrollY: number;
  
  // Actions
  setTool: (tool: Tool) => void;
  setGridSnap: (snap: GridSnap) => void;
  setHorizontalZoom: (zoom: number) => void;
  setVerticalZoom: (zoom: number) => void;
  zoomIn: () => void;
  zoomOut: () => void;
  
  // Selection actions
  setSelection: (selection: Selection) => void;
  clearSelection: () => void;
  addToSelection: (noteId: string, trackId: string) => void;
  removeFromSelection: (noteId: string) => void;
  
  // Scroll actions
  setScrollX: (x: number) => void;
  setScrollY: (y: number) => void;
}

const MIN_ZOOM = 0.25;
const MAX_ZOOM = 4;

export const useUIStore = create<UIState>((set, get) => ({
  currentTool: initialState.currentTool ?? 'draw',
  gridSnap: initialState.gridSnap ?? '1/8',
  horizontalZoom: initialState.horizontalZoom ?? 1,
  verticalZoom: initialState.verticalZoom ?? 1,
  selection: {
    noteIds: [],
    trackId: null,
  },
  scrollX: 0,
  scrollY: 0,

  setTool: (tool) => set({ currentTool: tool }),

  setGridSnap: (snap) => set({ gridSnap: snap }),

  setHorizontalZoom: (zoom) => {
    set({ horizontalZoom: Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom)) });
  },

  setVerticalZoom: (zoom) => {
    set({ verticalZoom: Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, zoom)) });
  },

  zoomIn: () => {
    const current = get().horizontalZoom;
    get().setHorizontalZoom(current * 1.25);
  },

  zoomOut: () => {
    const current = get().horizontalZoom;
    get().setHorizontalZoom(current / 1.25);
  },

  setSelection: (selection) => set({ selection }),

  clearSelection: () => set({ selection: { noteIds: [], trackId: null } }),

  addToSelection: (noteId, trackId) => {
    set((state) => {
      // If selecting from a different track, clear previous selection
      if (state.selection.trackId && state.selection.trackId !== trackId) {
        return {
          selection: {
            noteIds: [noteId],
            trackId,
          },
        };
      }
      // Add to existing selection
      if (!state.selection.noteIds.includes(noteId)) {
        return {
          selection: {
            noteIds: [...state.selection.noteIds, noteId],
            trackId,
          },
        };
      }
      return state;
    });
  },

  removeFromSelection: (noteId) => {
    set((state) => ({
      selection: {
        ...state.selection,
        noteIds: state.selection.noteIds.filter((id) => id !== noteId),
      },
    }));
  },

  setScrollX: (x) => set({ scrollX: x }),
  setScrollY: (y) => set({ scrollY: y }),
}));

// Subscribe to state changes and save to localStorage
useUIStore.subscribe((state) => {
  saveToStorage({
    currentTool: state.currentTool,
    gridSnap: state.gridSnap,
    horizontalZoom: state.horizontalZoom,
    verticalZoom: state.verticalZoom,
  });
});

