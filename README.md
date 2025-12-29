# CottDAW - Web-Based Waveform Synthesizer

A modern, web-based Digital Audio Workstation (DAW) built with BunJS, React, and Tone.js. Create music using basic waveforms (sine, square, sawtooth, triangle) on a multi-track piano roll interface.

## Features

### Audio Engine

- **Waveform Types**: Sine, Square, Sawtooth, Triangle oscillators
- **ADSR Envelope**: Attack, Decay, Sustain, Release controls per track
- **Effects Chain**: Reverb, Delay, Filter (lowpass/highpass), and Distortion
- **Real-time Playback**: Powered by Tone.js and Web Audio API

### Piano Roll Editor

- **Multi-track Composition**: Create multiple tracks with different instruments
- **Note Drawing Tools**: Pencil (P), Eraser (E), Selection (V)
- **Grid Snap**: 1/4, 1/8, 1/16, 1/32 note quantization
- **Zoom Controls**: Horizontal zoom for detailed editing
- **Velocity Support**: Each note has individual velocity

### Track Controls

- **Volume & Pan**: Per-track mixing controls with visual knobs
- **Mute/Solo**: Standard DAW workflow controls
- **Color Coding**: Each track gets a unique vibrant color

### Transport

- **Playback Controls**: Play, Pause, Stop, Seek
- **BPM Control**: 40-240 BPM range
- **Time Signature**: 4/4, 3/4, 6/8, 2/4, 5/4
- **Loop Region**: Set loop points for focused composition
- **Metronome**: With volume control

### File Operations

- **Save/Load Projects**: Export and import JSON project files
- **WAV Export**: Render your composition to a WAV file

## Tech Stack

- **Runtime/Bundler**: [Bun](https://bun.sh)
- **UI Framework**: React 18
- **Audio Engine**: [Tone.js](https://tonejs.github.io/)
- **State Management**: [Zustand](https://zustand-demo.pmnd.rs/)
- **Icons**: [Lucide React](https://lucide.dev/)
- **Styling**: Custom CSS with CSS variables

## Getting Started

### Prerequisites

- [Bun](https://bun.sh) v1.0 or later

### Installation

```bash
# Clone the repository
cd CottDAW

# Install dependencies
bun install

# Start the development server
bun run dev
```

The app will be available at `http://localhost:3000`

## Usage

### Creating Music

1. **Add a Track**: Click the "+ Add" button to create a new track
2. **Select Wave Type**: Choose from Sine, Square, Sawtooth, or Triangle
3. **Draw Notes**: Use the Pencil tool (P) and click/drag on the piano roll
4. **Edit Notes**: Use the Select tool (V) to move/resize, or Eraser (E) to delete
5. **Adjust Sound**: Tweak volume, pan, ADSR envelope, and effects
6. **Play**: Press Space or click Play to hear your composition

### Keyboard Shortcuts

| Key | Action |
| ----- | -------- |
| `Space` | Play/Pause |
| `P` | Pencil tool |
| `E` | Eraser tool |
| `V` | Select tool |
| `Delete/Backspace` | Delete selected notes |

### Saving Your Work

- **Save Project**: Click "Save" to download a `.cottdaw.json` file
- **Load Project**: Click "Load" and select a previously saved project
- **Export Audio**: Click "Export WAV" to render your composition

## Project Structure

```bash
CottDAW/
├── index.ts                 # Bun server entry
├── src/
│   ├── index.html          # HTML entry point
│   ├── main.tsx            # React entry
│   ├── App.tsx             # Main app component
│   ├── styles.css          # Global styles
│   ├── components/         # React components
│   ├── stores/             # Zustand state stores
│   ├── audio/              # Tone.js audio engine
│   ├── utils/              # Helper functions
│   └── types/              # TypeScript types
├── package.json
└── tsconfig.json
```

## License

CC-BY-SA
