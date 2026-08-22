# CottDAW development

## Prerequisites

Arch Linux (or equivalent) with Rust toolchain, PipeWire, and the packages listed in the [README](../README.md).

```bash
rustc --version   # edition 2024 workspace
ffmpeg -version   # for Opus / Gonio export tests by hand
```

## Workspace layout

```
Cargo.toml                 # workspace + dependency versions + truce-rack-vst3 patch
.cargo/config.toml         # aliases (`bundle-synth`, …)
crates/cott-core/          # library (unit-tested)
crates/cott-ipc/           # IPC protocol library
crates/cott-daw/           # GUI binary `cott-daw`
crates/cott-vst-worker/    # worker binary `cott-vst-worker`
crates/cott-synth-dsp/     # CottSynth DSP (built-in + VST3)
crates/cott-synth/         # CottSynth VST3 cdylib
crates/cott-filter-dsp/    # CottFilter biquad DSP + response probe
crates/cott-filter/        # CottFilter VST3 cdylib
crates/cott-whistle-dsp/   # CottWhistle voice (divider, resonators, 4034)
crates/cott-whistle/       # CottWhistle VST3 cdylib
crates/cott-haze-dsp/      # CottHaze voices (electric piano, tape, dust)
crates/cott-haze/          # CottHaze VST3 cdylib
crates/cott-vinyl-dsp/     # CottVinyl wear (pops, hiss, muffle, rumble)
crates/cott-vinyl/         # CottVinyl VST3 cdylib
crates/cott-tape-dsp/      # CottTape delay
crates/cott-tape/          # CottTape VST3 cdylib
crates/cott-bass-dsp/      # CottBass sub
crates/cott-bass/          # CottBass VST3 cdylib
crates/cott-pluck-dsp/     # CottPluck guitar
crates/cott-pluck/         # CottPluck VST3 cdylib
crates/cott-kit-dsp/       # CottKit drums
crates/cott-kit/           # CottKit VST3 cdylib
crates/cott-plugin-ui/     # shared skeuomorphic egui panel kit for the VST3s
crates/cott-xtask/         # nih-plug bundler entry
vendor/truce-rack-vst3/    # patched VST3 host bindings
docs/                      # this documentation
```

Default member is `cott-daw`.

## Build

```bash
# Typical (script): bundle CottSynth, then host + worker
./scripts/build-daw.sh
./scripts/build-daw.sh release

# Or manually (one alias per first-party plugin):
cargo bundle-synth-debug && cargo bundle-filter-debug && cargo bundle-vinyl-debug \
  && cargo bundle-whistle-debug && cargo bundle-haze-debug && cargo bundle-tape-debug \
  && cargo bundle-bass-debug && cargo bundle-pluck-debug && cargo bundle-kit-debug \
  && cargo build -p cott-daw -p cott-vst-worker
cargo bundle-synth && cargo bundle-filter && cargo bundle-vinyl && cargo bundle-whistle \
  && cargo bundle-haze && cargo bundle-tape && cargo bundle-bass && cargo bundle-pluck \
  && cargo bundle-kit && cargo build --release -p cott-daw -p cott-vst-worker
```

The DAW always lists the first-party VST3s in the browser and loads the bundles under `target/bundled/` through the worker. Bundle before building `cott-daw` so `build.rs` can embed the absolute paths.

Install the same bundles for other hosts with:

```bash
cp -a target/bundled/cott-{synth,filter,vinyl,whistle,haze,tape,bass,pluck,kit}.vst3 ~/.vst3/
```

The host resolves the worker binary as:

1. `cott-vst-worker` next to the `cott-daw` executable
2. Else `target/debug/cott-vst-worker` or `target/release/cott-vst-worker`

Always build **both** packages when changing IPC or worker code.

## Run

```bash
cargo run -p cott-daw
RUST_LOG=cott_daw=debug,cott_vst_worker=debug cargo run -p cott-daw
```

Startup forces `WINIT_UNIX_BACKEND=x11` if unset (needed for VST editors).

## Test

```bash
# Core model / DSP / graph / import / commands
cargo test -p cott-core --lib

# Compile host + worker (smoke that the workspace links)
cargo build -p cott-daw -p cott-vst-worker

# CottWhistle: circuit + VST3 panel
cargo test -p cott-whistle-dsp -p cott-whistle
cargo check -p cott-whistle && cargo bundle-whistle-debug

# CottHaze: electric piano + VST3 panel
cargo test -p cott-haze-dsp -p cott-haze
cargo check -p cott-haze && cargo bundle-haze-debug

# CottVinyl: wear + VST3 panel
cargo test -p cott-vinyl-dsp -p cott-vinyl
cargo check -p cott-vinyl && cargo bundle-vinyl-debug

# Lofi suite
cargo test -p cott-tape-dsp -p cott-tape -p cott-bass-dsp -p cott-bass \
  -p cott-pluck-dsp -p cott-pluck -p cott-kit-dsp -p cott-kit
```

Throw a paddle, play a note. Aftertouch (or CC 2) only does something when a touch switch is latched. Harpsichord bypasses the 4034, so Brilliance is dead on that paddle, on purpose.

To open the whistle editor standalone, point the worker at the **bundle directory** (not the inner `.so`, which the scanner cannot resolve):

```bash
RUST_LOG=info ./target/debug/cott-vst-worker --probe-editor "$PWD/target/bundled/cott-whistle.vst3"
```

There is no fake in-process plugin path anymore; plugin IPC is exercised against real `cott-vst-worker` builds when testing by hand.

Useful `cott-core` areas covered by unit tests include tempo/sample conversion, graph cycle rejection, topological compile, automation interpolation, project save/load, and undo/redo.

## Module map (where to change things)

| Concern | Start here |
|---------|------------|
| Project `.ctgdaw` archives / tracks / default wiring | `cott-core/src/project.rs`, `archive.rs`, `clips.rs` |
| Graph validation & compile / PDC | `cott-core/src/graph.rs` |
| Routing canvas layout (columns, auto-arrange) | `cott-core/src/graph.rs` (`layout`, `arranged_positions`), `cott-daw/src/ui/graph_editor.rs` |
| Block DSP | `cott-core/src/dsp.rs` |
| CottSynth voices / waveforms / ADSR | `cott-synth-dsp` |
| CottSynth VST3 wrapper | `cott-synth` |
| CottFilter biquad + response curve | `cott-filter-dsp` |
| CottWhistle circuit / recipes | `cott-whistle-dsp` |
| CottWhistle `v4-` parameters and panel | `cott-whistle/src/lib.rs` |
| CottHaze voices / tape / dust | `cott-haze-dsp` |
| CottHaze VST3 wrapper | `cott-haze` |
| CottVinyl pops / hiss / rumble / Dusty-Radio-Tape wear | `cott-vinyl-dsp` |
| CottVinyl VST3 wrapper | `cott-vinyl` |
| CottTape delay | `cott-tape-dsp` / `cott-tape` |
| CottBass sub | `cott-bass-dsp` / `cott-bass` |
| CottPluck guitar | `cott-pluck-dsp` / `cott-pluck` |
| CottKit drums | `cott-kit-dsp` / `cott-kit` |
| Plugin panel look (chassis, knobs, wells) | `cott-plugin-ui` |
| First-party plugin catalog entries | `cott-daw/src/builtin_*.rs` |
| Engine commands & offline render | `cott-core/src/engine.rs` |
| Undoable edits | `cott-core/src/commands.rs` |
| Export formats | `cott-core/src/export.rs`, `visualizers/` |
| IPC types / SHM | `cott-ipc/src/lib.rs` |
| App shell & persistence UX | `cott-daw/src/app.rs` |
| Audio device | `cott-daw/src/audio.rs` |
| Worker spawn / scan / process | `cott-daw/src/plugins.rs` |
| UI panels | `cott-daw/src/ui/*` |
| VST3/CLAP/LV2 load & process | `cott-vst-worker/src/vst.rs` |
| VST2/yabridge legacy host | `cott-vst-worker/src/vst2.rs` |
| X11 editor embed | `cott-vst-worker/src/x11_editor.rs` |
| Instrument vs effect heuristics | `cott-vst-worker/src/classify.rs` |

## Protocol changes

If you change `HostToWorker`, `WorkerToHost`, or `ShmLayout`:

1. Bump or keep `PROTOCOL_VERSION` in `cott-ipc` deliberately.
2. Update both host (`plugins.rs`) and worker (`main.rs` / `host.rs`).
3. Rebuild both binaries before running.

Realtime process path must stay allocation-light on the audio thread; prefer preallocated buffers and non-blocking host locks (`try_lock` → silence/bypass for the block on contention).

## Debugging plugins

```bash
RUST_LOG=cott_daw=debug,cott_vst_worker=debug cargo run -p cott-daw
```

Worker stderr is forwarded into host tracing. Failed instances show in the Plugins tab with **Restart**.

For yabridge: catalog scan defers VST2/VST3/CLAP wrappers so it does not spawn Wine; first **Load** may be slow. The vendored `truce-rack-vst3` patch ensures `ModuleEntry` runs before factory lookup.

## Project file version

`PROJECT_VERSION = 2` in `cott-core/src/project.rs`. Version 1 VST3 node names deserialize through aliases and default to the VST3 format. Loading a newer version than the binary supports is an error.

Plugin state lives in opaque VST3 blobs and is versioned per plugin, not by `PROJECT_VERSION`. CottWhistle kept its class ID and moved every parameter to a `v4-` ID, so earlier whistle blobs match nothing and those instances load the new defaults. Use the same trick (new IDs, same class ID) when a plugin's controls change so much that migrating the old values would produce a wrong patch instead of a missing one.

## Style notes

- Prefer reversible `commands` for user-visible mutations.
- Keep graph edits validated before swapping `CompiledPlan`.
- The routing canvas re-arranges itself whenever a project opens, until the user
  positions a node by hand (`AudioGraph::user_arranged`). Node geometry changes
  therefore need no migration — set the sizes in `graph::layout` and old
  projects lay themselves out again.
- Do not load plugin `.so` code inside `cott-daw`.
- Stereo (`MAX_CHANNELS = 2`) is assumed end-to-end today.

## Further reading

- [Architecture](architecture.md) — runtime and data model
- [User guide](user-guide.md) — product behavior
- `.cursor/plans/rust_daw_mvp_*.plan.md` — original MVP plan (historical)
