//! Material painting: brushed deck, raised plates, recessed wells, header badge.
//!
//! Everything is lit from the top-left, so highlights land on top/left faces and
//! contact shadows on bottom/right faces.

use nih_plug_egui::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
};

use crate::skin::Skin;

/// Blend two colours in sRGB space (good enough for these low-contrast ramps).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// Cheap deterministic hash so the grain never shimmers between frames.
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

fn rand01(seed: u32) -> f32 {
    (hash(seed) >> 8) as f32 / 16_777_216.0
}

/// Vertical gradient fill, clipped to `rect`.
fn gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32, steps: usize) {
    let steps = steps.max(1);
    let h = rect.height() / steps as f32;
    let mut shapes = Vec::with_capacity(steps);
    for i in 0..steps {
        let t = i as f32 / (steps - 1).max(1) as f32;
        let y = rect.top() + i as f32 * h;
        // Overlap by a hair so seams do not show on fractional scaling.
        let band = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), h + 1.0));
        shapes.push(Shape::rect_filled(band, 0.0, mix(top, bottom, t)));
    }
    painter.extend(shapes);
}

/// The main deck: brushed aluminium with a vignette and corner screws.
pub fn paint_chassis(painter: &Painter, rect: Rect, skin: &Skin) {
    gradient(painter, rect, skin.deck_top, skin.deck_bottom, 48);

    // Brushed grain: fine vertical striations, direction consistent everywhere.
    let mut shapes = Vec::new();
    let mut x = rect.left();
    let mut seed = 1u32;
    while x < rect.right() {
        let n = rand01(seed);
        let alpha = (n * 16.0) as u8;
        let color = if n > 0.5 {
            with_alpha(Color32::WHITE, alpha)
        } else {
            with_alpha(Color32::BLACK, alpha + 4)
        };
        shapes.push(Shape::line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, color),
        ));
        x += 1.0 + rand01(seed ^ 0x9e37) * 2.0;
        seed = seed.wrapping_add(1);
    }

    // Matte grain so large flat areas do not look like flat vector fills.
    let dots = ((rect.area() / 900.0) as usize).min(900);
    for i in 0..dots {
        let px = rect.left() + rand01(i as u32 * 3 + 7) * rect.width();
        let py = rect.top() + rand01(i as u32 * 3 + 11) * rect.height();
        let bright = rand01(i as u32 * 3 + 13) > 0.5;
        let c = if bright {
            with_alpha(Color32::WHITE, 12)
        } else {
            with_alpha(Color32::BLACK, 16)
        };
        shapes.push(Shape::rect_filled(
            Rect::from_min_size(pos2(px, py), vec2(1.0, 1.0)),
            0.0,
            c,
        ));
    }
    painter.extend(shapes);

    // Vignette: darker towards the rim, keeps focus on the controls.
    let mut shapes = Vec::new();
    for i in 0..14 {
        let inset = i as f32 * 3.0;
        let alpha = (14 - i) as u8 * 2;
        shapes.push(Shape::rect_stroke(
            rect.shrink(inset),
            0.0,
            Stroke::new(3.0, with_alpha(Color32::BLACK, alpha)),
            StrokeKind::Inside,
        ));
    }
    painter.extend(shapes);

    // Case bevel.
    painter.line_segment(
        [
            pos2(rect.left(), rect.top() + 0.5),
            pos2(rect.right(), rect.top() + 0.5),
        ],
        Stroke::new(1.0, with_alpha(skin.edge_light, 130)),
    );
    painter.line_segment(
        [
            pos2(rect.left() + 0.5, rect.top()),
            pos2(rect.left() + 0.5, rect.bottom()),
        ],
        Stroke::new(1.0, with_alpha(skin.edge_light, 70)),
    );
    painter.line_segment(
        [
            pos2(rect.left(), rect.bottom() - 0.5),
            pos2(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, with_alpha(Color32::BLACK, 160)),
    );

    for (dx, dy) in [(13.0, 13.0), (-13.0, 13.0), (13.0, -13.0), (-13.0, -13.0)] {
        let cx = if dx > 0.0 {
            rect.left() + dx
        } else {
            rect.right() + dx
        };
        let cy = if dy > 0.0 {
            rect.top() + dy
        } else {
            rect.bottom() + dy
        };
        paint_screw(painter, pos2(cx, cy), 5.0, skin);
    }
}

/// Countersunk slot screw — the one deliberately literal detail.
pub fn paint_screw(painter: &Painter, center: Pos2, radius: f32, skin: &Skin) {
    painter.circle_filled(
        center + vec2(0.0, 1.0),
        radius,
        with_alpha(Color32::BLACK, 90),
    );
    painter.circle_filled(center, radius, mix(skin.cap_dark, skin.cap_light, 0.35));
    painter.circle_filled(
        center - vec2(radius * 0.18, radius * 0.18),
        radius * 0.8,
        mix(skin.cap_dark, skin.cap_light, 0.62),
    );
    let angle = 0.6f32;
    let (s, c) = angle.sin_cos();
    let d = vec2(c, s) * (radius * 0.62);
    painter.line_segment(
        [center - d, center + d],
        Stroke::new(1.6, with_alpha(Color32::BLACK, 170)),
    );
    painter.line_segment(
        [center - d + vec2(0.0, 1.0), center + d + vec2(0.0, 1.0)],
        Stroke::new(1.0, with_alpha(Color32::WHITE, 26)),
    );
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 120)),
    );
}

/// A raised section plate. Returns the padded content rect.
pub fn paint_plate(painter: &Painter, rect: Rect, skin: &Skin) -> Rect {
    let r = 7.0;
    painter.rect_filled(
        rect.translate(vec2(0.0, 3.0)),
        r,
        with_alpha(Color32::BLACK, 100),
    );
    painter.rect_filled(rect, r, skin.plate);

    // Key light across the upper half, shade towards the bottom. Both bands are
    // inset horizontally so they never spill past the rounded corners.
    let gloss = Rect::from_min_size(
        rect.min + vec2(r, 1.0),
        vec2(rect.width() - r * 2.0, (rect.height() * 0.30).min(24.0)),
    );
    gradient(
        painter,
        gloss,
        with_alpha(Color32::WHITE, 18),
        with_alpha(Color32::WHITE, 0),
        10,
    );
    let shade_h = (rect.height() * 0.3).min(24.0);
    let shade = Rect::from_min_size(
        pos2(rect.left() + r, rect.bottom() - shade_h - 1.0),
        vec2(rect.width() - r * 2.0, shade_h),
    );
    gradient(
        painter,
        shade,
        with_alpha(Color32::BLACK, 0),
        with_alpha(Color32::BLACK, 34),
        10,
    );
    painter.line_segment(
        [
            pos2(rect.left() + r, rect.top() + 0.5),
            pos2(rect.right() - r, rect.top() + 0.5),
        ],
        Stroke::new(1.0, with_alpha(skin.plate_lip, 200)),
    );
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, with_alpha(skin.edge_dark, 180)),
        StrokeKind::Inside,
    );
    rect.shrink(10.0)
}

/// Small engraved legend above a plate's contents. Returns the remaining rect.
pub fn plate_legend(painter: &Painter, rect: Rect, skin: &Skin, title: &str) -> Rect {
    let baseline = pos2(rect.left() + 1.0, rect.top());
    engrave(
        painter,
        baseline,
        Align2::LEFT_TOP,
        &spaced(title),
        FontId::proportional(12.0),
        skin.legend,
    );
    let line_y = rect.top() + 17.5;
    painter.line_segment(
        [pos2(rect.left(), line_y), pos2(rect.right(), line_y)],
        Stroke::new(1.0, with_alpha(Color32::BLACK, 90)),
    );
    painter.line_segment(
        [
            pos2(rect.left(), line_y + 1.0),
            pos2(rect.right(), line_y + 1.0),
        ],
        Stroke::new(1.0, with_alpha(Color32::WHITE, 14)),
    );
    Rect::from_min_max(pos2(rect.left(), line_y + 8.0), rect.max)
}

/// Text with a cut shadow above and a light bounce below — reads as engraved.
pub fn engrave(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    color: Color32,
) -> Rect {
    painter.text(
        pos - vec2(0.0, 1.0),
        anchor,
        text,
        font.clone(),
        with_alpha(Color32::BLACK, 150),
    );
    painter.text(
        pos + vec2(0.0, 1.0),
        anchor,
        text,
        font.clone(),
        with_alpha(Color32::WHITE, 22),
    );
    painter.text(pos, anchor, text, font, color)
}

/// Letter-spaced caps for instrument legends.
pub fn spaced(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for (i, ch) in text.chars().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        for up in ch.to_uppercase() {
            out.push(up);
        }
    }
    out
}

/// A recessed glass well (scopes, graphs, readouts). Returns the inner rect.
pub fn paint_well(painter: &Painter, rect: Rect, skin: &Skin) -> Rect {
    let r = 5.0;
    painter.rect_filled(rect, r, skin.well);

    // Inner shadow on the lit faces, light bounce on the far faces.
    let mut shapes = Vec::new();
    for i in 0..4 {
        let a = (70 - i * 16).max(0) as u8;
        let y = rect.top() + i as f32 + 0.5;
        shapes.push(Shape::line_segment(
            [pos2(rect.left() + r, y), pos2(rect.right() - r, y)],
            Stroke::new(1.0, with_alpha(Color32::BLACK, a)),
        ));
        let x = rect.left() + i as f32 + 0.5;
        shapes.push(Shape::line_segment(
            [pos2(x, rect.top() + r), pos2(x, rect.bottom() - r)],
            Stroke::new(1.0, with_alpha(Color32::BLACK, a.saturating_sub(20))),
        ));
    }
    painter.extend(shapes);
    painter.line_segment(
        [
            pos2(rect.left() + r, rect.bottom() - 0.5),
            pos2(rect.right() - r, rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, with_alpha(skin.edge_light, 90)),
    );

    // Glass: a shallow sheen along the top edge only.
    let sheen = Rect::from_min_size(
        rect.min + vec2(r, 1.0),
        vec2(rect.width() - r * 2.0, (rect.height() * 0.18).min(10.0)),
    );
    gradient(
        painter,
        sheen,
        with_alpha(Color32::WHITE, 12),
        with_alpha(Color32::WHITE, 0),
        6,
    );

    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 200)),
        StrokeKind::Inside,
    );
    rect.shrink(4.0)
}

/// Name badge with an activity jewel. `level` is a 0..1 output level.
pub fn paint_header(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    title: &str,
    subtitle: &str,
    level: f32,
) {
    let inner = paint_well(painter, rect, skin);
    let jewel_r = (inner.height() * 0.24).clamp(5.0, 9.0);
    let jewel = pos2(inner.right() - jewel_r - 6.0, inner.center().y);

    engrave(
        painter,
        pos2(inner.left() + 6.0, inner.center().y - 8.0),
        Align2::LEFT_CENTER,
        &spaced(title),
        FontId::proportional(18.0),
        skin.legend,
    );
    painter.text(
        pos2(inner.left() + 7.0, inner.center().y + 12.0),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::monospace(11.0),
        skin.legend,
    );

    // Jewel LED: bezel, glow proportional to output, glass highlight.
    let level = level.clamp(0.0, 1.0);
    painter.circle_filled(
        jewel,
        jewel_r + 2.0,
        mix(skin.cap_dark, Color32::BLACK, 0.4),
    );
    painter.circle_stroke(
        jewel,
        jewel_r + 2.0,
        Stroke::new(1.0, with_alpha(skin.edge_light, 70)),
    );
    for i in 0..3 {
        let spread = jewel_r * (1.4 + i as f32 * 0.7);
        let alpha = (level * 70.0) as u8 >> i;
        painter.circle_filled(jewel, spread, with_alpha(skin.accent, alpha));
    }
    painter.circle_filled(
        jewel,
        jewel_r,
        mix(skin.accent_dim, skin.accent, 0.15 + level * 0.85),
    );
    painter.circle_filled(
        jewel - vec2(jewel_r * 0.3, jewel_r * 0.35),
        jewel_r * 0.34,
        with_alpha(Color32::WHITE, 90),
    );
}

/// Right-aligned monospace value readout with a subtle accent glow.
pub fn readout(painter: &Painter, rect: Rect, skin: &Skin, text: &str, size: f32) {
    painter.text(
        rect.center() + vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(size),
        with_alpha(Color32::BLACK, 160),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(size),
        skin.readout,
    );
}

/// Points along an arc. Angles are screen-space radians (y down, clockwise).
pub fn arc_points(center: Pos2, radius: f32, from: f32, to: f32, segments: usize) -> Vec<Pos2> {
    let segments = segments.max(2);
    (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            let a = from + (to - from) * t;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

pub fn stroke_arc(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    from: f32,
    to: f32,
    stroke: Stroke,
) {
    let segments = (((to - from).abs() * radius / 3.0) as usize).clamp(6, 96);
    painter.add(Shape::line(
        arc_points(center, radius, from, to, segments),
        stroke,
    ));
}

/// Reset egui's own frame so only our painting shows through.
pub fn frame(skin: &Skin) -> egui::Frame {
    egui::Frame::NONE.fill(skin.deck_bottom)
}
