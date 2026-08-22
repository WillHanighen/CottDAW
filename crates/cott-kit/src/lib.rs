//! CottKit VST3 — synthesized dusty drums (kick, snare, clap, hats).

use std::sync::Arc;

use cott_kit_dsp::{KitEngine, KitParams, MidiNoteEvent};
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

const SKIN: Skin = Skin::ink();

struct CottKit {
    params: Arc<CottKitParams>,
    engine: KitEngine,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
}

#[derive(Params)]
struct CottKitParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "kick"]
    kick: FloatParam,

    #[id = "snare"]
    snare: FloatParam,

    #[id = "hats"]
    hats: FloatParam,

    #[id = "dirt"]
    dirt: FloatParam,

    #[id = "tune"]
    tune: FloatParam,

    #[id = "level"]
    level: FloatParam,
}

impl Default for CottKit {
    fn default() -> Self {
        Self {
            params: Arc::new(CottKitParams::default()),
            engine: KitEngine::new(48_000.0),
            events: Vec::with_capacity(32),
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

impl Default for CottKitParams {
    fn default() -> Self {
        let dsp = KitParams::default();
        Self {
            editor_state: EguiState::from_size(640, 400),
            kick: unit_param("Kick", dsp.kick),
            snare: unit_param("Snare", dsp.snare),
            hats: unit_param("Hats", dsp.hats),
            dirt: unit_param("Dirt", dsp.dirt),
            tune: unit_param("Tune", dsp.tune),
            level: unit_param("Level", dsp.level),
        }
    }
}

impl CottKitParams {
    fn to_dsp(&self) -> KitParams {
        KitParams {
            kick: self.kick.value(),
            snare: self.snare.value(),
            hats: self.hats.value(),
            dirt: self.dirt.value(),
            tune: self.tune.value(),
            level: self.level.value(),
        }
    }
}

impl Plugin for CottKit {
    const NAME: &'static str = "CottKit";
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
                ResizableWindow::new("cott_kit_resize")
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
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine.set_sample_rate(buffer_config.sample_rate);
        context.set_current_voice_capacity(5);
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
                } => self.events.push(MidiNoteEvent {
                    sample_offset: timing,
                    note,
                    velocity: (velocity * 127.0).round().clamp(1.0, 127.0) as u8,
                    channel,
                    on: true,
                }),
                NoteEvent::NoteOff { .. } => {}
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
        if params.dirt > 0.0 {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottKitParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottKit",
        "Kick 36 · Snare 38 · Clap 39 · Hats 42/46",
        scope.level(),
    );

    let (knobs, trace) = layout::split_top(rest, rest.height() * 0.58, 10.0);
    let inner = paint_plate(ui.painter(), knobs, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Kit");
    let cells = layout::columns(inner, 6, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.kick, "Kick");
    param_knob(ui, cells[1], &SKIN, setter, &params.snare, "Snare");
    param_knob(ui, cells[2], &SKIN, setter, &params.hats, "Hats");
    param_knob(ui, cells[3], &SKIN, setter, &params.dirt, "Dirt");
    param_knob(ui, cells[4], &SKIN, setter, &params.tune, "Tune");
    param_knob(ui, cells[5], &SKIN, setter, &params.level, "Level");

    let inner = paint_plate(ui.painter(), trace, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Out");
    let well = paint_well(ui.painter(), inner, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottKit {
    const VST3_CLASS_ID: [u8; 16] = *b"CottKitVST3CE!!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Drum];
}

nih_export_vst3!(CottKit);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_is_sixteen_bytes() {
        assert_eq!(&CottKit::VST3_CLASS_ID, b"CottKitVST3CE!!!");
        assert_eq!(CottKit::VST3_CLASS_ID.len(), 16);
    }
}
