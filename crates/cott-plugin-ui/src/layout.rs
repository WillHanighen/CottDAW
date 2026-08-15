//! Rect splitting helpers.
//!
//! The panels are painted into explicit rects rather than egui layouts so the
//! hardware alignment (knob rows, plate margins) stays exact at any size.

use nih_plug_egui::egui::{Rect, Vec2};

/// Split `rect` into `n` equal columns separated by `gap`.
pub fn columns(rect: Rect, n: usize, gap: f32) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let total_gap = gap * (n.saturating_sub(1)) as f32;
    let w = ((rect.width() - total_gap) / n as f32).max(0.0);
    (0..n)
        .map(|i| {
            let x = rect.left() + i as f32 * (w + gap);
            Rect::from_min_size(
                nih_plug_egui::egui::pos2(x, rect.top()),
                Vec2::new(w, rect.height()),
            )
        })
        .collect()
}

/// Split `rect` into `n` equal rows separated by `gap`.
pub fn rows(rect: Rect, n: usize, gap: f32) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let total_gap = gap * (n.saturating_sub(1)) as f32;
    let h = ((rect.height() - total_gap) / n as f32).max(0.0);
    (0..n)
        .map(|i| {
            let y = rect.top() + i as f32 * (h + gap);
            Rect::from_min_size(
                nih_plug_egui::egui::pos2(rect.left(), y),
                Vec2::new(rect.width(), h),
            )
        })
        .collect()
}

/// Take `height` off the top, returning `(taken, remainder_after_gap)`.
pub fn split_top(rect: Rect, height: f32, gap: f32) -> (Rect, Rect) {
    let height = height.min(rect.height());
    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), height));
    let rest = Rect::from_min_max(
        nih_plug_egui::egui::pos2(rect.left(), (rect.top() + height + gap).min(rect.bottom())),
        rect.max,
    );
    (top, rest)
}

/// Take `width` off the left, returning `(taken, remainder_after_gap)`.
pub fn split_left(rect: Rect, width: f32, gap: f32) -> (Rect, Rect) {
    let width = width.min(rect.width());
    let left = Rect::from_min_size(rect.min, Vec2::new(width, rect.height()));
    let rest = Rect::from_min_max(
        nih_plug_egui::egui::pos2((rect.left() + width + gap).min(rect.right()), rect.top()),
        rect.max,
    );
    (left, rest)
}

/// Centre a fixed-size box inside `rect`.
pub fn centered(rect: Rect, size: Vec2) -> Rect {
    Rect::from_center_size(rect.center(), size.min(rect.size()))
}
