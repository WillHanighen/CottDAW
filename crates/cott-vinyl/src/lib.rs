//! CottVinyl VST3 — stereo vinyl wear with pops, hiss, muffle, and rumble.

use std::sync::Arc;

use cott_plugin_ui::{
    begin_panel, layout, paint_header, paint_plate, paint_well, param_knob, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    segment_button, Skin,
};
use cott_vinyl_dsp::{VinylEngine, VinylParams, Wear};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::grain();

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
enum WearParam {
    #[id = "dusty"]
    #[name = "Dusty"]
    Dusty,
    #[id = "radio"]
    #[name = "Radio"]
    Radio,
    #[id = "tape"]
    #[name = "Tape"]
    Tape,
}

impl WearParam {
    const ALL: [WearParam; 3] = [WearParam::Dusty, WearParam::Radio, WearParam::Tape];

    fn label(self) -> &'static str {
        match self {
            WearParam::Dusty => "Dusty",
            WearParam::Radio => "Radio",
            WearParam::Tape => "Tape",
        }
    }

    fn to_wear(self) -> Wear {
        match self {
            WearParam::Dusty => Wear::Dusty,
            WearParam::Radio => Wear::Radio,
            WearParam::Tape => Wear::Tape,
        }
    }
}

struct CottVinyl {
    params: Arc<CottVinylParams>,
    engine: VinylEngine,
    scope: Arc<ScopeBuffer>,
}

#[derive(Params)]
struct CottVinylParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "wear"]
    wear: EnumParam<WearParam>,

    #[id = "pops"]
    pops: FloatParam,

    #[id = "hiss"]
    hiss: FloatParam,

    #[id = "muffle"]
    muffle: FloatParam,

    #[id = "rumble"]
    rumble: FloatParam,

    #[id = "mix"]
    mix: FloatParam,
}

impl Default for CottVinyl {
    fn default() -> Self {
        Self {
            params: Arc::new(CottVinylParams::default()),
            engine: VinylEngine::new(48_000.0),
            scope: Arc::new(ScopeBuffer::new()),
        }
    }
}

fn unit_param(name: &'static str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_step_size(0.01)
        .with_unit(" %")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl Default for CottVinylParams {
    fn default() -> Self {
        let dsp = VinylParams::default();
        Self {
            editor_state: EguiState::from_size(640, 420),
            wear: EnumParam::new("Wear", WearParam::Dusty),
            pops: unit_param("Pops", dsp.pops),
            hiss: unit_param("Hiss", dsp.hiss),
            muffle: unit_param("Muffle", dsp.muffle),
            rumble: unit_param("Rumble", dsp.rumble),
            mix: unit_param("Mix", dsp.mix),
        }
    }
}

impl CottVinylParams {
    fn to_dsp(&self) -> VinylParams {
        VinylParams {
            wear: self.wear.value().to_wear(),
            pops: self.pops.value(),
            hiss: self.hiss.value(),
            muffle: self.muffle.value(),
            rumble: self.rumble.value(),
            mix: self.mix.value(),
        }
    }
}

impl Plugin for CottVinyl {
    const NAME: &'static str = "CottVinyl";
    const VENDOR: &'static str = "Cottage";
    const URL: &'static str = "https://github.com/cottage-end/CottDAW";
    const EMAIL: &'static str = "dev@cottage-end.local";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let egui_state = self.params.editor_state.clone();
        let scope = self.scope.clone();

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |ctx, _| cott_plugin_ui::apply_visuals(ctx, &SKIN),
            move |egui_ctx, setter, _state| {
                egui_ctx.request_repaint();
                ResizableWindow::new("cott_vinyl_resize")
                    .min_size(Vec2::new(400.0, 260.0))
                    .show(egui_ctx, egui_state.as_ref(), |ui| {
                        draw_panel(ui, setter, &params, &scope);
                    });
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let params = self.params.to_dsp();
        let slices = buffer.as_slice();
        if slices.len() >= 2 {
            let (left, right) = slices.split_at_mut(1);
            self.engine.process_block(&params, left[0], right[0]);
            self.scope.push(left[0]);
        } else if let Some(mono) = slices.first_mut() {
            let mut right = mono.to_vec();
            self.engine.process_block(&params, mono, &mut right);
            self.scope.push(mono);
        }
        // Keep running on silence so pops and hiss don't die when the track is empty.
        ProcessStatus::KeepAlive
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottVinylParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottVinyl",
        "Wear and dust",
        scope.level(),
    );

    let (wear_row, rest) = layout::split_top(rest, 42.0, 10.0);
    let inner = paint_plate(ui.painter(), wear_row, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Wear");
    let current = params.wear.value();
    let cells = layout::columns(inner, WearParam::ALL.len(), 8.0);
    for (cell, wear) in cells.iter().zip(WearParam::ALL) {
        let selected = wear == current;
        let key = format!("wear_{}", wear.label());
        if segment_button(ui, *cell, &SKIN, &key, wear.label(), selected).clicked() && !selected {
            setter.begin_set_parameter(&params.wear);
            setter.set_parameter(&params.wear, wear);
            setter.end_set_parameter(&params.wear);
        }
    }

    let (knobs, trace) = layout::split_top(rest, rest.height() * 0.58, 10.0);
    let inner = paint_plate(ui.painter(), knobs, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Record");
    let cells = layout::columns(inner, 5, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.pops, "Pops");
    param_knob(ui, cells[1], &SKIN, setter, &params.hiss, "Hiss");
    param_knob(ui, cells[2], &SKIN, setter, &params.muffle, "Muffle");
    param_knob(ui, cells[3], &SKIN, setter, &params.rumble, "Rumble");
    param_knob(ui, cells[4], &SKIN, setter, &params.mix, "Mix");

    let inner = paint_plate(ui.painter(), trace, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Out");
    let well = paint_well(ui.painter(), inner, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottVinyl {
    const VST3_CLASS_ID: [u8; 16] = *b"CottVinylVST3CE!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Filter];
}

nih_export_vst3!(CottVinyl);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_is_sixteen_bytes() {
        assert_eq!(&CottVinyl::VST3_CLASS_ID, b"CottVinylVST3CE!");
        assert_eq!(CottVinyl::VST3_CLASS_ID.len(), 16);
    }
}
