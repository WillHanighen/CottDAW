# CottDAW architecture

## Goals

- **Authoritative graph** — the routing editor mutates the same typed DAG the engine compiles; the UI is not a decoration.
- **Crash isolation** — third-party VST2/VST3/CLAP/LV2 code never loads in the GUI or audio process.
- **Realtime-safe callback** — the audio thread processes a precompiled plan; graph edits compile off-thread and swap atomically.
- **Acyclic signal flow** — feedback loops are rejected at connect time.

## Crate map

```
crates/
  cott-core/          Project model, graph, DSP, engine bridge, import/export
  cott-ipc/           Host ↔ worker protocol + POSIX shared-memory layout
  cott-daw/           egui app, cpal/PipeWire I/O, plugin host manager
  cott-vst-worker/    Sandboxed multi-format process (load, process, X11 editor)
vendor/
  truce-rack-vst3/    Patched VST3 bindings (ModuleEntry before GetPluginFactory)
```

| Crate | Responsibility |
|-------|----------------|
| `cott-core` | Domain types, validation, `CompiledPlan`, `process_block`, offline render |
| `cott-ipc` | `HostToWorker` / `WorkerToHost`, `ShmLayout`, encode/decode |
| `cott-daw` | UI, transport UX, spawning workers, forwarding SHM audio into the engine |
| `cott-vst-worker` | One plugin instance; scan mode or process mode |

Workspace root patches `truce-rack-vst3` so Linux/yabridge chainloaders call `ModuleEntry` before `GetPluginFactory`.

## Runtime topology

```
┌─────────────────────────────────────────────┐
│  cott-daw                                   │
│  ┌──────────┐    EngineCommand (rtrb)       │
│  │ egui UI  │ ──────────────────────────►   │
│  └────┬─────┘                               │
│       │ project snapshot / commands         │
│  ┌────▼──────────────────────────────────┐  │
│  │ cpal callback → AudioProcessor        │  │
│  │   process_block(CompiledPlan)         │  │
│  │   PluginAudioHost → PluginHost        │  │
│  └────┬──────────────────────────────────┘  │
│       │ Unix socket + POSIX shm               │
└───────┼─────────────────────────────────────┘
        ▼
┌───────────────────┐
│ cott-vst-worker   │  × N instances
│ format backend     │
│ optional X11 UI   │
└───────────────────┘
```

## Project model (`cott-core`)

### `Project`

Holds tempo/transport, tracks, clips, `AudioGraph`, assets, automation lanes, and opaque plugin state blobs. Runtime-only `root_dir` is skipped by serde (it points at an extracted temporary workspace while editing).

**On-disk format (v2):** a `.ctgdaw` ZIP archive containing `project.json` (pretty JSON) and `assets/`. Plugin nodes persist their format, stable ID, path, and opaque state. Saves write a sibling `*.ctgdaw.tmp` then atomically rename. Autosaves land under the OS data dir as `CottDAW/autosave/autosave-*.ctgdaw`. Legacy directory projects (`project.json` + `assets/`) can still be opened and are converted on the next Save.

### Tracks and clips

- `TrackKind::Midi | Audio` with pointers to source / instrument / gain nodes.
- `ClipContent::Midi { notes }` or `Audio { asset_id, … }`.
- Timebase: `BeatPos` / `SamplePos` via `TempoMap` (constant BPM for MVP).

### Graph (`graph.rs`)

**Node kinds:** `MidiClipSource`, `AudioClipSource`, `GainPan`, `SumMixer`, `MasterOutput`, `PluginInstrument`, `PluginEffect`.

**Rules:**

- Connect output → input only; `PortType` must match.
- Directed cycles → `GraphError::Cycle` (edit rejected; previous plan kept).
- Compile → `CompiledPlan`: topological order, per-node latency, delay-compensation samples (PDC).

### Commands

User mutations go through `CommandStack` (max depth 256) for undo/redo: tracks, clips, notes, mix, graph edges/nodes, tempo, automation.

### Automation

Targets: `NodeGain`, `NodePan`, `PluginParam`. Points are beat + normalized value; gain uses a dB mapping for playback.

## Audio engine

### Live path

1. UI pushes `EngineCommand`s (`SetPlan`, transport, clips, automation, sample cache, preview notes).
2. `audio.rs` opens a cpal stream (PipeWire feature), **F32**, typically **256** frames.
3. `AudioProcessor::process` drains commands, then `dsp::process_block`.
4. Events (`Position`, `Meters`, …) return to the UI.

### `process_block` (summary)

1. Apply plugin-param automation at block start.
2. Walk `CompiledPlan.order`.
3. Sum upstream audio into each node; schedule MIDI for instruments.
4. Built-ins (gain/pan/mute/solo, sum, master) or delegate to `PluginAudioHost`.
5. Apply PDC delay rings; update meters; soft-clip master to ±1.0.

### MIDI

- `clips::schedule_midi_for_block` streams note-on/off across block boundaries.
- Stop / seek / loop wrap → MIDI panic (NoteOff for held notes + all-notes-off style CCs).
- Piano-roll audition uses `PreviewNote` while stopped.

### Offline

`engine::render_offline` uses the same `process_block` path with `TransportState::Playing`. Export and tests share this code.

## Plugin sandboxing

### Launch

```text
cott-vst-worker --shm /cott-daw-{uuid} --sock /tmp/cott-plug-{uuid}.sock
```

Binary resolution: sibling of `cott-daw`, else `target/debug|release/cott-vst-worker`. Socket mode `0600`.

### Protocol (`cott-ipc`)

Length-prefixed **bincode** messages. `PROTOCOL_VERSION = 3`; each descriptor/load request carries `PluginFormat`.

**Host → worker:** `Hello`, `ScanPaths`, `Load`, `Unload`, `SetParam`, `GetParams`, `GetState` / `SetState`, `OpenEditor` / `CloseEditor`, `ProcessNotify`, `OfflineProcess`, `Shutdown`.

**Worker → host:** `HelloAck`, `ScanResult`, `Loaded` / `LoadFailed`, `Params`, `State`, `ProcessDone`, editor status, `Crashed`, `Log`, …

### Shared memory

Layout: `ShmHeader` → MIDI events (`MAX_MIDI_EVENTS = 512`) → planar `f32` in/out (`MAX_CHANNELS = 2`, `MAX_BLOCK_FRAMES = 4096`).

Flags: `REQUEST_PROCESS`, `PROCESS_DONE`, `WORKER_FAILED`, `BYPASS`.

Realtime handshake: host fills MIDI + audio + header → `ProcessNotify` → worker processes → `ProcessDone` → host reads outputs.

### Scan

The disposable worker aggregates VST2, VST3, CLAP, and LV2 catalogs. It honors the standard paths, `VST_PATH`, `VST3_PATH`, `CLAP_PATH`, and Lilv/`LV2_PATH`. Yabridge VST2/VST3/CLAP wrappers are listed from the filesystem without starting Wine. Individual VST3 and CLAP bundles use isolated temp-dir symlink scans at load time.

### Editors

`OpenEditor` with no parent window ID creates a floating X11 shell. The backend receives the appropriate X11 handle for VST2, VST3, CLAP, or LV2 UI embedding. `cott-daw` forces `WINIT_UNIX_BACKEND=x11` for this reason. Generic sliders remain available in the Plugins tab.

### Failure policy

Worker death → instance `failed`; instrument silence / effect bypass; UI offers restart. Plugin state blobs are stored in the project on save for restore.

## UI structure (`cott-daw`)

| Module | Role |
|--------|------|
| `app.rs` | `CottApp` — project, commands, plugins, engine sync, autosave |
| `audio.rs` | cpal stream lifecycle |
| `plugins.rs` | Catalog scan, `PluginHost`, SHM process bridge |
| `ui/mod.rs` | Shell layout and lower tabs |
| `ui/arrangement.rs` | Timeline |
| `ui/piano_roll.rs` | MIDI editor |
| `ui/graph_editor.rs` | Routing canvas |
| `ui/transport.rs` | Top controls |
| `ui/export_dialog.rs` | Export options |
| `ui/shortcuts.rs` | Keyboard bindings |

## Import / export

| Path | Stack |
|------|--------|
| Audio import | symphonia decode → rubato resample → `assets/` + `SampleCache` |
| MIDI import | midly SMF → `MidiNote`s |
| WAV export | offline render → hound 16-bit PCM |
| Opus export | render → (resample 48 kHz) → temp WAV → ffmpeg libopus |
| Gonio MP4 | render → `GonioRenderer` frames → ffmpeg x264 + AAC |

## Design constraints (intentional)

| Choice | Why |
|--------|-----|
| Acyclic graph | Simple scheduling and deterministic offline render |
| One process per plugin | Contain crashes and hangs |
| `.ctgdaw` ZIP archives | Portable single-file projects with bundled assets |
| No feedback | Loops rejected rather than delayed-feedback engines |
| Stereo host path | Matches SHM and MVP mixer |

## Related docs

- [User guide](user-guide.md) — workflows and shortcuts
- [Development](development.md) — build, test, module entry points
