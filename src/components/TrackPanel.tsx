import React, { useState } from 'react';
import { Volume2, VolumeX, Headphones, Trash2, ChevronDown, ChevronRight } from 'lucide-react';
import Button from './ui/Button.tsx';
import Fader from './ui/Fader.tsx';
import Knob from './ui/Knob.tsx';
import Select from './ui/Select.tsx';
import { useProjectStore } from '../stores/projectStore.ts';
import type { Track, WaveType } from '../types/index.ts';

const WAVE_OPTIONS: { value: WaveType; label: string }[] = [
  { value: 'sine', label: '∿ Sine' },
  { value: 'square', label: '⊓ Square' },
  { value: 'sawtooth', label: '⋀ Sawtooth' },
  { value: 'triangle', label: '△ Triangle' },
];

interface TrackPanelProps {
  track: Track;
}

export default function TrackPanel({ track }: TrackPanelProps) {
  const [showEnvelope, setShowEnvelope] = useState(false);
  const [showEffects, setShowEffects] = useState(false);

  const selectedTrackId = useProjectStore((s) => s.selectedTrackId);
  const selectTrack = useProjectStore((s) => s.selectTrack);
  const removeTrack = useProjectStore((s) => s.removeTrack);
  const setTrackWaveType = useProjectStore((s) => s.setTrackWaveType);
  const setTrackVolume = useProjectStore((s) => s.setTrackVolume);
  const setTrackPan = useProjectStore((s) => s.setTrackPan);
  const toggleTrackMute = useProjectStore((s) => s.toggleTrackMute);
  const toggleTrackSolo = useProjectStore((s) => s.toggleTrackSolo);
  const setTrackEnvelope = useProjectStore((s) => s.setTrackEnvelope);
  const setTrackEffect = useProjectStore((s) => s.setTrackEffect);

  const isSelected = selectedTrackId === track.id;

  return (
    <div
      className={`p-3 cursor-pointer transition-colors ${
        isSelected
          ? 'bg-[var(--color-bg-hover)]'
          : 'hover:bg-[var(--color-bg-tertiary)]'
      }`}
      onClick={() => selectTrack(track.id)}
    >
      {/* Track Header */}
      <div className="flex items-center gap-2 mb-3">
        {/* Color indicator */}
        <div
          className="w-3 h-3 rounded-full flex-shrink-0"
          style={{ backgroundColor: track.color }}
        />
        
        {/* Track name */}
        <span className="font-medium text-sm text-[var(--color-text-primary)] truncate flex-1">
          {track.name}
        </span>

        {/* Delete button */}
        <Button
          variant="ghost"
          size="sm"
          onClick={(e) => {
            e.stopPropagation();
            removeTrack(track.id);
          }}
          className="opacity-50 hover:opacity-100 hover:text-red-400"
          title="Delete track"
        >
          <Trash2 size={14} />
        </Button>
      </div>

      {/* Wave Type Selector */}
      <Select
        value={track.waveType}
        options={WAVE_OPTIONS}
        onChange={(wt) => setTrackWaveType(track.id, wt)}
        className="mb-3"
      />

      {/* Volume & Pan */}
      <div className="flex gap-4 mb-3">
        <Fader
          value={track.volume}
          min={0}
          max={1}
          onChange={(v) => setTrackVolume(track.id, v)}
          label="Volume"
          formatValue={(v) => `${Math.round(v * 100)}%`}
          className="flex-1"
        />
        <Knob
          value={track.pan}
          min={-1}
          max={1}
          onChange={(v) => setTrackPan(track.id, v)}
          label="Pan"
          size={36}
        />
      </div>

      {/* Mute / Solo */}
      <div className="flex gap-2 mb-3">
        <Button
          variant={track.muted ? 'danger' : 'ghost'}
          size="sm"
          onClick={(e) => {
            e.stopPropagation();
            toggleTrackMute(track.id);
          }}
          className="flex-1"
        >
          {track.muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
          <span>Mute</span>
        </Button>
        <Button
          variant={track.solo ? 'primary' : 'ghost'}
          size="sm"
          onClick={(e) => {
            e.stopPropagation();
            toggleTrackSolo(track.id);
          }}
          className="flex-1"
        >
          <Headphones size={14} />
          <span>Solo</span>
        </Button>
      </div>

      {/* ADSR Envelope (collapsible) */}
      <div className="border-t border-[var(--color-border)] pt-2 mt-2">
        <button
          className="flex items-center gap-1 text-xs text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] w-full"
          onClick={(e) => {
            e.stopPropagation();
            setShowEnvelope(!showEnvelope);
          }}
        >
          {showEnvelope ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          ADSR Envelope
        </button>
        
        {showEnvelope && (
          <div className="grid grid-cols-4 gap-2 mt-2" onClick={(e) => e.stopPropagation()}>
            <Fader
              value={track.envelope.attack}
              min={0}
              max={2}
              step={0.01}
              onChange={(v) => setTrackEnvelope(track.id, { attack: v })}
              label="A"
              formatValue={(v) => `${v.toFixed(2)}s`}
            />
            <Fader
              value={track.envelope.decay}
              min={0}
              max={2}
              step={0.01}
              onChange={(v) => setTrackEnvelope(track.id, { decay: v })}
              label="D"
              formatValue={(v) => `${v.toFixed(2)}s`}
            />
            <Fader
              value={track.envelope.sustain}
              min={0}
              max={1}
              step={0.01}
              onChange={(v) => setTrackEnvelope(track.id, { sustain: v })}
              label="S"
              formatValue={(v) => `${Math.round(v * 100)}%`}
            />
            <Fader
              value={track.envelope.release}
              min={0}
              max={5}
              step={0.01}
              onChange={(v) => setTrackEnvelope(track.id, { release: v })}
              label="R"
              formatValue={(v) => `${v.toFixed(2)}s`}
            />
          </div>
        )}
      </div>

      {/* Effects (collapsible) */}
      <div className="border-t border-[var(--color-border)] pt-2 mt-2">
        <button
          className="flex items-center gap-1 text-xs text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] w-full"
          onClick={(e) => {
            e.stopPropagation();
            setShowEffects(!showEffects);
          }}
        >
          {showEffects ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          Effects
        </button>
        
        {showEffects && (
          <div className="space-y-3 mt-2" onClick={(e) => e.stopPropagation()}>
            {/* Reverb */}
            <Fader
              value={track.effects.reverb.wet}
              min={0}
              max={1}
              onChange={(v) => setTrackEffect(track.id, 'reverb', { wet: v })}
              label="Reverb"
              formatValue={(v) => `${Math.round(v * 100)}%`}
            />

            {/* Delay */}
            <div className="space-y-1">
              <span className="text-xs text-[var(--color-text-secondary)]">Delay</span>
              <div className="grid grid-cols-2 gap-2">
                <Fader
                  value={track.effects.delay.feedback}
                  min={0}
                  max={0.9}
                  onChange={(v) => setTrackEffect(track.id, 'delay', { feedback: v })}
                  label="Feedback"
                  formatValue={(v) => `${Math.round(v * 100)}%`}
                />
                <Fader
                  value={track.effects.delay.wet}
                  min={0}
                  max={1}
                  onChange={(v) => setTrackEffect(track.id, 'delay', { wet: v })}
                  label="Mix"
                  formatValue={(v) => `${Math.round(v * 100)}%`}
                />
              </div>
            </div>

            {/* Filter */}
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-xs text-[var(--color-text-secondary)]">Filter</span>
                <select
                  value={track.effects.filter.type}
                  onChange={(e) => setTrackEffect(track.id, 'filter', { type: e.target.value as 'lowpass' | 'highpass' })}
                  className="text-xs px-1 py-0.5 bg-[var(--color-bg-tertiary)] border border-[var(--color-border)] rounded"
                >
                  <option value="lowpass">Lowpass</option>
                  <option value="highpass">Highpass</option>
                </select>
              </div>
              <div className="grid grid-cols-2 gap-2">
                <Fader
                  value={track.effects.filter.frequency}
                  min={20}
                  max={20000}
                  step={1}
                  onChange={(v) => setTrackEffect(track.id, 'filter', { frequency: v })}
                  label="Freq"
                  formatValue={(v) => v >= 1000 ? `${(v / 1000).toFixed(1)}kHz` : `${Math.round(v)}Hz`}
                />
                <Fader
                  value={track.effects.filter.Q}
                  min={0.1}
                  max={20}
                  step={0.1}
                  onChange={(v) => setTrackEffect(track.id, 'filter', { Q: v })}
                  label="Q"
                  formatValue={(v) => v.toFixed(1)}
                />
              </div>
            </div>

            {/* Distortion */}
            <Fader
              value={track.effects.distortion.amount}
              min={0}
              max={1}
              onChange={(v) => setTrackEffect(track.id, 'distortion', { amount: v })}
              label="Distortion"
              formatValue={(v) => `${Math.round(v * 100)}%`}
            />
          </div>
        )}
      </div>
    </div>
  );
}

