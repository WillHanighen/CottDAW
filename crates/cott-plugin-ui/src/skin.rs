//! Palette for the shared plugin chassis.
//!
//! One implied key light from the top-left: every highlight sits on a top/left
//! face and every shadow on a bottom/right face. Widgets read the same [`Skin`]
//! so all first-party plugins look like the same piece of hardware with a
//! different jewel colour.

use nih_plug_egui::egui::{self, Color32};

#[derive(Debug, Clone, Copy)]
pub struct Skin {
    /// Brushed deck, lit face.
    pub deck_top: Color32,
    /// Brushed deck, shaded face.
    pub deck_bottom: Color32,
    /// Raised section plate.
    pub plate: Color32,
    pub plate_lip: Color32,
    /// Recessed wells (scopes, engraved header).
    pub well: Color32,
    /// Shared edge colours.
    pub edge_light: Color32,
    pub edge_dark: Color32,
    /// Knob / button cap dome.
    pub cap_light: Color32,
    pub cap_dark: Color32,
    /// Engraved legends and secondary text.
    pub legend: Color32,
    pub legend_dim: Color32,
    /// Jewel colour: arcs, LEDs, lit buttons.
    pub accent: Color32,
    pub accent_dim: Color32,
    /// Numeric readouts inside wells.
    pub readout: Color32,
}

impl Skin {
    const fn chassis(accent: Color32, accent_dim: Color32, readout: Color32) -> Self {
        Self {
            deck_top: Color32::from_rgb(54, 57, 64),
            deck_bottom: Color32::from_rgb(26, 28, 32),
            plate: Color32::from_rgb(52, 55, 62),
            plate_lip: Color32::from_rgb(82, 87, 97),
            well: Color32::from_rgb(15, 16, 20),
            edge_light: Color32::from_rgb(104, 109, 120),
            edge_dark: Color32::from_rgb(12, 13, 16),
            cap_light: Color32::from_rgb(84, 89, 99),
            cap_dark: Color32::from_rgb(33, 35, 41),
            legend: Color32::from_rgb(236, 232, 220),
            legend_dim: Color32::from_rgb(188, 186, 176),
            accent,
            accent_dim,
            readout,
        }
    }

    /// CottWhistle — warm amber.
    pub const fn amber() -> Self {
        Self::chassis(
            Color32::from_rgb(255, 176, 74),
            Color32::from_rgb(118, 74, 30),
            Color32::from_rgb(255, 208, 146),
        )
    }

    /// CottSynth — the existing teal.
    pub const fn teal() -> Self {
        Self::chassis(
            Color32::from_rgb(112, 214, 194),
            Color32::from_rgb(38, 94, 88),
            Color32::from_rgb(168, 238, 222),
        )
    }

    /// CottFilter — cool steel / cyan.
    pub const fn steel() -> Self {
        Self::chassis(
            Color32::from_rgb(122, 190, 236),
            Color32::from_rgb(38, 82, 110),
            Color32::from_rgb(180, 220, 250),
        )
    }
}

/// Neutral egui visuals so stray built-in widgets do not flash white.
pub fn apply_visuals(ctx: &egui::Context, skin: &Skin) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = skin.deck_bottom;
    visuals.window_fill = skin.deck_bottom;
    visuals.extreme_bg_color = skin.well;
    visuals.override_text_color = Some(skin.legend);
    visuals.widgets.inactive.bg_fill = skin.plate;
    visuals.widgets.hovered.bg_fill = skin.plate_lip;
    visuals.widgets.active.bg_fill = skin.accent_dim;
    visuals.selection.bg_fill = skin.accent_dim;
    ctx.set_visuals(visuals);
}
