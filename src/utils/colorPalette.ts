// Vibrant color palette for tracks
export const TRACK_COLORS = [
  '#FF6B6B', // Coral
  '#4ECDC4', // Cyan/Teal
  '#95E879', // Lime
  '#A78BFA', // Violet
  '#FBBF24', // Amber
  '#F472B6', // Rose
  '#38BDF8', // Sky Blue
  '#FB923C', // Orange
  '#34D399', // Emerald
  '#E879F9', // Fuchsia
];

let colorIndex = 0;

export function getNextTrackColor(): string {
  const color = TRACK_COLORS[colorIndex % TRACK_COLORS.length];
  colorIndex++;
  return color;
}

export function resetColorIndex(): void {
  colorIndex = 0;
}

export function setColorIndex(index: number): void {
  colorIndex = index;
}

