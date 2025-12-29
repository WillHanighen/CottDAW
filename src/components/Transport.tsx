import React, { useEffect, useRef, useState, useCallback } from 'react';
import { Play, Pause, Square, SkipBack, SkipForward, RefreshCw, Volume2 } from 'lucide-react';
import Button from './ui/Button.tsx';
import Fader from './ui/Fader.tsx';
import Select from './ui/Select.tsx';
import { useTransportStore } from '../stores/transportStore.ts';
import { useProjectStore } from '../stores/projectStore.ts';
import { formatTime } from '../utils/timeHelpers.ts';

const TIME_SIGNATURES: { value: string; label: string }[] = [
  { value: '4,4', label: '4/4' },
  { value: '3,4', label: '3/4' },
  { value: '6,8', label: '6/8' },
  { value: '2,4', label: '2/4' },
  { value: '5,4', label: '5/4' },
];

export default function Transport() {
  const isPlaying = useTransportStore((s) => s.isPlaying);
  const currentBeat = useTransportStore((s) => s.currentBeat);
  const loop = useTransportStore((s) => s.loop);
  const metronomeEnabled = useTransportStore((s) => s.metronomeEnabled);
  const metronomeVolume = useTransportStore((s) => s.metronomeVolume);
  const play = useTransportStore((s) => s.play);
  const pause = useTransportStore((s) => s.pause);
  const stop = useTransportStore((s) => s.stop);
  const seek = useTransportStore((s) => s.seek);
  const toggleLoop = useTransportStore((s) => s.toggleLoop);
  const toggleMetronome = useTransportStore((s) => s.toggleMetronome);
  const setMetronomeVolume = useTransportStore((s) => s.setMetronomeVolume);

  const bpm = useProjectStore((s) => s.bpm);
  const setBpm = useProjectStore((s) => s.setBpm);
  const timeSignature = useProjectStore((s) => s.timeSignature);
  const setTimeSignature = useProjectStore((s) => s.setTimeSignature);

  const seekBarRef = useRef<HTMLDivElement>(null);
  const [isScrubbing, setIsScrubbing] = useState(false);

  // Calculate total project length (in beats) - for now, 64 beats (16 bars in 4/4)
  const projectLength = 64;

  // Calculate beat position from mouse event
  const getBeatFromMouseEvent = useCallback((clientX: number) => {
    if (!seekBarRef.current) return null;
    const rect = seekBarRef.current.getBoundingClientRect();
    const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
    const percentage = x / rect.width;
    return percentage * projectLength;
  }, [projectLength]);

  // Handle seek bar mouse down - start scrubbing
  const handleSeekBarMouseDown = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return; // Only left click
    e.preventDefault();
    
    const beat = getBeatFromMouseEvent(e.clientX);
    if (beat !== null) {
      seek(beat);
      setIsScrubbing(true);
    }
  }, [getBeatFromMouseEvent, seek]);

  // Handle mouse move while scrubbing
  useEffect(() => {
    if (!isScrubbing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const beat = getBeatFromMouseEvent(e.clientX);
      if (beat !== null) {
        seek(beat);
      }
    };

    const handleMouseUp = () => {
      setIsScrubbing(false);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isScrubbing, getBeatFromMouseEvent, seek]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      
      if (e.code === 'Space') {
        e.preventDefault();
        if (isPlaying) {
          pause();
        } else {
          play();
        }
      } else if (e.code === 'Escape') {
        e.preventDefault();
        stop();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isPlaying, play, pause, stop]);

  const handleTimeSignatureChange = (value: string) => {
    const [beats, noteValue] = value.split(',').map(Number);
    setTimeSignature([beats, noteValue] as [number, number]);
  };

  return (
    <div className="flex items-center justify-between px-4 py-3 bg-[var(--color-bg-primary)] border-t border-[var(--color-border)] overflow-visible">
      {/* Left: Transport Controls */}
      <div className="flex items-center gap-3">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => seek(0)}
          className="transport-btn"
          title="Go to start"
        >
          <SkipBack size={18} />
        </Button>

        <Button
          variant={isPlaying ? 'primary' : 'default'}
          size="md"
          onClick={() => (isPlaying ? pause() : play())}
          className="transport-btn w-10 h-10"
          title={isPlaying ? 'Pause (Space)' : 'Play (Space)'}
        >
          {isPlaying ? <Pause size={20} /> : <Play size={20} />}
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={stop}
          className="transport-btn"
          title="Stop (Esc)"
        >
          <Square size={18} />
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={() => seek(currentBeat + 4)}
          className="transport-btn"
          title="Skip forward"
        >
          <SkipForward size={18} />
        </Button>

        {/* Loop Toggle */}
        <Button
          variant="ghost"
          size="sm"
          active={loop.enabled}
          onClick={toggleLoop}
          className="transport-btn gap-1"
          title="Toggle loop"
        >
          <RefreshCw size={14} />
          <span className="text-xs">Loop</span>
        </Button>
      </div>

      {/* Center: Time Display & BPM */}
      <div className="flex items-center gap-6">
        {/* Time Display */}
        <div className="text-center">
          <div className="font-mono text-2xl text-[var(--color-text-primary)] tracking-wide">
            {formatTime(currentBeat, timeSignature[0])}
          </div>
          <div className="text-xs text-[var(--color-text-muted)]">
            Bar : Beat : 16th
          </div>
        </div>

        {/* BPM */}
        <div className="flex items-center gap-2">
          <input
            type="number"
            value={bpm}
            onChange={(e) => setBpm(parseInt(e.target.value) || 120)}
            min={40}
            max={240}
            className="w-16 px-2 py-1 text-center font-mono bg-[var(--color-bg-tertiary)] border border-[var(--color-border)] rounded text-[var(--color-text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]"
          />
          <span className="text-sm text-[var(--color-text-secondary)]">BPM</span>
        </div>

        {/* Time Signature */}
        <Select
          value={`${timeSignature[0]},${timeSignature[1]}`}
          options={TIME_SIGNATURES}
          onChange={handleTimeSignatureChange}
        />

        {/* Metronome */}
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            active={metronomeEnabled}
            onClick={toggleMetronome}
            title="Toggle metronome"
          >
            <Volume2 size={16} />
          </Button>
          {metronomeEnabled && (
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={metronomeVolume}
              onChange={(e) => setMetronomeVolume(parseFloat(e.target.value))}
              className="w-16"
            />
          )}
        </div>
      </div>

      {/* Right: Seek Bar */}
      <div className="flex-1 max-w-md ml-6 py-3 px-2 overflow-visible">
        <div
          ref={seekBarRef}
          onMouseDown={handleSeekBarMouseDown}
          className="relative h-3 rounded-full cursor-pointer group select-none"
          style={{
            background: 'rgba(255, 255, 255, 0.2)',
            border: '1px solid rgba(255, 255, 255, 0.1)',
            overflow: 'visible'
          }}
        >
          {/* Loop Region */}
          {loop.enabled && (
            <div
              className="absolute h-full bg-[var(--color-accent)] opacity-30 rounded-full"
              style={{
                left: `${(loop.start / projectLength) * 100}%`,
                width: `${((loop.end - loop.start) / projectLength) * 100}%`,
              }}
            />
          )}
          
          {/* Progress */}
          <div
            className="absolute h-full bg-[var(--color-accent)] rounded-l-full"
            style={{ width: `${(currentBeat / projectLength) * 100}%` }}
          />
          
          {/* Playhead */}
          <div
            className="absolute w-5 h-5 bg-white rounded-full z-10"
            style={{ 
              left: `max(0px, calc(${(currentBeat / projectLength) * 100}% - 10px))`,
              top: '-4px',
              boxShadow: '0 0 10px rgba(255,255,255,0.8), 0 2px 4px rgba(0,0,0,0.3)'
            }}
          />
        </div>
      </div>
    </div>
  );
}

