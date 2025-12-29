import React, { useRef, useCallback, useState } from 'react';

interface KnobProps {
  value: number;
  min?: number;
  max?: number;
  onChange: (value: number) => void;
  label?: string;
  size?: number;
  color?: string;
}

export default function Knob({
  value,
  min = -1,
  max = 1,
  onChange,
  label,
  size = 40,
  color = 'var(--color-accent)',
}: KnobProps) {
  const knobRef = useRef<HTMLDivElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const startYRef = useRef(0);
  const startValueRef = useRef(0);

  // Convert value to rotation angle (-135 to 135 degrees)
  const normalizedValue = (value - min) / (max - min);
  const rotation = -135 + normalizedValue * 270;

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsDragging(true);
    startYRef.current = e.clientY;
    startValueRef.current = value;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaY = startYRef.current - e.clientY;
      const range = max - min;
      const sensitivity = 100; // pixels for full range
      const deltaValue = (deltaY / sensitivity) * range;
      const newValue = Math.max(min, Math.min(max, startValueRef.current + deltaValue));
      onChange(newValue);
    };

    const handleMouseUp = () => {
      setIsDragging(false);
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }, [value, min, max, onChange]);

  return (
    <div className="flex flex-col items-center gap-1">
      {label && (
        <span className="text-xs text-[var(--color-text-secondary)]">{label}</span>
      )}
      <div
        ref={knobRef}
        onMouseDown={handleMouseDown}
        className="relative cursor-pointer select-none"
        style={{ width: size, height: size }}
      >
        {/* Knob background */}
        <div
          className="absolute inset-0 rounded-full"
          style={{
            background: `conic-gradient(from -135deg, ${color} 0deg, ${color} ${normalizedValue * 270}deg, var(--color-bg-tertiary) ${normalizedValue * 270}deg, var(--color-bg-tertiary) 270deg)`,
            border: '2px solid var(--color-border)',
          }}
        />
        {/* Inner circle */}
        <div
          className="absolute rounded-full bg-[var(--color-bg-secondary)]"
          style={{
            top: 4,
            left: 4,
            right: 4,
            bottom: 4,
          }}
        />
        {/* Indicator line */}
        <div
          className="absolute top-1/2 left-1/2 origin-bottom"
          style={{
            width: 2,
            height: size / 2 - 8,
            background: color,
            transform: `translate(-50%, -100%) rotate(${rotation}deg)`,
            borderRadius: 1,
            transition: isDragging ? 'none' : 'transform 0.1s ease',
          }}
        />
      </div>
      <span className="text-xs font-mono text-[var(--color-text-muted)]">
        {value.toFixed(2)}
      </span>
    </div>
  );
}

