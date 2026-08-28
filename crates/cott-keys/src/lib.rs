//! CottKeys VST3 — keys, bells, and pluck for chords that aren't a saw lead.

use std::sync::Arc;

use cott_keys_dsp::{KeysEngine, KeysParams, MidiNoteEvent, VoiceKind, MAX_VOICES};
use cott_plugin_ui::{
    begin_panel, layout, paint_header, paint_plate, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    param_knob, segment_button, Skin,
};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::lilac();

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
enum KindParam {
    #[id = "keys"]
    #[name = "Keys"]
    Keys,
    #[id = "bells"]
    #[name = "Bells"]
    Bells,
    #[id = "pluck"]
    #[name = "Pluck"]
    Pluck,
}

impl KindParam {
    const ALL: [KindParam; 3] = [KindParam::Keys, KindParam::Bells, KindParam::Pluck];

    fn label(self) -> &'static str {
        match self {
            KindParam::Keys => "Keys",
            KindParam::Bells => "Bells",
            KindParam::Pluck => "Pluck",
        }
    }

    fn to_kind(self) -> VoiceKind {
        match self {
            KindParam::Keys => VoiceKind::Keys,
            KindParam::Bells => VoiceKind::Bells,
            KindParam::Pluck => VoiceKind::Pluck,
        }
    }
}

struct CottKeys {
    params: Arc<CottKeysParams>,
    engine: KeysEngine,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
}

#[derive(Params)]
struct CottKeysParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "kind"]
    kind: EnumParam<KindParam>,
    #[id = "decay"]
    decay: FloatParam,
    #[id = "warmth"]
    warmth: FloatParam,
    #[id = "chorus"]
    chorus: FloatParam,
    #[id = "volume"]
    volume: FloatParam,
}

fn percent(name: &'static str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_step_size(0.01)
        .with_unit(" %")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl Default for CottKeys {
    fn default() -> Self {
        Self {
            params: Arc::new(CottKeysParams::default()),
            engine: KeysEngine::new(48_000.0),
            events: Vec::with_capacity(64),
            scope: Arc::new(ScopeBuffer::new()),
        }
    }
}

impl Default for CottKeysParams {
    fn default() -> Self {
        let d = KeysParams::default();
        Self {
            editor_state: EguiState::from_size(600, 420),
            kind: EnumParam::new("Voice", KindParam::Keys),
            decay: percent("Decay", d.decay),
            warmth: percent("Warmth", d.warmth),
            chorus: percent("Chorus", d.chorus),
            volume: percent("Volume", d.volume),
        }
    }
}

impl CottKeysParams {
    fn to_dsp(&self) -> KeysParams {
        KeysParams {
            kind: self.kind.value().to_kind(),
            decay: self.decay.value(),
            warmth: self.warmth.value(),
            chorus: self.chorus.value(),
            volume: self.volume.value(),
        }
    }
}

impl Plugin for CottKeys {
    const NAME: &'static str = "CottKeys";
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
                ResizableWindow::new("cott_keys_resize")
                    .min_size(Vec2::new(380.0, 280.0))
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

        if self.engine.is_active() {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottKeysParams,
    scope: &ScopeBuffer,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottKeys",
        &format!("{MAX_VOICES}-Voice"),
        scope.level(),
    );

    let (voice_rect, rest) = layout::split_top(rest, rest.height() * 0.32, 10.0);
    let (knobs_rect, trace_rect) = layout::split_top(rest, rest.height() * 0.55, 10.0);

    let inner = paint_plate(ui.painter(), voice_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Voice");
    let current = params.kind.value();
    let cells = layout::columns(inner, 3, 8.0);
    for (cell, kind) in cells.iter().zip(KindParam::ALL) {
        let selected = kind == current;
        let key = format!("kind_{}", kind.label());
        if segment_button(ui, *cell, &SKIN, &key, kind.label(), selected).clicked() && !selected
        {
            setter.begin_set_parameter(&params.kind);
            setter.set_parameter(&params.kind, kind);
            setter.end_set_parameter(&params.kind);
        }
    }

    let inner = paint_plate(ui.painter(), knobs_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Tone");
    let cells = layout::columns(inner, 4, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.decay, "Decay");
    param_knob(ui, cells[1], &SKIN, setter, &params.warmth, "Warmth");
    param_knob(ui, cells[2], &SKIN, setter, &params.chorus, "Chorus");
    param_knob(ui, cells[3], &SKIN, setter, &params.volume, "Volume");

    let well = cott_plugin_ui::paint_well(ui.painter(), trace_rect, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottKeys {
    const VST3_CLASS_ID: [u8; 16] = *b"CottKeysVST3CE!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(CottKeys);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_is_sixteen_bytes() {
        assert_eq!(&CottKeys::VST3_CLASS_ID, b"CottKeysVST3CE!!");
        assert_eq!(CottKeys::VST3_CLASS_ID.len(), 16);
    }
}
