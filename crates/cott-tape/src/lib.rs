//! CottTape VST3 — stereo tape delay with wow, dark repeats, and drive.

use std::sync::Arc;

use cott_plugin_ui::{
    begin_panel, layout, paint_header, paint_plate, paint_well, param_knob, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    Skin,
};
use cott_tape_dsp::{TapeEngine, TapeParams};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::grain();

struct CottTape {
    params: Arc<CottTapeParams>,
    engine: TapeEngine,
    scope: Arc<ScopeBuffer>,
}

#[derive(Params)]
struct CottTapeParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "time"]
    time_ms: FloatParam,

    #[id = "feedback"]
    feedback: FloatParam,

    #[id = "wow"]
    wow: FloatParam,

    #[id = "drive"]
    drive: FloatParam,

    #[id = "mix"]
    mix: FloatParam,
}

impl Default for CottTape {
    fn default() -> Self {
        Self {
            params: Arc::new(CottTapeParams::default()),
            engine: TapeEngine::new(48_000.0),
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

impl Default for CottTapeParams {
    fn default() -> Self {
        let dsp = TapeParams::default();
        Self {
            editor_state: EguiState::from_size(640, 400),
            time_ms: FloatParam::new(
                "Time",
                dsp.time_ms,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 1_200.0,
                    factor: FloatRange::skew_factor(-0.6),
                },
            )
            .with_step_size(1.0)
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            feedback: unit_param("Feedback", dsp.feedback),
            wow: unit_param("Wow", dsp.wow),
            drive: unit_param("Drive", dsp.drive),
            mix: unit_param("Mix", dsp.mix),
        }
    }
}

impl CottTapeParams {
    fn to_dsp(&self) -> TapeParams {
        TapeParams {
            time_ms: self.time_ms.value(),
            feedback: self.feedback.value(),
            wow: self.wow.value(),
            drive: self.drive.value(),
            mix: self.mix.value(),
        }
    }
}

impl Plugin for CottTape {
    const NAME: &'static str = "CottTape";
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
                ResizableWindow::new("cott_tape_resize")
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
        ProcessStatus::KeepAlive
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottTapeParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottTape",
        "Dark repeats",
        scope.level(),
    );

    let (knobs, trace) = layout::split_top(rest, rest.height() * 0.58, 10.0);
    let inner = paint_plate(ui.painter(), knobs, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Echo");
    let cells = layout::columns(inner, 5, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.time_ms, "Time");
    param_knob(ui, cells[1], &SKIN, setter, &params.feedback, "Feedback");
    param_knob(ui, cells[2], &SKIN, setter, &params.wow, "Wow");
    param_knob(ui, cells[3], &SKIN, setter, &params.drive, "Drive");
    param_knob(ui, cells[4], &SKIN, setter, &params.mix, "Mix");

    let inner = paint_plate(ui.painter(), trace, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Out");
    let well = paint_well(ui.painter(), inner, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottTape {
    const VST3_CLASS_ID: [u8; 16] = *b"CottTapeVST3CE!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

nih_export_vst3!(CottTape);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_is_sixteen_bytes() {
        assert_eq!(&CottTape::VST3_CLASS_ID, b"CottTapeVST3CE!!");
        assert_eq!(CottTape::VST3_CLASS_ID.len(), 16);
    }
}
