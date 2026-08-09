//! CottFilter VST3 — stereo low-pass / high-pass with egui editor.

use cott_filter_dsp::{FilterMode, FilterParams, StereoFilter};
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, FontId, RichText, ScrollArea, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};
use std::ops::RangeInclusive;
use std::sync::Arc;

struct CottFilter {
    params: Arc<CottFilterParams>,
    filter: StereoFilter,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
enum ModeParam {
    #[id = "lowpass"]
    #[name = "Low Pass"]
    LowPass,
    #[id = "highpass"]
    #[name = "High Pass"]
    HighPass,
}

impl ModeParam {
    const ALL: [ModeParam; 2] = [ModeParam::LowPass, ModeParam::HighPass];

    fn label(self) -> &'static str {
        match self {
            ModeParam::LowPass => "Low Pass",
            ModeParam::HighPass => "High Pass",
        }
    }

    fn to_mode(self) -> FilterMode {
        match self {
            ModeParam::LowPass => FilterMode::LowPass,
            ModeParam::HighPass => FilterMode::HighPass,
        }
    }
}

#[derive(Params)]
struct CottFilterParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "mode"]
    mode: EnumParam<ModeParam>,

    #[id = "cutoff"]
    cutoff_hz: FloatParam,

    #[id = "q"]
    resonance: FloatParam,

    #[id = "mix"]
    mix: FloatParam,
}

impl Default for CottFilter {
    fn default() -> Self {
        Self {
            params: Arc::new(CottFilterParams::default()),
            filter: StereoFilter::new(48_000.0),
        }
    }
}

impl Default for CottFilterParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(360, 340),
            mode: EnumParam::new("Mode", ModeParam::LowPass),
            cutoff_hz: FloatParam::new(
                "Cutoff",
                2_000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_step_size(1.0)
            .with_unit(" Hz"),
            resonance: FloatParam::new(
                "Resonance",
                0.707,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 12.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_step_size(0.01),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(0.01),
        }
    }
}

impl Plugin for CottFilter {
    const NAME: &'static str = "CottFilter";
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
        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |ctx, _| {
                let mut visuals = egui::Visuals::dark();
                visuals.panel_fill = Color32::from_rgb(22, 24, 28);
                visuals.window_fill = Color32::from_rgb(22, 24, 28);
                visuals.extreme_bg_color = Color32::from_rgb(22, 24, 28);
                visuals.override_text_color = Some(Color32::from_rgb(230, 228, 220));
                visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 44, 52);
                visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 62, 74);
                visuals.widgets.active.bg_fill = Color32::from_rgb(70, 110, 130);
                visuals.selection.bg_fill = Color32::from_rgb(70, 130, 140);
                ctx.set_visuals(visuals);
            },
            move |egui_ctx, setter, _state| {
                ResizableWindow::new("cott_filter_resize")
                    .min_size(Vec2::new(280.0, 240.0))
                    .show(egui_ctx, egui_state.as_ref(), |ui| {
                        let full = ui.max_rect();
                        ui.painter()
                            .rect_filled(full, 0.0, Color32::from_rgb(22, 24, 28));

                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("CottFilter")
                                        .font(FontId::proportional(20.0))
                                        .strong(),
                                );
                                ui.label(
                                    RichText::new("Stereo low-pass / high-pass")
                                        .size(12.0)
                                        .color(Color32::from_rgb(160, 165, 175)),
                                );
                                ui.add_space(12.0);

                                ui.horizontal(|ui| {
                                    ui.label("Mode");
                                    let current = params.mode.value();
                                    egui::ComboBox::from_id_salt("cott_filter_mode")
                                        .selected_text(current.label())
                                        .show_ui(ui, |ui| {
                                            for mode in ModeParam::ALL {
                                                let selected = current == mode;
                                                if ui
                                                    .selectable_label(selected, mode.label())
                                                    .clicked()
                                                    && !selected
                                                {
                                                    setter.begin_set_parameter(&params.mode);
                                                    setter.set_parameter(&params.mode, mode);
                                                    setter.end_set_parameter(&params.mode);
                                                }
                                            }
                                        });
                                });

                                ui.add_space(10.0);
                                float_param_row(
                                    ui,
                                    setter,
                                    "Cutoff",
                                    &params.cutoff_hz,
                                    20.0..=20_000.0,
                                    " Hz",
                                    true,
                                );
                                float_param_row(
                                    ui,
                                    setter,
                                    "Reso",
                                    &params.resonance,
                                    0.1..=12.0,
                                    "",
                                    false,
                                );
                                float_param_row(
                                    ui,
                                    setter,
                                    "Mix",
                                    &params.mix,
                                    0.0..=1.0,
                                    "",
                                    false,
                                );
                                ui.add_space(16.0);
                            });
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
        self.filter.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.filter.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let params = FilterParams {
            mode: self.params.mode.value().to_mode(),
            cutoff_hz: self.params.cutoff_hz.value(),
            q: self.params.resonance.value(),
            mix: self.params.mix.value(),
        };

        let slices = buffer.as_slice();
        if slices.len() >= 2 {
            let (left, right) = slices.split_at_mut(1);
            self.filter.process_block(&params, left[0], right[0]);
        } else if let Some(mono) = slices.first_mut() {
            let mut right = mono.to_vec();
            self.filter.process_block(&params, mono, &mut right);
            // Keep mono path consistent (discard mirrored right).
        }

        ProcessStatus::Normal
    }
}

fn float_param_row(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    label: &str,
    param: &FloatParam,
    range: RangeInclusive<f32>,
    suffix: &str,
    logarithmic: bool,
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(56.0, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );
        let id = ui.id().with(param.name()).with("cott_filter_slider");
        let mut value = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(id))
            .unwrap_or_else(|| param.value());
        let mut slider = egui::Slider::new(&mut value, range).clamping(egui::SliderClamping::Always);
        if logarithmic {
            slider = slider.logarithmic(true);
        }
        if !suffix.is_empty() {
            slider = slider.suffix(suffix);
        }
        let response = ui.add(slider);
        if response.drag_started() {
            setter.begin_set_parameter(param);
        }
        if response.changed() {
            if !response.dragged() && !response.drag_started() {
                setter.begin_set_parameter(param);
                setter.set_parameter(param, value);
                setter.end_set_parameter(param);
            } else {
                setter.set_parameter(param, value);
            }
        }
        if response.drag_stopped() {
            setter.end_set_parameter(param);
            ui.ctx().data_mut(|d| d.remove::<f32>(id));
        } else if response.dragged() || response.changed() {
            ui.ctx().data_mut(|d| d.insert_temp(id, value));
        } else {
            ui.ctx().data_mut(|d| d.remove::<f32>(id));
        }
    });
}

impl Vst3Plugin for CottFilter {
    const VST3_CLASS_ID: [u8; 16] = *b"CottFiltVST3CE!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Filter];
}

nih_export_vst3!(CottFilter);
