use crate::app::CottApp;
use cott_core::clips::{Clip, ClipContent, MidiNote, TrackKind};
use cott_core::commands::Command;
use cott_core::graph::NodeKind;
use eframe::egui;

/// How long the arrangement timeline should be, in beats.
/// Grows with project content; always at least 32 bars so the ruler isn't tiny.
pub fn timeline_length_beats(app: &CottApp) -> f64 {
    let bar = app.project.tempo.bar_length_beats().max(1.0);
    let min_beats = 32.0 * bar;
    let content_end = app
        .project
        .clips
        .iter()
        .map(|c| c.end_beats())
        .fold(0.0_f64, f64::max);
    let playhead = app.playhead_beats();
    // Eight bars of padding past the last clip / playhead.
    (content_end + 8.0 * bar)
        .max(playhead + 4.0 * bar)
        .max(min_beats)
}

pub fn draw(app: &mut CottApp, ui: &mut egui::Ui) {
    let track_header_w = 160.0;
    let beat_px = 1.0 / app.ui.beats_per_pixel.max(0.001);
    let beats_per_bar = app.project.tempo.bar_length_beats();
    let beats_per_bar_i = beats_per_bar.round().max(1.0) as i32;
    let total_beats = timeline_length_beats(app) as f32;
    let timeline_w = total_beats * beat_px;
    // Inclusive end beat for grid lines; bar *labels* stop before this (see ruler).
    let beat_limit = total_beats.ceil() as i32;
    let bar_count = (total_beats / beats_per_bar as f32).floor().max(1.0) as i32;
    const CLIP_QUANTIZE: f64 = 0.25;

    // Commit clip move/resize when the mouse is released.
    if app.ui.clip_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        if let Some(finished) = app.ui.clip_drag.take() {
            if (finished.current_start - finished.original_start).abs() > 1e-9
                || (finished.current_length - finished.original_length).abs() > 1e-9
            {
                app.move_clip(
                    finished.clip_id,
                    finished.current_start,
                    finished.current_length,
                );
            }
        }
    }

    ui.horizontal(|ui| {
        ui.label("Zoom");
        ui.add(egui::Slider::new(&mut app.ui.beats_per_pixel, 0.005..=0.1));
    });

    // Ruler
    ui.horizontal(|ui| {
        ui.allocate_exact_size(egui::vec2(track_header_w, 20.0), egui::Sense::hover());
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(timeline_w.min(ui.available_width()), 20.0),
            egui::Sense::click_and_drag(),
        );
        if resp.dragged() {
            app.ui.scroll_x = (app.ui.scroll_x - resp.drag_delta().x).max(0.0);
        }
        let scroll = ui.input(|i| i.smooth_scroll_delta.x);
        if scroll != 0.0 {
            app.ui.scroll_x = (app.ui.scroll_x - scroll).max(0.0);
        }
        let max_scroll = (timeline_w - rect.width()).max(0.0);
        app.ui.scroll_x = app.ui.scroll_x.clamp(0.0, max_scroll);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(40, 42, 48));
        // Bar ticks + labels 1..=bar_count (no phantom label on the final edge).
        for bar in 0..=bar_count {
            let b = bar * beats_per_bar_i;
            let x = rect.left() + b as f32 * beat_px - app.ui.scroll_x;
            if x < rect.left() - 1.0 || x > rect.right() + 1.0 {
                continue;
            }
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, egui::Color32::GRAY),
            );
            if bar < bar_count {
                painter.text(
                    egui::pos2(x + 2.0, rect.top()),
                    egui::Align2::LEFT_TOP,
                    format!("{}", bar + 1),
                    egui::FontId::monospace(10.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        }
        // Playhead
        let ph = app.playhead_beats() as f32 * beat_px - app.ui.scroll_x;
        let x = rect.left() + ph;
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 120, 80)),
            );
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let beat = ((pos.x - rect.left()) + app.ui.scroll_x) / beat_px;
                if let Some(audio) = &mut app.audio {
                    let sample = app
                        .project
                        .tempo
                        .beat_to_sample(cott_core::time::BeatPos(beat as f64));
                    let _ = audio
                        .cmd_tx
                        .push(cott_core::engine::EngineCommand::Seek(sample));
                    audio.shared.set_position(sample);
                }
            }
        }
    });

    // Deferred track/clip edits — applied after the scroll area so we never
    // mutate the project (or app selections) while iterating cloned tracks.
    let mut pending_delete_track: Option<cott_core::ids::TrackId> = None;
    let mut pending_move_clip: Option<(cott_core::ids::ClipId, cott_core::ids::TrackId)> = None;
    let mut pending_start_rename: Option<(cott_core::ids::TrackId, String)> = None;
    let mut pending_rename_commit: Option<(cott_core::ids::TrackId, String)> = None;
    let track_list: Vec<(cott_core::ids::TrackId, String, TrackKind)> = app
        .project
        .tracks
        .iter()
        .map(|t| (t.id, t.name.clone(), t.kind))
        .collect();

    egui::ScrollArea::vertical().show(ui, |ui| {
        let tracks: Vec<_> = app.project.tracks.iter().cloned().collect();
        for track in tracks {
            ui.horizontal(|ui| {
                // Fixed width matching the ruler spacer — shrink-wrapping here was what
                // made bar numbers drift relative to the lane grid.
                let header = ui.allocate_ui_with_layout(
                    egui::vec2(track_header_w, track.height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(track_header_w);
                        ui.set_max_width(track_header_w);
                        let selected = app.ui.selected_track == Some(track.id);
                        let fill = if selected {
                            egui::Color32::from_rgb(55, 70, 90)
                        } else {
                            egui::Color32::from_rgb(45, 48, 54)
                        };
                        egui::Frame::NONE
                            .fill(fill)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                let editing = matches!(
                                    &app.ui.renaming_track,
                                    Some((rid, _)) if *rid == track.id
                                );
                                if editing {
                                    let mut commit = false;
                                    let mut cancel = false;
                                    if let Some((_, buf)) = app.ui.renaming_track.as_mut() {
                                        let resp = ui.add(
                                            egui::TextEdit::singleline(buf)
                                                .desired_width(track_header_w - 12.0),
                                        );
                                        resp.request_focus();
                                        if ui.input(|i| i.key_pressed(egui::Key::Enter))
                                            || resp.clicked_elsewhere()
                                        {
                                            commit = true;
                                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                            cancel = true;
                                        }
                                    }
                                    if commit {
                                        if let Some((rid, buf)) = app.ui.renaming_track.take() {
                                            pending_rename_commit = Some((rid, buf));
                                        }
                                    } else if cancel {
                                        app.ui.renaming_track = None;
                                    }
                                } else {
                                    let label = ui.selectable_label(selected, &track.name);
                                    if label.clicked() {
                                        app.ui.selected_track = Some(track.id);
                                    }
                                    label.context_menu(|ui| {
                                        if ui.button("Rename").clicked() {
                                            pending_start_rename =
                                                Some((track.id, track.name.clone()));
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if ui.button("Delete track").clicked() {
                                            pending_delete_track = Some(track.id);
                                            ui.close_menu();
                                        }
                                    });
                                }
                                ui.label(match track.kind {
                                    TrackKind::Midi => "MIDI",
                                    TrackKind::Audio => "Audio",
                                });
                                if let Some(gain_id) = track.gain_node {
                                    let gain_state =
                                        app.project.graph.nodes.get(&gain_id).and_then(|node| {
                                            match &node.kind {
                                                NodeKind::GainPan {
                                                    gain_db, mute, pan, ..
                                                } => Some((*gain_db, *mute, *pan)),
                                                _ => None,
                                            }
                                        });
                                    if let Some((gain_db, mute, pan)) = gain_state {
                                        let mut g = gain_db;
                                        if ui
                                            .add(
                                                egui::Slider::new(&mut g, -60.0..=12.0)
                                                    .show_value(false),
                                            )
                                            .changed()
                                        {
                                            app.set_gain(gain_id, g);
                                        }
                                        let mut m = mute;
                                        if ui.checkbox(&mut m, "M").changed() {
                                            app.commands.push(
                                                &mut app.project,
                                                Command::SetGainPan {
                                                    node_id: gain_id,
                                                    old_gain: gain_db,
                                                    new_gain: gain_db,
                                                    old_pan: pan,
                                                    new_pan: pan,
                                                    old_mute: mute,
                                                    new_mute: m,
                                                },
                                            );
                                            app.sync_engine();
                                        }
                                        if let Some(m) = app.meters.get(&gain_id) {
                                            let peak = m.peak_l.max(m.peak_r);
                                            ui.add(
                                                egui::ProgressBar::new(peak.clamp(0.0, 1.0))
                                                    .desired_width(track_header_w - 20.0),
                                            );
                                        }
                                    }
                                    if ui.small_button("Select FX").clicked() {
                                        app.ui.selected_node = Some(gain_id);
                                    }
                                }
                                ui.horizontal(|ui| {
                                    if ui.small_button("+ Clip").clicked() {
                                        app.ui.selected_track = Some(track.id);
                                        let start = app.playhead_beats();
                                        let bar = app.project.tempo.bar_length_beats();
                                        match track.kind {
                                            TrackKind::Midi => {
                                                let clip =
                                                    Clip::new_midi(track.id, "Clip", start, bar);
                                                app.commands.push(
                                                    &mut app.project,
                                                    Command::AddClip { clip },
                                                );
                                                app.sync_engine();
                                            }
                                            TrackKind::Audio => {
                                                app.status =
                                                    "Import audio onto audio tracks".into();
                                            }
                                        }
                                    }
                                    if ui
                                        .small_button("🗑")
                                        .on_hover_text("Delete this track")
                                        .clicked()
                                    {
                                        pending_delete_track = Some(track.id);
                                    }
                                });
                            });
                        // Pin width even if controls are narrower than the strip.
                        ui.expand_to_include_x(ui.min_rect().left() + track_header_w);
                    },
                );
                let row_h = header.response.rect.height().max(track.height);

                // Lane — height matches header so the row stays aligned.
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(timeline_w.min(ui.available_width()), row_h),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(32, 34, 38));

                // Track hover for clip paste anchoring.
                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                    if rect.contains(pos) {
                        let beat =
                            ((pos.x - rect.left()) + app.ui.scroll_x) as f64 / beat_px as f64;
                        app.ui.arrangement_hover_beat = Some(beat.max(0.0));
                        app.ui.arrangement_hover_track = Some(track.id);
                    }
                }
                // grid — bar lines follow time signature; faint beat lines in between
                for b in 0..=beat_limit {
                    let x = rect.left() + b as f32 * beat_px - app.ui.scroll_x;
                    if x < rect.left() - 1.0 || x > rect.right() + 1.0 {
                        continue;
                    }
                    let is_bar = b % beats_per_bar_i == 0;
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(
                            if is_bar { 1.0 } else { 0.5 },
                            if is_bar {
                                egui::Color32::from_rgb(50, 52, 58)
                            } else {
                                egui::Color32::from_rgb(38, 40, 44)
                            },
                        ),
                    );
                }

                // Live-update an in-progress drag for clips on this track.
                if let Some(drag) = app.ui.clip_drag.as_mut() {
                    if drag.track_id == track.id {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let beat =
                                ((pos.x - rect.left()) + app.ui.scroll_x) as f64 / beat_px as f64;
                            if drag.resizing {
                                let end = quantize_beat(beat, CLIP_QUANTIZE)
                                    .max(drag.current_start + CLIP_QUANTIZE);
                                drag.current_length = end - drag.current_start;
                            } else {
                                drag.current_start =
                                    quantize_beat(beat - drag.grab_offset_beats, CLIP_QUANTIZE)
                                        .max(0.0);
                            }
                        }
                    }
                }

                let clips: Vec<_> = app
                    .project
                    .clips
                    .iter()
                    .filter(|c| c.track_id == track.id)
                    .cloned()
                    .collect();
                let mut delete_clip: Option<cott_core::ids::ClipId> = None;
                let mut clip_handled_pointer = app
                    .ui
                    .clip_drag
                    .as_ref()
                    .is_some_and(|d| d.track_id == track.id);
                const RESIZE_EDGE_PX: f32 = 8.0;

                for clip in &clips {
                    let (draw_start, draw_len) = if let Some(drag) = &app.ui.clip_drag {
                        if drag.clip_id == clip.id {
                            (drag.current_start, drag.current_length)
                        } else {
                            (clip.start_beats, clip.length_beats)
                        }
                    } else {
                        (clip.start_beats, clip.length_beats)
                    };
                    let x0 = rect.left() + draw_start as f32 * beat_px - app.ui.scroll_x;
                    let w = draw_len as f32 * beat_px;
                    let clip_rect = egui::Rect::from_min_size(
                        egui::pos2(x0, rect.top() + 4.0),
                        egui::vec2(w.max(4.0), (rect.height() - 8.0).max(4.0)),
                    );
                    let color =
                        egui::Color32::from_rgb(clip.color[0], clip.color[1], clip.color[2]);
                    let selected = app.ui.selected_clip == Some(clip.id);
                    painter.rect_filled(
                        clip_rect,
                        4.0,
                        color.gamma_multiply(if selected { 1.0 } else { 0.85 }),
                    );
                    painter.rect_stroke(
                        clip_rect,
                        4.0,
                        egui::Stroke::new(if selected { 2.0 } else { 1.0 }, egui::Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );
                    // Resize handle cue
                    let handle = egui::Rect::from_min_max(
                        egui::pos2(
                            clip_rect.right() - RESIZE_EDGE_PX.min(clip_rect.width()),
                            clip_rect.top(),
                        ),
                        clip_rect.max,
                    );
                    painter.rect_filled(
                        handle,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50),
                    );
                    painter.text(
                        clip_rect.left_top() + egui::vec2(4.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        &clip.name,
                        egui::FontId::proportional(12.0),
                        egui::Color32::BLACK,
                    );
                    if let ClipContent::Midi { notes, .. } = &clip.content {
                        paint_midi_preview(&painter, clip_rect, notes, draw_len);
                    }

                    let clip_id = clip.id;
                    let clip_resp = ui.interact(
                        clip_rect,
                        ui.id().with(("clip", clip_id)),
                        egui::Sense::click_and_drag(),
                    );
                    let clip_is_midi = matches!(clip.content, ClipContent::Midi { .. });
                    let clip_track_id = clip.track_id;
                    let mut pending_copy_clip: Option<cott_core::ids::ClipId> = None;
                    let mut pending_duplicate_clip: Option<cott_core::ids::ClipId> = None;
                    clip_resp.context_menu(|ui| {
                        ui.label(&clip.name);
                        ui.separator();
                        if ui.button("Copy").clicked() {
                            pending_copy_clip = Some(clip_id);
                            ui.close_menu();
                        }
                        if ui.button("Duplicate").clicked() {
                            pending_duplicate_clip = Some(clip_id);
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.menu_button("Move to track", |ui| {
                            let mut any = false;
                            for (tid, tname, tkind) in &track_list {
                                if *tid == clip_track_id {
                                    continue;
                                }
                                if matches!(tkind, TrackKind::Midi) != clip_is_midi {
                                    continue;
                                }
                                any = true;
                                if ui.button(tname).clicked() {
                                    pending_move_clip = Some((clip_id, *tid));
                                    ui.close_menu();
                                }
                            }
                            if !any {
                                ui.label("No compatible tracks");
                            }
                        });
                        if ui.button("Delete clip").clicked() {
                            delete_clip = Some(clip_id);
                            ui.close_menu();
                        }
                    });
                    if let Some(id) = pending_copy_clip {
                        app.ui.selected_clip = Some(id);
                        app.copy_selected_clip();
                    }
                    if let Some(id) = pending_duplicate_clip {
                        app.ui.selected_clip = Some(id);
                        app.duplicate_selected_clip();
                    }

                    if clip_resp.hovered() && app.ui.clip_drag.is_none() {
                        let on_resize = ui
                            .input(|i| i.pointer.hover_pos())
                            .map(|p| p.x >= clip_rect.right() - RESIZE_EDGE_PX)
                            .unwrap_or(false);
                        ui.ctx().set_cursor_icon(if on_resize {
                            egui::CursorIcon::ResizeHorizontal
                        } else {
                            egui::CursorIcon::Grab
                        });
                    }

                    if clip_resp.drag_started() {
                        clip_handled_pointer = true;
                        if let Some(pos) = clip_resp.interact_pointer_pos() {
                            let beat =
                                ((pos.x - rect.left()) + app.ui.scroll_x) as f64 / beat_px as f64;
                            let resizing = pos.x >= clip_rect.right() - RESIZE_EDGE_PX;
                            app.ui.selected_clip = Some(clip_id);
                            app.ui.selected_track = Some(track.id);
                            app.ui.clip_drag = Some(crate::ui::ArrangementClipDrag {
                                clip_id,
                                track_id: track.id,
                                original_start: clip.start_beats,
                                original_length: clip.length_beats,
                                grab_offset_beats: beat - clip.start_beats,
                                current_start: clip.start_beats,
                                current_length: clip.length_beats,
                                resizing,
                            });
                        }
                    }

                    if clip_resp.clicked() && app.ui.clip_drag.is_none() {
                        clip_handled_pointer = true;
                        app.ui.selected_track = Some(track.id);
                        if app.ui.selected_clip != Some(clip_id) {
                            app.clear_note_selection();
                        }
                        app.ui.selected_clip = Some(clip_id);
                        app.ui.lower_tab = crate::ui::LowerTab::PianoRoll;
                    }

                    if clip_resp.hovered() || clip_resp.dragged() || clip_resp.clicked() {
                        clip_handled_pointer = true;
                    }
                }

                if let Some(id) = delete_clip {
                    app.remove_clip(id);
                }

                // Lane scroll / empty-lane select — skip if a clip took the pointer.
                if app.ui.clip_drag.is_none() && !clip_handled_pointer {
                    if resp.dragged() && resp.drag_delta().x.abs() > resp.drag_delta().y.abs() {
                        app.ui.scroll_x = (app.ui.scroll_x - resp.drag_delta().x).max(0.0);
                    }
                    if resp.clicked() {
                        app.ui.selected_track = Some(track.id);
                    }
                }

                // Playhead on lane
                let ph = app.playhead_beats() as f32 * beat_px - app.ui.scroll_x;
                let x = rect.left() + ph;
                if x >= rect.left() && x <= rect.right() {
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 120, 80)),
                    );
                }
            });
        }
    });

    // Apply deferred track/clip edits now that the scroll area released `app`.
    if let Some((tid, buf)) = pending_rename_commit {
        app.rename_track(tid, buf);
    }
    if let Some((tid, name)) = pending_start_rename {
        app.ui.renaming_track = Some((tid, name));
    }
    if let Some((clip_id, tid)) = pending_move_clip {
        app.move_clip_to_track(clip_id, tid);
    }
    if let Some(tid) = pending_delete_track {
        app.remove_track(tid);
    }
}

fn quantize_beat(beat: f64, step: f64) -> f64 {
    (beat / step).round() * step
}

/// Draw note bars inside a clip so arrangement previews match the piano roll.
fn paint_midi_preview(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    notes: &[MidiNote],
    clip_length: f64,
) {
    if notes.is_empty() || clip_length <= 0.0 {
        return;
    }

    let mut pitch_lo = u8::MAX;
    let mut pitch_hi = 0u8;
    for note in notes {
        pitch_lo = pitch_lo.min(note.pitch);
        pitch_hi = pitch_hi.max(note.pitch);
    }
    // Keep a little vertical room so single notes aren't full-height slabs.
    let span = (pitch_hi.saturating_sub(pitch_lo)).max(12) as f32;
    let pad = 3.0;
    let usable_h = (clip_rect.height() - pad * 2.0).max(4.0);
    let note_h = (usable_h / (span + 1.0)).clamp(2.0, 6.0);
    let clip_w = clip_rect.width();
    let len = clip_length.max(0.001) as f32;

    for note in notes {
        let x = clip_rect.left() + (note.start_beats as f32 / len) * clip_w;
        let w = ((note.length_beats as f32 / len) * clip_w).max(2.0);
        let y_norm = (note.pitch.saturating_sub(pitch_lo) as f32) / span;
        let y = clip_rect.bottom() - pad - y_norm * usable_h - note_h;
        let note_rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, note_h));
        // Clip drawing to the clip body so overflow never paints outside.
        let clipped = note_rect.intersect(clip_rect);
        if clipped.width() > 0.5 && clipped.height() > 0.5 {
            painter.rect_filled(clipped, 1.0, egui::Color32::from_rgb(18, 22, 28));
        }
    }
}
