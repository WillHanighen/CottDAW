import React from 'react';

interface SelectOption<T extends string> {
  value: T;
  label: string;
}

interface SelectProps<T extends string> {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  label?: string;
  className?: string;
}

export default function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  className = '',
}: SelectProps<T>) {
  return (
    <div className={`flex flex-col gap-1 ${className}`}>
      {label && (
        <label className="text-xs text-[var(--color-text-secondary)]">
          {label}
        </label>
      )}
      <select
        value={value}
        onChange={(e) => onChange(e.target.value as T)}
        className="px-2 py-1 text-sm rounded bg-[var(--color-bg-tertiary)] border border-[var(--color-border)] text-[var(--color-text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)] cursor-pointer"
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
  );
}

