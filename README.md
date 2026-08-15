# CottDAW

Linux-first Ableton-style DAW written in Rust.

Arrangement timeline, piano-roll MIDI editing, an authoritative acyclic audio/MIDI routing graph, and sandboxed VST2/VST3/CLAP/LV2 hosting (one worker process per plugin). Built for PipeWire on Arch Linux.

## Features

- Arrangement timeline with MIDI and audio tracks
- Piano-roll MIDI editing with note audition
- Authoritative acyclic audio/MIDI routing graph (cycles rejected)
- Built-in **CottSynth**, **CottFilter**, and **CottWhistle** VST3s (always in the browser; synth default on MIDI tracks)
- Built-in gain/pan/mute, summing, and master bus
- Sandboxed VST2, VST3, CLAP, and LV2 hosting (one worker process per plugin)
- yabridge support for Windows VST2, VST3, and CLAP plugins
- Parameter automation lanes
- Undo / redo
- Project `.ctgdaw` save/load with periodic autosave
- Offline export to WAV, Ogg Opus, or Gonio MP4 (via `ffmpeg`)
- Redistributable CottSynth / CottFilter / CottWhistle VST3s (`cargo bundle-synth`, `cargo bundle-filter`, `cargo bundle-whistle`)

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
./scripts/build-daw.sh
# or: cargo bundle-synth-debug && cargo bundle-filter-debug && cargo bundle-whistle-debug && cargo build -p cott-daw -p cott-vst-worker
```

Both binaries land in `target/debug/`. The DAW looks for `cott-vst-worker` next to itself (or under `target/debug|release/`). Bundle the first-party VSTs so they show up in the browser.

### Built-in VST3s (redistribution)

```bash
cargo bundle-synth          # → target/bundled/cott-synth.vst3
cargo bundle-filter         # → target/bundled/cott-filter.vst3
cargo bundle-whistle        # → target/bundled/cott-whistle.vst3
# debug: bundle-synth-debug / bundle-filter-debug / bundle-whistle-debug
```

Copy those bundles into `~/.vst3/` (or another host’s VST3 path) to use them outside CottDAW. The DAW injects them into the plugin browser automatically (CottSynth = default MIDI instrument; CottFilter = stereo LP/HP effect; CottWhistle = monophonic G-funk whistle lead).

### CottWhistle

The "whistle" is a nickname for a filtered, harmonically rich analog lead, **not** a sine oscillator — there is no sine anywhere in the instrument. Every character builds its tone from a narrow pulse, a stepped saw, or a square, then shapes it through a four-pole ladder low-pass and a bank of parallel band-pass resonators. One monophonic voice with last-note priority, legato (no envelope retrigger on overlap), and exponential portamento does the rest.

Four characters set the circuit routing and a calibrated set of defaults, which the shared macro knobs then adjust:

| Character | Circuit | Inspired by |
|-----------|---------|-------------|
| **Worm** | 1/14 pulse → reed resonator bank → VCF | ARP Pro Soloist "Oboe", *Funky Worm* |
| **West Coast** | 2' saw + detuned square → ladder | Minimoog G-funk leads |
| **Silk** | saw-led, soft resonator body | smoother mid-90s leads |
| **San Andreas** | narrow pulse, tight glide, brighter | game-theme-style lead |

Controls: character, glide, octave, pulse/saw blend, detune, brilliance, emphasis, body, vibrato rate/depth/delay, attack, release, drive, output. Unison and chorus are gone; the single centered voice is sent to both channels so the output stays mono-compatible.

**State reset:** the plugin keeps its name and VST3 class ID (`CottWhstlVST3CE!`) so existing projects still resolve it, but every parameter now uses a `v3-` ID. Earlier whistle state no longer matches any parameter and is ignored — those instances come back with the new defaults rather than a half-mapped patch.

## Run

```bash
cargo run -p cott-daw
# Or after a normal build
./target/debug/cott-daw
```

Logging uses the `RUST_LOG` env filter (defaults include `cott_daw=info` and `cott_vst_worker=info`).

## Quick workflow

1. Select a MIDI track (it already has **CottSynth**; its editor opens on startup).
2. Click **+ Clip**, select the clip, draw notes in the Piano Roll (left-click add, right-click remove).
3. Press Play (Space). Re-open the editor anytime from **Plugins** → **Open Native Editor**.
4. CottSynth stays listed in the left browser; load anything else to replace it. Adjust gain on the track header.
5. Open **Routing** to reconnect nodes; invalid cycles are rejected.
6. **Export** (Ctrl+E) writes `.opus`, `.wav`, or goniometer `.mp4`.

## Architecture (overview)

```
cott-daw (GUI + PipeWire/cpal)
   │ Unix socket + POSIX shm
   ▼
cott-vst-worker  (one process per plugin instance)
```

- **`cott-core`** — project model, typed DAG, DSP graph compiler, offline render (includes built-in CottSynth)
- **`cott-synth-dsp` / `cott-synth`** — shared synth engine + redistributable VST3
- **`cott-filter-dsp` / `cott-filter`** — stereo LP/HP biquad + redistributable VST3
- **`cott-whistle-dsp` / `cott-whistle`** — monophonic pulse/saw G-funk lead (ladder + resonators, four characters) + redistributable VST3
- **`cott-plugin-ui`** — shared skeuomorphic panel kit used by all three plugin editors
- **`cott-ipc`** — length-prefixed bincode protocol + shared-memory audio/MIDI ring
- Plugin crashes kill only the worker; the DAW silences/bypasses that node and keeps transport running

## Tests

```bash
cargo test -p cott-core --lib
cargo build -p cott-daw -p cott-vst-worker

# CottWhistle: DSP behaviour + spectra, then the plugin wrapper
cargo test -p cott-whistle-dsp -p cott-whistle
cargo run -p cott-whistle-dsp --example audition   # per-character partials and levels
```

## Limitations

- No live audio or MIDI keyboard recording in this milestone
- Native editor embedding needs X11/XWayland; generic parameter sliders always work
- Opus and Gonio MP4 export require `ffmpeg` (libopus / libx264 + AAC)
- Feedback loops are intentionally unsupported
- Constant tempo map only (no tempo automation)

## License

MIT
