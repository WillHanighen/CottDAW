# CottDAW

Linux-first Ableton-style DAW written in Rust.

Arrangement timeline, piano-roll MIDI editing, an authoritative acyclic audio/MIDI routing graph, and sandboxed VST2/VST3/CLAP/LV2 hosting (one worker process per plugin). Built for PipeWire on Arch Linux.

## Features

- Arrangement timeline with MIDI and audio tracks
- Piano-roll MIDI editing with note audition
- Authoritative acyclic audio/MIDI routing graph (cycles rejected)
- Built-in gain/pan/mute, summing, and master bus
- Sandboxed VST2, VST3, CLAP, and LV2 hosting (one worker process per plugin)
- yabridge support for Windows VST2, VST3, and CLAP plugins
- Parameter automation lanes
- Undo / redo
- Project save/load with periodic autosave
- Offline export to WAV, Ogg Opus, or Gonio MP4 (via `ffmpeg`)

## Documentation

| Doc | Audience |
|-----|----------|
| [User guide](docs/user-guide.md) | Workflows, shortcuts, import/export |
| [Architecture](docs/architecture.md) | Crates, engine, IPC, graph model |
| [Development](docs/development.md) | Build, test, project layout |

## Requirements (Arch Linux)

```bash
sudo pacman -S --needed rust pipewire pipewire-alsa pipewire-pulse \
  libpipewire alsa-lib cmake pkgconf ffmpeg lilv
```

Plugin search paths:

- VST2: `~/.vst`, `/usr/lib/vst`, `/usr/local/lib/vst`, plus `VST_PATH`
- VST3: `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3`, plus `VST3_PATH`
- CLAP: `~/.clap`, `/usr/lib/clap`, `/usr/local/lib/clap`, plus `CLAP_PATH`
- LV2: Lilv's standard paths plus `LV2_PATH`

For Windows plugins, install Wine Staging and yabridge, register the Windows plugin directories with `yabridgectl add`, then run `yabridgectl sync`. CottDAW discovers the resulting wrappers under `~/.vst/yabridge`, `~/.vst3/yabridge`, and `~/.clap/yabridge`. Yabridge wrappers are catalogued without starting Wine; Wine starts when a plugin is loaded.

**Display:** run under **X11 / XWayland** so native plugin editors can embed. CottDAW sets `WINIT_UNIX_BACKEND=x11` at startup.

## Build

```bash
cargo build -p cott-daw -p cott-vst-worker
```

Both binaries land in `target/debug/`. The DAW looks for `cott-vst-worker` next to itself (or under `target/debug|release/`).

## Run

```bash
cargo run -p cott-daw
# Or after a normal build
./target/debug/cott-daw
```

Logging uses the `RUST_LOG` env filter (defaults include `cott_daw=info` and `cott_vst_worker=info`).

## Quick workflow

1. Select a MIDI track in the arrangement.
2. Load a plugin from the left browser (click an entry), or right-click the routing canvas.
3. Click **+ Clip**, select the clip, draw notes in the Piano Roll (left-click add, right-click remove).
4. Press Play (Space). Adjust gain on the track header.
5. Open **Routing** to reconnect nodes; invalid cycles are rejected.
6. **Export** (Ctrl+E) writes `.opus`, `.wav`, or goniometer `.mp4`.

## Architecture (overview)

```
cott-daw (GUI + PipeWire/cpal)
   │ Unix socket + POSIX shm
   ▼
cott-vst-worker  (one process per plugin instance)
```

- **`cott-core`** — project model, typed DAG, DSP graph compiler, offline render
- **`cott-ipc`** — length-prefixed bincode protocol + shared-memory audio/MIDI ring
- Plugin crashes kill only the worker; the DAW silences/bypasses that node and keeps transport running

## Tests

```bash
cargo test -p cott-core --lib
cargo build -p cott-daw -p cott-vst-worker
```

## Limitations

- No live audio or MIDI keyboard recording in this milestone
- Native editor embedding needs X11/XWayland; generic parameter sliders always work
- Opus and Gonio MP4 export require `ffmpeg` (libopus / libx264 + AAC)
- Feedback loops are intentionally unsupported
- Constant tempo map only (no tempo automation)

## License

MIT
