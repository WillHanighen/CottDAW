# CottDAW

Linux-first Ableton-style DAW written in Rust.

Arrangement timeline, piano-roll MIDI editing, an authoritative acyclic audio/MIDI routing graph, and sandboxed VST2/VST3/CLAP/LV2 hosting (one worker process per plugin). Built for PipeWire on Arch Linux.

## Features

- Arrangement timeline with MIDI and audio tracks
- Piano-roll MIDI editing with note audition
- Authoritative acyclic audio/MIDI routing graph (cycles rejected)
- Built-in **CottSynth**, **CottFilter**, **CottVinyl**, **CottWhistle**, **CottHaze**, **CottTape**, **CottBass**, **CottPluck**, and **CottKit** VST3s (always in the browser; synth default on MIDI tracks)
- Built-in gain/pan/mute, summing, and master bus
- Sandboxed VST2, VST3, CLAP, and LV2 hosting (one worker process per plugin)
- yabridge support for Windows VST2, VST3, and CLAP plugins
- Parameter automation lanes
- Undo / redo
- Project `.ctgdaw` save/load with periodic autosave
- Offline export to WAV, Ogg Opus, or Gonio MP4 (via `ffmpeg`)
- Redistributable first-party VST3s (`cargo bundle-synth`, `cargo bundle-filter`, `cargo bundle-vinyl`, `cargo bundle-whistle`, `cargo bundle-haze`, `cargo bundle-tape`, `cargo bundle-bass`, `cargo bundle-pluck`, `cargo bundle-kit`)

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
# or: cargo bundle-synth-debug && cargo bundle-filter-debug && cargo bundle-vinyl-debug && cargo bundle-whistle-debug && cargo bundle-haze-debug && cargo bundle-tape-debug && cargo bundle-bass-debug && cargo bundle-pluck-debug && cargo bundle-kit-debug && cargo build -p cott-daw -p cott-vst-worker
```

Both binaries land in `target/debug/`. The DAW looks for `cott-vst-worker` next to itself (or under `target/debug|release/`). Bundle the first-party VSTs so they show up in the browser.

### Built-in VST3s (redistribution)

```bash
cargo bundle-synth          # → target/bundled/cott-synth.vst3
cargo bundle-filter         # → target/bundled/cott-filter.vst3
cargo bundle-vinyl          # → target/bundled/cott-vinyl.vst3
cargo bundle-whistle        # → target/bundled/cott-whistle.vst3
cargo bundle-haze           # → target/bundled/cott-haze.vst3
cargo bundle-tape           # → target/bundled/cott-tape.vst3
cargo bundle-bass           # → target/bundled/cott-bass.vst3
cargo bundle-pluck          # → target/bundled/cott-pluck.vst3
cargo bundle-kit            # → target/bundled/cott-kit.vst3
# debug: add -debug to any of those aliases
```

Copy those bundles into `~/.vst3/` (or another host's VST3 path) to use them outside CottDAW. The DAW injects them into the plugin browser automatically (CottSynth = default MIDI instrument; CottFilter = stereo LP/HP; CottVinyl = record wear; CottWhistle = monophonic lead; CottHaze = lofi electric piano; CottTape = tape delay; CottBass = sub-bass; CottPluck = dusty guitar; CottKit = dusty drums).

### CottWhistle

Thirty factory paddles, a 4034 ladder, the resonator bank, and a pressure strip. Channel aftertouch (or breath CC 2) drives Bend / Wow / Growl / Brilliance / Volume / Vibrato when those switches are on. Velocity is ignored. Oboe plus the portamento slider is the Funky Worm line; it is not a second synth.

Class ID stays `CottWhstlVST3CE!`. Parameters use `v4-` IDs, so older `v3-` state loads as the new defaults.

### CottHaze

Twelve-voice electric piano. Soft hammer, a decaying bell, tape flutter, a smear wash so chords melt, and a little vinyl dust. MIDI tracks still default to CottSynth; load Haze from the browser when you want held chords.

### CottVinyl

Stereo effect. Pops and crackle, surface hiss, Dusty / Radio / Tape muffle with a short smear, and turntable rumble. Keeps hissing with no input. Drop it after Haze or on the master.

### CottTape

Stereo tape delay. Time, feedback, wow, drive, mix. Darker repeats. Tails keep going on silence.

### CottBass

Mono sub-bass. Sine sub plus a folded triangle body, glide, punch, drive. Sit it under Haze chords.

### CottPluck

Six-voice Karplus-Strong guitar. Mute, body, tone, dust. Soft and dusty, not a Strat model.

### CottKit

Synthesized kit. Kick 36, snare 38, clap 39, closed hat 42, open hat 46. Dirt is a hiss bus on the kit.

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
- **`cott-whistle-dsp` / `cott-whistle`** — CottWhistle VST3 (Pro Soloist circuit, thirty paddles)
- **`cott-haze-dsp` / `cott-haze`** — CottHaze VST3 (12-voice lofi electric piano)
- **`cott-vinyl-dsp` / `cott-vinyl`** — CottVinyl VST3 (pops, hiss, muffle, rumble)
- **`cott-tape-dsp` / `cott-tape`** — CottTape VST3 (tape delay)
- **`cott-bass-dsp` / `cott-bass`** — CottBass VST3 (mono sub-bass)
- **`cott-pluck-dsp` / `cott-pluck`** — CottPluck VST3 (dusty guitar)
- **`cott-kit-dsp` / `cott-kit`** — CottKit VST3 (dusty drums)
- **`cott-plugin-ui`** — shared skeuomorphic panel kit used by the first-party plugin editors
- **`cott-ipc`** — length-prefixed bincode protocol + shared-memory audio/MIDI ring
- Plugin crashes kill only the worker; the DAW silences/bypasses that node and keeps transport running

## Tests

```bash
cargo test -p cott-core --lib
cargo build -p cott-daw -p cott-vst-worker

# CottWhistle: voice + VST3 wrapper
cargo test -p cott-whistle-dsp -p cott-whistle

# CottHaze: electric piano + VST3 wrapper
cargo test -p cott-haze-dsp -p cott-haze

# CottVinyl: wear + VST3 wrapper
cargo test -p cott-vinyl-dsp -p cott-vinyl

# Lofi suite
cargo test -p cott-tape-dsp -p cott-tape
cargo test -p cott-bass-dsp -p cott-bass
cargo test -p cott-pluck-dsp -p cott-pluck
cargo test -p cott-kit-dsp -p cott-kit
```

## Limitations

- No live audio or MIDI keyboard recording in this milestone
- Native editor embedding needs X11/XWayland; generic parameter sliders always work
- Opus and Gonio MP4 export require `ffmpeg` (libopus / libx264 + AAC)
- Feedback loops are intentionally unsupported
- Constant tempo map only (no tempo automation)

## License

MIT
