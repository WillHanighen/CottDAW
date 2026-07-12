# CottDAW

Linux-first Ableton-style DAW written in Rust.

## Features (MVP)

- Arrangement timeline with MIDI and audio tracks
- Piano-roll MIDI editing
- Authoritative acyclic audio/MIDI routing graph (cycles rejected)
- Built-in gain/pan/mute, summing, and master bus
- Sandboxed VST3 hosting (one worker process per plugin)
- Fake sine instrument / gain effect for testing without plugins
- Parameter automation lanes
- Undo / redo
- Project save/load with autosave
- Offline export to WAV or Ogg Opus (via `ffmpeg`)

## Requirements (Arch Linux)

```bash
sudo pacman -S --needed rust pipewire pipewire-alsa pipewire-pulse \
  libpipewire alsa-lib cmake pkgconf ffmpeg
```

Optional: install VST3 instruments under `~/.vst3`, `/usr/lib/vst3`, or `/usr/local/lib/vst3`.

**Display:** run under **X11 / XWayland** so native VST3 editors can embed. CottDAW sets `WINIT_UNIX_BACKEND=x11` at startup.

## Build

```bash
cargo build -p cott-daw -p cott-vst-worker
```

Both binaries land in `target/debug/`. The DAW looks for `cott-vst-worker` next to itself.

## Run

```bash
# With fake plugins (no VST3 scan required)
COTT_FAKE_PLUGINS=1 cargo run -p cott-daw -- --fake-plugins

# Or after a normal build
./target/debug/cott-daw --fake-plugins
```

With real VST3s:

```bash
./target/debug/cott-daw
```

## Quick workflow

1. Select a MIDI track in the arrangement.
2. Load **Fake Sine Instrument** (or a VST3) from the left browser (click an entry), or right-click the routing canvas.
3. Click **+ Clip**, select the clip, draw notes in the Piano Roll (left-click add, right-click remove).
4. Press Play. Adjust gain on the track header.
5. Open **Routing** to reconnect nodes; invalid cycles are rejected.
6. **Export** writes `.opus` (ffmpeg + libopus) or `.wav`.

## Architecture

```
cott-daw (GUI + PipeWire/cpal)
   │ Unix socket + POSIX shm
   ▼
cott-vst-worker  (one process per plugin instance)
```

- `cott-core` — project model, typed DAG, DSP graph compiler, offline render
- `cott-ipc` — length-prefixed bincode protocol + shared-memory audio/MIDI ring
- Plugin crashes kill only the worker; the DAW silences/bypasses that node and keeps transport running

## Tests

```bash
cargo test -p cott-core --lib
cargo build -p cott-vst-worker
cargo test -p cott-daw --test fake_worker_ipc
```

## Limitations

- No live audio or MIDI keyboard recording in this milestone
- Native editor embedding needs a parent X11 window; generic parameter sliders always work
- Opus export requires `ffmpeg` with libopus
- Feedback loops are intentionally unsupported
