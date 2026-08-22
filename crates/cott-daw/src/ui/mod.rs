//! Ableton-style editor UI.

mod arrangement;
mod export_dialog;
pub mod graph_editor;
mod node_editors;
mod piano_roll;
pub mod scale;
mod shortcuts;
mod transport;

pub use export_dialog::ExportDialogState;

use crate::app::CottApp;
use cott_core::clips::MidiNote;
use cott_core::ids::{ClipId, NodeId, NoteId, PortId, TrackId};
use eframe::egui;
use indexmap::IndexSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LowerTab {
    #[default]
    PianoRoll,
    Graph,
    Automation,
    Plugins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChordKind {
    #[default]
    Major,
    Minor,
    Power,
    Diminished,
    Augmented,
    Suspended2,
    Suspended4,
    Major6,
    Minor6,
    Major7,
    Minor7,
    Dominant7,
    Diminished7,
    HalfDiminished,
    MinorMajor7,
    Dominant7Sus4,
    Add9,
    Major9,
    Minor9,
    Dominant9,
}

impl ChordKind {
    pub const ALL: [Self; 20] = [
        Self::Major,
        Self::Minor,
        Self::Power,
        Self::Diminished,
        Self::Augmented,
        Self::Suspended2,
        Self::Suspended4,
        Self::Major6,
        Self::Minor6,
        Self::Major7,
        Self::Minor7,
        Self::Dominant7,
        Self::Diminished7,
        Self::HalfDiminished,
        Self::MinorMajor7,
        Self::Dominant7Sus4,
        Self::Add9,
        Self::Major9,
        Self::Minor9,
        Self::Dominant9,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Major => "Major",
            Self::Minor => "Minor",
            Self::Power => "Power",
            Self::Diminished => "Diminished",
            Self::Augmented => "Augmented",
            Self::Suspended2 => "Suspended 2",
            Self::Suspended4 => "Suspended 4",
            Self::Major6 => "Major 6",
            Self::Minor6 => "Minor 6",
            Self::Major7 => "Major 7",
            Self::Minor7 => "Minor 7",
            Self::Dominant7 => "Dominant 7",
            Self::Diminished7 => "Diminished 7",
            Self::HalfDiminished => "Half-diminished",
            Self::MinorMajor7 => "Minor-major 7",
            Self::Dominant7Sus4 => "7sus4",
            Self::Add9 => "Add 9",
            Self::Major9 => "Major 9",
            Self::Minor9 => "Minor 9",
            Self::Dominant9 => "Dominant 9",
        }
    }

    /// Chord tones as semitone offsets from the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Self::Major => &[0, 4, 7],
            Self::Minor => &[0, 3, 7],
            Self::Power => &[0, 7],
            Self::Diminished => &[0, 3, 6],
            Self::Augmented => &[0, 4, 8],
            Self::Suspended2 => &[0, 2, 7],
            Self::Suspended4 => &[0, 5, 7],
            Self::Major6 => &[0, 4, 7, 9],
            Self::Minor6 => &[0, 3, 7, 9],
            Self::Major7 => &[0, 4, 7, 11],
            Self::Minor7 => &[0, 3, 7, 10],
            Self::Dominant7 => &[0, 4, 7, 10],
            Self::Diminished7 => &[0, 3, 6, 9],
            Self::HalfDiminished => &[0, 3, 6, 10],
            Self::MinorMajor7 => &[0, 3, 7, 11],
            Self::Dominant7Sus4 => &[0, 5, 7, 10],
            Self::Add9 => &[0, 4, 7, 14],
            Self::Major9 => &[0, 4, 7, 11, 14],
            Self::Minor9 => &[0, 3, 7, 10, 14],
            Self::Dominant9 => &[0, 4, 7, 10, 14],
        }
    }
}

/// In-progress piano-roll note interaction (survives frame-to-frame).
#[derive(Debug, Clone)]
pub enum PianoNoteDrag {
    /// Click-drag empty grid to draw a note.
    Draw {
        clip_id: ClipId,
        pitch: u8,
        origin_beat: f64,
        end_beat: f64,
        velocity: u8,
        chord: Option<ChordKind>,
    },
    /// Drag note body to move pitch/start (moves the whole selection).
    Move {
        clip_id: ClipId,
        /// Note under the pointer — drives grab offset and live pitch/start.
        note_id: NoteId,
        /// Snapshots of every selected note at drag start (includes `note_id`).
        before: Vec<MidiNote>,
        /// Live pitch of the grabbed note.
        pitch: u8,
        /// Live start of the grabbed note.
        start_beats: f64,
        grab_offset_beats: f64,
    },
    /// Drag note right edge to change length.
    Resize {
        clip_id: ClipId,
        note_id: NoteId,
        before: MidiNote,
        start_beats: f64,
        length_beats: f64,
    },
    /// Drag in the velocity lane (or Alt+drag a note) to change velocity.
    Velocity {
        clip_id: ClipId,
        note_id: NoteId,
        /// Snapshots of every selected note at drag start (includes `note_id`).
        before: Vec<MidiNote>,
        /// Live velocity of the grabbed note (1..=127).
        velocity: u8,
    },
    /// Shift-drag empty grid to lasso-select notes.
    SelectLasso {
        clip_id: ClipId,
        origin: egui::Pos2,
        current: egui::Pos2,
    },
}

/// Clipboard payload for piano-roll notes (relative timing preserved).
#[derive(Debug, Clone)]
pub struct NoteClipboard {
    pub notes: Vec<MidiNote>,
    /// Earliest start among copied notes — paste aligns this to the target beat.
    pub anchor_beat: f64,
}

/// Clipboard payload for arrangement clips.
#[derive(Debug, Clone)]
pub struct ClipClipboard {
    pub template: cott_core::clips::Clip,
}

/// In-progress arrangement clip drag (survives frame-to-frame).
#[derive(Debug, Clone)]
pub struct ArrangementClipDrag {
    pub clip_id: ClipId,
    pub track_id: TrackId,
    pub original_start: f64,
    pub original_length: f64,
    pub grab_offset_beats: f64,
    pub current_start: f64,
    pub current_length: f64,
    /// True when dragging the right edge to resize.
    pub resizing: bool,
}

pub struct UiState {
    pub lower_tab: LowerTab,
    pub selected_track: Option<TrackId>,
    pub selected_clip: Option<ClipId>,
    pub selected_node: Option<NodeId>,
    pub beats_per_pixel: f32,
    pub scroll_x: f32,
    /// Piano-roll zoom factor (1.0 = default key/beat size).
    pub piano_zoom: f32,
    /// Last known piano-roll scroll offset (used to anchor zoom under cursor).
    pub piano_scroll_offset: egui::Vec2,
    /// Last known piano-roll viewport rect (screen space).
    pub piano_viewport: egui::Rect,
    /// One-shot scroll offset to apply after a zoom (keeps content anchored).
    pub piano_pending_offset: Option<egui::Vec2>,
    pub show_browser: bool,
    pub plugin_filter: String,
    /// User-chosen height for the lower editor panel. Content must not change this.
    pub lower_panel_height: f32,
    /// Routing canvas drag — kept in app state so it survives frame-to-frame id churn.
    pub graph_drag_node: Option<NodeId>,
    pub graph_connect_from: Option<(NodeId, PortId)>,
    /// Camera offset for the routing canvas (infinite pan).
    pub graph_pan: egui::Vec2,
    /// Camera zoom for the routing canvas (1.0 = 100%).
    pub graph_zoom: f32,
    /// Previous canvas top-left; used to keep nodes stable when the panel resizes.
    pub graph_canvas_origin: Option<egui::Pos2>,
    /// True while dragging empty canvas space to pan.
    pub graph_panning: bool,
    pub piano_drag: Option<PianoNoteDrag>,
    /// Last pitch auditioned from the piano roll (avoid retrigger spam).
    pub piano_preview_pitch: Option<u8>,
    /// Velocity used for newly drawn notes (updated whenever velocity is edited).
    pub draw_velocity: u8,
    /// When enabled, drawing one note stamps the selected chord.
    pub chord_stamp_enabled: bool,
    pub chord_kind: ChordKind,
    /// Selected note IDs for the currently edited MIDI clip.
    pub selected_notes: Vec<NoteId>,
    /// Clip that owns `selected_notes` (cleared when the active clip changes).
    pub selected_notes_clip: Option<ClipId>,
    pub note_clipboard: Option<NoteClipboard>,
    pub clip_clipboard: Option<ClipClipboard>,
    /// When true, seed the OS clipboard this frame so egui emits Paste on Ctrl+V.
    pub seed_os_clipboard: bool,
    /// Last hovered clip-local beat in the piano roll (for paste anchoring).
    pub piano_hover_beat: Option<f64>,
    /// Last hovered arrangement beat / track (for clip paste anchoring).
    pub arrangement_hover_beat: Option<f64>,
    pub arrangement_hover_track: Option<TrackId>,
    pub clip_drag: Option<ArrangementClipDrag>,
    /// Track being renamed inline, plus the in-progress edit buffer.
    pub renaming_track: Option<(TrackId, String)>,
    /// Clip being renamed inline from the piano-roll toolbar.
    pub renaming_clip: Option<(ClipId, String)>,
    /// Export settings window (path chosen after confirm).
    pub show_export_dialog: bool,
    pub export_dialog: ExportDialogState,
    /// Floating editors for built-in gain / mixer / splitter / synth nodes.
    pub open_node_editors: IndexSet<NodeId>,
}

impl UiState {
    /// Scroll Y that puts middle C (MIDI 60) near the top of the piano roll.
    fn piano_default_scroll_y() -> f32 {
        const KEY_H: f32 = 14.0; // matches piano_roll::BASE_KEY_H
        const MIDDLE_C: u8 = 60;
        let row = 127i32 - MIDDLE_C as i32; // top row is MIDI 127
        (row as f32 * KEY_H - 40.0).max(0.0)
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            lower_tab: LowerTab::PianoRoll,
            selected_track: None,
            selected_clip: None,
            selected_node: None,
            beats_per_pixel: 0.02,
            scroll_x: 0.0,
            piano_zoom: 1.0,
            // Full MIDI roll is tall; open near middle C (MIDI 60) instead of G9.
            piano_scroll_offset: egui::vec2(0.0, Self::piano_default_scroll_y()),
            piano_viewport: egui::Rect::ZERO,
            piano_pending_offset: Some(egui::vec2(0.0, Self::piano_default_scroll_y())),
            show_browser: true,
            plugin_filter: String::new(),
            lower_panel_height: 280.0,
            graph_drag_node: None,
            graph_connect_from: None,
            graph_pan: egui::Vec2::ZERO,
            graph_zoom: 1.0,
            graph_canvas_origin: None,
            graph_panning: false,
            piano_drag: None,
            piano_preview_pitch: None,
            draw_velocity: 100,
            chord_stamp_enabled: false,
            chord_kind: ChordKind::default(),
            selected_notes: Vec::new(),
            selected_notes_clip: None,
            note_clipboard: None,
            clip_clipboard: None,
            seed_os_clipboard: false,
            piano_hover_beat: None,
            arrangement_hover_beat: None,
            arrangement_hover_track: None,
            clip_drag: None,
            renaming_track: None,
            renaming_clip: None,
            show_export_dialog: false,
            export_dialog: ExportDialogState::default(),
            open_node_editors: IndexSet::new(),
        }
    }
}

pub fn draw(app: &mut CottApp, ctx: &egui::Context) {
    shortcuts::handle(app, ctx);
    if app.ui.seed_os_clipboard {
        app.ui.seed_os_clipboard = false;
        // Seed the OS clipboard so egui emits Event::Paste on Ctrl+V even when
        // our real payload lives in app.ui.{note,clip}_clipboard.
        ctx.copy_text("cottdaw".into());
    }
    transport::draw_top_bar(app, ctx);
    export_dialog::draw(app, ctx);

    // Outermost bottom panel first so the status bar stays pinned to the screen edge.
    egui::TopBottomPanel::bottom("status")
        .exact_height(22.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&app.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let pos = app.playhead_beats();
                    let (bar, beat) = app.project.tempo.bar_beat_from_beats(pos);
                    ui.label(format!(
                        "{}:{:.2}  ({}/{})",
                        bar, beat, app.project.tempo.beats_per_bar, app.project.tempo.beat_unit
                    ));
                    if let Some(audio) = &app.audio {
                        ui.label(format!("{} Hz", audio.sample_rate));
                    }
                });
            });
        });

    draw_lower_panel(app, ctx);

    if app.ui.show_browser {
        egui::SidePanel::left("browser")
            .default_width(240.0)
            .show(ctx, |ui| {
                draw_browser(app, ui);
            });
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        arrangement::draw(app, ui);
    });

    // After panels so floating editors sit above the routing canvas for hit-testing.
    node_editors::draw(app, ctx);
}

fn draw_lower_panel(app: &mut CottApp, ctx: &egui::Context) {
    const PANEL_ID: &str = "lower";
    const MIN_HEIGHT: f32 = 120.0;

    let max_height = (ctx.available_rect().height() * 0.85).max(MIN_HEIGHT);
    app.ui.lower_panel_height = app.ui.lower_panel_height.clamp(MIN_HEIGHT, max_height);

    // Fixed height — no egui edge-resize overlay (it steals canvas drags).
    // A dedicated grip in the tab row handles resizing instead.
    let height = app.ui.lower_panel_height;
    egui::TopBottomPanel::bottom(PANEL_ID)
        .exact_height(height)
        .show(ctx, |ui| {
            let grip =
                ui.allocate_response(egui::vec2(ui.available_width(), 8.0), egui::Sense::drag());
            let grip_rect = grip.rect;
            ui.painter().rect_filled(
                grip_rect,
                0.0,
                if grip.dragged() || grip.hovered() {
                    egui::Color32::from_rgb(70, 78, 92)
                } else {
                    egui::Color32::from_rgb(42, 46, 54)
                },
            );
            ui.painter().hline(
                grip_rect.x_range().shrink(grip_rect.width() * 0.35),
                grip_rect.center().y,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(140, 148, 160)),
            );
            if grip.dragged() {
                if let Some(pos) = grip.interact_pointer_pos() {
                    let bottom = ctx.screen_rect().bottom() - 22.0; // leave status bar
                    app.ui.lower_panel_height = (bottom - pos.y).clamp(MIN_HEIGHT, max_height);
                }
            }
            grip.on_hover_cursor(egui::CursorIcon::ResizeVertical);

            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.ui.lower_tab, LowerTab::PianoRoll, "Piano Roll");
                ui.selectable_value(&mut app.ui.lower_tab, LowerTab::Graph, "Routing");
                ui.selectable_value(&mut app.ui.lower_tab, LowerTab::Automation, "Automation");
                ui.selectable_value(&mut app.ui.lower_tab, LowerTab::Plugins, "Plugins");
            });
            ui.separator();
            let body_size = ui.available_size();
            ui.allocate_ui_with_layout(body_size, egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_min_size(body_size);
                ui.set_max_size(body_size);
                ui.set_clip_rect(ui.max_rect());
                match app.ui.lower_tab {
                    LowerTab::PianoRoll => piano_roll::draw(app, ui),
                    LowerTab::Graph => graph_editor::draw(app, ui),
                    LowerTab::Automation => draw_automation(app, ui),
                    LowerTab::Plugins => draw_plugin_inspector(app, ui),
                }
            });
        });
}

fn draw_browser(app: &mut CottApp, ui: &mut egui::Ui) {
    ui.heading("Browser");
    ui.horizontal(|ui| {
        let scanning = app.is_scanning_plugins();
        if ui
            .add_enabled(
                !scanning,
                egui::Button::new(if scanning {
                    "Scanning…"
                } else {
                    "Rescan VSTs"
                }),
            )
            .clicked()
        {
            app.start_plugin_scan();
        }
    });
    ui.text_edit_singleline(&mut app.ui.plugin_filter)
        .on_hover_text("Filter VSTs");
    ui.weak("Click a plugin to load it, or right-click the routing canvas.");
    ui.weak("CottSynth, CottFilter, and CottWhistle are always listed (built-in VST3s).");
    if app.is_scanning_plugins() {
        ui.weak("Scanning… (filesystem only; Wine starts when you load a plugin)");
    }
    ui.separator();
    ui.label("Instruments / Effects");
    egui::ScrollArea::vertical().show(ui, |ui| {
        let filter = app.ui.plugin_filter.to_lowercase();
        let (catalog, catalog_is_empty): (Vec<_>, bool) =
            if let Some(host) = app.plugin_host.try_lock() {
                let is_empty = host.catalog.is_empty();
                let filtered = host
                    .catalog
                    .iter()
                    .filter(|p| {
                        filter.is_empty()
                            || p.name.to_lowercase().contains(&filter)
                            || p.vendor.to_lowercase().contains(&filter)
                    })
                    .cloned()
                    .collect();
                (filtered, is_empty)
            } else {
                // Audio processing owns the host; retry on the next UI frame.
                (Vec::new(), false)
            };
        if catalog.is_empty() && app.is_scanning_plugins() {
            ui.weak("Building plugin list…");
        }
        for plugin in catalog {
            let category = match (plugin.is_instrument, plugin.is_effect) {
                (true, true) => "Instrument / Effect",
                (true, false) => "Instrument",
                _ => "Effect",
            };
            let label = format!(
                "{} [{} · {}] — {}",
                plugin.name, plugin.format, category, plugin.vendor,
            );
            if ui
                .button(label)
                .on_hover_text(format!(
                    "{}\nClick to load on the selected track / graph",
                    plugin.path.display()
                ))
                .clicked()
            {
                if plugin.is_instrument {
                    app.load_instrument_on_selected_track(
                        plugin.format,
                        plugin.uid,
                        plugin.path,
                        plugin.name,
                    );
                } else {
                    app.load_effect(
                        plugin.format,
                        plugin.uid,
                        plugin.path,
                        plugin.name,
                        [280.0, 120.0],
                    );
                }
            }
        }
        if catalog_is_empty && !app.is_scanning_plugins() {
            ui.weak("No third-party plugins found (built-ins should still appear above)");
        }
    });
}

fn draw_automation(app: &mut CottApp, ui: &mut egui::Ui) {
    ui.label("Automation lanes");
    if ui.button("Add Gain Lane for selected gain node").clicked() {
        if let Some(node_id) = app.ui.selected_node {
            use cott_core::automation::{AutomationLane, AutomationTarget};
            let lane = AutomationLane::new(AutomationTarget::NodeGain { node_id });
            app.project.automation.push(lane);
            app.sync_engine();
        }
    }
    let lanes: Vec<_> = app
        .project
        .automation
        .iter()
        .map(|l| (l.id, format!("{:?}", l.target), l.points.len()))
        .collect();
    let mut add_point: Option<cott_core::ids::AutomationLaneId> = None;
    for (id, target, count) in lanes {
        ui.horizontal(|ui| {
            ui.label(target);
            ui.label(format!("{count} points"));
            if ui.button("Add point @ playhead (0.5)").clicked() {
                add_point = Some(id);
            }
        });
    }
    if let Some(id) = add_point {
        let beat = app.playhead_beats();
        if let Some(l) = app.project.automation.iter_mut().find(|l| l.id == id) {
            l.add_point(beat, 0.5);
            app.sync_engine();
        }
    }
}

fn draw_plugin_inspector(app: &mut CottApp, ui: &mut egui::Ui) {
    let Some(node_id) = app.ui.selected_node else {
        ui.weak("Select a node in the routing graph");
        return;
    };
    let Some(node) = app.project.graph.nodes.get(&node_id).cloned() else {
        return;
    };
    ui.heading(&node.name);
    let instance = match &node.kind {
        cott_core::graph::NodeKind::PluginInstrument {
            instance_id,
            failed,
            ..
        }
        | cott_core::graph::NodeKind::PluginEffect {
            instance_id,
            failed,
            ..
        } => {
            if *failed {
                ui.colored_label(egui::Color32::RED, "Plugin failed — transport continues");
                if ui.button("Restart").clicked() {
                    let sr = app.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48_000) as f64;
                    let bs = app.audio.as_ref().map(|a| a.buffer_size).unwrap_or(256);
                    let state = app
                        .project
                        .plugin_states
                        .get(instance_id)
                        .map(|b| b.data.clone());
                    match app
                        .plugin_host
                        .lock()
                        .restart_failed(*instance_id, sr, bs, state)
                    {
                        Ok(()) => app.status = "Plugin restarted".into(),
                        Err(e) => app.status = format!("Restart failed: {e}"),
                    }
                }
            }
            Some(*instance_id)
        }
        cott_core::graph::NodeKind::BuiltinSynth { params } => {
            if ui.button("Open editor window").clicked() {
                node_editors::open_editor(app, node_id);
            }
            ui.separator();
            draw_builtin_synth_inspector(app, ui, node_id, *params);
            return;
        }
        cott_core::graph::NodeKind::GainPan { .. }
        | cott_core::graph::NodeKind::SumMixer { .. }
        | cott_core::graph::NodeKind::StereoSplitter { .. } => {
            ui.label("Built-in node — use the floating editor (double-click in Routing).");
            if ui.button("Open editor").clicked() {
                node_editors::open_editor(app, node_id);
            }
            return;
        }
        _ => None,
    };

    let Some(instance_id) = instance else {
        ui.weak("Not a plugin node");
        return;
    };

    if ui.button("Open Native Editor").clicked() {
        // Worker creates a floating X11 parent when none is supplied.
        match app.plugin_host.lock().open_editor(instance_id, None) {
            Ok(()) => app.status = "Editor opened".into(),
            Err(e) => app.status = format!("Editor: {e} (generic params below)"),
        }
    }
    if app.can_remove_graph_node(node_id) {
        if ui
            .button("Delete plugin")
            .on_hover_text("Remove this effect/instrument from the project (Delete)")
            .clicked()
        {
            app.remove_graph_node(node_id);
            return;
        }
    }

    ui.separator();
    ui.label("Generic parameters");
    let params = app
        .plugin_host
        .try_lock()
        .and_then(|host| {
            host.instances.get(&instance_id).map(|i| {
                i.params
                    .iter()
                    .map(|p| {
                        let v = i.param_values.get(&p.id).copied().unwrap_or(p.default);
                        (p.id, p.name.clone(), p.min, p.max, v)
                    })
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();

    for (id, name, min, max, mut value) in params {
        if ui
            .add(egui::Slider::new(&mut value, min..=max).text(name))
            .changed()
        {
            app.plugin_host.lock().set_param(instance_id, id, value);
        }
    }
}

pub(crate) fn draw_builtin_synth_inspector(
    app: &mut CottApp,
    ui: &mut egui::Ui,
    node_id: NodeId,
    mut params: cott_core::SynthParams,
) {
    ui.label(format!(
        "Built-in CottSynth · {} voices",
        cott_core::MAX_VOICES
    ));
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Waveform");
        egui::ComboBox::from_id_salt("cott_synth_wave")
            .selected_text(params.waveform.label())
            .show_ui(ui, |ui| {
                for wave in cott_core::Waveform::ALL {
                    ui.selectable_value(&mut params.waveform, wave, wave.label());
                }
            });
    });

    let mut changed = false;
    if ui
        .add(egui::Slider::new(&mut params.adsr.attack_ms, 0.0..=2000.0).text("Attack ms"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut params.adsr.decay_ms, 0.0..=2000.0).text("Decay ms"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut params.adsr.sustain, 0.0..=1.0).text("Sustain"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut params.adsr.release_ms, 0.0..=5000.0).text("Release ms"))
        .changed()
    {
        changed = true;
    }
    if matches!(params.waveform, cott_core::Waveform::Pulse)
        && ui
            .add(egui::Slider::new(&mut params.pulse_width, 0.05..=0.95).text("Pulse width"))
            .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut params.gain, 0.0..=1.0).text("Gain"))
        .changed()
    {
        changed = true;
    }

    // ComboBox doesn't report .changed() on the outer slider path — compare against graph.
    if let Some(node) = app.project.graph.nodes.get(&node_id)
        && let cott_core::graph::NodeKind::BuiltinSynth { params: old } = &node.kind
        && *old != params
    {
        changed = true;
    }

    if changed
        && let Some(node) = app.project.graph.nodes.get_mut(&node_id)
        && let cott_core::graph::NodeKind::BuiltinSynth { params: slot } = &mut node.kind
    {
        *slot = params.clamped();
        app.project.touch();
        app.sync_engine();
    }
}
