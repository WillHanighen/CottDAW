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
crates/cott-core/          # library (unit-tested)
crates/cott-ipc/           # IPC protocol library
crates/cott-daw/           # GUI binary `cott-daw`
crates/cott-vst-worker/    # worker binary `cott-vst-worker`
vendor/truce-rack-vst3/    # patched VST3 host bindings
docs/                      # this documentation
```

Default member is `cott-daw`.

## Build

```bash
# Debug (typical)
cargo build -p cott-daw -p cott-vst-worker

# Release
cargo build --release -p cott-daw -p cott-vst-worker
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
```

There is no fake in-process plugin path anymore; plugin IPC is exercised against real `cott-vst-worker` builds when testing by hand.

Useful `cott-core` areas covered by unit tests include tempo/sample conversion, graph cycle rejection, topological compile, automation interpolation, project save/load, and undo/redo.

## Module map (where to change things)

| Concern | Start here |
|---------|------------|
| Project `.ctgdaw` archives / tracks / default wiring | `cott-core/src/project.rs`, `archive.rs`, `clips.rs` |
| Graph validation & compile / PDC | `cott-core/src/graph.rs` |
| Block DSP | `cott-core/src/dsp.rs` |
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

## Style notes

- Prefer reversible `commands` for user-visible mutations.
- Keep graph edits validated before swapping `CompiledPlan`.
- Do not load plugin `.so` code inside `cott-daw`.
- Stereo (`MAX_CHANNELS = 2`) is assumed end-to-end today.

## Further reading

- [Architecture](architecture.md) — runtime and data model
- [User guide](user-guide.md) — product behavior
- `.cursor/plans/rust_daw_mvp_*.plan.md` — original MVP plan (historical)
