//! CottFilter VST3 — stereo low-pass / high-pass with a hardware-style panel.

use std::sync::Arc;

use cott_filter_dsp::{FilterMode, FilterParams, ResponseProbe, StereoFilter};
use cott_plugin_ui::{
    begin_panel, layout, paint_curve_filled, paint_grid, paint_header, paint_marker, paint_plate,
    paint_well, param_knob, plate_legend, scope::ScopeBuffer, segment_button, Skin,
};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Align2, FontId, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::steel();
/// Display range of the response graph.
const GRAPH_MIN_HZ: f32 = 20.0;
const GRAPH_MAX_HZ: f32 = 20_000.0;
const GRAPH_MIN_DB: f32 = -36.0;
const GRAPH_MAX_DB: f32 = 18.0;

struct CottFilter {
    params: Arc<CottFilterParams>,
    filter: StereoFilter,
    scope: Arc<ScopeBuffer>,
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
            scope: Arc::new(ScopeBuffer::new()),
        }
    }
}

impl Default for CottFilterParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(560, 430),
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
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            resonance: FloatParam::new(
                "Resonance",
                0.707,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 12.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_step_size(0.01)
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(0.01)
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

impl CottFilterParams {
    fn to_dsp(&self) -> FilterParams {
        FilterParams {
            mode: self.mode.value().to_mode(),
            cutoff_hz: self.cutoff_hz.value(),
            q: self.resonance.value(),
            mix: self.mix.value(),
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
        let scope = self.scope.clone();

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |ctx, _| cott_plugin_ui::apply_visuals(ctx, &SKIN),
            move |egui_ctx, setter, _state| {
                egui_ctx.request_repaint();
                ResizableWindow::new("cott_filter_resize")
                    .min_size(Vec2::new(320.0, 280.0))
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
        let params = self.params.to_dsp();

        let slices = buffer.as_slice();
        if slices.len() >= 2 {
            let (left, right) = slices.split_at_mut(1);
            self.filter.process_block(&params, left[0], right[0]);
            self.scope.push(left[0]);
        } else if let Some(mono) = slices.first_mut() {
            let mut right = mono.to_vec();
            self.filter.process_block(&params, mono, &mut right);
            // Keep mono path consistent (discard mirrored right).
            self.scope.push(mono);
        }

        ProcessStatus::Normal
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottFilterParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottFilter",
        "Stereo LP / HP",
        scope.level(),
    );

    let (graph_rect, controls) = layout::split_top(rest, rest.height() * 0.52, 10.0);
    draw_response(ui, params, graph_rect);

    let (mode_rect, knobs_rect) = layout::split_left(controls, controls.width() * 0.32, 10.0);

    let inner = paint_plate(ui.painter(), mode_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Mode");
    let current = params.mode.value();
    let (buttons, caption) = layout::split_top(inner, (inner.height() - 16.0).max(40.0), 2.0);
    ui.painter().text(
        caption.center_bottom() - Vec2::new(0.0, 1.0),
        Align2::CENTER_BOTTOM,
        cott_plugin_ui::spaced("12 dB / oct"),
        FontId::proportional(8.5),
        cott_plugin_ui::with_alpha(SKIN.legend_dim, 120),
    );
    let cells = layout::rows(buttons, 2, 6.0);
    for (cell, mode) in cells.iter().zip(ModeParam::ALL) {
        let selected = mode == current;
        let cell = egui::Rect::from_center_size(
            cell.center(),
            Vec2::new(cell.width(), cell.height().min(30.0)),
        );
        let key = format!("mode_{}", mode.label());
        if segment_button(ui, cell, &SKIN, &key, mode.label(), selected).clicked() && !selected {
            setter.begin_set_parameter(&params.mode);
            setter.set_parameter(&params.mode, mode);
            setter.end_set_parameter(&params.mode);
        }
    }

    let inner = paint_plate(ui.painter(), knobs_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Filter");
    let cells = layout::columns(inner, 3, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.cutoff_hz, "Cutoff");
    param_knob(ui, cells[1], &SKIN, setter, &params.resonance, "Reso");
    param_knob(ui, cells[2], &SKIN, setter, &params.mix, "Mix");
}

/// Log-frequency, dB-scaled magnitude plot of the current setting.
fn draw_response(ui: &mut egui::Ui, params: &CottFilterParams, rect: egui::Rect) {
    let well = paint_well(ui.painter(), rect, &SKIN);
    paint_grid(ui.painter(), well, &SKIN, 1, 3);

    // Decade lines where they actually fall on a log axis.
    for hz in [100.0f32, 1_000.0, 10_000.0] {
        let label = if hz >= 1_000.0 {
            format!("{:.0}k", hz / 1_000.0)
        } else {
            format!("{hz:.0}")
        };
        let x = freq_to_norm(hz);
        let line_x = well.left() + well.width() * x;
        let width = label.chars().count() as f32 * 5.2;
        let (anchor, text_x) = if line_x + width + 6.0 > well.right() {
            (Align2::RIGHT_BOTTOM, line_x - 3.0)
        } else {
            (Align2::LEFT_BOTTOM, line_x + 3.0)
        };
        ui.painter().text(
            egui::pos2(text_x, well.bottom() - 5.0),
            anchor,
            label,
            FontId::monospace(8.0),
            cott_plugin_ui::with_alpha(SKIN.legend_dim, 160),
        );
        ui.painter().line_segment(
            [
                egui::pos2(line_x, well.top()),
                egui::pos2(line_x, well.bottom()),
            ],
            egui::Stroke::new(1.0, cott_plugin_ui::with_alpha(SKIN.legend_dim, 26)),
        );
    }

    // 0 dB reference.
    let zero_y = well.bottom() - well.height() * db_to_norm(0.0);
    ui.painter().line_segment(
        [
            egui::pos2(well.left(), zero_y),
            egui::pos2(well.right(), zero_y),
        ],
        egui::Stroke::new(1.0, cott_plugin_ui::with_alpha(SKIN.legend_dim, 60)),
    );

    let dsp = params.to_dsp();
    let probe = ResponseProbe::new(&dsp, 48_000.0);
    paint_curve_filled(ui.painter(), well, &SKIN, 220, |t| {
        db_to_norm(probe.magnitude_db(norm_to_freq(t)))
    });

    let cutoff = dsp.cutoff_hz.clamp(GRAPH_MIN_HZ, GRAPH_MAX_HZ);
    let cutoff_label = if cutoff >= 1_000.0 {
        format!("{:.2} kHz", cutoff / 1_000.0)
    } else {
        format!("{cutoff:.0} Hz")
    };
    paint_marker(
        ui.painter(),
        well,
        &SKIN,
        freq_to_norm(cutoff),
        &cutoff_label,
    );

    ui.painter().text(
        well.left_top() + Vec2::new(3.0, 2.0),
        Align2::LEFT_TOP,
        format!("+{GRAPH_MAX_DB:.0} dB"),
        FontId::monospace(8.0),
        cott_plugin_ui::with_alpha(SKIN.legend_dim, 130),
    );
}

fn freq_to_norm(hz: f32) -> f32 {
    let lo = GRAPH_MIN_HZ.ln();
    let hi = GRAPH_MAX_HZ.ln();
    ((hz.max(1.0).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn norm_to_freq(t: f32) -> f32 {
    let lo = GRAPH_MIN_HZ.ln();
    let hi = GRAPH_MAX_HZ.ln();
    (lo + (hi - lo) * t.clamp(0.0, 1.0)).exp()
}

fn db_to_norm(db: f32) -> f32 {
    ((db - GRAPH_MIN_DB) / (GRAPH_MAX_DB - GRAPH_MIN_DB)).clamp(0.0, 1.0)
}

impl Vst3Plugin for CottFilter {
    const VST3_CLASS_ID: [u8; 16] = *b"CottFiltVST3CE!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Filter];
}

nih_export_vst3!(CottFilter);
