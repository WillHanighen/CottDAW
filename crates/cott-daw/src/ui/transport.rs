use crate::app::CottApp;
use eframe::egui;

pub fn draw_top_bar(app: &mut CottApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("transport").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("☰").on_hover_text("Toggle browser (B)").clicked() {
                app.ui.show_browser = !app.ui.show_browser;
            }
            ui.separator();
            if ui
                .button("⏹")
                .on_hover_text("Stop and rewind (Home)")
                .clicked()
            {
                app.stop();
            }
            if ui
                .button(
                    if app.project.transport == cott_core::time::TransportState::Playing {
                        "⏹"
                    } else {
                        "▶"
                    },
                )
                .on_hover_text("Play/Stop (Space)")
                .clicked()
            {
                app.toggle_play_stop();
            }
            ui.separator();
            ui.label("BPM");
            let mut bpm = app.project.tempo.bpm as f32;
            if ui
                .add(
                    egui::DragValue::new(&mut bpm)
                        .speed(0.5)
                        .range(20.0..=400.0),
                )
                .changed()
            {
                let old = app.project.tempo.bpm;
                app.commands.push(
                    &mut app.project,
                    cott_core::commands::Command::SetTempo {
                        old_bpm: old,
                        new_bpm: bpm as f64,
                    },
                );
                app.sync_engine();
            }
            ui.label("Time");
            let mut beats_per_bar = app.project.tempo.beats_per_bar as i32;
            let mut beat_unit = app.project.tempo.beat_unit as i32;
            let old_bpb = app.project.tempo.beats_per_bar;
            let old_unit = app.project.tempo.beat_unit;
            let mut sig_changed = false;
            if ui
                .add(
                    egui::DragValue::new(&mut beats_per_bar)
                        .speed(0.2)
                        .range(1..=16),
                )
                .on_hover_text("Beats per bar (numerator)")
                .changed()
            {
                sig_changed = true;
            }
            ui.label("/");
            if ui
                .add(
                    egui::DragValue::new(&mut beat_unit)
                        .speed(0.2)
                        .range(1..=16),
                )
                .on_hover_text("Beat unit (denominator)")
                .changed()
            {
                sig_changed = true;
            }
            let new_bpb = beats_per_bar.clamp(1, 16) as u32;
            let new_unit = beat_unit.clamp(1, 16) as u32;
            if sig_changed && (new_bpb != old_bpb || new_unit != old_unit) {
                app.commands.push(
                    &mut app.project,
                    cott_core::commands::Command::SetTimeSignature {
                        old_beats_per_bar: old_bpb,
                        new_beats_per_bar: new_bpb,
                        old_beat_unit: old_unit,
                        new_beat_unit: new_unit,
                    },
                );
                app.sync_engine();
            }
            let loop_enabled = app.project.loop_enabled;
            if ui
                .checkbox(&mut app.project.loop_enabled, "Loop")
                .on_hover_text("Toggle loop (L)")
                .changed()
            {
                app.sync_engine();
                app.status = if loop_enabled {
                    "Loop disabled".into()
                } else {
                    "Loop enabled".into()
                };
            }
            ui.separator();
            if ui.button("Undo").on_hover_text("Undo (Ctrl+Z)").clicked() {
                app.undo();
            }
            if ui
                .button("Redo")
                .on_hover_text("Redo (Ctrl+Shift+Z or Ctrl+Y)")
                .clicked()
            {
                app.redo();
            }
            ui.separator();
            if ui.button("Save").on_hover_text("Save (Ctrl+S)").clicked() {
                app.save_project();
            }
            if ui.button("Open").on_hover_text("Open (Ctrl+O)").clicked() {
                app.load_project();
            }
            if ui.button("Import Audio").clicked() {
                app.import_audio();
            }
            if ui.button("Import MIDI").clicked() {
                app.import_midi();
            }
            if ui
                .button("Export")
                .on_hover_text("Export (Ctrl+E)")
                .clicked()
            {
                app.open_export_dialog();
            }
            ui.separator();
            if ui.button("+ MIDI Track").clicked() {
                app.add_midi_track();
            }
            if ui.button("+ Audio Track").clicked() {
                app.add_audio_track();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.heading("CottDAW");
            });
        });
    });
}
