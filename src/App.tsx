import React, { useEffect, useCallback } from 'react';
import Toolbar from './components/Toolbar.tsx';
import Transport from './components/Transport.tsx';
import TrackList from './components/TrackList.tsx';
import PianoRoll from './components/PianoRoll.tsx';
import { useProjectStore } from './stores/projectStore.ts';
import { useAudioEngine } from './hooks/useAudioEngine.ts';

export default function App() {
  const selectedTrackId = useProjectStore((state) => state.selectedTrackId);
  const tracks = useProjectStore((state) => state.tracks);
  const selectedTrack = tracks.find((t) => t.id === selectedTrackId);
  
  const { initAudio, exportWav } = useAudioEngine();

  // Initialize audio on first click
  const handleFirstInteraction = useCallback(async () => {
    await initAudio();
    document.removeEventListener('click', handleFirstInteraction);
  }, [initAudio]);

  useEffect(() => {
    document.addEventListener('click', handleFirstInteraction);
    return () => document.removeEventListener('click', handleFirstInteraction);
  }, [handleFirstInteraction]);

  // Wire up WAV export button
  useEffect(() => {
    const btn = document.getElementById('export-wav-btn');
    if (btn) {
      const handleClick = async () => {
        await exportWav();
      };
      btn.addEventListener('click', handleClick);
      return () => btn.removeEventListener('click', handleClick);
    }
  }, [exportWav]);

  return (
    <div className="h-full flex flex-col bg-[var(--color-bg-primary)]">
      {/* Header */}
      <header className="flex-shrink-0 border-b border-[var(--color-border)]">
        <Toolbar />
        <Transport />
      </header>

      {/* Main Content */}
      <main className="flex-1 flex overflow-hidden">
        {/* Track List - Left Panel */}
        <aside className="w-72 flex-shrink-0 border-r border-[var(--color-border)] overflow-y-auto bg-[var(--color-bg-secondary)]">
          <TrackList />
        </aside>

        {/* Piano Roll - Main Area */}
        <section className="flex-1 overflow-hidden">
          {selectedTrack ? (
            <PianoRoll track={selectedTrack} />
          ) : (
            <div className="h-full flex items-center justify-center text-[var(--color-text-muted)]">
              <div className="text-center">
                <p className="text-lg mb-2">No track selected</p>
                <p className="text-sm">Select a track or create a new one to start composing</p>
              </div>
            </div>
          )}
        </section>
      </main>
    </div>
  );
}

