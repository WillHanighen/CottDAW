//! CottSynth VST3 — polyphonic multi-waveform synth with ADSR + egui editor.
//!
//! Same DSP as the optional in-process DAW fallback (`cott-synth-dsp`); the
//! panel is built from the shared `cott-plugin-ui` hardware kit.

use std::sync::Arc;

use cott_plugin_ui::{
    begin_panel, layout, paint_curve, paint_envelope, paint_header, paint_plate, paint_well,
    param_knob, param_knob_enabled, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    segment_button, Skin,
};
use cott_synth_dsp::{MidiNoteEvent, PolySynth, Waveform, MAX_VOICES};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Align2, FontId, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::teal();

struct CottSynth {
    params: Arc<CottSynthParams>,
    engine: PolySynth,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
enum WaveParam {
    #[id = "sine"]
    #[name = "Sine"]
    Sine,
    #[id = "saw"]
    #[name = "Saw"]
    Saw,
    #[id = "square"]
    #[name = "Square"]
    Square,
    #[id = "triangle"]
    #[name = "Triangle"]
    Triangle,
    #[id = "pulse"]
    #[name = "Pulse"]
    Pulse,
    #[id = "noise"]
    #[name = "Noise"]
    Noise,
}

impl WaveParam {
    const ALL: [WaveParam; 6] = [
        WaveParam::Sine,
        WaveParam::Saw,
        WaveParam::Square,
        WaveParam::Triangle,
        WaveParam::Pulse,
        WaveParam::Noise,
    ];

    fn label(self) -> &'static str {
        match self {
            WaveParam::Sine => "Sine",
            WaveParam::Saw => "Saw",
            WaveParam::Square => "Square",
            WaveParam::Triangle => "Triangle",
            WaveParam::Pulse => "Pulse",
            WaveParam::Noise => "Noise",
        }
    }

    fn to_waveform(self) -> Waveform {
        match self {
            WaveParam::Sine => Waveform::Sine,
            WaveParam::Saw => Waveform::Saw,
            WaveParam::Square => Waveform::Square,
            WaveParam::Triangle => Waveform::Triangle,
            WaveParam::Pulse => Waveform::Pulse,
            WaveParam::Noise => Waveform::Noise,
        }
    }
}

#[derive(Params)]
struct CottSynthParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "wave"]
    waveform: EnumParam<WaveParam>,

    #[id = "attack"]
    attack_ms: FloatParam,

    #[id = "decay"]
    decay_ms: FloatParam,

    #[id = "sustain"]
    sustain: FloatParam,

    #[id = "release"]
    release_ms: FloatParam,

    #[id = "pwidth"]
    pulse_width: FloatParam,

    #[id = "gain"]
    gain: FloatParam,
}

impl Default for CottSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(CottSynthParams::default()),
            engine: PolySynth::new(48_000.0),
            events: Vec::with_capacity(64),
            scope: Arc::new(ScopeBuffer::new()),
        }
    }
}

impl Default for CottSynthParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(620, 500),
            waveform: EnumParam::new("Waveform", WaveParam::Sine),
            attack_ms: FloatParam::new(
                "Attack",
                10.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_step_size(0.1)
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            decay_ms: FloatParam::new(
                "Decay",
                100.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_step_size(0.1)
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            sustain: FloatParam::new("Sustain", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(0.01)
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            release_ms: FloatParam::new(
                "Release",
                200.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 5000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_step_size(0.1)
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            pulse_width: FloatParam::new(
                "Pulse Width",
                0.25,
                FloatRange::Linear {
                    min: 0.05,
                    max: 0.95,
                },
            )
            .with_step_size(0.01)
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            gain: FloatParam::new("Gain", 0.25, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(0.01)
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

impl Plugin for CottSynth {
    const NAME: &'static str = "CottSynth";
    const VENDOR: &'static str = "Cottage";
    const URL: &'static str = "https://github.com/cottage-end/CottDAW";
    const EMAIL: &'static str = "dev@cottage-end.local";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    // ADSR/gain don't need sample-accurate automation; keeping this false makes
    // host performEdit → process() apply params immediately (no event queue).
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
                // Keep this below any size a host (or a persisted editor state
                // from an older build) might hand us — a larger minimum makes
                // egui push the panel off the left edge of the window.
                ResizableWindow::new("cott_synth_resize")
                    .min_size(Vec2::new(380.0, 330.0))
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
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine.set_sample_rate(buffer_config.sample_rate);
        context.set_current_voice_capacity(MAX_VOICES as u32);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
        self.events.clear();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.events.clear();
        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn {
                    timing,
                    channel,
                    note,
                    velocity,
                    ..
                } => {
                    self.events.push(MidiNoteEvent {
                        sample_offset: timing,
                        note,
                        velocity: (velocity * 127.0).round().clamp(1.0, 127.0) as u8,
                        channel,
                        on: true,
                    });
                }
                NoteEvent::NoteOff {
                    timing,
                    channel,
                    note,
                    ..
                } => {
                    self.events.push(MidiNoteEvent {
                        sample_offset: timing,
                        note,
                        velocity: 0,
                        channel,
                        on: false,
                    });
                }
                _ => {}
            }
        }
        self.events.sort_by_key(|e| e.sample_offset);

        let params = cott_synth_dsp::SynthParams {
            waveform: self.params.waveform.value().to_waveform(),
            adsr: cott_synth_dsp::AdsrParams {
                attack_ms: self.params.attack_ms.value(),
                decay_ms: self.params.decay_ms.value(),
                sustain: self.params.sustain.value(),
                release_ms: self.params.release_ms.value(),
            },
            pulse_width: self.params.pulse_width.value(),
            gain: self.params.gain.value(),
        };

        let slices = buffer.as_slice();
        if slices.len() >= 2 {
            let (left, right) = slices.split_at_mut(1);
            self.engine
                .process_block(&params, &self.events, left[0], right[0]);
            self.scope.push(left[0]);
        } else if let Some(mono) = slices.first_mut() {
            let mut right = mono.to_vec();
            self.engine
                .process_block(&params, &self.events, mono, &mut right);
            self.scope.push(mono);
        }

        ProcessStatus::Normal
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottSynthParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottSynth",
        &format!("{MAX_VOICES}-Voice Poly"),
        scope.level(),
    );

    let (osc_rect, rest) = layout::split_top(rest, rest.height() * 0.34, 10.0);
    let (env_rect, out_rect) = layout::split_top(rest, rest.height() * 0.52, 10.0);

    draw_oscillator(ui, setter, params, osc_rect);
    draw_envelope(ui, setter, params, env_rect);
    draw_output(ui, setter, params, scope, out_rect);
}

fn draw_oscillator(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottSynthParams,
    rect: egui::Rect,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Oscillator");

    let (buttons, tail) = layout::split_left(inner, inner.width() * 0.40, 10.0);
    let (preview, pw_cell) = layout::split_left(tail, tail.width() * 0.62, 10.0);

    // Waveform selector: two columns of latching caps.
    let current = params.waveform.value();
    let cols = layout::columns(buttons, 2, 6.0);
    for (col_idx, col) in cols.iter().enumerate() {
        let cells = layout::rows(*col, 3, 5.0);
        for (row_idx, cell) in cells.iter().enumerate() {
            let Some(wave) = WaveParam::ALL.get(col_idx * 3 + row_idx).copied() else {
                continue;
            };
            let selected = wave == current;
            let key = format!("wave_{}", wave.label());
            if segment_button(ui, *cell, &SKIN, &key, wave.label(), selected).clicked() && !selected
            {
                setter.begin_set_parameter(&params.waveform);
                setter.set_parameter(&params.waveform, wave);
                setter.end_set_parameter(&params.waveform);
            }
        }
    }

    // One cycle of the selected wave.
    let well = paint_well(ui.painter(), preview, &SKIN);
    let wave = current.to_waveform();
    let pulse_width = params.pulse_width.value();
    let mut noise = 0xC0FF_EE42u32;
    paint_curve(ui.painter(), well, &SKIN, 160, |t| {
        let sample = cott_synth_dsp::sample_waveform(wave, t, pulse_width, &mut noise);
        0.5 + sample * 0.42
    });
    ui.painter().text(
        well.left_bottom() + Vec2::new(3.0, -2.0),
        Align2::LEFT_BOTTOM,
        cott_plugin_ui::spaced(current.label()),
        FontId::monospace(8.5),
        cott_plugin_ui::with_alpha(SKIN.legend_dim, 180),
    );

    param_knob_enabled(
        ui,
        pw_cell,
        &SKIN,
        setter,
        &params.pulse_width,
        "Width",
        matches!(wave, Waveform::Pulse),
    );
}

fn draw_envelope(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottSynthParams,
    rect: egui::Rect,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Envelope");
    let (knobs, graph) = layout::split_left(inner, inner.width() * 0.56, 10.0);

    let cells = layout::columns(knobs, 4, 6.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.attack_ms, "Attack");
    param_knob(ui, cells[1], &SKIN, setter, &params.decay_ms, "Decay");
    param_knob(ui, cells[2], &SKIN, setter, &params.sustain, "Sustain");
    param_knob(ui, cells[3], &SKIN, setter, &params.release_ms, "Release");

    let well = paint_well(ui.painter(), graph, &SKIN);
    paint_envelope(
        ui.painter(),
        well,
        &SKIN,
        params.attack_ms.value() * 0.001,
        params.decay_ms.value() * 0.001,
        params.sustain.value(),
        params.release_ms.value() * 0.001,
    );
}

fn draw_output(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottSynthParams,
    scope: &ScopeBuffer,
    rect: egui::Rect,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Output");
    let (gain_cell, trace) = layout::split_left(inner, 78.0f32.min(inner.width() * 0.3), 10.0);

    param_knob(ui, gain_cell, &SKIN, setter, &params.gain, "Gain");

    let well = paint_well(ui.painter(), trace, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"CottSynthVST3CE!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(CottSynth);
