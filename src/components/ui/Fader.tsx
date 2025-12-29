import React from 'react';

interface FaderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (value: number) => void;
  label?: string;
  showValue?: boolean;
  formatValue?: (value: number) => string;
  className?: string;
  vertical?: boolean;
}

export default function Fader({
  value,
  min = 0,
  max = 1,
  step = 0.01,
  onChange,
  label,
  showValue = true,
  formatValue,
  className = '',
  vertical = false,
}: FaderProps) {
  const displayValue = formatValue ? formatValue(value) : value.toFixed(2);

  return (
    <div className={`flex ${vertical ? 'flex-col items-center' : 'flex-col'} gap-1 ${className}`}>
      {label && (
        <label className="text-xs text-[var(--color-text-secondary)]">
          {label}
        </label>
      )}
      <div className={`relative ${vertical ? 'w-20 -rotate-90 origin-center' : 'w-full'} h-5 flex items-center`}>
        {/* Track background for visibility */}
        <div 
          className="absolute left-0 right-0 h-2 rounded-full"
          style={{ 
            pointerEvents: 'none',
            background: 'rgba(255, 255, 255, 0.15)',
            border: '1px solid rgba(255, 255, 255, 0.1)'
          }}
        />
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => onChange(parseFloat(e.target.value))}
          className="relative w-full"
        />
      </div>
      {showValue && (
        <span className="text-xs font-mono text-[var(--color-text-muted)]">
          {displayValue}
        </span>
      )}
    </div>
  );
}

