# CottDAW user guide

## Layout

| Region | What it does |
|--------|----------------|
| **Top bar** | Transport, tempo, loop, undo/redo, save/open, import, export, add tracks |
| **Left** | VST2/VST3/CLAP/LV2 plugin browser (filter + rescan) |
| **Center** | Arrangement timeline — tracks, clips, playhead |
| **Bottom tabs** | Piano Roll · Routing · Automation · Plugins |
| **Status bar** | Status text, bar:beat position, sample rate |

## Transport

| Control | Action |
|---------|--------|
| Play / Stop | Start or stop playback from the current playhead |
| Stop (⏹) | Stop and rewind to the start |
| BPM | Drag to change tempo (default 120) |
| Time signature | Beats per bar / beat unit (default 4/4) |
| Loop | Enable looping between loop start/end |

Playback uses the same compiled routing graph as offline export.

## Tracks and clips

### Adding tracks

Use **+ MIDI Track** or **+ Audio Track** in the top bar.

Default signal paths:

- **MIDI:** clip source → (optional plugin instrument) → gain/pan → master
- **Audio:** clip source → gain/pan → master

### Arrangement

- Click the ruler or a lane to **seek** the playhead.
- On a MIDI track, **+ Clip** creates a one-bar clip at the playhead.
- **Drag** clips to move (¼-beat quantize); drag the **right edge** to resize.
- Right-click a clip for **Copy**, **Duplicate**, **Move to track**, or **Delete** (or use Delete / Backspace with the clip selected).
- **Ctrl+C / Ctrl+V** copy and paste clips; paste starts at the mouse (hovered lane) or the playhead.
- **Ctrl+D** duplicates the selected clip immediately after itself.
- **F2** renames the selected track inline.
- Track headers expose **gain**, **mute**, and peak meters.

### Piano roll

1. Select a MIDI clip (or create one and switch to **Piano Roll**).
2. **Left-click** to add, move, or resize notes (¼-beat quantize).
3. **Ctrl+click** toggles notes in a multi-selection; **Shift+drag** on empty grid draws a lasso selection. Dragging any selected note moves the whole selection together. Chord stamp selects the notes it places (replacing any previous selection).
4. **Ctrl+C / Ctrl+V** copy and paste selected notes; paste aligns the earliest note to the mouse (or the playhead).
5. **Delete / Backspace** removes the selected notes (or the whole clip when no notes are selected).
6. **Escape** cancels an in-progress draw/move/resize/lasso, or clears the note selection.
7. **Right-click** a note to remove it (or the whole selection if that note is selected).
8. Double-click **Editing: …** to rename the clip.
9. Click piano keys to audition pitches while stopped.

Pitch range shown is roughly C2–C6. Editing notes can grow or shrink the clip length. Each MIDI clip stores its own scale guide in the project file.

## Plugins

### Loading

1. Open the left browser (**B** or the ☰ button).
2. Click a catalog entry to load onto the selected MIDI track (instruments), or use the **Routing** canvas right-click menu.

Scan locations:

- VST2: `~/.vst`, `/usr/lib/vst`, `/usr/local/lib/vst`, and `VST_PATH`
- VST3: `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3`, and `VST3_PATH`
- CLAP: `~/.clap`, `/usr/lib/clap`, `/usr/local/lib/clap`, and `CLAP_PATH`
- LV2: standard Lilv locations and `LV2_PATH`

For yabridge, install Windows plugins through Wine, add their directories with `yabridgectl add`, and run `yabridgectl sync`. Rescan in CottDAW afterward. VST2, VST3, and CLAP wrappers are supported.

### Plugins tab

With a plugin node selected:

- **Open editor** — floating native UI (X11 / XWayland)
- Generic parameter sliders — always available
- **Restart** — respawn a crashed or failed worker
- **Delete** — remove the node from the graph

If a worker crashes, transport keeps running: failed instruments are silenced; failed effects are bypassed.

### Effects

Effects are not auto-wired into insert slots. Add them from **Routing** (right-click → add effect) and connect ports yourself.

## Routing graph

The **Routing** tab edits the live project graph — the same DAG the audio engine compiles.

- Drag nodes; drag from ports to connect.
- Matching port types only (audio ↔ audio, MIDI ↔ MIDI).
- Cycles are rejected with an error; the last valid graph stays active.
- Toolbar: add gain/mixer, zoom. Right-click for instruments, effects, delete.

## Automation

In the **Automation** tab:

1. Add a lane (e.g. node gain).
2. Add points at the playhead.
3. Points use normalized 0–1 values; gain maps to roughly −60…+12 dB.

Automation also targets plugin parameters when available.

## Projects

Projects are single **`.ctgdaw`** files (ZIP archives in disguise, similar to `.mrpack`):

```
song.ctgdaw
  project.json    # versioned manifest
  assets/         # imported media
```

| Action | How |
|--------|-----|
| Save | Ctrl+S or **Save** (creates/overwrites a `.ctgdaw` file) |
| Open | Ctrl+O or **Open** (`.ctgdaw`, or legacy `project.json` for conversion) |
| Autosave | Every ~60s under `~/.local/share/CottDAW/autosave/autosave-*.ctgdaw` |

Relative media paths are stored inside the archive; missing assets are marked on load. Opening a legacy folder project's `project.json` imports it into a temporary workspace — use **Save** once to write a `.ctgdaw` file.

## Import

| Button | Formats | Notes |
|--------|---------|--------|
| Import Audio | WAV, FLAC, Ogg, MP3, AAC, … | Resampled to the project sample rate; creates a clip on the selected audio track |
| Import MIDI | Standard MIDI File (`.mid`) | Creates a MIDI clip at the playhead; project tempo is authoritative |

## Export

**Export** (Ctrl+E) opens settings, then a save dialog. Rendering is offline through the same graph as live playback.

| Format | Extension | Needs |
|--------|-----------|--------|
| Ogg Opus (default) | `.opus` | `ffmpeg` with libopus |
| WAV | `.wav` | — (16-bit stereo PCM) |
| Gonio MP4 | `.mp4` | `ffmpeg` with libx264 + AAC |

Options include tail padding (extra beats of silence), Opus bitrate, and goniometer video settings (resolution, FPS, draw mode, etc.).

## Keyboard shortcuts

Shortcuts are ignored while a text field has focus.

| Key | Action |
|-----|--------|
| Space | Toggle play / stop |
| Home | Stop and rewind |
| L | Toggle loop |
| B | Toggle plugin browser |
| F2 | Rename selected track |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z / Ctrl+Y | Redo |
| Ctrl+S | Save project |
| Ctrl+O | Open project |
| Ctrl+E | Export |
| Ctrl+C | Copy selected notes (piano roll) or selected clip |
| Ctrl+V | Paste notes/clip near the mouse (or playhead fallback) |
| Ctrl+D | Duplicate selected clip |
| Delete / Backspace | Delete selected notes, else selected clip / graph node / track |
| Escape | Cancel piano-roll drag/draw, else deselect notes |

## Tips

- Prefer **X11 or XWayland** for native plugin UIs.
- Keep `cott-vst-worker` on `PATH` next to `cott-daw` after building both packages.
- If audio fails to start, the UI still opens; check PipeWire and `RUST_LOG=debug`.
- yabridge plugins appear in the browser; first load may take longer while Wine starts.
