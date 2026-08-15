//! Hardware-style selector buttons and indicator lamps.

use nih_plug_egui::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui,
};

use crate::chassis::{mix, spaced, with_alpha};
use crate::skin::Skin;

/// A latching selector cap. Returns the click response.
pub fn segment_button(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    key: &str,
    label: &str,
    selected: bool,
) -> Response {
    let id = ui.id().with("cott_segment").with(key);
    let response = ui.interact(rect, id, Sense::click());
    let held = response.is_pointer_button_down_on();
    paint_cap(
        ui.painter(),
        rect,
        skin,
        label,
        selected,
        response.hovered(),
        held,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_cap(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    label: &str,
    selected: bool,
    hovered: bool,
    held: bool,
) {
    let r = 4.0;
    let face = rect.translate(if held { vec2(0.0, 1.0) } else { vec2(0.0, 0.0) });

    if !held {
        painter.rect_filled(
            rect.translate(vec2(0.0, 2.0)),
            r,
            with_alpha(Color32::BLACK, 90),
        );
    }

    let (top, bottom) = if selected {
        (
            mix(skin.cap_light, skin.accent, 0.30),
            mix(skin.cap_dark, skin.accent_dim, 0.55),
        )
    } else if hovered {
        (
            mix(skin.cap_light, Color32::WHITE, 0.08),
            mix(skin.cap_dark, skin.cap_light, 0.18),
        )
    } else {
        (skin.cap_light, skin.cap_dark)
    };

    let steps = 8;
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

    if held || selected {
        // Pressed / latched: shadow across the lit top edge.
        painter.line_segment(
            [
                pos2(face.left() + r, face.top() + 0.5),
                pos2(face.right() - r, face.top() + 0.5),
            ],
            Stroke::new(1.5, with_alpha(Color32::BLACK, 120)),
        );
    } else {
        painter.line_segment(
            [
                pos2(face.left() + r, face.top() + 0.5),
                pos2(face.right() - r, face.top() + 0.5),
            ],
            Stroke::new(1.0, with_alpha(Color32::WHITE, 45)),
        );
    }

    painter.rect_stroke(
        face,
        r,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 170)),
        StrokeKind::Inside,
    );

    let lamp = pos2(face.left() + 9.0, face.center().y);
    paint_lamp(painter, lamp, 2.8, skin, if selected { 1.0 } else { 0.06 });

    let text_color = if selected { skin.legend } else { skin.legend_dim };
    painter.text(
        pos2(face.left() + 17.0, face.center().y + 1.0),
        Align2::LEFT_CENTER,
        spaced(label),
        FontId::proportional(9.5),
        with_alpha(Color32::BLACK, 130),
    );
    painter.text(
        pos2(face.left() + 17.0, face.center().y),
        Align2::LEFT_CENTER,
        spaced(label),
        FontId::proportional(9.5),
        text_color,
    );
}

/// Small indicator lamp; `level` in 0..1 drives the glow.
pub fn paint_lamp(painter: &Painter, center: Pos2, radius: f32, skin: &Skin, level: f32) {
    let level = level.clamp(0.0, 1.0);
    painter.circle_filled(center, radius + 1.5, with_alpha(Color32::BLACK, 140));
    if level > 0.02 {
        painter.circle_filled(center, radius * 2.4, with_alpha(skin.accent, (level * 55.0) as u8));
    }
    painter.circle_filled(
        center,
        radius,
        mix(skin.accent_dim, skin.accent, 0.1 + level * 0.9),
    );
    painter.circle_filled(
        center - vec2(radius * 0.3, radius * 0.3),
        radius * 0.35,
        with_alpha(Color32::WHITE, 80),
    );
}
