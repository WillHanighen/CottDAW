//! Lock-free waveform capture shared between the audio thread and the editor,
//! plus the drawing routines for the glass wells.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use nih_plug_egui::egui::{pos2, Painter, Pos2, Rect, Shape, Stroke};

use crate::chassis::with_alpha;
use crate::skin::Skin;

/// Captured samples. Long enough to trigger a stable window out of it.
pub const SCOPE_LEN: usize = 512;
/// Samples actually drawn.
pub const SCOPE_WINDOW: usize = 256;

/// Ring of samples written by `process()` and read by the editor.
///
/// Torn reads are possible and harmless here: the worst case is one frame of a
/// slightly mixed waveform, and nothing on the audio thread ever blocks.
pub struct ScopeBuffer {
    samples: [AtomicU32; SCOPE_LEN],
    write: AtomicUsize,
    level: AtomicU32,
}

impl Default for ScopeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScopeBuffer {
    pub fn new() -> Self {
        Self {
            samples: std::array::from_fn(|_| AtomicU32::new(0)),
            write: AtomicUsize::new(0),
            level: AtomicU32::new(0),
        }
    }

    /// Push one processed block. Realtime safe: no locks, no allocation.
    pub fn push(&self, block: &[f32]) {
        if block.is_empty() {
            return;
        }
        let mut peak = 0.0f32;
        let mut idx = self.write.load(Ordering::Relaxed);
        for &s in block {
            peak = peak.max(s.abs());
            self.samples[idx].store(s.to_bits(), Ordering::Relaxed);
            idx = (idx + 1) % SCOPE_LEN;
        }
        self.write.store(idx, Ordering::Relaxed);

        let prev = f32::from_bits(self.level.load(Ordering::Relaxed));
        let smoothed = peak.max(prev * 0.82);
        self.level.store(smoothed.to_bits(), Ordering::Relaxed);
    }

    /// Current smoothed output level (0..1-ish) for the header jewel.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }

    /// Copy the most recent samples in chronological order.
    pub fn snapshot(&self, out: &mut [f32; SCOPE_LEN]) {
        let write = self.write.load(Ordering::Relaxed);
        for (i, slot) in out.iter_mut().enumerate() {
            let idx = (write + i) % SCOPE_LEN;
            *slot = f32::from_bits(self.samples[idx].load(Ordering::Relaxed));
        }
    }
}

/// Faint measurement grid for a well.
pub fn paint_grid(painter: &Painter, rect: Rect, skin: &Skin, cols: usize, rows: usize) {
    let mut shapes = Vec::new();
    let line = with_alpha(skin.legend_dim, 26);
    for i in 1..cols {
        let x = rect.left() + rect.width() * i as f32 / cols as f32;
        shapes.push(Shape::line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, line),
        ));
    }
    for i in 1..rows {
        let y = rect.top() + rect.height() * i as f32 / rows as f32;
        shapes.push(Shape::line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, line),
        ));
    }
    painter.extend(shapes);
}

/// Trace with a soft bloom under it, the way a lit display reads.
pub fn paint_trace(painter: &Painter, points: Vec<Pos2>, skin: &Skin) {
    if points.len() < 2 {
        return;
    }
    painter.add(Shape::line(
        points.clone(),
        Stroke::new(4.0, with_alpha(skin.accent, 34)),
    ));
    painter.add(Shape::line(points, Stroke::new(1.6, skin.readout)));
}

/// Draw a live waveform, triggered on a rising zero crossing so it holds still.
pub fn paint_waveform(painter: &Painter, rect: Rect, skin: &Skin, samples: &[f32; SCOPE_LEN]) {
    paint_grid(painter, rect, skin, 8, 4);
    painter.line_segment(
        [
            pos2(rect.left(), rect.center().y),
            pos2(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0, with_alpha(skin.legend_dim, 50)),
    );

    let search = SCOPE_LEN - SCOPE_WINDOW;
    let mut start = 0usize;
    for i in 1..search {
        if samples[i - 1] <= 0.0 && samples[i] > 0.0 {
            start = i;
            break;
        }
    }

    let points: Vec<Pos2> = (0..SCOPE_WINDOW)
        .map(|i| {
            let s = samples[start + i].clamp(-1.0, 1.0);
            let x = rect.left() + rect.width() * i as f32 / (SCOPE_WINDOW - 1) as f32;
            let y = rect.center().y - s * rect.height() * 0.44;
            pos2(x, y)
        })
        .collect();
    paint_trace(painter, points, skin);
}

/// Draw an arbitrary curve. `f` maps x in 0..1 to y in 0..1 (1 = top).
pub fn paint_curve(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    resolution: usize,
    f: impl FnMut(f32) -> f32,
) {
    paint_trace(painter, curve_points(rect, resolution, f), skin);
}

/// As [`paint_curve`], with the area under the curve tinted. Use for shapes
/// read against a baseline (filter responses), not for bipolar waveforms.
pub fn paint_curve_filled(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    resolution: usize,
    f: impl FnMut(f32) -> f32,
) {
    let points = curve_points(rect, resolution, f);
    fill_under(painter, &points, rect.bottom(), with_alpha(skin.accent, 22));
    paint_trace(painter, points, skin);
}

/// Tint the area between a polyline and a baseline.
///
/// Emitted as one trapezoid per segment: egui only tessellates *convex*
/// polygons correctly, and response / envelope shapes are not convex.
fn fill_under(
    painter: &Painter,
    points: &[Pos2],
    baseline_y: f32,
    color: nih_plug_egui::egui::Color32,
) {
    if points.len() < 2 {
        return;
    }
    let mut shapes = Vec::with_capacity(points.len() - 1);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if (b.x - a.x).abs() < f32::EPSILON {
            continue;
        }
        shapes.push(Shape::convex_polygon(
            vec![a, b, pos2(b.x, baseline_y), pos2(a.x, baseline_y)],
            color,
            Stroke::NONE,
        ));
    }
    painter.extend(shapes);
}

fn curve_points(rect: Rect, resolution: usize, mut f: impl FnMut(f32) -> f32) -> Vec<Pos2> {
    let resolution = resolution.max(2);
    (0..resolution)
        .map(|i| {
            let t = i as f32 / (resolution - 1) as f32;
            let v = f(t).clamp(0.0, 1.0);
            pos2(
                rect.left() + rect.width() * t,
                rect.bottom() - rect.height() * v,
            )
        })
        .collect()
}

/// Marker line with a caption, used for cutoff / formant position.
pub fn paint_marker(painter: &Painter, rect: Rect, skin: &Skin, x_norm: f32, label: &str) {
    let x = rect.left() + rect.width() * x_norm.clamp(0.0, 1.0);
    painter.line_segment(
        [pos2(x, rect.top()), pos2(x, rect.bottom())],
        Stroke::new(1.0, with_alpha(skin.accent, 120)),
    );
    // Flip the caption inward once the line gets close to the right edge.
    let width = label.chars().count() as f32 * 5.6;
    let (anchor, tx) = if x + width + 6.0 > rect.right() {
        (nih_plug_egui::egui::Align2::RIGHT_TOP, x - 3.0)
    } else {
        (nih_plug_egui::egui::Align2::LEFT_TOP, x + 3.0)
    };
    painter.text(
        pos2(tx, rect.top() + 2.0),
        anchor,
        label,
        nih_plug_egui::egui::FontId::monospace(8.5),
        with_alpha(skin.readout, 190),
    );
}

/// Envelope shape display (attack / decay / sustain / release in seconds).
pub fn paint_envelope(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    attack_s: f32,
    decay_s: f32,
    sustain: f32,
    release_s: f32,
) {
    paint_grid(painter, rect, skin, 6, 3);
    let hold = ((attack_s + decay_s + release_s) * 0.35).max(0.08);
    let total = (attack_s + decay_s + hold + release_s).max(1e-3);
    let x = |t: f32| rect.left() + rect.width() * (t / total).clamp(0.0, 1.0);
    let y = |v: f32| rect.bottom() - rect.height() * v.clamp(0.0, 1.0) * 0.92 - 2.0;

    let t_a = attack_s;
    let t_d = t_a + decay_s;
    let t_s = t_d + hold;
    let t_r = t_s + release_s;
    let points = vec![
        pos2(x(0.0), y(0.0)),
        pos2(x(t_a), y(1.0)),
        pos2(x(t_d), y(sustain)),
        pos2(x(t_s), y(sustain)),
        pos2(x(t_r), y(0.0)),
    ];

    fill_under(painter, &points, rect.bottom(), with_alpha(skin.accent, 20));
    paint_trace(painter, points, skin);

    // Stage ticks along the baseline.
    for (t, tag) in [(t_a, "A"), (t_d, "D"), (t_s, "S"), (t_r, "R")] {
        let px = x(t);
        painter.line_segment(
            [pos2(px, rect.bottom() - 3.0), pos2(px, rect.bottom())],
            Stroke::new(1.0, with_alpha(skin.legend_dim, 90)),
        );
        painter.text(
            pos2(px, rect.bottom() - 4.0),
            nih_plug_egui::egui::Align2::CENTER_BOTTOM,
            tag,
            nih_plug_egui::egui::FontId::monospace(8.0),
            with_alpha(skin.legend_dim, 130),
        );
    }
}
