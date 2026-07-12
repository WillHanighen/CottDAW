use crate::app::CottApp;
use crate::ui::PianoNoteDrag;
use cott_core::clips::MidiNote;
use cott_core::commands::Command;
use cott_core::ids::NoteId;
use eframe::egui;

const KEY_H: f32 = 14.0;
const BEAT_W: f32 = 40.0;
const KEYS: usize = 48; // C2..C6 roughly
const BASE_PITCH: u8 = 36;
const QUANTIZE: f64 = 0.25;
const MIN_NOTE_LEN: f64 = 0.25;
const RESIZE_HANDLE_PX: f32 = 8.0;

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
    // Always show two full bars past the clip end so the grid keeps repeating.
    let view_beats = clip.length_beats + 2.0 * beats_per_bar;

    ui.horizontal(|ui| {
        ui.label(format!("Editing: {}", clip.name));
        ui.weak(format!(
            "{}/{} · {:.0} beats (+2 bars)",
            app.project.tempo.beats_per_bar,
            app.project.tempo.beat_unit,
            view_beats
        ));
        if ui.button("Add C4 @ 0").clicked() {
            app.add_note_at(clip_id, 60, 0.0, 1.0);
        }
        ui.weak("LMB drag: draw / move / resize · RMB: delete · keys: audition");
    });

    let height = KEYS as f32 * KEY_H;
    let width = (view_beats as f32 * BEAT_W).max(beats_per_bar as f32 * 2.0 * BEAT_W);

    egui::ScrollArea::both().show(ui, |ui| {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width + 40.0, height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let grid =
            egui::Rect::from_min_size(rect.min + egui::vec2(40.0, 0.0), egui::vec2(width, height));
        painter.rect_filled(grid, 0.0, egui::Color32::from_rgb(30, 32, 36));

        // Shade the region past the current clip length (editable padding).
        let clip_end_x = grid.left() + clip.length_beats as f32 * BEAT_W;
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
            let y = grid.top() + i as f32 * KEY_H;
            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let key_rect =
                egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(40.0, KEY_H));
            painter.rect_filled(
                key_rect,
                0.0,
                if black {
                    egui::Color32::from_rgb(20, 20, 22)
                } else {
                    egui::Color32::from_rgb(220, 220, 220)
                },
            );
            let row =
                egui::Rect::from_min_size(egui::pos2(grid.left(), y), egui::vec2(width, KEY_H));
            painter.rect_filled(
                row,
                0.0,
                if black {
                    egui::Color32::from_rgb(36, 38, 42)
                } else {
                    egui::Color32::from_rgb(42, 44, 50)
                },
            );
            painter.line_segment(
                [
                    egui::pos2(grid.left(), y + KEY_H),
                    egui::pos2(grid.right(), y + KEY_H),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 52, 58)),
            );
        }
        let beat_count = view_beats.ceil() as i32;
        for b in 0..=beat_count {
            let x = grid.left() + b as f32 * BEAT_W;
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
            paint_note(&painter, &grid, note, egui::Color32::from_rgb(100, 180, 255));
        }

        // Ghost / live drag note
        if let Some(ghost) = drag_ghost(&app.ui.piano_drag) {
            paint_note(
                &painter,
                &grid,
                &ghost,
                egui::Color32::from_rgb(140, 210, 255),
            );
        }

        // Playhead relative to clip start (same orange bar as arrangement).
        let local_play = app.playhead_beats() - clip.start_beats;
        let play_x = grid.left() + local_play as f32 * BEAT_W;
        if play_x >= grid.left() - 1.0 && play_x <= grid.right() + 1.0 {
            painter.line_segment(
                [egui::pos2(play_x, grid.top()), egui::pos2(play_x, grid.bottom())],
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
                    if pos.x < grid.left()
                        && pos.x >= rect.left()
                        && grid.y_range().contains(pos.y)
                    {
                        if let Some(pitch) = pitch_at_y(grid, pos.y) {
                            app.preview_note(track_id, pitch);
                        }
                    } else if grid.contains(pos) {
                        start_drag(app, clip_id, &notes, &grid, pos, track_id);
                    }
                }
            }

            if app.ui.piano_drag.is_some() {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    update_drag(app, &grid, pos, track_id, view_beats);
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
                    if let Some(note) = hit_note(&notes, &grid, pos) {
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
}

fn quantize_floor(beat: f64) -> f64 {
    (beat / QUANTIZE).floor() * QUANTIZE
}

fn quantize_round(beat: f64) -> f64 {
    (beat / QUANTIZE).round() * QUANTIZE
}

fn pitch_at_y(grid: egui::Rect, y: f32) -> Option<u8> {
    let row = ((y - grid.top()) / KEY_H).floor() as i32;
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

fn beat_at_x(grid: egui::Rect, x: f32) -> f64 {
    ((x - grid.left()) / BEAT_W) as f64
}

fn note_rect(grid: egui::Rect, note: &MidiNote) -> Option<egui::Rect> {
    if note.pitch < BASE_PITCH || note.pitch >= BASE_PITCH + KEYS as u8 {
        return None;
    }
    let row = (BASE_PITCH + KEYS as u8 - 1 - note.pitch) as f32;
    let x = grid.left() + note.start_beats as f32 * BEAT_W;
    let w = (note.length_beats as f32 * BEAT_W).max(4.0);
    let y = grid.top() + row * KEY_H + 1.0;
    Some(egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(w, KEY_H - 2.0),
    ))
}

fn paint_note(painter: &egui::Painter, grid: &egui::Rect, note: &MidiNote, color: egui::Color32) {
    if let Some(nrect) = note_rect(*grid, note) {
        painter.rect_filled(nrect, 2.0, color);
        // Resize handle cue on the right edge
        let handle = egui::Rect::from_min_max(
            egui::pos2(nrect.right() - RESIZE_HANDLE_PX.min(nrect.width()), nrect.top()),
            nrect.max,
        );
        painter.rect_filled(
            handle,
            0.0,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40),
        );
    }
}

fn hit_note<'a>(notes: &'a [MidiNote], grid: &egui::Rect, pos: egui::Pos2) -> Option<&'a MidiNote> {
    // Prefer top-most (last) note under the pointer.
    notes.iter().rev().find(|n| {
        note_rect(*grid, n)
            .map(|r| r.contains(pos))
            .unwrap_or(false)
    })
}

fn hit_resize_edge(note: &MidiNote, grid: &egui::Rect, pos: egui::Pos2) -> bool {
    let Some(r) = note_rect(*grid, note) else {
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
) {
    if let Some(note) = hit_note(notes, grid, pos) {
        if hit_resize_edge(note, grid, pos) {
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
        let beat = beat_at_x(*grid, pos.x);
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

    let Some(pitch) = pitch_at_y(*grid, pos.y) else {
        return;
    };
    let beat = quantize_floor(beat_at_x(*grid, pos.x).max(0.0));
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
            if let Some(p) = pitch_at_y(*grid, pos.y) {
                if *pitch != p {
                    *pitch = p;
                    preview_pitch = Some(p);
                }
            }
            let beat = beat_at_x(*grid, pos.x).clamp(0.0, clip_length);
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
            if let Some(p) = pitch_at_y(*grid, pos.y) {
                if *pitch != p {
                    *pitch = p;
                    preview_pitch = Some(p);
                }
            }
            let beat = beat_at_x(*grid, pos.x) - *grab_offset_beats;
            let q = quantize_floor(beat).max(0.0);
            let max_start = (clip_length - *length_beats).max(0.0);
            *start_beats = q.min(max_start);
        }
        PianoNoteDrag::Resize {
            start_beats,
            length_beats,
            ..
        } => {
            let beat = beat_at_x(*grid, pos.x).clamp(0.0, clip_length);
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
