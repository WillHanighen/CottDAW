//! Shared skeuomorphic panel kit for the Cottage VST3 plugins.
//!
//! Every first-party plugin paints the same chassis — brushed deck, raised
//! plates, recessed wells, machined dials — and differs only by its accent
//! jewel ([`Skin::amber`], [`Skin::teal`], [`Skin::steel`], [`Skin::dusk`],
//! [`Skin::grain`], [`Skin::rust`], [`Skin::ink`]).
//!
//! Panels are laid out with explicit rects (see [`layout`]) instead of egui
//! layouts, so the hardware alignment survives resizing.

pub mod button;
pub mod chassis;
pub mod knob;
pub mod layout;
pub mod paddle;
pub mod scale;
pub mod scope;
pub mod skin;
pub mod slider;

pub use button::{paint_lamp, segment_button};
pub use chassis::{
    engrave, mix, paint_chassis, paint_header, paint_plate, paint_screw, paint_well, plate_legend,
    readout, spaced, with_alpha,
};
pub use knob::{display_knob, paint_knob, param_knob, param_knob_enabled};
pub use paddle::{paddle, PaddleThrow};
pub use scale::{display_scale, physical_size};
pub use scope::{
    paint_curve, paint_curve_filled, paint_envelope, paint_grid, paint_marker, paint_waveform,
    ScopeBuffer, SCOPE_LEN,
};
pub use skin::{apply_visuals, Skin};
pub use slider::param_slider;

use nih_plug_egui::egui::{self, Rect, Sense, Ui};

/// Allocate the whole editor area and paint the deck into it.
///
/// Returns the padded content rect that panels should lay out inside.
pub fn begin_panel(ui: &mut Ui, skin: &Skin) -> Rect {
    let (rect, _response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    paint_chassis(ui.painter(), rect, skin);
    rect.shrink(14.0)
}

/// Frame for the plugin window itself, so nothing flashes white on resize.
pub fn window_frame(skin: &Skin) -> egui::Frame {
    chassis::frame(skin)
}
