//! Floating egui editors for built-in routing nodes (gain, mixer, splitter, synth).

use crate::app::CottApp;
use cott_core::graph::{MixerStrip, NodeKind, SplitterBranch, MIXER_STRIP_COUNT};
use cott_core::ids::NodeId;
use eframe::egui::{self, RichText};

pub fn draw(app: &mut CottApp, ctx: &egui::Context) {
    let open: Vec<NodeId> = app.ui.open_node_editors.iter().copied().collect();
    for node_id in open {
        let Some(node) = app.project.graph.nodes.get(&node_id).cloned() else {
            app.ui.open_node_editors.shift_remove(&node_id);
            continue;
        };
        if !node.kind.has_builtin_editor() {
            app.ui.open_node_editors.shift_remove(&node_id);
            continue;
        }

        let mut open = true;
        let title = match &node.kind {
            NodeKind::GainPan { .. } => format!("Gain — {}", node.name),
            NodeKind::SumMixer { .. } => format!("Mixer — {}", node.name),
            NodeKind::StereoSplitter { .. } => format!("Splitter — {}", node.name),
            NodeKind::BuiltinSynth { .. } => format!("CottSynth — {}", node.name),
            _ => node.name.clone(),
        };

        egui::Window::new(title)
            .id(egui::Id::new(("cott_node_editor", node_id)))
            .open(&mut open)
            .default_width(320.0)
            .resizable(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| match &node.kind {
                NodeKind::GainPan {
                    gain_db,
                    pan,
                    mute,
                    solo,
                } => draw_gain_editor(app, ui, node_id, *gain_db, *pan, *mute, *solo),
                NodeKind::SumMixer {
                    strips,
                    master_gain_db,
                    master_pan,
                    mute,
                } => draw_mixer_editor(
                    app,
                    ui,
                    node_id,
                    *strips,
                    *master_gain_db,
                    *master_pan,
                    *mute,
                ),
                NodeKind::StereoSplitter { a, b } => {
                    draw_splitter_editor(app, ui, node_id, *a, *b)
                }
                NodeKind::BuiltinSynth { params } => {
                    super::draw_builtin_synth_inspector(app, ui, node_id, *params)
                }
                _ => {}
            });

        if !open {
            app.ui.open_node_editors.shift_remove(&node_id);
        }
    }
}

pub fn open_editor(app: &mut CottApp, node_id: NodeId) {
    let Some(node) = app.project.graph.nodes.get(&node_id) else {
        app.status = "No node selected".into();
        return;
    };
    if node.kind.has_builtin_editor() {
        app.ui.open_node_editors.insert(node_id);
        app.ui.selected_node = Some(node_id);
        app.status = format!("Opened editor for {}", node.name);
        return;
    }
    if node.kind.is_midi_router() {
        app.ui.selected_node = Some(node_id);
        app.status = format!("{} — MIDI router (no parameters)", node.name);
        return;
    }
    // Fall through to native plugin editor for VST/CLAP/LV2.
    app.open_plugin_editor_for_node(node_id);
}

fn draw_gain_editor(
    app: &mut CottApp,
    ui: &mut egui::Ui,
    node_id: NodeId,
    gain_db: f32,
    pan: f32,
    mute: bool,
    solo: bool,
) {
    ui.label(RichText::new("Stereo gain / pan").strong());
    ui.separator();

    let mut g = gain_db;
    let mut p = pan;
    let mut m = mute;
    let mut s = solo;

    let mut changed = false;
    if ui
        .add(egui::Slider::new(&mut g, -60.0..=12.0).text("Gain dB"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut p, -1.0..=1.0).text("Pan"))
        .changed()
    {
        changed = true;
    }
    if ui.checkbox(&mut m, "Mute").changed() {
        changed = true;
    }
    if ui.checkbox(&mut s, "Solo").changed() {
        changed = true;
    }

    if changed {
        apply_gain_pan(app, node_id, g, p, m, s);
    }
}

fn draw_mixer_editor(
    app: &mut CottApp,
    ui: &mut egui::Ui,
    node_id: NodeId,
    strips: [MixerStrip; MIXER_STRIP_COUNT],
    master_gain_db: f32,
    master_pan: f32,
    mute: bool,
) {
    ui.label(RichText::new("Bus mixer — 4 stereo inputs").strong());
    ui.label("Each strip has its own gain and pan.");
    ui.separator();

    let mut strips = strips;
    let mut master_gain_db = master_gain_db;
    let mut master_pan = master_pan;
    let mut mute = mute;
    let mut changed = false;

    for (i, strip) in strips.iter_mut().enumerate() {
        ui.label(RichText::new(format!("In {}", i + 1)).strong());
        if ui
            .add(egui::Slider::new(&mut strip.gain_db, -60.0..=12.0).text("Gain dB"))
            .changed()
        {
            changed = true;
        }
        if ui
            .add(egui::Slider::new(&mut strip.pan, -1.0..=1.0).text("Pan"))
            .changed()
        {
            changed = true;
        }
        if ui.checkbox(&mut strip.mute, "Mute").changed() {
            changed = true;
        }
        ui.add_space(6.0);
    }

    ui.separator();
    ui.label(RichText::new("Master").strong());
    if ui
        .add(egui::Slider::new(&mut master_gain_db, -60.0..=12.0).text("Gain dB"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut master_pan, -1.0..=1.0).text("Pan"))
        .changed()
    {
        changed = true;
    }
    if ui.checkbox(&mut mute, "Mute bus").changed() {
        changed = true;
    }

    if changed
        && let Some(node) = app.project.graph.nodes.get_mut(&node_id)
        && let NodeKind::SumMixer {
            strips: slot_strips,
            master_gain_db: slot_gain,
            master_pan: slot_pan,
            mute: slot_mute,
        } = &mut node.kind
    {
        *slot_strips = strips.map(MixerStrip::clamped);
        *slot_gain = master_gain_db.clamp(-60.0, 12.0);
        *slot_pan = master_pan.clamp(-1.0, 1.0);
        *slot_mute = mute;
        app.project.touch();
        app.sync_engine();
    }
}

fn draw_splitter_editor(
    app: &mut CottApp,
    ui: &mut egui::Ui,
    node_id: NodeId,
    a: SplitterBranch,
    b: SplitterBranch,
) {
    ui.label(RichText::new("Stereo splitter — A / B outs").strong());
    ui.label("Same input is sent to both branches; each has its own gain and pan.");
    ui.separator();

    let mut a = a;
    let mut b = b;
    let mut changed = false;

    ui.label(RichText::new("Branch A").strong());
    if ui
        .add(egui::Slider::new(&mut a.gain_db, -60.0..=12.0).text("Gain dB"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut a.pan, -1.0..=1.0).text("Pan"))
        .changed()
    {
        changed = true;
    }

    ui.add_space(8.0);
    ui.label(RichText::new("Branch B").strong());
    if ui
        .add(egui::Slider::new(&mut b.gain_db, -60.0..=12.0).text("Gain dB"))
        .changed()
    {
        changed = true;
    }
    if ui
        .add(egui::Slider::new(&mut b.pan, -1.0..=1.0).text("Pan"))
        .changed()
    {
        changed = true;
    }

    if changed
        && let Some(node) = app.project.graph.nodes.get_mut(&node_id)
        && let NodeKind::StereoSplitter {
            a: slot_a,
            b: slot_b,
        } = &mut node.kind
    {
        *slot_a = a.clamped();
        *slot_b = b.clamped();
        app.project.touch();
        app.sync_engine();
    }
}

fn apply_gain_pan(
    app: &mut CottApp,
    node_id: NodeId,
    gain_db: f32,
    pan: f32,
    mute: bool,
    solo: bool,
) {
    let Some(node) = app.project.graph.nodes.get_mut(&node_id) else {
        return;
    };
    let NodeKind::GainPan {
        gain_db: g,
        pan: p,
        mute: m,
        solo: s,
    } = &mut node.kind
    else {
        return;
    };
    let old = (*g, *p, *m, *s);
    *g = gain_db.clamp(-60.0, 12.0);
    *p = pan.clamp(-1.0, 1.0);
    *m = mute;
    *s = solo;
    // Prefer command stack when only gain/pan/mute change (solo is live-only here).
    if old.3 == solo {
        app.commands.record(cott_core::commands::Command::SetGainPan {
            node_id,
            old_gain: old.0,
            new_gain: *g,
            old_pan: old.1,
            new_pan: *p,
            old_mute: old.2,
            new_mute: *m,
        });
    }
    app.project.touch();
    app.sync_engine();
}
