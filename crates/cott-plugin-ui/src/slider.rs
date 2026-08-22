//! Vertical fader bound to a nih-plug parameter.

use nih_plug::prelude::{Param, ParamSetter};
use nih_plug_egui::egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, Ui,
};

use crate::chassis::{engrave, mix, with_alpha};
use crate::skin::Skin;

const DRAG_RANGE_PX: f32 = 160.0;
const FINE_DRAG_RANGE_PX: f32 = 900.0;

/// Vertical slider. Drag up to increase, shift for fine, double-click to reset.
pub fn param_slider<P: Param>(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    setter: &ParamSetter,
    param: &P,
    label: &str,
) {
    let id = ui.id().with("cott_slider").with(param.name());
    let response = ui.interact(rect, id, Sense::click_and_drag());

    let stored: Option<f32> = ui.ctx().data(|d| d.get_temp(id));
    let mut norm = stored.unwrap_or_else(|| param.unmodulated_normalized_value());
    let mut interacting = stored.is_some();

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

    if interacting {
        ui.ctx().data_mut(|d| d.insert_temp(id, norm));
    } else {
        ui.ctx().data_mut(|d| d.remove::<f32>(id));
    }

    let shown = match param.step_count() {
        Some(steps) if steps > 0 => {
            let steps = steps as f32;
            (norm * steps).round() / steps
        }
        _ => norm,
    };

    paint_slider(
        ui,
        rect,
        skin,
        shown,
        label,
        &param.normalized_value_to_string(shown, true),
        response.hovered() || response.dragged(),
    );
    if response.hovered() {
        response.on_hover_cursor(CursorIcon::ResizeVertical);
    }
}

fn paint_slider(
    ui: &mut Ui,
    rect: Rect,
    skin: &Skin,
    norm: f32,
    label: &str,
    value: &str,
    hot: bool,
) {
    let painter = ui.painter();
    let label_h = 16.0;
    let value_h = 16.0;
    engrave(
        painter,
        pos2(rect.center().x, rect.top() + 8.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        skin.legend,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_CENTER,
        value,
        FontId::monospace(11.0),
        skin.readout,
    );

    let track = Rect::from_center_size(
        pos2(rect.center().x, rect.center().y + 2.0),
        vec2(12.0, (rect.height() - label_h - value_h - 10.0).max(20.0)),
    );
    painter.rect_filled(track, 4.0, with_alpha(Color32::BLACK, 180));
    painter.rect_stroke(
        track,
        4.0,
        Stroke::new(1.0, with_alpha(skin.edge_light, 40)),
        StrokeKind::Inside,
    );

    let fill_h = track.height() * norm.clamp(0.0, 1.0);
    let fill = Rect::from_min_max(
        pos2(track.left() + 1.0, track.bottom() - fill_h),
        pos2(track.right() - 1.0, track.bottom() - 1.0),
    );
    painter.rect_filled(fill, 3.0, mix(skin.accent_dim, skin.accent, 0.35));

    let cap_y = track.bottom() - fill_h;
    let cap = Rect::from_center_size(pos2(track.center().x, cap_y), vec2(18.0, 8.0));
    painter.rect_filled(
        cap.translate(vec2(0.0, 1.5)),
        2.0,
        with_alpha(Color32::BLACK, 90),
    );
    painter.rect_filled(
        cap,
        2.0,
        if hot {
            mix(skin.cap_light, skin.accent, 0.25)
        } else {
            skin.cap_light
        },
    );
    painter.rect_stroke(
        cap,
        2.0,
        Stroke::new(1.0, with_alpha(Color32::BLACK, 150)),
        StrokeKind::Inside,
    );
}
