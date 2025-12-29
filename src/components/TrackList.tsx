import React from 'react';
import { Plus } from 'lucide-react';
import Button from './ui/Button.tsx';
import TrackPanel from './TrackPanel.tsx';
import { useProjectStore } from '../stores/projectStore.ts';

export default function TrackList() {
  const tracks = useProjectStore((s) => s.tracks);
  const addTrack = useProjectStore((s) => s.addTrack);

  return (
    <div className="flex flex-col h-full">
      {/* Track List Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--color-border)] bg-[var(--color-bg-tertiary)]">
        <span className="text-sm font-medium text-[var(--color-text-secondary)]">
          Tracks ({tracks.length})
        </span>
        <Button
          variant="primary"
          size="sm"
          onClick={() => addTrack()}
          title="Add new track"
        >
          <Plus size={14} />
          <span>Add</span>
        </Button>
      </div>

      {/* Track Panels */}
      <div className="flex-1 overflow-y-auto">
        {tracks.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full p-4 text-center">
            <p className="text-[var(--color-text-muted)] mb-3">No tracks yet</p>
            <Button variant="default" size="sm" onClick={() => addTrack()}>
              <Plus size={14} />
              Create your first track
            </Button>
          </div>
        ) : (
          <div className="divide-y divide-[var(--color-border)]">
            {tracks.map((track) => (
              <TrackPanel key={track.id} track={track} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

