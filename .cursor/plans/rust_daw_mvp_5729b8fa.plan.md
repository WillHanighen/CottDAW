---
name: Rust DAW MVP
overview: Build a Linux-first, Ableton-style Rust DAW with an editable acyclic audio/MIDI dataflow graph, arrangement and piano-roll editing, sandboxed VST3 instruments/effects, automation, undo/redo, project persistence, and Ogg Opus export. The implementation will be staged so the audio engine and crash boundaries are proven before the full editor is layered on top.
todos:
  - id: engine-foundation
    content: Create the workspace, project model, typed DAG, graph compiler, realtime-safe engine bridge, and core DSP tests.
    status: completed
  - id: playback-clips
    content: Add native PipeWire playback, transport/tempo, MIDI and audio clip scheduling, imports, mixer nodes, meters, and latency compensation.
    status: completed
  - id: vst-sandbox
    content: Implement per-plugin VST3 worker processes, scanning, shared-memory processing, state, crash recovery, and test doubles.
    status: completed
  - id: editor-ui
    content: Build the Ableton-style arrangement, track mixer, piano roll, authoritative dataflow editor, and plugin browser.
    status: completed
  - id: editing-persistence
    content: Add automation, reversible commands, versioned project save/load, asset handling, autosave, and recovery.
    status: completed
  - id: plugin-ui-export
    content: Add X11 native plugin editor hosting with generic fallback and deterministic Ogg Opus offline export.
    status: completed
  - id: verification-docs
    content: Run unit/integration/smoke verification and document Arch Linux setup, limitations, and workflows.
    status: completed
isProject: false
---

# CottDAW Linux MVP

## Product shape
- Use an Ableton-style arrangement: track controls and clips in the main timeline, with a lower panel switching among piano roll, editable routing graph, automation, and plugin controls.
- Make the graph authoritative, not decorative. Tracks create sensible default nodes, while users can reconnect, split, and merge audio/MIDI paths; invalid socket types and cycles are rejected.
- Support MIDI clips edited in a piano roll, Standard MIDI File import, audio clip import, VST3 instruments/effects, mixer controls, buses/sends through graph routing, transport/looping, parameter automation, undo/redo, and offline `.opus`/Ogg export.
- Exclude live audio recording and live MIDI keyboard recording from this milestone, since they were not selected.

## Architecture and dependencies
- Turn [Cargo.toml](/home/cottage-end/projects/CottDAW/Cargo.toml) into a workspace with a main GUI binary plus a private VST3 worker binary.
- Use `eframe`/`egui` for the desktop UI and `egui_graph` for the editable node canvas, forcing the X11 backend so native Linux VST3 editors can use X11/XWayland.
- Use current `cpal` with its native PipeWire and realtime features for output, a lock-free command/event bridge, preallocated planar `f32` buffers, and no allocation, blocking locks, filesystem access, or UI work on the audio callback.
- Use a typed project model (`serde`), stable IDs, an acyclic graph with topological compilation, latency propagation/compensation, tempo-map conversion, and a command stack shared by editing and undo/redo.
- Use `symphonia` for common audio imports, `rubato` for non-realtime resampling/cache preparation, `midly` for `.mid` import, and `libopusenc` for Ogg Opus export.
- Build VST3 hosting around `truce-rack-vst3`/`vst3-rs`, keeping the app-facing plugin protocol independent from the binding so the implementation can be replaced without changing the engine.

## Core model and audio engine
- Add modules under [src/](/home/cottage-end/projects/CottDAW/src) for project state, tempo/transport, clips, automation, graph editing/validation, graph compilation, DSP nodes, engine messaging, persistence, import, export, and UI state.
- Model MIDI and audio as distinct typed ports. Initial node kinds: MIDI clip source, audio clip source, VST3 instrument, VST3 effect, gain/pan, summing mixer, and hardware/master output.
- Compile every accepted graph edit off the realtime thread into an immutable processing plan, then atomically swap it at an audio-block boundary. Preserve the last valid plan when an edit is rejected.
- Implement clip scheduling, looping, note-on/off correctness, per-block transport context for tempo-synced plugins, gain/pan/mute/solo, stereo mixing, meters, plugin latency compensation, and automation interpolation.

## Sandboxed VST3 host
- Add a `cott-vst-worker` executable and an IPC crate/module using Unix sockets for lifecycle/state/control plus shared-memory audio/event ring buffers and `eventfd` wakeups. Run each active plugin instance in its own worker process.
- Scan standard Linux VST3 locations in disposable workers, cache metadata, blacklist timed-out/crashing binaries, and never load plugin code in the GUI process.
- Support instruments and effects, MIDI events, parameter discovery/changes, transport context, bus negotiation, plugin state save/restore, latency changes, and offline processing.
- Host native editors from the worker through X11/XEmbed/reparenting where supported; provide searchable generic parameter controls when a plugin has no editor or embedding fails.
- On worker failure, keep the DAW and transport alive: silence a failed instrument, dry-bypass a failed effect, mark the node failed, and offer restart with the last saved plugin state.

## Editor and workflows
- Replace [src/main.rs](/home/cottage-end/projects/CottDAW/src/main.rs) with application startup, recovery handling, X11 selection, and the main editor shell.
- Implement transport and tempo controls, arrangement zoom/scroll/selection, track add/remove/reorder, MIDI/audio clip placement and resizing, piano-roll note creation/move/resize/velocity, drag/drop imports, mixer controls/meters, and lower-panel tabs.
- Bind graph operations directly to project routing. Newly created instrument tracks default to `MIDI clip -> VST3 instrument -> gain/pan -> master`; audio tracks default to `audio clip -> gain/pan -> master`.
- Add automation lanes that target mixer or VST3 parameter IDs, with point editing and deterministic playback. Route all user mutations through reversible commands for undo/redo.

## Projects and rendering
- Save projects as a versioned human-readable manifest plus a project asset directory and opaque plugin-state blobs. Store relative media references, graph layout, clips, automation, tempo, mixer state, plugin identifiers, and state; add migration hooks from version one.
- Add periodic crash-safe autosaves using temporary-file plus atomic rename, startup recovery prompts, and explicit missing-media/missing-plugin placeholders.
- Implement deterministic faster-than-realtime rendering through the same compiled graph, resample the final stereo stream to 48 kHz when needed, and encode Ogg Opus with selectable bitrate.

## Verification
- Add unit tests for tempo/sample conversion, MIDI import, note scheduling, graph type/cycle rejection, topological order, fan-out/mixing, latency compensation, automation interpolation, serialization migrations, and undo/redo round trips.
- Add integration tests with a controllable fake plugin worker for IPC, timeout/crash recovery, state restoration, and offline rendering; keep real third-party plugins out of automated tests.
- Add a small end-to-end smoke project and verify PipeWire playback, MIDI-to-synth routing, parallel effects, project reopen, native/generic plugin UI fallback, worker crash containment, and valid Ogg Opus export.
- Configure formatting, strict Clippy checks, and CI-friendly tests; document Arch Linux runtime/build packages and the X11/XWayland requirement in a new README.

## Delivery order
1. Workspace, project model, graph validation/compiler, and headless DSP tests.
2. PipeWire output, transport, clip scheduling, and built-in mixer nodes.
3. Per-plugin VST3 worker protocol, scanner, processing, state, and crash recovery.
4. Arrangement, mixer, piano roll, authoritative graph editor, imports, and persistence.
5. Native/generic plugin UI, automation, undo/redo, autosave/recovery, Opus export, and end-to-end hardening.