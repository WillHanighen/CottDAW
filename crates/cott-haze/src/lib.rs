//! CottHaze VST3 — a 12-voice electric piano with tape flutter and vinyl dust.
//!
//! The panel is built from the shared `cott-plugin-ui` hardware kit.

use std::sync::Arc;

use cott_haze_dsp::{HazeEngine, HazeParams, MidiNoteEvent, MAX_VOICES};
use cott_plugin_ui::{
    begin_panel, layout, paint_header, paint_plate, paint_well, param_knob, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    Skin,
};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::dusk();

struct CottHaze {
    params: Arc<CottHazeParams>,
    engine: HazeEngine,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
}

#[derive(Params)]
struct CottHazeParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "tone"]
    tone: FloatParam,

    #[id = "bell"]
    bell: FloatParam,

    #[id = "flutter"]
    flutter: FloatParam,

    #[id = "warmth"]
    warmth: FloatParam,

    #[id = "smear"]
    smear: FloatParam,

    #[id = "dust"]
    dust: FloatParam,

    #[id = "attack"]
    attack_ms: FloatParam,

    #[id = "release"]
    release_ms: FloatParam,

    #[id = "level"]
    level: FloatParam,
}

impl Default for CottHaze {
    fn default() -> Self {
        Self {
            params: Arc::new(CottHazeParams::default()),
            engine: HazeEngine::new(48_000.0),
            events: Vec::with_capacity(64),
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

fn time_param(name: &'static str, default: f32, max: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min: 0.0,
            max,
            factor: FloatRange::skew_factor(-1.0),
        },
    )
    .with_step_size(0.1)
    .with_unit(" ms")
    .with_value_to_string(formatters::v2s_f32_rounded(0))
}

impl Default for CottHazeParams {
    fn default() -> Self {
        let dsp = HazeParams::default();
        Self {
            editor_state: EguiState::from_size(720, 460),
            tone: unit_param("Tone", dsp.tone),
            bell: unit_param("Bell", dsp.bell),
            flutter: unit_param("Flutter", dsp.flutter),
            warmth: unit_param("Warmth", dsp.warmth),
            smear: unit_param("Smear", dsp.smear),
            dust: unit_param("Dust", dsp.dust),
            attack_ms: time_param("Attack", dsp.attack_ms, 2000.0),
            release_ms: time_param("Release", dsp.release_ms, 5000.0),
            level: unit_param("Level", dsp.level),
        }
    }
}

impl CottHazeParams {
    fn to_dsp(&self) -> HazeParams {
        HazeParams {
            tone: self.tone.value(),
            bell: self.bell.value(),
            flutter: self.flutter.value(),
            warmth: self.warmth.value(),
            smear: self.smear.value(),
            dust: self.dust.value(),
            attack_ms: self.attack_ms.value(),
            release_ms: self.release_ms.value(),
            level: self.level.value(),
        }
    }
}

impl Plugin for CottHaze {
    const NAME: &'static str = "CottHaze";
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
                ResizableWindow::new("cott_haze_resize")
                    .min_size(Vec2::new(420.0, 300.0))
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

        let params = self.params.to_dsp();
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

        if params.smear > 0.0 || params.dust > 0.0 {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottHazeParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottHaze",
        &format!("{MAX_VOICES}-Voice Keys"),
        scope.level(),
    );

    let (knobs, trace) = layout::split_top(rest, rest.height() * 0.62, 10.0);
    draw_knobs(ui, setter, params, knobs);
    draw_scope(ui, scope, trace);
}

fn draw_knobs(ui: &mut egui::Ui, setter: &ParamSetter, params: &CottHazeParams, rect: egui::Rect) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Keys");
    let rows = layout::rows(inner, 3, 8.0);
    let top = layout::columns(rows[0], 3, 8.0);
    let mid = layout::columns(rows[1], 3, 8.0);
    let bot = layout::columns(rows[2], 3, 8.0);

    param_knob(ui, top[0], &SKIN, setter, &params.tone, "Tone");
    param_knob(ui, top[1], &SKIN, setter, &params.bell, "Bell");
    param_knob(ui, top[2], &SKIN, setter, &params.flutter, "Flutter");
    param_knob(ui, mid[0], &SKIN, setter, &params.warmth, "Warmth");
    param_knob(ui, mid[1], &SKIN, setter, &params.smear, "Smear");
    param_knob(ui, mid[2], &SKIN, setter, &params.dust, "Dust");
    param_knob(ui, bot[0], &SKIN, setter, &params.attack_ms, "Attack");
    param_knob(ui, bot[1], &SKIN, setter, &params.release_ms, "Release");
    param_knob(ui, bot[2], &SKIN, setter, &params.level, "Level");
}

fn draw_scope(ui: &mut egui::Ui, scope: &ScopeBuffer, rect: egui::Rect) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Out");
    let well = paint_well(ui.painter(), inner, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottHaze {
    const VST3_CLASS_ID: [u8; 16] = *b"CottHazeVST3CE!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(CottHaze);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_is_sixteen_bytes() {
        assert_eq!(&CottHaze::VST3_CLASS_ID, b"CottHazeVST3CE!!");
        assert_eq!(CottHaze::VST3_CLASS_ID.len(), 16);
    }
}
