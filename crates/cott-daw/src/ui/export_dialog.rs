//! Export settings modal — configure format/options, then pick destination via save dialog.
//!
//! Pattern follows DAW export dialogs (Ableton Export Audio/Video, Reaper Render):
//! settings first, path last.

use crate::app::CottApp;
use cott_core::export::ExportFormat;
use cott_core::visualizers::{GonioColorMode, GonioDrawMode};
use eframe::egui;

#[derive(Debug, Clone)]
pub struct ExportDialogState {
    pub format: ExportFormat,
    pub tail_beats: f64,
    pub bitrate_bps: i32,
    pub gonio: cott_core::visualizers::GonioOptions,
}

impl Default for ExportDialogState {
    fn default() -> Self {
        Self {
            format: ExportFormat::Opus,
            tail_beats: 1.0,
            bitrate_bps: 128_000,
            gonio: cott_core::visualizers::GonioOptions::default(),
        }
    }
}

pub fn draw(app: &mut CottApp, ctx: &egui::Context) {
    if !app.ui.show_export_dialog {
        return;
    }

    let mut do_export = false;
    let mut do_cancel = false;

    let modal = egui::Modal::new(egui::Id::new("export_dialog")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading("Export");
        ui.label("Configure the bounce, then Export… to choose the file name and folder.");
        ui.add_space(8.0);

        // —— Format ——
        ui.heading("Format");
        ui.horizontal(|ui| {
            ui.radio_value(&mut app.ui.export_dialog.format, ExportFormat::Wav, "WAV");
            ui.radio_value(&mut app.ui.export_dialog.format, ExportFormat::Opus, "Opus");
            ui.radio_value(
                &mut app.ui.export_dialog.format,
                ExportFormat::GonioMp4,
                "Gonio MP4",
            );
        });
        ui.weak(match app.ui.export_dialog.format {
            ExportFormat::Wav => "Uncompressed stereo PCM.",
            ExportFormat::Opus => "Lossy Ogg Opus via ffmpeg.",
            ExportFormat::GonioMp4 => {
                "H.264 video of a stereo goniometer + AAC audio (requires ffmpeg)."
            }
        });

        ui.add_space(6.0);
        ui.separator();

        // —— Mix ——
        ui.heading("Mix");
        ui.horizontal(|ui| {
            ui.label("Tail beats");
            ui.add(
                egui::DragValue::new(&mut app.ui.export_dialog.tail_beats)
                    .speed(0.05)
                    .range(0.0..=64.0)
                    .fixed_decimals(2),
            )
            .on_hover_text("Extra silence after the last clip (reverb/delay tails)");
        });

        match app.ui.export_dialog.format {
            ExportFormat::Opus => {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Bitrate");
                    let mut kbps = (app.ui.export_dialog.bitrate_bps / 1000).max(16);
                    if ui
                        .add(
                            egui::DragValue::new(&mut kbps)
                                .speed(1.0)
                                .range(16..=510)
                                .suffix(" kbps"),
                        )
                        .changed()
                    {
                        app.ui.export_dialog.bitrate_bps = kbps * 1000;
                    }
                });
            }
            ExportFormat::GonioMp4 => {
                draw_gonio_section(ui, &mut app.ui.export_dialog.gonio);
            }
            ExportFormat::Wav => {}
        }

        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                do_cancel = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !app.is_exporting(),
                        egui::Button::new("Export…").min_size(egui::vec2(96.0, 0.0)),
                    )
                    .on_hover_text("Choose file name and destination")
                    .clicked()
                {
                    do_export = true;
                }
            });
        });
    });

    if do_cancel || modal.should_close() {
        app.ui.show_export_dialog = false;
    }
    if do_export {
        app.ui.show_export_dialog = false;
        app.confirm_export();
    }
}

fn draw_gonio_section(ui: &mut egui::Ui, g: &mut cott_core::visualizers::GonioOptions) {
    ui.add_space(6.0);
    ui.separator();
    ui.heading("Video");
    ui.horizontal(|ui| {
        ui.label("Resolution");
        ui.add(
            egui::DragValue::new(&mut g.width)
                .speed(4.0)
                .range(256..=2160)
                .suffix(" px"),
        );
        ui.label("×");
        ui.add(
            egui::DragValue::new(&mut g.height)
                .speed(4.0)
                .range(256..=2160)
                .suffix(" px"),
        );
        if ui.small_button("1:1 1080").clicked() {
            g.width = 1080;
            g.height = 1080;
        }
        if ui.small_button("16:9 1080").clicked() {
            g.width = 1920;
            g.height = 1080;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Frame rate");
        ui.add(
            egui::DragValue::new(&mut g.fps)
                .speed(0.5)
                .range(1..=60)
                .suffix(" fps"),
        );
        ui.label("CRF");
        ui.add(egui::DragValue::new(&mut g.crf).speed(0.5).range(0..=51))
            .on_hover_text("x264 quality — lower is better / larger (18 ≈ visually lossless)");
    });

    ui.add_space(6.0);
    ui.separator();
    ui.heading("Goniometer");
    ui.weak("Stereo field: X = L−R, Y = L+R. Line mode matches common gonio visualizers.");

    ui.horizontal(|ui| {
        ui.label("Draw mode");
        ui.selectable_value(&mut g.draw_mode, GonioDrawMode::Line, "Line");
        ui.selectable_value(&mut g.draw_mode, GonioDrawMode::Dots, "Dots");
    });
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.selectable_value(&mut g.color_mode, GonioColorMode::Static, "Static");
        ui.selectable_value(&mut g.color_mode, GonioColorMode::Gradient, "Gradient");
        ui.selectable_value(&mut g.color_mode, GonioColorMode::Spectrum, "Spectrum");
    });

    if g.color_mode == GonioColorMode::Static {
        ui.horizontal(|ui| {
            ui.label("Tint");
            let mut color = egui::Color32::from_rgb(g.color_rgb[0], g.color_rgb[1], g.color_rgb[2]);
            if ui.color_edit_button_srgba(&mut color).changed() {
                g.color_rgb = [color.r(), color.g(), color.b()];
            }
        });
    }

    ui.add(
        egui::Slider::new(&mut g.persistence, 0.0..=0.99)
            .text("Persistence")
            .fixed_decimals(2),
    )
    .on_hover_text("Trail length — higher keeps denser history");
    ui.add(
        egui::Slider::new(&mut g.intensity, 0.05..=4.0)
            .text("Intensity")
            .fixed_decimals(2),
    )
    .on_hover_text("Extra gain after auto-normalization");

    match g.draw_mode {
        GonioDrawMode::Line => {
            ui.horizontal(|ui| {
                ui.label("Line width");
                ui.add(
                    egui::DragValue::new(&mut g.line_width)
                        .speed(0.2)
                        .range(1..=8),
                );
                ui.label("Sample stride");
                ui.add(
                    egui::DragValue::new(&mut g.sample_stride)
                        .speed(0.2)
                        .range(1..=32),
                )
                .on_hover_text("Plot every Nth sample (higher = thinner / faster)");
            });
        }
        GonioDrawMode::Dots => {
            ui.horizontal(|ui| {
                ui.label("Point size");
                ui.add(
                    egui::DragValue::new(&mut g.point_size)
                        .speed(0.2)
                        .range(1..=8),
                );
                ui.label("Sample stride");
                ui.add(
                    egui::DragValue::new(&mut g.sample_stride)
                        .speed(0.2)
                        .range(1..=32),
                );
            });
        }
    }

    ui.checkbox(&mut g.auto_normalize, "Auto-normalize amplitude")
        .on_hover_text("Smoothly scale so peaks fill the display");
    ui.checkbox(&mut g.show_guides, "Show axes / circle guides");
}
