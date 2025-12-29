import React, { useCallback, useRef, useState } from 'react';
import { useTransportStore } from '../stores/transportStore.ts';

interface LoopRegionProps {
  pixelsPerBeat: number;
  totalBeats: number;
  height: number;
}

export default function LoopRegion({ pixelsPerBeat, totalBeats, height }: LoopRegionProps) {
  const loop = useTransportStore((s) => s.loop);
  const setLoopRegion = useTransportStore((s) => s.setLoopRegion);

  const [dragging, setDragging] = useState<'start' | 'end' | 'region' | null>(null);
  const startPosRef = useRef({ x: 0, start: 0, end: 0 });

  const left = loop.start * pixelsPerBeat;
  const width = (loop.end - loop.start) * pixelsPerBeat;

  const handleMouseDown = useCallback((e: React.MouseEvent, type: 'start' | 'end' | 'region') => {
    e.preventDefault();
    e.stopPropagation();
    setDragging(type);
    startPosRef.current = { x: e.clientX, start: loop.start, end: loop.end };

    const handleMouseMove = (e: MouseEvent) => {
      const deltaX = e.clientX - startPosRef.current.x;
      const deltaBeat = deltaX / pixelsPerBeat;

      if (type === 'start') {
        const newStart = Math.max(0, Math.min(startPosRef.current.end - 1, startPosRef.current.start + deltaBeat));
        setLoopRegion(newStart, startPosRef.current.end);
      } else if (type === 'end') {
        const newEnd = Math.max(startPosRef.current.start + 1, Math.min(totalBeats, startPosRef.current.end + deltaBeat));
        setLoopRegion(startPosRef.current.start, newEnd);
      } else {
        const duration = startPosRef.current.end - startPosRef.current.start;
        let newStart = startPosRef.current.start + deltaBeat;
        newStart = Math.max(0, Math.min(totalBeats - duration, newStart));
        setLoopRegion(newStart, newStart + duration);
      }
    };

    const handleMouseUp = () => {
      setDragging(null);
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }, [loop, pixelsPerBeat, totalBeats, setLoopRegion]);

  if (!loop.enabled) return null;

  return (
    <div
      className="absolute top-0 pointer-events-auto"
      style={{ left, width, height }}
    >
      {/* Loop region background */}
      <div 
        className="absolute inset-0 bg-[var(--color-accent)] opacity-15 cursor-move"
        onMouseDown={(e) => handleMouseDown(e, 'region')}
      />
      
      {/* Start handle */}
      <div
        className="absolute left-0 top-0 bottom-0 w-2 bg-[var(--color-accent)] opacity-80 cursor-ew-resize hover:opacity-100"
        onMouseDown={(e) => handleMouseDown(e, 'start')}
      />
      
      {/* End handle */}
      <div
        className="absolute right-0 top-0 bottom-0 w-2 bg-[var(--color-accent)] opacity-80 cursor-ew-resize hover:opacity-100"
        onMouseDown={(e) => handleMouseDown(e, 'end')}
      />
    </div>
  );
}

