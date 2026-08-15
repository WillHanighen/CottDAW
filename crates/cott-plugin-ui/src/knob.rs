//! Machined dial widget bound to nih-plug parameters.

use nih_plug::prelude::{Param, ParamSetter};
use nih_plug_egui::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, Sense,
    Stroke, Ui,
};

use crate::chassis::{engrave, mix, spaced, stroke_arc, with_alpha};
use crate::skin::Skin;

/// Sweep start / end in screen-space radians (y down, clockwise from +x).
const ARC_START: f32 = std::f32::consts::PI * 0.75;
const ARC_SWEEP: f32 = std::f32::consts::PI * 1.5;

/// Pixels of vertical drag for the full range.
const DRAG_RANGE_PX: f32 = 190.0;
const FINE_DRAG_RANGE_PX: f32 = 1400.0;

fn angle_of(norm: f32) -> f32 {
    ARC_START + norm.clamp(0.0, 1.0) * ARC_SWEEP
}

/// Dial for a nih-plug parameter. Vertical drag, shift for fine, double-click
/// to restore the default, scroll wheel for nudges.
pub fn param_knob<P: Param>(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    setter: &ParamSetter,
    param: &P,
    label: &str,
) -> Response {
    param_knob_enabled(ui, rect, skin, setter, param, label, true)
}

/// As [`param_knob`], but greyed out and inert when `enabled` is false.
pub fn param_knob_enabled<P: Param>(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    setter: &ParamSetter,
    param: &P,
    label: &str,
    enabled: bool,
) -> Response {
    let id = ui.id().with("cott_knob").with(param.name());
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);

    // nih-plug only echoes GUI writes back through `process()`, so binding the
    // dial straight to `param.value()` snaps it back mid-drag. Hold the gesture
    // value in egui memory until the drag ends.
    let stored: Option<f32> = ui.ctx().data(|d| d.get_temp(id));
    let mut norm = stored.unwrap_or_else(|| param.unmodulated_normalized_value());
    let mut interacting = stored.is_some();

    if enabled {
        if response.drag_started() {
            setter.begin_set_parameter(param);
            norm = param.unmodulated_normalized_value();
            interacting = true;
        }

        let drag = response.drag_delta();
        if response.dragged() && drag.y != 0.0 {
            let fine = ui.input(|i| i.modifiers.shift);
            let span = if fine {
                FINE_DRAG_RANGE_PX
            } else {
                DRAG_RANGE_PX
            };
            norm = (norm - drag.y / span).clamp(0.0, 1.0);
            setter.set_parameter_normalized(param, norm);
            interacting = true;
        }

        if response.drag_stopped() {
            setter.end_set_parameter(param);
            interacting = false;
        }

        if response.double_clicked() {
            norm = param.default_normalized_value();
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, norm);
            setter.end_set_parameter(param);
            interacting = false;
        }

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let step = match param.step_count() {
                    Some(steps) if steps > 0 => 1.0 / steps as f32,
                    _ => 0.02,
                };
                norm = (norm + step * scroll.signum()).clamp(0.0, 1.0);
                setter.begin_set_parameter(param);
                setter.set_parameter_normalized(param, norm);
                setter.end_set_parameter(param);
            }
        }
    }

    if interacting {
        ui.ctx().data_mut(|d| d.insert_temp(id, norm));
    } else {
        ui.ctx().data_mut(|d| d.remove::<f32>(id));
    }

    // Stepped params should click between detents rather than sweep.
    let shown = match param.step_count() {
        Some(steps) if steps > 0 => {
            let steps = steps as f32;
            (norm * steps).round() / steps
        }
        _ => norm,
    };

    let value_text = param.normalized_value_to_string(shown, true);
    paint_knob(
        ui.painter(),
        rect,
        skin,
        shown,
        label,
        &value_text,
        response.hovered() || response.dragged(),
        enabled,
    );

    if enabled {
        response.on_hover_cursor(CursorIcon::ResizeVertical)
    } else {
        response
    }
}

/// Paint a dial without any parameter binding (previews, static displays).
#[allow(clippy::too_many_arguments)]
pub fn paint_knob(
    painter: &Painter,
    rect: Rect,
    skin: &Skin,
    norm: f32,
    label: &str,
    value_text: &str,
    hot: bool,
    enabled: bool,
) {
    // Text shrinks with the cell so cramped hosts still get readable dials.
    let label_h = (rect.height() * 0.16).clamp(8.0, 12.0);
    let value_h = (rect.height() * 0.20).clamp(9.0, 15.0);
    let label_font = (label_h * 0.78).clamp(6.5, 9.0);
    let value_font = (value_h * 0.66).clamp(7.0, 9.5);
    let dial_area = Rect::from_min_max(
        pos2(rect.left(), rect.top() + label_h),
        pos2(rect.right(), rect.bottom() - value_h),
    );
    let radius = (dial_area.width().min(dial_area.height()) * 0.5 - 7.0).max(6.0);
    let center = pos2(dial_area.center().x, dial_area.center().y);

    let accent = if enabled {
        skin.accent
    } else {
        with_alpha(skin.legend_dim, 120)
    };
    let legend = if enabled {
        skin.legend_dim
    } else {
        with_alpha(skin.legend_dim, 110)
    };

    engrave(
        painter,
        pos2(center.x, rect.top() + label_h * 0.5),
        Align2::CENTER_CENTER,
        &spaced(label),
        FontId::proportional(label_font),
        legend,
    );

    paint_track(painter, center, radius, norm, skin, accent, enabled);
    paint_cap(painter, center, radius, norm, skin, accent, hot && enabled);

    let value_rect = Rect::from_min_max(
        pos2(rect.left(), rect.bottom() - value_h),
        pos2(rect.right(), rect.bottom()),
    );
    let value_color = if enabled {
        skin.readout
    } else {
        with_alpha(skin.readout, 110)
    };
    painter.text(
        value_rect.center() + vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        value_text,
        FontId::monospace(value_font),
        with_alpha(Color32::BLACK, 150),
    );
    painter.text(
        value_rect.center(),
        Align2::CENTER_CENTER,
        value_text,
        FontId::monospace(value_font),
        value_color,
    );
}

fn paint_track(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    norm: f32,
    skin: &Skin,
    accent: Color32,
    enabled: bool,
) {
    let track_r = radius + 6.0;
    let end = angle_of(norm);

    // Groove: dark channel with a light bounce on its lower lip.
    stroke_arc(
        painter,
        center,
        track_r,
        ARC_START,
        ARC_START + ARC_SWEEP,
        Stroke::new(5.0, with_alpha(Color32::BLACK, 190)),
    );
    stroke_arc(
        painter,
        center,
        track_r + 2.5,
        ARC_START,
        ARC_START + ARC_SWEEP,
        Stroke::new(1.0, with_alpha(skin.edge_light, 55)),
    );

    if enabled && norm > 0.001 {
        stroke_arc(
            painter,
            center,
            track_r,
            ARC_START,
            end,
            Stroke::new(7.0, with_alpha(accent, 40)),
        );
        stroke_arc(
            painter,
            center,
            track_r,
            ARC_START,
            end,
            Stroke::new(3.0, accent),
        );
    }

    // Detent marks at the ends and centre.
    for t in [0.0f32, 0.5, 1.0] {
        let a = angle_of(t);
        let dir = vec2(a.cos(), a.sin());
        painter.line_segment(
            [
                center + dir * (track_r + 4.0),
                center + dir * (track_r + 7.0),
            ],
            Stroke::new(1.0, with_alpha(skin.legend_dim, 110)),
        );
    }
}

fn paint_cap(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    norm: f32,
    skin: &Skin,
    accent: Color32,
    hot: bool,
) {
    painter.circle_filled(
        center + vec2(0.5, 2.5),
        radius,
        with_alpha(Color32::BLACK, 110),
    );

    // Dome: concentric circles drifting towards the key light.
    let steps = 16;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let r = radius * (1.0 - 0.5 * t);
        let c = center - vec2(radius * 0.20, radius * 0.26) * t;
        painter.circle_filled(c, r, mix(skin.cap_dark, skin.cap_light, t.powf(0.75)));
    }

    // Knurled rim: fine grip lines, only strong enough to read as texture.
    let knurls = 30;
    for i in 0..knurls {
        let a = i as f32 / knurls as f32 * std::f32::consts::TAU;
        let dir = vec2(a.cos(), a.sin());
        let light = a.cos() + a.sin() < 0.0;
        let color = if light {
            with_alpha(Color32::WHITE, 26)
        } else {
            with_alpha(Color32::BLACK, 46)
        };
        painter.line_segment(
            [center + dir * (radius * 0.84), center + dir * (radius * 0.98)],
            Stroke::new(1.0, color),
        );
    }

    // Machined step around the pointer field.
    painter.circle_stroke(
        center,
        radius * 0.62,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 55)),
    );
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 150)),
    );
    stroke_arc(
        painter,
        center,
        radius - 0.5,
        std::f32::consts::PI * 1.05,
        std::f32::consts::PI * 1.85,
        Stroke::new(1.2, with_alpha(Color32::WHITE, if hot { 90 } else { 55 })),
    );

    // Pointer.
    let a = angle_of(norm);
    let dir = vec2(a.cos(), a.sin());
    let tip = center + dir * (radius * 0.88);
    painter.line_segment(
        [center + dir * (radius * 0.26) + vec2(0.0, 1.0), tip + vec2(0.0, 1.0)],
        Stroke::new(3.0, with_alpha(Color32::BLACK, 120)),
    );
    painter.line_segment(
        [center + dir * (radius * 0.26), tip],
        Stroke::new(2.5, skin.legend),
    );
    if hot {
        painter.circle_filled(tip, 2.6, with_alpha(accent, 200));
        painter.circle_filled(tip, 5.0, with_alpha(accent, 45));
    }
}

/// Convenience: an inert dial used for previews in other panels.
pub fn display_knob(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    norm: f32,
    label: &str,
    value_text: &str,
) {
    paint_knob(
        ui.painter(),
        rect,
        skin,
        norm,
        label,
        value_text,
        false,
        true,
    );
}

/// Shared id helper so panels can reserve interaction ids deterministically.
pub fn widget_id(ui: &Ui, key: &str) -> egui::Id {
    ui.id().with("cott_ui").with(key)
}
