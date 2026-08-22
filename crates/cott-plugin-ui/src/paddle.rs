//! Three-throw rocker paddle: up / off / down, both names on the cap.

use nih_plug_egui::egui::{
    pos2, vec2, Align2, Color32, FontId, Painter, Rect, Sense, Stroke, StrokeKind, Ui,
};

use crate::button::paint_lamp;
use crate::chassis::{mix, with_alpha};
use crate::skin::Skin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddleThrow {
    Off,
    Up,
    Down,
}

/// Click the top half to throw up, the bottom half to throw down. Clicking the
/// already-selected throw leaves it. Returns the new throw when it changes.
pub fn paddle(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    key: &str,
    up_label: &str,
    down_label: &str,
    throw: PaddleThrow,
) -> Option<PaddleThrow> {
    let id = ui.id().with("cott_paddle").with(key);
    let response = ui.interact(rect, id, Sense::click());
    paint_paddle(
        ui.painter(),
        rect,
        skin,
        up_label,
        down_label,
        throw,
        response.hovered(),
        response.is_pointer_button_down_on(),
    );

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let next = if pos.y < rect.center().y {
                PaddleThrow::Up
            } else {
                PaddleThrow::Down
            };
            if next != throw {
                return Some(next);
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn paint_paddle(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    up_label: &str,
    down_label: &str,
    throw: PaddleThrow,
    hovered: bool,
    held: bool,
) {
    let size = (rect.width() * 0.14).clamp(13.0, 22.0);
    let label_h = size * 2.35 + 6.0;
    let up_box = Rect::from_min_size(rect.min, vec2(rect.width(), label_h));
    let down_box = Rect::from_min_max(pos2(rect.left(), rect.bottom() - label_h), rect.max);

    paint_voice_name(
        painter,
        up_box,
        up_label,
        size,
        if throw == PaddleThrow::Up {
            skin.legend
        } else {
            mix(skin.legend_dim, skin.legend, 0.45)
        },
    );
    paint_voice_name(
        painter,
        down_box,
        down_label,
        size,
        if throw == PaddleThrow::Down {
            skin.legend
        } else {
            mix(skin.legend_dim, skin.legend, 0.45)
        },
    );

    let body = Rect::from_center_size(
        rect.center(),
        vec2(
            (rect.width() * 0.42).clamp(28.0, 96.0),
            (rect.height() - label_h * 2.05).max(48.0),
        ),
    );
    let r = 5.0;
    let tilt = match throw {
        PaddleThrow::Up => -1.6,
        PaddleThrow::Down => 1.6,
        PaddleThrow::Off => 0.0,
    };
    let face = body.translate(vec2(0.0, tilt + if held { 1.0 } else { 0.0 }));

    painter.rect_filled(
        body.translate(vec2(0.0, 2.5)),
        r,
        with_alpha(Color32::BLACK, 100),
    );

    let top = if throw != PaddleThrow::Off {
        mix(skin.cap_light, skin.accent, 0.22)
    } else if hovered {
        mix(skin.cap_light, Color32::WHITE, 0.08)
    } else {
        skin.cap_light
    };
    let bottom = if throw != PaddleThrow::Off {
        mix(skin.cap_dark, skin.accent_dim, 0.45)
    } else {
        skin.cap_dark
    };

    let steps = 10;
    let h = face.height() / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let band = Rect::from_min_size(
            pos2(face.left(), face.top() + i as f32 * h),
            vec2(face.width(), h + 1.0),
        );
        let corner = if i == 0 || i == steps - 1 { r } else { 0.0 };
        painter.rect_filled(band, corner, mix(top, bottom, t));
    }

    painter.line_segment(
        [
            pos2(face.left() + 3.0, face.center().y),
            pos2(face.right() - 3.0, face.center().y),
        ],
        Stroke::new(1.0, with_alpha(Color32::BLACK, 140)),
    );
    painter.rect_stroke(
        face,
        r,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 170)),
        StrokeKind::Inside,
    );

    let lamp_y = match throw {
        PaddleThrow::Up => face.top() + 10.0,
        PaddleThrow::Down => face.bottom() - 10.0,
        PaddleThrow::Off => face.center().y,
    };
    paint_lamp(
        painter,
        pos2(face.center().x, lamp_y),
        4.6,
        skin,
        if throw == PaddleThrow::Off { 0.08 } else { 1.0 },
    );
}

/// Two-line name, no letter-spacing. "English Horn" becomes English / Horn.
fn paint_voice_name(painter: &Painter, rect: Rect, text: &str, size: f32, color: Color32) {
    let font = FontId::proportional(size);
    let (top, bottom) = split_name(text);
    if let Some(bottom) = bottom {
        let gap = size * 0.15;
        painter.text(
            pos2(rect.center().x, rect.center().y - size * 0.55 - gap),
            Align2::CENTER_CENTER,
            &top,
            font.clone(),
            color,
        );
        painter.text(
            pos2(rect.center().x, rect.center().y + size * 0.55 + gap),
            Align2::CENTER_CENTER,
            &bottom,
            font,
            color,
        );
    } else {
        painter.text(rect.center(), Align2::CENTER_CENTER, &top, font, color);
    }
}

fn split_name(text: &str) -> (String, Option<String>) {
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.len() {
        0 | 1 => (text.to_string(), None),
        2 => (parts[0].to_string(), Some(parts[1].to_string())),
        _ => (parts[0].to_string(), Some(parts[1..].join(" "))),
    }
}
