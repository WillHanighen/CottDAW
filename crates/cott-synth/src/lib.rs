//! CottSynth VST3 — polyphonic multi-waveform synth with ADSR + egui editor.
//!
//! Same DSP as the optional in-process DAW fallback (`cott-synth-dsp`).

use cott_synth_dsp::{MidiNoteEvent, PolySynth, Waveform, MAX_VOICES};
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, FontId, RichText, ScrollArea, Stroke, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};
use std::ops::RangeInclusive;
use std::sync::Arc;

struct CottSynth {
    params: Arc<CottSynthParams>,
    engine: PolySynth,
    events: Vec<MidiNoteEvent>,
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
        }
    }
}

impl Default for CottSynthParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(440, 520),
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
            .with_unit(" ms"),
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
            .with_unit(" ms"),
            sustain: FloatParam::new("Sustain", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(0.01),
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
            .with_unit(" ms"),
            pulse_width: FloatParam::new(
                "Pulse Width",
                0.25,
                FloatRange::Linear {
                    min: 0.05,
                    max: 0.95,
                },
            )
            .with_step_size(0.01),
            gain: FloatParam::new("Gain", 0.25, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_step_size(0.01),
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
                ResizableWindow::new("cott_synth_resize")
                    .min_size(Vec2::new(320.0, 280.0))
                    .show(egui_ctx, egui_state.as_ref(), |ui| {
                        // Fill any area beyond the scroll content (avoids white flash).
                        let full = ui.max_rect();
                        ui.painter()
                            .rect_filled(full, 0.0, Color32::from_rgb(22, 24, 28));

                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("CottSynth")
                                        .font(FontId::proportional(26.0))
                                        .color(Color32::from_rgb(180, 220, 210)),
                                );
                                ui.label(
                                    RichText::new(format!("{MAX_VOICES}-voice · Cottage"))
                                        .size(12.0)
                                        .color(Color32::from_rgb(140, 148, 160)),
                                );
                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(RichText::new("Oscillator").strong());
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label("Wave");
                                    let current = params.waveform.value();
                                    egui::ComboBox::from_id_salt("cott_synth_wave")
                                        .selected_text(current.label())
                                        .width(220.0)
                                        .show_ui(ui, |ui| {
                                            for wave in WaveParam::ALL {
                                                let selected = current == wave;
                                                if ui.selectable_label(selected, wave.label()).clicked()
                                                    && !selected
                                                {
                                                    setter.begin_set_parameter(&params.waveform);
                                                    setter.set_parameter(&params.waveform, wave);
                                                    setter.end_set_parameter(&params.waveform);
                                                }
                                            }
                                        });
                                });

                                let wave = params.waveform.value().to_waveform();
                                let pw = params.pulse_width.value();
                                draw_waveform_preview(ui, wave, pw);
                                ui.add_space(6.0);

                                if matches!(wave, Waveform::Pulse) {
                                    float_param_row(
                                        ui,
                                        setter,
                                        "Pulse",
                                        &params.pulse_width,
                                        0.05..=0.95,
                                        "",
                                    );
                                }

                                ui.add_space(10.0);
                                ui.label(RichText::new("Envelope (ADSR)").strong());
                                ui.add_space(4.0);
                                float_param_row(
                                    ui,
                                    setter,
                                    "Attack",
                                    &params.attack_ms,
                                    0.0..=2000.0,
                                    " ms",
                                );
                                float_param_row(
                                    ui,
                                    setter,
                                    "Decay",
                                    &params.decay_ms,
                                    0.0..=2000.0,
                                    " ms",
                                );
                                float_param_row(
                                    ui,
                                    setter,
                                    "Sustain",
                                    &params.sustain,
                                    0.0..=1.0,
                                    "",
                                );
                                float_param_row(
                                    ui,
                                    setter,
                                    "Release",
                                    &params.release_ms,
                                    0.0..=5000.0,
                                    " ms",
                                );

                                ui.add_space(10.0);
                                ui.label(RichText::new("Output").strong());
                                float_param_row(ui, setter, "Gain", &params.gain, 0.0..=1.0, "");
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
        } else if let Some(mono) = slices.first_mut() {
            let mut right = mono.to_vec();
            self.engine
                .process_block(&params, &self.events, mono, &mut right);
        }

        ProcessStatus::Normal
    }
}

/// Slider with a single automation gesture for the whole drag.
///
/// While the host is processing audio, nih-plug does not update `param.value()`
/// from GUI writes until the host echoes `performEdit` through `process()`.
/// Re-binding egui's slider to `param.value()` every frame therefore snaps the
/// widget back to the old/default value mid-drag. Keep a local value in egui
/// memory for the duration of the interaction.
fn float_param_row(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    label: &str,
    param: &FloatParam,
    range: RangeInclusive<f32>,
    suffix: &str,
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(64.0, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );
        let id = ui.id().with(param.name()).with("cott_synth_slider");
        let mut value = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(id))
            .unwrap_or_else(|| param.value());
        let mut slider = egui::Slider::new(&mut value, range).clamping(egui::SliderClamping::Always);
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
            // Idle: follow the processor/host value.
            ui.ctx().data_mut(|d| d.remove::<f32>(id));
        }
    });
}

fn draw_waveform_preview(ui: &mut egui::Ui, wave: Waveform, pulse_width: f32) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_rgb(14, 16, 20));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(50, 56, 66)),
        egui::StrokeKind::Outside,
    );

    let mut noise = 0xC0FF_EE42u32;
    let n = 128;
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        let phase = i as f32 / n as f32;
        let sample = cott_synth_dsp::sample_waveform(wave, phase, pulse_width, &mut noise);
        let x = rect.left() + 6.0 + (rect.width() - 12.0) * phase;
        let y = rect.center().y - sample * (rect.height() * 0.35);
        points.push(egui::pos2(x, y));
    }
    for w in points.windows(2) {
        painter.line_segment(
            [w[0], w[1]],
            Stroke::new(1.5, Color32::from_rgb(120, 200, 180)),
        );
    }
}

impl Vst3Plugin for CottSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"CottSynthVST3CE!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(CottSynth);
