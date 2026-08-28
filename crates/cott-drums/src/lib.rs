//! CottDrums VST3 — analog pad kit with a Kit knob that leans bedroom.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use cott_drums_dsp::{DrumEngine, DrumParams, MidiNoteEvent, Pad, PadParams, PAD_COUNT};
use cott_plugin_ui::{
    begin_panel, layout, paint_header, paint_lamp, paint_plate, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    param_knob, segment_button, Skin,
};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, pos2, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::rust();
const NO_TRIGGER: u8 = 255;

#[derive(Enum, Debug, Clone, Copy, PartialEq)]
enum PadParam {
    #[id = "kick"]
    #[name = "Kick"]
    Kick,
    #[id = "rim"]
    #[name = "Rim"]
    Rim,
    #[id = "snare"]
    #[name = "Snare"]
    Snare,
    #[id = "clap"]
    #[name = "Clap"]
    Clap,
    #[id = "tom"]
    #[name = "Tom"]
    Tom,
    #[id = "ch"]
    #[name = "CH"]
    ClosedHat,
    #[id = "oh"]
    #[name = "OH"]
    OpenHat,
}

impl PadParam {
    const ALL: [PadParam; PAD_COUNT] = [
        PadParam::Kick,
        PadParam::Rim,
        PadParam::Snare,
        PadParam::Clap,
        PadParam::Tom,
        PadParam::ClosedHat,
        PadParam::OpenHat,
    ];

    fn label(self) -> &'static str {
        self.to_pad().label()
    }

    fn to_pad(self) -> Pad {
        match self {
            PadParam::Kick => Pad::Kick,
            PadParam::Rim => Pad::Rim,
            PadParam::Snare => Pad::Snare,
            PadParam::Clap => Pad::Clap,
            PadParam::Tom => Pad::Tom,
            PadParam::ClosedHat => Pad::ClosedHat,
            PadParam::OpenHat => Pad::OpenHat,
        }
    }
}

struct CottDrums {
    params: Arc<CottDrumsParams>,
    engine: DrumEngine,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
    meters: Arc<[AtomicU32; PAD_COUNT]>,
    trigger: Arc<AtomicU8>,
}

#[derive(Params)]
struct CottDrumsParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "kit"]
    kit: FloatParam,
    #[id = "room"]
    room: FloatParam,
    #[id = "warmth"]
    warmth: FloatParam,
    #[id = "volume"]
    volume: FloatParam,
    #[id = "pad"]
    selected: EnumParam<PadParam>,

    #[id = "kick_pitch"]
    kick_pitch: FloatParam,
    #[id = "kick_decay"]
    kick_decay: FloatParam,
    #[id = "kick_tone"]
    kick_tone: FloatParam,
    #[id = "kick_level"]
    kick_level: FloatParam,

    #[id = "rim_pitch"]
    rim_pitch: FloatParam,
    #[id = "rim_decay"]
    rim_decay: FloatParam,
    #[id = "rim_tone"]
    rim_tone: FloatParam,
    #[id = "rim_level"]
    rim_level: FloatParam,

    #[id = "snare_pitch"]
    snare_pitch: FloatParam,
    #[id = "snare_decay"]
    snare_decay: FloatParam,
    #[id = "snare_tone"]
    snare_tone: FloatParam,
    #[id = "snare_level"]
    snare_level: FloatParam,

    #[id = "clap_pitch"]
    clap_pitch: FloatParam,
    #[id = "clap_decay"]
    clap_decay: FloatParam,
    #[id = "clap_tone"]
    clap_tone: FloatParam,
    #[id = "clap_level"]
    clap_level: FloatParam,

    #[id = "tom_pitch"]
    tom_pitch: FloatParam,
    #[id = "tom_decay"]
    tom_decay: FloatParam,
    #[id = "tom_tone"]
    tom_tone: FloatParam,
    #[id = "tom_level"]
    tom_level: FloatParam,

    #[id = "ch_pitch"]
    ch_pitch: FloatParam,
    #[id = "ch_decay"]
    ch_decay: FloatParam,
    #[id = "ch_tone"]
    ch_tone: FloatParam,
    #[id = "ch_level"]
    ch_level: FloatParam,

    #[id = "oh_pitch"]
    oh_pitch: FloatParam,
    #[id = "oh_decay"]
    oh_decay: FloatParam,
    #[id = "oh_tone"]
    oh_tone: FloatParam,
    #[id = "oh_level"]
    oh_level: FloatParam,
}

fn percent(name: &'static str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_step_size(0.01)
        .with_unit(" %")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

fn pitch(name: &'static str) -> FloatParam {
    FloatParam::new(
        name,
        0.0,
        FloatRange::Linear {
            min: -12.0,
            max: 12.0,
        },
    )
    .with_step_size(0.1)
    .with_unit(" st")
    .with_value_to_string(formatters::v2s_f32_rounded(1))
}

impl Default for CottDrums {
    fn default() -> Self {
        Self {
            params: Arc::new(CottDrumsParams::default()),
            engine: DrumEngine::new(48_000.0),
            events: Vec::with_capacity(64),
            scope: Arc::new(ScopeBuffer::new()),
            meters: Arc::new(std::array::from_fn(|_| AtomicU32::new(0))),
            trigger: Arc::new(AtomicU8::new(NO_TRIGGER)),
        }
    }
}

impl Default for CottDrumsParams {
    fn default() -> Self {
        let d = DrumParams::default();
        let p = |pad: Pad| d.pads[pad as usize];
        Self {
            editor_state: EguiState::from_size(680, 560),
            kit: percent("Kit", d.kit),
            room: percent("Room", d.room),
            warmth: percent("Warmth", d.warmth),
            volume: percent("Volume", d.volume),
            selected: EnumParam::new("Pad", PadParam::Kick),
            kick_pitch: pitch("Kick Pitch"),
            kick_decay: percent("Kick Decay", p(Pad::Kick).decay),
            kick_tone: percent("Kick Tone", p(Pad::Kick).tone),
            kick_level: percent("Kick Level", p(Pad::Kick).level),
            rim_pitch: pitch("Rim Pitch"),
            rim_decay: percent("Rim Decay", p(Pad::Rim).decay),
            rim_tone: percent("Rim Tone", p(Pad::Rim).tone),
            rim_level: percent("Rim Level", p(Pad::Rim).level),
            snare_pitch: pitch("Snare Pitch"),
            snare_decay: percent("Snare Decay", p(Pad::Snare).decay),
            snare_tone: percent("Snare Tone", p(Pad::Snare).tone),
            snare_level: percent("Snare Level", p(Pad::Snare).level),
            clap_pitch: pitch("Clap Pitch"),
            clap_decay: percent("Clap Decay", p(Pad::Clap).decay),
            clap_tone: percent("Clap Tone", p(Pad::Clap).tone),
            clap_level: percent("Clap Level", p(Pad::Clap).level),
            tom_pitch: pitch("Tom Pitch"),
            tom_decay: percent("Tom Decay", p(Pad::Tom).decay),
            tom_tone: percent("Tom Tone", p(Pad::Tom).tone),
            tom_level: percent("Tom Level", p(Pad::Tom).level),
            ch_pitch: pitch("CH Pitch"),
            ch_decay: percent("CH Decay", p(Pad::ClosedHat).decay),
            ch_tone: percent("CH Tone", p(Pad::ClosedHat).tone),
            ch_level: percent("CH Level", p(Pad::ClosedHat).level),
            oh_pitch: pitch("OH Pitch"),
            oh_decay: percent("OH Decay", p(Pad::OpenHat).decay),
            oh_tone: percent("OH Tone", p(Pad::OpenHat).tone),
            oh_level: percent("OH Level", p(Pad::OpenHat).level),
        }
    }
}

impl CottDrumsParams {
    fn pad_dsp(&self, pad: Pad) -> PadParams {
        let (pitch, decay, tone, level) = match pad {
            Pad::Kick => (
                self.kick_pitch.value(),
                self.kick_decay.value(),
                self.kick_tone.value(),
                self.kick_level.value(),
            ),
            Pad::Rim => (
                self.rim_pitch.value(),
                self.rim_decay.value(),
                self.rim_tone.value(),
                self.rim_level.value(),
            ),
            Pad::Snare => (
                self.snare_pitch.value(),
                self.snare_decay.value(),
                self.snare_tone.value(),
                self.snare_level.value(),
            ),
            Pad::Clap => (
                self.clap_pitch.value(),
                self.clap_decay.value(),
                self.clap_tone.value(),
                self.clap_level.value(),
            ),
            Pad::Tom => (
                self.tom_pitch.value(),
                self.tom_decay.value(),
                self.tom_tone.value(),
                self.tom_level.value(),
            ),
            Pad::ClosedHat => (
                self.ch_pitch.value(),
                self.ch_decay.value(),
                self.ch_tone.value(),
                self.ch_level.value(),
            ),
            Pad::OpenHat => (
                self.oh_pitch.value(),
                self.oh_decay.value(),
                self.oh_tone.value(),
                self.oh_level.value(),
            ),
        };
        PadParams {
            pitch,
            decay,
            tone,
            level,
        }
    }

    fn to_dsp(&self) -> DrumParams {
        DrumParams {
            kit: self.kit.value(),
            room: self.room.value(),
            warmth: self.warmth.value(),
            volume: self.volume.value(),
            pads: std::array::from_fn(|i| {
                self.pad_dsp(Pad::from_index(i).unwrap_or(Pad::Kick))
            }),
        }
    }

    fn selected_knobs(
        &self,
        pad: Pad,
    ) -> (
        &FloatParam,
        &FloatParam,
        &FloatParam,
        &FloatParam,
    ) {
        match pad {
            Pad::Kick => (
                &self.kick_pitch,
                &self.kick_decay,
                &self.kick_tone,
                &self.kick_level,
            ),
            Pad::Rim => (
                &self.rim_pitch,
                &self.rim_decay,
                &self.rim_tone,
                &self.rim_level,
            ),
            Pad::Snare => (
                &self.snare_pitch,
                &self.snare_decay,
                &self.snare_tone,
                &self.snare_level,
            ),
            Pad::Clap => (
                &self.clap_pitch,
                &self.clap_decay,
                &self.clap_tone,
                &self.clap_level,
            ),
            Pad::Tom => (
                &self.tom_pitch,
                &self.tom_decay,
                &self.tom_tone,
                &self.tom_level,
            ),
            Pad::ClosedHat => (
                &self.ch_pitch,
                &self.ch_decay,
                &self.ch_tone,
                &self.ch_level,
            ),
            Pad::OpenHat => (
                &self.oh_pitch,
                &self.oh_decay,
                &self.oh_tone,
                &self.oh_level,
            ),
        }
    }
}

impl Plugin for CottDrums {
    const NAME: &'static str = "CottDrums";
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
        let meters = self.meters.clone();
        let trigger = self.trigger.clone();

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |ctx, _| cott_plugin_ui::apply_visuals(ctx, &SKIN),
            move |egui_ctx, setter, _state| {
                egui_ctx.request_repaint();
                ResizableWindow::new("cott_drums_resize")
                    .min_size(Vec2::new(440.0, 380.0))
                    .show(egui_ctx, egui_state.as_ref(), |ui| {
                        draw_panel(ui, setter, &params, &scope, &meters, &trigger);
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
        context.set_current_voice_capacity(PAD_COUNT as u32);
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
        let tap = self.trigger.swap(NO_TRIGGER, Ordering::Relaxed);
        if tap != NO_TRIGGER {
            self.events.push(MidiNoteEvent {
                sample_offset: 0,
                note: tap,
                velocity: 110,
                channel: 0,
                on: true,
            });
        }
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

        for (i, meter) in self.meters.iter().enumerate() {
            let pad = Pad::from_index(i).unwrap_or(Pad::Kick);
            meter.store(self.engine.pad_level(pad).to_bits(), Ordering::Relaxed);
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
    params: &CottDrumsParams,
    scope: &ScopeBuffer,
    meters: &[AtomicU32; PAD_COUNT],
    trigger: &AtomicU8,
) {
    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottDrums",
        "Bedroom Kit",
        scope.level(),
    );

    let (pads_rect, rest) = layout::split_top(rest, rest.height() * 0.42, 10.0);
    let (edit_rect, out_rect) = layout::split_top(rest, rest.height() * 0.52, 10.0);

    draw_pads(ui, setter, params, meters, trigger, pads_rect);
    draw_pad_edit(ui, setter, params, edit_rect);
    draw_output(ui, setter, params, scope, out_rect);
}

fn draw_pads(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottDrumsParams,
    meters: &[AtomicU32; PAD_COUNT],
    trigger: &AtomicU8,
    rect: egui::Rect,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Pads");
    let current = params.selected.value();
    let rows = layout::rows(inner, 2, 8.0);
    let top = layout::columns(rows[0], 4, 8.0);
    let bottom = layout::columns(rows[1], 3, 8.0);

    for (i, pad) in PadParam::ALL.iter().copied().enumerate() {
        let cell = if i < 4 { top[i] } else { bottom[i - 4] };
        let selected = pad == current;
        let level = f32::from_bits(meters[pad.to_pad() as usize].load(Ordering::Relaxed));
        let key = format!("pad_{}", pad.label());
        if segment_button(ui, cell, &SKIN, &key, pad.label(), selected).clicked() {
            if !selected {
                setter.begin_set_parameter(&params.selected);
                setter.set_parameter(&params.selected, pad);
                setter.end_set_parameter(&params.selected);
            }
            trigger.store(pad.to_pad().midi_note(), Ordering::Relaxed);
        }
        let lamp = pos2(cell.right() - 10.0, cell.top() + 10.0);
        paint_lamp(ui.painter(), lamp, 3.2, &SKIN, level.clamp(0.0, 1.0));
    }
}

fn draw_pad_edit(ui: &mut egui::Ui, setter: &ParamSetter, params: &CottDrumsParams, rect: egui::Rect) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let pad = params.selected.value().to_pad();
    let inner = plate_legend(
        ui.painter(),
        inner,
        &SKIN,
        &format!("Pad  {}", pad.label()),
    );
    let (pitch, decay, tone, level) = params.selected_knobs(pad);
    let cells = layout::columns(inner, 4, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, pitch, "Pitch");
    param_knob(ui, cells[1], &SKIN, setter, decay, "Decay");
    param_knob(ui, cells[2], &SKIN, setter, tone, "Tone");
    param_knob(ui, cells[3], &SKIN, setter, level, "Level");
}

fn draw_output(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottDrumsParams,
    scope: &ScopeBuffer,
    rect: egui::Rect,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Kit");
    let (knobs, trace) = layout::split_left(inner, inner.width() * 0.62, 10.0);
    let cells = layout::columns(knobs, 4, 6.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.kit, "Kit");
    param_knob(ui, cells[1], &SKIN, setter, &params.room, "Room");
    param_knob(ui, cells[2], &SKIN, setter, &params.warmth, "Warmth");
    param_knob(ui, cells[3], &SKIN, setter, &params.volume, "Volume");

    let well = cott_plugin_ui::paint_well(ui.painter(), trace, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
}

impl Vst3Plugin for CottDrums {
    const VST3_CLASS_ID: [u8; 16] = *b"CottDrumsVST3CE!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Drum];
}

nih_export_vst3!(CottDrums);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_is_sixteen_bytes() {
        assert_eq!(&CottDrums::VST3_CLASS_ID, b"CottDrumsVST3CE!");
        assert_eq!(CottDrums::VST3_CLASS_ID.len(), 16);
    }
}
