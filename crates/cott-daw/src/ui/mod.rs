//! Ableton-style editor UI.

mod arrangement;
mod export_dialog;
mod graph_editor;
mod piano_roll;
mod shortcuts;
mod transport;

pub use export_dialog::ExportDialogState;

use crate::app::CottApp;
use cott_core::clips::MidiNote;
use cott_core::ids::{ClipId, NodeId, NoteId, PortId, TrackId};
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LowerTab {
    #[default]
    PianoRoll,
    Graph,
    Automation,
    Plugins,
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
    },
    /// Drag note body to move pitch/start.
    Move {
        clip_id: ClipId,
        note_id: NoteId,
        before: MidiNote,
        pitch: u8,
        start_beats: f64,
        length_beats: f64,
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
    pub piano_scroll: f32,
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
    pub clip_drag: Option<ArrangementClipDrag>,
    /// Export settings window (path chosen after confirm).
    pub show_export_dialog: bool,
    pub export_dialog: ExportDialogState,
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
            piano_scroll: 48.0,
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
            clip_drag: None,
            show_export_dialog: false,
            export_dialog: ExportDialogState::default(),
        }
    }
}

pub fn draw(app: &mut CottApp, ctx: &egui::Context) {
    shortcuts::handle(app, ctx);
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
                        bar,
                        beat,
                        app.project.tempo.beats_per_bar,
                        app.project.tempo.beat_unit
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
            .add_enabled(!scanning, egui::Button::new(if scanning {
                "Scanning…"
            } else {
                "Rescan VSTs"
            }))
            .clicked()
        {
            app.start_plugin_scan();
        }
    });
    ui.text_edit_singleline(&mut app.ui.plugin_filter)
        .on_hover_text("Filter VSTs");
    ui.weak("Click a plugin to load it, or right-click the routing canvas.");
    if app.is_scanning_plugins() {
        ui.weak("Scanning… (filesystem only; Wine starts when you load a plugin)");
    }
    ui.separator();
    ui.label("Instruments / Effects");
    egui::ScrollArea::vertical().show(ui, |ui| {
        let filter = app.ui.plugin_filter.to_lowercase();
        let catalog: Vec<_> = app
            .plugin_host
            .lock()
            .catalog
            .iter()
            .filter(|p| {
                filter.is_empty()
                    || p.name.to_lowercase().contains(&filter)
                    || p.vendor.to_lowercase().contains(&filter)
            })
            .cloned()
            .collect();
        if catalog.is_empty() && app.is_scanning_plugins() {
            ui.weak("Building plugin list…");
        }
        for plugin in catalog {
            let label = format!(
                "{} [{}] — {}",
                plugin.name,
                if plugin.is_instrument {
                    "Instrument"
                } else {
                    "Effect"
                },
                plugin.vendor,
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
                        plugin.uid,
                        plugin.path,
                        plugin.name,
                        [200.0, 120.0],
                    );
                } else {
                    app.load_effect(plugin.uid, plugin.path, plugin.name, [280.0, 120.0]);
                }
            }
        }
        if app.plugin_host.lock().catalog.is_empty() && !app.is_scanning_plugins() {
            ui.weak("No plugins found. Install VST3s in ~/.vst3");
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
        ui.weak("Select a plugin node in the routing graph");
        return;
    };
    let Some(node) = app.project.graph.nodes.get(&node_id).cloned() else {
        return;
    };
    ui.heading(&node.name);
    let instance = match &node.kind {
        cott_core::graph::NodeKind::Vst3Instrument {
            instance_id,
            failed,
            ..
        }
        | cott_core::graph::NodeKind::Vst3Effect {
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
        cott_core::graph::NodeKind::GainPan {
            gain_db, pan, mute, ..
        } => {
            let mut g = *gain_db;
            let mut p = *pan;
            let mut m = *mute;
            if ui
                .add(egui::Slider::new(&mut g, -60.0..=12.0).text("Gain dB"))
                .changed()
            {
                app.set_gain(node_id, g);
            }
            if ui
                .add(egui::Slider::new(&mut p, -1.0..=1.0).text("Pan"))
                .changed()
            {
                let old = (*gain_db, *pan, *mute);
                app.commands.push(
                    &mut app.project,
                    cott_core::commands::Command::SetGainPan {
                        node_id,
                        old_gain: old.0,
                        new_gain: old.0,
                        old_pan: old.1,
                        new_pan: p,
                        old_mute: old.2,
                        new_mute: old.2,
                    },
                );
                app.sync_engine();
            }
            if ui.checkbox(&mut m, "Mute").changed() {
                app.commands.push(
                    &mut app.project,
                    cott_core::commands::Command::SetGainPan {
                        node_id,
                        old_gain: *gain_db,
                        new_gain: *gain_db,
                        old_pan: *pan,
                        new_pan: *pan,
                        old_mute: *mute,
                        new_mute: m,
                    },
                );
                app.sync_engine();
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
        .lock()
        .instances
        .get(&instance_id)
        .map(|i| {
            i.params
                .iter()
                .map(|p| {
                    let v = i.param_values.get(&p.id).copied().unwrap_or(p.default);
                    (p.id, p.name.clone(), p.min, p.max, v)
                })
                .collect::<Vec<_>>()
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
