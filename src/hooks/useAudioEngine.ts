import { useEffect, useRef, useCallback } from 'react';
import { useProjectStore } from '../stores/projectStore.ts';
import { useTransportStore, setSeekCallback } from '../stores/transportStore.ts';
import * as engine from '../audio/engine.ts';
import * as recorder from '../audio/recorder.ts';
import { updateTrackSynth, disposeTrackSynth, createTrackSynth } from '../audio/synth.ts';

export function useAudioEngine() {
  const isInitializedRef = useRef(false);
  const prevTracksRef = useRef<string[]>([]);

  const tracks = useProjectStore((s) => s.tracks);
  const bpm = useProjectStore((s) => s.bpm);
  const timeSignature = useProjectStore((s) => s.timeSignature);
  const projectName = useProjectStore((s) => s.projectName);

  const isPlaying = useTransportStore((s) => s.isPlaying);
  const isPaused = useTransportStore((s) => s.isPaused);
  const loop = useTransportStore((s) => s.loop);
  const metronomeEnabled = useTransportStore((s) => s.metronomeEnabled);
  const metronomeVolume = useTransportStore((s) => s.metronomeVolume);
  const setCurrentBeat = useTransportStore((s) => s.setCurrentBeat);
  const stop = useTransportStore((s) => s.stop);

  // Set up seek callback to sync with Tone.js
  useEffect(() => {
    setSeekCallback((beat: number) => {
      engine.seekTo(beat, useProjectStore.getState().bpm);
    });
    return () => setSeekCallback(null);
  }, []);

  // Initialize audio on first user interaction
  const initAudio = useCallback(async () => {
    if (!isInitializedRef.current) {
      await engine.initAudio();
      isInitializedRef.current = true;
    }
  }, []);

  // Sync synths with tracks
  useEffect(() => {
    const currentTrackIds = tracks.map((t) => t.id);
    const prevTrackIds = prevTracksRef.current;

    // Create synths for new tracks
    tracks.forEach((track) => {
      if (!prevTrackIds.includes(track.id)) {
        createTrackSynth(track);
      } else {
        updateTrackSynth(track);
      }
    });

    // Dispose synths for removed tracks
    prevTrackIds.forEach((id) => {
      if (!currentTrackIds.includes(id)) {
        disposeTrackSynth(id);
      }
    });

    prevTracksRef.current = currentTrackIds;
  }, [tracks]);

  // Handle playback state
  useEffect(() => {
    if (!isInitializedRef.current) return;

    if (isPlaying) {
      engine.scheduleNotes(tracks, loop, bpm, (beat) => {
        setCurrentBeat(beat);
      });
      engine.startPlayback();

      if (metronomeEnabled) {
        engine.setupMetronome(timeSignature[0], metronomeVolume);
        engine.startMetronome();
      }
    } else {
      // If isPaused is false, it's a stop (not pause) - fully reset
      if (!isPaused) {
        engine.stopPlayback();
        engine.clearScheduledNotes();
      } else {
        engine.pausePlayback();
      }
      engine.stopMetronome();
    }
  }, [isPlaying, isPaused]);

  // Update BPM
  useEffect(() => {
    engine.setBPM(bpm);
  }, [bpm]);

  // Update metronome
  useEffect(() => {
    if (metronomeEnabled && isPlaying) {
      engine.setupMetronome(timeSignature[0], metronomeVolume);
      engine.startMetronome();
    } else {
      engine.stopMetronome();
    }
  }, [metronomeEnabled, metronomeVolume, timeSignature, isPlaying]);

  // Handle stop
  const handleStop = useCallback(() => {
    engine.stopPlayback();
    engine.stopMetronome();
    engine.clearScheduledNotes();
  }, []);

  // Export WAV
  const exportWav = useCallback(async () => {
    await initAudio();
    await recorder.exportProject(tracks, bpm, projectName);
  }, [tracks, bpm, projectName, initAudio]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      engine.cleanup();
    };
  }, []);

  return {
    initAudio,
    exportWav,
    handleStop,
  };
}

