use crate::app::CottApp;
use crate::ui::PianoNoteDrag;
use crate::ui::scale::{self, ScaleMode, ScaleSettings};
use cott_core::clips::MidiNote;
use cott_core::commands::Command;
use cott_core::ids::NoteId;
use eframe::egui;

const BASE_KEY_H: f32 = 14.0;
const BASE_BEAT_W: f32 = 40.0;
const GUTTER_W: f32 = 40.0;
const KEYS: usize = 48; // C2..C6 roughly
const BASE_PITCH: u8 = 36;
const QUANTIZE: f64 = 0.25;
const MIN_NOTE_LEN: f64 = 0.25;
const RESIZE_HANDLE_PX: f32 = 8.0;

/// Accent for the tonic / root of the scale.
const ROOT_ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 176, 74);
/// Accent for the other in-scale degrees.
const SCALE_ACCENT: egui::Color32 = egui::Color32::from_rgb(78, 204, 150);

const PIANO_MIN_ZOOM: f32 = 0.25;
const PIANO_MAX_ZOOM: f32 = 4.0;
const PIANO_MIN_ZOOM_PCT: i32 = 25;
const PIANO_MAX_ZOOM_PCT: i32 = 400;
/// Scroll-wheel (Shift+scroll) zoom step, percent — one step per wheel notch.
const PIANO_SCROLL_ZOOM_STEP_PCT: i32 = 6;
/// Toolbar +/- zoom step, percent.
const PIANO_BUTTON_ZOOM_STEP_PCT: i32 = 10;

pub fn draw(app: &mut CottApp, ui: &mut egui::Ui) {
    let Some(clip_id) = app.ui.selected_clip else {
        ui.weak("Select a MIDI clip in the arrangement");
        return;
    };
    let Some(clip) = app.project.clips.iter().find(|c| c.id == clip_id).cloned() else {
        return;
    };
    let Some(notes) = clip.notes().map(|n| n.to_vec()) else {
        ui.weak("Selected clip is not MIDI");
        return;
    };
    let track_id = clip.track_id;

    let beats_per_bar = app.project.tempo.bar_length_beats();
    let beats_per_bar_i = beats_per_bar.round().max(1.0) as i32;
    // At least 16 bars (or the arrangement length), plus two bars past the clip
    // so you can draw past the end and grow it. Default 1-bar clips used to
    // cap the editor at 12 beats with no horizontal range left to scroll.
    let min_view = (16.0 * beats_per_bar).max(super::arrangement::timeline_length_beats(app));
    let view_beats = (clip.length_beats + 2.0 * beats_per_bar).max(min_view);

    let mut pending_zoom_pct: Option<i32> = None;

    ui.horizontal(|ui| {
        ui.label(format!("Editing: {}", clip.name));
        ui.weak(format!(
            "{}/{} · {:.0} beats visible",
            app.project.tempo.beats_per_bar, app.project.tempo.beat_unit, view_beats
        ));
        if ui.button("Add C4 @ 0").clicked() {
            app.add_note_at(clip_id, 60, 0.0, 1.0);
        }
        ui.separator();
        if ui.button("−").on_hover_text("Zoom out").clicked() {
            pending_zoom_pct = Some(
                (zoom_to_percent(app.ui.piano_zoom) - PIANO_BUTTON_ZOOM_STEP_PCT)
                    .clamp(PIANO_MIN_ZOOM_PCT, PIANO_MAX_ZOOM_PCT),
            );
        }
        let mut zoom_pct = zoom_to_percent(app.ui.piano_zoom);
        if ui
            .add(
                egui::DragValue::new(&mut zoom_pct)
                    .range(PIANO_MIN_ZOOM_PCT as f64..=PIANO_MAX_ZOOM_PCT as f64)
                    .suffix("%")
                    .speed(2.0)
                    .clamp_existing_to_range(true),
            )
            .on_hover_text("Zoom (Shift+scroll over the grid)")
            .changed()
        {
            pending_zoom_pct = Some(zoom_pct.clamp(PIANO_MIN_ZOOM_PCT, PIANO_MAX_ZOOM_PCT));
        }
        if ui.button("+").on_hover_text("Zoom in").clicked() {
            pending_zoom_pct = Some(
                (zoom_to_percent(app.ui.piano_zoom) + PIANO_BUTTON_ZOOM_STEP_PCT)
                    .clamp(PIANO_MIN_ZOOM_PCT, PIANO_MAX_ZOOM_PCT),
            );
        }
        if ui
            .button("Reset")
            .on_hover_text("Reset zoom to 100%")
            .clicked()
        {
            pending_zoom_pct = Some(100);
        }
        ui.weak("LMB: draw/move/resize · RMB: delete · Shift+scroll: zoom");
    });

    scale_toolbar(app, ui);

    let scale = app.ui.scale;

    // Region the scroll area will occupy (used for hover-test + zoom anchor).
    let region = ui.available_rect_before_wrap();
    let old_zoom = app.ui.piano_zoom.clamp(PIANO_MIN_ZOOM, PIANO_MAX_ZOOM);
    let mut new_zoom = old_zoom;
    let mut anchor = region.center();

    if let Some(pct) = pending_zoom_pct {
        new_zoom = percent_to_zoom(pct);
    }

    // Shift+scroll zooms (anchored under the cursor); plain scroll still pans.
    let pointer = ui.ctx().pointer_latest_pos();
    if pointer.is_some_and(|p| region.contains(p)) {
        let (shift, raw) = ui.input(|i| (i.modifiers.shift, i.raw_scroll_delta));
        if shift {
            // Use only the raw (discrete) delta so one wheel notch = one step.
            // egui smears each notch across several smoothed frames; stepping on
            // those extra frames used to ramp the zoom straight to the limit.
            let pick = |v: egui::Vec2| if v.x.abs() > v.y.abs() { v.x } else { v.y };
            let s = pick(raw);
            if s.abs() > f32::EPSILON {
                let dir = if s > 0.0 { 1 } else { -1 };
                new_zoom = percent_to_zoom(
                    (zoom_to_percent(old_zoom) + dir * PIANO_SCROLL_ZOOM_STEP_PCT)
                        .clamp(PIANO_MIN_ZOOM_PCT, PIANO_MAX_ZOOM_PCT),
                );
                anchor = pointer.unwrap_or(anchor);
            }
            // Suppress panning for the whole notch, including the smoothed
            // follow-up frames, so Shift+scroll only ever zooms.
            ui.input_mut(|i| {
                i.raw_scroll_delta = egui::Vec2::ZERO;
                i.smooth_scroll_delta = egui::Vec2::ZERO;
            });
        }
    }

    if (new_zoom - old_zoom).abs() > f32::EPSILON {
        // Only anchor once we know last frame's viewport (skip on first frame).
        if app.ui.piano_viewport.is_positive() {
            let off = zoomed_offset(
                anchor,
                app.ui.piano_viewport,
                app.ui.piano_scroll_offset,
                BASE_KEY_H * old_zoom,
                BASE_BEAT_W * old_zoom,
                BASE_KEY_H * new_zoom,
                BASE_BEAT_W * new_zoom,
            );
            app.ui.piano_pending_offset = Some(off);
        }
        app.ui.piano_zoom = new_zoom;
    }

    let zoom = app.ui.piano_zoom.clamp(PIANO_MIN_ZOOM, PIANO_MAX_ZOOM);
    let key_h = BASE_KEY_H * zoom;
    let beat_w = BASE_BEAT_W * zoom;

    let height = KEYS as f32 * key_h;
    let width = (view_beats as f32 * beat_w).max(beats_per_bar as f32 * 2.0 * beat_w);

    // Solid bars reserve a gutter instead of floating/expanding over the
    // bottom keys when you aim for the lowest pitches.
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();

    // Don't let note-drag steal scroll-wheel panning; disable drag-to-scroll
    // so LMB stays available for draw/move/resize.
    let mut scroll_area = egui::ScrollArea::both()
        .auto_shrink([false, false])
        .drag_to_scroll(false);
    if let Some(off) = app.ui.piano_pending_offset.take() {
        scroll_area = scroll_area.scroll_offset(off);
    }
    let scroll_out = scroll_area.show(ui, |ui| {
        // allocate_exact_size expands ScrollArea content without bubbling a
        // huge min-size up to the window (which was shoving the UI off-screen).
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(width + GUTTER_W, height),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let grid = egui::Rect::from_min_size(
            rect.min + egui::vec2(GUTTER_W, 0.0),
            egui::vec2(width, height),
        );
        painter.rect_filled(grid, 0.0, egui::Color32::from_rgb(30, 32, 36));

        // Shade the region past the current clip length (editable padding).
        let clip_end_x = grid.left() + clip.length_beats as f32 * beat_w;
        if clip_end_x < grid.right() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(clip_end_x, grid.top()),
                    egui::pos2(grid.right(), grid.bottom()),
                ),
                0.0,
                egui::Color32::from_rgba_unmultiplied(20, 24, 30, 90),
            );
        }

        for i in 0..KEYS {
            let pitch = BASE_PITCH + (KEYS - 1 - i) as u8;
            let y = grid.top() + i as f32 * key_h;
            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let in_scale = scale.highlight && scale.contains(pitch);
            let is_root = scale.highlight && scale.is_root(pitch);

            // Piano key strip
            let key_rect =
                egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(GUTTER_W, key_h));
            let key_color = if black {
                egui::Color32::from_rgb(20, 20, 22)
            } else {
                egui::Color32::from_rgb(220, 220, 220)
            };
            painter.rect_filled(key_rect, 0.0, key_color);
            // Root notes get a colored tab on the key so the scale shape reads
            // at a glance ("blueprint").
            if is_root {
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(4.0, key_h)),
                    0.0,
                    ROOT_ACCENT,
                );
            } else if in_scale {
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(4.0, key_h)),
                    0.0,
                    SCALE_ACCENT,
                );
            }

            // Note-name label on the key (dark on white keys, light on black).
            // Hidden when rows get too short to keep the strip legible.
            if key_h >= 9.0 {
                painter.text(
                    egui::pos2(key_rect.right() - 3.0, key_rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    scale::note_name(pitch),
                    egui::FontId::proportional((key_h * 0.62).clamp(8.0, 13.0)),
                    if black {
                        egui::Color32::from_rgb(200, 200, 205)
                    } else {
                        egui::Color32::from_rgb(40, 40, 45)
                    },
                );
            }

            // Grid row
            let base_row = if black {
                egui::Color32::from_rgb(36, 38, 42)
            } else {
                egui::Color32::from_rgb(42, 44, 50)
            };
            let row_color = if !scale.highlight {
                base_row
            } else if is_root {
                blend(base_row, ROOT_ACCENT, 0.32)
            } else if in_scale {
                blend(base_row, SCALE_ACCENT, 0.22)
            } else {
                // Dim out-of-scale rows toward the grid background.
                blend(base_row, egui::Color32::from_rgb(24, 25, 28), 0.55)
            };
            let row =
                egui::Rect::from_min_size(egui::pos2(grid.left(), y), egui::vec2(width, key_h));
            painter.rect_filled(row, 0.0, row_color);
            painter.line_segment(
                [
                    egui::pos2(grid.left(), y + key_h),
                    egui::pos2(grid.right(), y + key_h),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 52, 58)),
            );
        }
        let beat_count = view_beats.ceil() as i32;
        for b in 0..=beat_count {
            let x = grid.left() + b as f32 * beat_w;
            if x > grid.right() + 0.5 {
                break;
            }
            let is_bar = b % beats_per_bar_i == 0;
            painter.line_segment(
                [egui::pos2(x, grid.top()), egui::pos2(x, grid.bottom())],
                egui::Stroke::new(
                    if is_bar { 1.5 } else { 1.0 },
                    if is_bar {
                        egui::Color32::from_rgb(90, 96, 110)
                    } else {
                        egui::Color32::from_rgb(60, 62, 70)
                    },
                ),
            );
        }

        let drag_note_id = match &app.ui.piano_drag {
            Some(PianoNoteDrag::Move { note_id, .. } | PianoNoteDrag::Resize { note_id, .. }) => {
                Some(*note_id)
            }
            _ => None,
        };

        for note in &notes {
            if drag_note_id == Some(note.id) {
                continue; // drawn from live drag state below
            }
            paint_note(
                &painter,
                &grid,
                note,
                note_color(scale, note.pitch, false),
                key_h,
                beat_w,
            );
        }

        // Ghost / live drag note
        if let Some(ghost) = drag_ghost(&app.ui.piano_drag) {
            paint_note(
                &painter,
                &grid,
                &ghost,
                note_color(scale, ghost.pitch, true),
                key_h,
                beat_w,
            );
        }

        // Playhead relative to clip start (same orange bar as arrangement).
        let local_play = app.playhead_beats() - clip.start_beats;
        let play_x = grid.left() + local_play as f32 * beat_w;
        if play_x >= grid.left() - 1.0 && play_x <= grid.right() + 1.0 {
            painter.line_segment(
                [
                    egui::pos2(play_x, grid.top()),
                    egui::pos2(play_x, grid.bottom()),
                ],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 120, 80)),
            );
        }

        // Use the canvas Response — not global pointer state — so overlays
        // (export modal, arrangement track strip, etc.) own their clicks.
        // Prefer is_pointer_button_down_on over drag_started: click_and_drag
        // delays drag_started until the pointer moves.
        if resp.is_pointer_button_down_on() {
            if ui.input(|i| i.pointer.primary_pressed()) {
                if let Some(pos) = resp.interact_pointer_pos() {
                    // Piano keys: audition only
                    if pos.x < grid.left() && pos.x >= rect.left() && grid.y_range().contains(pos.y)
                    {
                        if let Some(pitch) = pitch_at_y(grid, pos.y, key_h) {
                            app.preview_note(track_id, pitch);
                        }
                    } else if grid.contains(pos) {
                        start_drag(app, clip_id, &notes, &grid, pos, track_id, key_h, beat_w);
                    }
                }
            }

            if app.ui.piano_drag.is_some() {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    update_drag(app, &grid, pos, track_id, view_beats, key_h, beat_w);
                }
            }
        }

        if ui.input(|i| i.pointer.primary_released()) && app.ui.piano_drag.is_some() {
            commit_drag(app);
        }

        // Right-click delete nearest note, then trim trailing empty space.
        if resp.secondary_clicked() {
            app.ui.piano_drag = None;
            if let Some(pos) = resp.interact_pointer_pos() {
                if pos.x >= grid.left() {
                    if let Some(note) = hit_note(&notes, &grid, pos, key_h, beat_w) {
                        app.commands.push(
                            &mut app.project,
                            Command::RemoveNote {
                                clip_id,
                                note: note.clone(),
                            },
                        );
                        app.shrink_clip_to_notes(clip_id);
                        app.sync_engine();
                    }
                }
            }
        }
    });

    // Remember scroll state so the next zoom can stay anchored under the cursor.
    app.ui.piano_scroll_offset = scroll_out.state.offset;
    app.ui.piano_viewport = scroll_out.inner_rect;
}

/// Root / mode pickers plus a live "blueprint" of the notes in the key.
fn scale_toolbar(app: &mut CottApp, ui: &mut egui::Ui) {
    let s = &mut app.ui.scale;
    ui.horizontal(|ui| {
        ui.checkbox(&mut s.highlight, "Scale guide")
            .on_hover_text("Highlight in-scale rows and flag out-of-scale notes");

        ui.add_enabled_ui(s.highlight, |ui| {
            let note_names = if s.mode.prefers_flats() {
                &scale::NOTE_NAMES_FLAT
            } else {
                &scale::NOTE_NAMES_SHARP
            };
            egui::ComboBox::from_id_salt("scale_root")
                .selected_text(note_names[s.root as usize % 12])
                .width(52.0)
                .show_ui(ui, |ui| {
                    for (pc, name) in note_names.iter().enumerate() {
                        ui.selectable_value(&mut s.root, pc as u8, *name);
                    }
                });

            egui::ComboBox::from_id_salt("scale_mode")
                .selected_text(s.mode.name())
                .width(140.0)
                .show_ui(ui, |ui| {
                    for mode in ScaleMode::ALL {
                        ui.selectable_value(&mut s.mode, mode, mode.name());
                    }
                });
        });

        if s.highlight {
            ui.separator();
            ui.colored_label(ROOT_ACCENT, format!("{} {}", s.root_name(), s.mode.name()));
            ui.weak("·");
            ui.colored_label(SCALE_ACCENT, s.blueprint());
        }
    });
}

/// Note fill color: in-scale notes stay blue, out-of-scale notes turn red so
/// you can see at a glance when you've strayed from the key.
fn note_color(scale: ScaleSettings, pitch: u8, ghost: bool) -> egui::Color32 {
    if scale.highlight && !scale.contains(pitch) {
        if ghost {
            egui::Color32::from_rgb(255, 140, 140)
        } else {
            egui::Color32::from_rgb(233, 96, 96)
        }
    } else if ghost {
        egui::Color32::from_rgb(140, 210, 255)
    } else {
        egui::Color32::from_rgb(100, 180, 255)
    }
}

/// Linear blend between two colors; `t` = 0 keeps `base`, `t` = 1 gives `tint`.
fn blend(base: egui::Color32, tint: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    egui::Color32::from_rgb(
        (base.r() as f32 * inv + tint.r() as f32 * t) as u8,
        (base.g() as f32 * inv + tint.g() as f32 * t) as u8,
        (base.b() as f32 * inv + tint.b() as f32 * t) as u8,
    )
}

fn zoom_to_percent(zoom: f32) -> i32 {
    (zoom.clamp(PIANO_MIN_ZOOM, PIANO_MAX_ZOOM) * 100.0).round() as i32
}

fn percent_to_zoom(pct: i32) -> f32 {
    (pct.clamp(PIANO_MIN_ZOOM_PCT, PIANO_MAX_ZOOM_PCT) as f32 / 100.0)
        .clamp(PIANO_MIN_ZOOM, PIANO_MAX_ZOOM)
}

/// New scroll offset that keeps the content under `anchor` fixed while the
/// key/beat sizes change. The fixed-width key gutter is left unscaled.
fn zoomed_offset(
    anchor: egui::Pos2,
    viewport: egui::Rect,
    old_offset: egui::Vec2,
    old_key_h: f32,
    old_beat_w: f32,
    new_key_h: f32,
    new_beat_w: f32,
) -> egui::Vec2 {
    let local = anchor - viewport.min;
    let cx = old_offset.x + local.x;
    let cy = old_offset.y + local.y;
    let cx_new = if cx <= GUTTER_W {
        cx
    } else {
        GUTTER_W + (cx - GUTTER_W) * (new_beat_w / old_beat_w)
    };
    let cy_new = cy * (new_key_h / old_key_h);
    egui::vec2((cx_new - local.x).max(0.0), (cy_new - local.y).max(0.0))
}

fn quantize_floor(beat: f64) -> f64 {
    (beat / QUANTIZE).floor() * QUANTIZE
}

fn quantize_round(beat: f64) -> f64 {
    (beat / QUANTIZE).round() * QUANTIZE
}

fn pitch_at_y(grid: egui::Rect, y: f32, key_h: f32) -> Option<u8> {
    let row = ((y - grid.top()) / key_h).floor() as i32;
    if row < 0 || row >= KEYS as i32 {
        return None;
    }
    let pitch = BASE_PITCH as i32 + KEYS as i32 - 1 - row;
    if (0..=127).contains(&pitch) {
        Some(pitch as u8)
    } else {
        None
    }
}

fn beat_at_x(grid: egui::Rect, x: f32, beat_w: f32) -> f64 {
    ((x - grid.left()) / beat_w) as f64
}

fn note_rect(grid: egui::Rect, note: &MidiNote, key_h: f32, beat_w: f32) -> Option<egui::Rect> {
    if note.pitch < BASE_PITCH || note.pitch >= BASE_PITCH + KEYS as u8 {
        return None;
    }
    let row = (BASE_PITCH + KEYS as u8 - 1 - note.pitch) as f32;
    let x = grid.left() + note.start_beats as f32 * beat_w;
    let w = (note.length_beats as f32 * beat_w).max(4.0);
    let y = grid.top() + row * key_h + 1.0;
    Some(egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(w, key_h - 2.0),
    ))
}

fn paint_note(
    painter: &egui::Painter,
    grid: &egui::Rect,
    note: &MidiNote,
    color: egui::Color32,
    key_h: f32,
    beat_w: f32,
) {
    if let Some(nrect) = note_rect(*grid, note, key_h, beat_w) {
        painter.rect_filled(nrect, 2.0, color);
        // Resize handle cue on the right edge
        let handle = egui::Rect::from_min_max(
            egui::pos2(
                nrect.right() - RESIZE_HANDLE_PX.min(nrect.width()),
                nrect.top(),
            ),
            nrect.max,
        );
        painter.rect_filled(
            handle,
            0.0,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40),
        );
    }
}

fn hit_note<'a>(
    notes: &'a [MidiNote],
    grid: &egui::Rect,
    pos: egui::Pos2,
    key_h: f32,
    beat_w: f32,
) -> Option<&'a MidiNote> {
    // Prefer top-most (last) note under the pointer.
    notes.iter().rev().find(|n| {
        note_rect(*grid, n, key_h, beat_w)
            .map(|r| r.contains(pos))
            .unwrap_or(false)
    })
}

fn hit_resize_edge(
    note: &MidiNote,
    grid: &egui::Rect,
    pos: egui::Pos2,
    key_h: f32,
    beat_w: f32,
) -> bool {
    let Some(r) = note_rect(*grid, note, key_h, beat_w) else {
        return false;
    };
    pos.x >= r.right() - RESIZE_HANDLE_PX && r.contains(pos)
}

fn start_drag(
    app: &mut CottApp,
    clip_id: cott_core::ids::ClipId,
    notes: &[MidiNote],
    grid: &egui::Rect,
    pos: egui::Pos2,
    track_id: cott_core::ids::TrackId,
    key_h: f32,
    beat_w: f32,
) {
    if let Some(note) = hit_note(notes, grid, pos, key_h, beat_w) {
        if hit_resize_edge(note, grid, pos, key_h, beat_w) {
            app.ui.piano_drag = Some(PianoNoteDrag::Resize {
                clip_id,
                note_id: note.id,
                before: note.clone(),
                start_beats: note.start_beats,
                length_beats: note.length_beats,
            });
            app.preview_note(track_id, note.pitch);
            return;
        }
        let beat = beat_at_x(*grid, pos.x, beat_w);
        app.ui.piano_drag = Some(PianoNoteDrag::Move {
            clip_id,
            note_id: note.id,
            before: note.clone(),
            pitch: note.pitch,
            start_beats: note.start_beats,
            length_beats: note.length_beats,
            grab_offset_beats: beat - note.start_beats,
        });
        app.preview_note(track_id, note.pitch);
        return;
    }

    let Some(pitch) = pitch_at_y(*grid, pos.y, key_h) else {
        return;
    };
    let beat = quantize_floor(beat_at_x(*grid, pos.x, beat_w).max(0.0));
    app.ui.piano_drag = Some(PianoNoteDrag::Draw {
        clip_id,
        pitch,
        origin_beat: beat,
        end_beat: beat + MIN_NOTE_LEN,
    });
    app.preview_note(track_id, pitch);
}

fn update_drag(
    app: &mut CottApp,
    grid: &egui::Rect,
    pos: egui::Pos2,
    track_id: cott_core::ids::TrackId,
    clip_length: f64,
    key_h: f32,
    beat_w: f32,
) {
    let Some(drag) = app.ui.piano_drag.as_mut() else {
        return;
    };
    let mut preview_pitch: Option<u8> = None;
    match drag {
        PianoNoteDrag::Draw {
            pitch,
            origin_beat,
            end_beat,
            ..
        } => {
            if let Some(p) = pitch_at_y(*grid, pos.y, key_h) {
                if *pitch != p {
                    *pitch = p;
                    preview_pitch = Some(p);
                }
            }
            let beat = beat_at_x(*grid, pos.x, beat_w).clamp(0.0, clip_length);
            if beat < *origin_beat {
                *end_beat = quantize_floor(beat);
            } else {
                *end_beat = quantize_round(beat).max(*origin_beat + MIN_NOTE_LEN);
            }
        }
        PianoNoteDrag::Move {
            pitch,
            start_beats,
            length_beats,
            grab_offset_beats,
            ..
        } => {
            if let Some(p) = pitch_at_y(*grid, pos.y, key_h) {
                if *pitch != p {
                    *pitch = p;
                    preview_pitch = Some(p);
                }
            }
            let beat = beat_at_x(*grid, pos.x, beat_w) - *grab_offset_beats;
            let q = quantize_floor(beat).max(0.0);
            let max_start = (clip_length - *length_beats).max(0.0);
            *start_beats = q.min(max_start);
        }
        PianoNoteDrag::Resize {
            start_beats,
            length_beats,
            ..
        } => {
            let beat = beat_at_x(*grid, pos.x, beat_w).clamp(0.0, clip_length);
            let end = quantize_round(beat).max(*start_beats + MIN_NOTE_LEN);
            *length_beats = (end - *start_beats).max(MIN_NOTE_LEN);
        }
    }
    if let Some(p) = preview_pitch {
        app.preview_note_if_new_pitch(track_id, p);
    }
}

fn drag_ghost(drag: &Option<PianoNoteDrag>) -> Option<MidiNote> {
    match drag {
        Some(PianoNoteDrag::Draw {
            pitch,
            origin_beat,
            end_beat,
            ..
        }) => {
            let start = origin_beat.min(*end_beat);
            let end = origin_beat.max(*end_beat);
            let len = (end - start).max(MIN_NOTE_LEN);
            Some(MidiNote {
                id: NoteId::new(),
                pitch: *pitch,
                velocity: 100,
                start_beats: start,
                length_beats: len,
                channel: 0,
            })
        }
        Some(PianoNoteDrag::Move {
            before,
            pitch,
            start_beats,
            length_beats,
            ..
        }) => Some(MidiNote {
            id: before.id,
            pitch: *pitch,
            velocity: before.velocity,
            start_beats: *start_beats,
            length_beats: *length_beats,
            channel: before.channel,
        }),
        Some(PianoNoteDrag::Resize {
            before,
            start_beats,
            length_beats,
            ..
        }) => Some(MidiNote {
            id: before.id,
            pitch: before.pitch,
            velocity: before.velocity,
            start_beats: *start_beats,
            length_beats: *length_beats,
            channel: before.channel,
        }),
        None => None,
    }
}

fn commit_drag(app: &mut CottApp) {
    let Some(drag) = app.ui.piano_drag.take() else {
        return;
    };
    match drag {
        PianoNoteDrag::Draw {
            clip_id,
            pitch,
            origin_beat,
            end_beat,
        } => {
            let start = origin_beat.min(end_beat);
            let end = origin_beat.max(end_beat);
            let len = (end - start).max(MIN_NOTE_LEN);
            app.ensure_clip_length(clip_id, start + len);
            app.add_note_at(clip_id, pitch, start, len);
        }
        PianoNoteDrag::Move {
            clip_id,
            before,
            pitch,
            start_beats,
            length_beats,
            ..
        } => {
            app.ensure_clip_length(clip_id, start_beats + length_beats);
            let after = MidiNote {
                id: before.id,
                pitch,
                velocity: before.velocity,
                start_beats,
                length_beats,
                channel: before.channel,
            };
            app.edit_note(clip_id, before, after);
        }
        PianoNoteDrag::Resize {
            clip_id,
            before,
            start_beats,
            length_beats,
            ..
        } => {
            app.ensure_clip_length(clip_id, start_beats + length_beats);
            let after = MidiNote {
                id: before.id,
                pitch: before.pitch,
                velocity: before.velocity,
                start_beats,
                length_beats,
                channel: before.channel,
            };
            app.edit_note(clip_id, before, after);
        }
    }
}
