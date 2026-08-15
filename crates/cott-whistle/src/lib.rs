//! CottWhistle VST3 — the whistle lead that runs through 90s hip-hop.
//!
//! Monophonic and gliding by design: overlap two notes and the pitch slides
//! instead of stepping. Four characters wire the voice for the ARP Pro Soloist
//! reed sound the records started from and for the Minimoog leads that replaced
//! it; none of them is a sine. DSP lives in `cott-whistle-dsp`, the panel is
//! built from the shared `cott-plugin-ui` hardware kit.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use cott_plugin_ui::{
    begin_panel, layout, paint_curve, paint_curve_filled, paint_grid, paint_header, paint_marker,
    paint_plate, paint_well, param_knob, plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    segment_button, Skin,
};
use cott_whistle_dsp::{
    plot_hz, plot_position, preview_wave, voice_magnitude_db, Character, MidiNoteEvent,
    WhistleEngine, WhistleParams,
};
use nih_plug::formatters;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Align2, FontId, Vec2},
    resizable_window::ResizableWindow,
    EguiState,
};

const SKIN: Skin = Skin::amber();
/// Host pitch bend is normalised; treat full deflection as two semitones.
const PITCH_BEND_SEMITONES: f32 = 2.0;
/// Response plot range, in decibels.
const PLOT_FLOOR_DB: f32 = -48.0;
const PLOT_CEIL_DB: f32 = 18.0;

struct CottWhistle {
    params: Arc<CottWhistleParams>,
    engine: WhistleEngine,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
    /// Pitch currently sounding, so the response plot can track the keyboard
    /// the way the filter does.
    sounding_hz: Arc<AtomicU32>,
}

/// The four voices, as a host-automatable choice.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterParam {
    #[id = "worm"]
    #[name = "Worm"]
    Worm,
    #[id = "westcoast"]
    #[name = "West Coast"]
    WestCoast,
    #[id = "silk"]
    #[name = "Silk"]
    Silk,
    #[id = "sanandreas"]
    #[name = "San Andreas"]
    SanAndreas,
}

impl CharacterParam {
    const ALL: [CharacterParam; 4] = [
        CharacterParam::Worm,
        CharacterParam::WestCoast,
        CharacterParam::Silk,
        CharacterParam::SanAndreas,
    ];

    fn dsp(self) -> Character {
        match self {
            CharacterParam::Worm => Character::Worm,
            CharacterParam::WestCoast => Character::WestCoast,
            CharacterParam::Silk => Character::Silk,
            CharacterParam::SanAndreas => Character::SanAndreas,
        }
    }

    fn label(self) -> &'static str {
        self.dsp().label()
    }
}

/// Every ID carries a `v3-` prefix. The plugin keeps its name and class ID so
/// hosts still find it, but earlier whistle parameter blobs no longer match
/// anything and the rebuilt controls come up on their calibrated defaults.
#[derive(Params)]
struct CottWhistleParams {
    #[persist = "v3-editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "v3-char"]
    character: EnumParam<CharacterParam>,

    #[id = "v3-glide"]
    glide_ms: FloatParam,

    #[id = "v3-octave"]
    octave: IntParam,

    #[id = "v3-blend"]
    blend: FloatParam,

    #[id = "v3-detune"]
    detune_cents: FloatParam,

    #[id = "v3-bril"]
    brilliance: FloatParam,

    #[id = "v3-emph"]
    emphasis: FloatParam,

    #[id = "v3-body"]
    body: FloatParam,

    #[id = "v3-vibrate"]
    vibrato_hz: FloatParam,

    #[id = "v3-vibdepth"]
    vibrato_cents: FloatParam,

    #[id = "v3-vibdelay"]
    vibrato_delay_ms: FloatParam,

    #[id = "v3-attack"]
    attack_ms: FloatParam,

    #[id = "v3-release"]
    release_ms: FloatParam,

    #[id = "v3-drive"]
    drive: FloatParam,

    #[id = "v3-gain"]
    gain: FloatParam,
}

impl Default for CottWhistle {
    fn default() -> Self {
        Self {
            params: Arc::new(CottWhistleParams::default()),
            engine: WhistleEngine::new(48_000.0),
            events: Vec::with_capacity(64),
            scope: Arc::new(ScopeBuffer::new()),
            sounding_hz: Arc::new(AtomicU32::new(523.25f32.to_bits())),
        }
    }
}

fn percent(name: &'static str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_unit(" %")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl Default for CottWhistleParams {
    fn default() -> Self {
        let defaults = WhistleParams::default();
        Self {
            editor_state: EguiState::from_size(700, 520),
            character: EnumParam::new("Character", CharacterParam::Worm),
            glide_ms: FloatParam::new(
                "Glide",
                defaults.glide_ms,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 1_500.0,
                    factor: FloatRange::skew_factor(-1.2),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            octave: IntParam::new(
                "Octave",
                defaults.octave,
                IntRange::Linear { min: -2, max: 3 },
            )
            .with_value_to_string(Arc::new(|v| format!("{v:+}"))),
            blend: percent("Blend", defaults.blend),
            detune_cents: FloatParam::new(
                "Detune",
                defaults.detune_cents,
                FloatRange::Linear {
                    min: 0.0,
                    max: 50.0,
                },
            )
            .with_unit(" ct")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            brilliance: percent("Brilliance", defaults.brilliance),
            emphasis: percent("Emphasis", defaults.emphasis),
            body: percent("Body", defaults.body),
            vibrato_hz: FloatParam::new(
                "Vib Rate",
                defaults.vibrato_hz,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 12.0,
                    factor: FloatRange::skew_factor(-0.6),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            vibrato_cents: FloatParam::new(
                "Vib Depth",
                defaults.vibrato_cents,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit(" ct")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            vibrato_delay_ms: FloatParam::new(
                "Vib Delay",
                defaults.vibrato_delay_ms,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2_000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            attack_ms: FloatParam::new(
                "Attack",
                defaults.attack_ms,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 2_000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            release_ms: FloatParam::new(
                "Release",
                defaults.release_ms,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 5_000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            drive: percent("Drive", defaults.drive),
            gain: percent("Output", defaults.gain),
        }
    }
}

impl CottWhistleParams {
    fn to_dsp(&self) -> WhistleParams {
        WhistleParams {
            character: self.character.value().dsp(),
            glide_ms: self.glide_ms.value(),
            octave: self.octave.value(),
            blend: self.blend.value(),
            detune_cents: self.detune_cents.value(),
            brilliance: self.brilliance.value(),
            emphasis: self.emphasis.value(),
            body: self.body.value(),
            vibrato_hz: self.vibrato_hz.value(),
            vibrato_cents: self.vibrato_cents.value(),
            vibrato_delay_ms: self.vibrato_delay_ms.value(),
            attack_ms: self.attack_ms.value(),
            release_ms: self.release_ms.value(),
            drive: self.drive.value(),
            gain: self.gain.value(),
        }
    }
}

impl Plugin for CottWhistle {
    const NAME: &'static str = "CottWhistle";
    const VENDOR: &'static str = "Cottage";
    const URL: &'static str = "https://github.com/cottage-end/CottDAW";
    const EMAIL: &'static str = "dev@cottage-end.local";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    // CCs so the mod wheel can deepen the vibrato and panics land.
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
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
        let sounding_hz = self.sounding_hz.clone();

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |ctx, _| cott_plugin_ui::apply_visuals(ctx, &SKIN),
            move |egui_ctx, setter, _state| {
                // Keep the scope moving while the editor is open.
                egui_ctx.request_repaint();
                ResizableWindow::new("cott_whistle_resize")
                    .min_size(Vec2::new(520.0, 400.0))
                    .show(egui_ctx, egui_state.as_ref(), |ui| {
                        let note_hz = f32::from_bits(sounding_hz.load(Ordering::Relaxed));
                        draw_panel(ui, setter, &params, &scope, note_hz);
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
                NoteEvent::NoteOff {
                    timing,
                    channel,
                    note,
                    ..
                } => self.events.push(MidiNoteEvent {
                    sample_offset: timing,
                    note,
                    velocity: 0,
                    channel,
                    on: false,
                }),
                NoteEvent::Choke { .. } => self.engine.all_notes_off(),
                NoteEvent::MidiPitchBend { value, .. } => self
                    .engine
                    .set_pitch_bend((value - 0.5) * 2.0 * PITCH_BEND_SEMITONES),
                NoteEvent::MidiCC { cc, value, .. } => match cc {
                    1 => self.engine.set_mod_wheel(value),
                    120 | 123 => self.engine.all_notes_off(),
                    _ => {}
                },
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
            let hz = self.engine.current_hz() * 2f32.powi(params.octave);
            self.sounding_hz.store(hz.to_bits(), Ordering::Relaxed);
        }

        ProcessStatus::Normal
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    scope: &ScopeBuffer,
    note_hz: f32,
) {
    let character = params.character.value();
    let voice = params.to_dsp();

    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 46.0, 10.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottWhistle",
        character.dsp().blurb(),
        scope.level(),
    );

    let (character_rect, rest) = layout::split_top(rest, 54.0, 10.0);
    draw_character_strip(ui, setter, params, character_rect, character);

    let wells_h = 100.0f32.min(rest.height() * 0.32);
    let (upper, wells_rect) =
        layout::split_top(rest, (rest.height() - wells_h - 10.0).max(0.0), 10.0);
    let rows = layout::rows(upper, 2, 10.0);

    // Row 1: how the note behaves, then the oscillators and the filter.
    let (pitch_rect, voice_rect) = layout::split_left(rows[0], rows[0].width() * 0.28, 10.0);

    let inner = paint_plate(ui.painter(), pitch_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Pitch");
    let cells = layout::columns(inner, 2, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.glide_ms, "Glide");
    param_knob(ui, cells[1], &SKIN, setter, &params.octave, "Octave");

    let inner = paint_plate(ui.painter(), voice_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Voice");
    let cells = layout::columns(inner, 5, 6.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.blend, "Blend");
    param_knob(ui, cells[1], &SKIN, setter, &params.detune_cents, "Detune");
    param_knob(
        ui,
        cells[2],
        &SKIN,
        setter,
        &params.brilliance,
        "Brilliance",
    );
    param_knob(ui, cells[3], &SKIN, setter, &params.emphasis, "Emphasis");
    param_knob(ui, cells[4], &SKIN, setter, &params.body, "Body");

    // Row 2: modulation, envelope, output.
    let (vib_rect, tail) = layout::split_left(rows[1], rows[1].width() * 0.40, 10.0);
    let (env_rect, out_rect) = layout::split_left(tail, tail.width() * 0.47, 10.0);

    let inner = paint_plate(ui.painter(), vib_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Vibrato");
    let cells = layout::columns(inner, 3, 6.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.vibrato_hz, "Rate");
    param_knob(ui, cells[1], &SKIN, setter, &params.vibrato_cents, "Depth");
    param_knob(
        ui,
        cells[2],
        &SKIN,
        setter,
        &params.vibrato_delay_ms,
        "Delay",
    );

    let inner = paint_plate(ui.painter(), env_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Envelope");
    let cells = layout::columns(inner, 2, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.attack_ms, "Attack");
    param_knob(ui, cells[1], &SKIN, setter, &params.release_ms, "Release");

    let inner = paint_plate(ui.painter(), out_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Output");
    let cells = layout::columns(inner, 2, 8.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.drive, "Drive");
    param_knob(ui, cells[1], &SKIN, setter, &params.gain, "Output");

    let wells = layout::columns(wells_rect, 3, 10.0);
    draw_osc_well(ui, wells[0], &voice);
    draw_voice_well(ui, wells[1], &voice, note_hz);
    draw_output_well(ui, wells[2], scope);
}

fn draw_character_strip(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    rect: egui::Rect,
    current: CharacterParam,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let cells = layout::columns(inner, CharacterParam::ALL.len(), 8.0);
    for (cell, choice) in cells.iter().zip(CharacterParam::ALL) {
        let cap = egui::Rect::from_center_size(
            cell.center(),
            Vec2::new(cell.width(), cell.height().min(26.0)),
        );
        let selected = choice == current;
        let key = format!("character_{}", choice.label());
        if segment_button(ui, cap, &SKIN, &key, choice.label(), selected).clicked() && !selected {
            apply_character(setter, params, choice);
        }
    }
}

/// Selecting a character rewires the voice *and* stamps its calibrated
/// settings onto the panel, because the numbers are half the sound.
///
/// Automating the character parameter on its own only changes the wiring, which
/// keeps the host in charge of every value it is recording.
fn apply_character(setter: &ParamSetter, params: &CottWhistleParams, choice: CharacterParam) {
    let settings = choice.dsp().recipe().defaults;

    setter.begin_set_parameter(&params.character);
    setter.set_parameter(&params.character, choice);
    setter.end_set_parameter(&params.character);

    setter.begin_set_parameter(&params.octave);
    setter.set_parameter(&params.octave, settings.octave);
    setter.end_set_parameter(&params.octave);

    for (param, value) in [
        (&params.glide_ms, settings.glide_ms),
        (&params.blend, settings.blend),
        (&params.detune_cents, settings.detune_cents),
        (&params.brilliance, settings.brilliance),
        (&params.emphasis, settings.emphasis),
        (&params.body, settings.body),
        (&params.vibrato_hz, settings.vibrato_hz),
        (&params.vibrato_cents, settings.vibrato_cents),
        (&params.vibrato_delay_ms, settings.vibrato_delay_ms),
        (&params.attack_ms, settings.attack_ms),
        (&params.release_ms, settings.release_ms),
        (&params.drive, settings.drive),
        (&params.gain, settings.gain),
    ] {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, value);
        setter.end_set_parameter(param);
    }
}

/// What the mixer is putting out: two cycles of the blended shape.
fn draw_osc_well(ui: &mut egui::Ui, rect: egui::Rect, voice: &WhistleParams) {
    let well = paint_well(ui.painter(), rect, &SKIN);
    paint_grid(ui.painter(), well, &SKIN, 8, 4);
    let width = voice.pulse_width();
    let blend = voice.blend;
    paint_curve(ui.painter(), well, &SKIN, 512, |t| {
        (preview_wave(t * 2.0, blend, width, voice.character.recipe().staircase) * 0.88 + 1.0) * 0.5
    });
    well_caption(
        ui,
        well,
        "OSC",
        &format!("1/{:.0} PULSE {:.0}%", 1.0 / width, (1.0 - blend) * 100.0),
    );
}

/// The filter section as it stands for the note being played, so the fixed
/// resonators and the tracking cutoff can be seen against each other.
fn draw_voice_well(ui: &mut egui::Ui, rect: egui::Rect, voice: &WhistleParams, note_hz: f32) {
    let well = paint_well(ui.painter(), rect, &SKIN);
    let note_hz = if note_hz.is_finite() && note_hz > 20.0 {
        note_hz
    } else {
        523.25
    };
    paint_curve_filled(ui.painter(), well, &SKIN, 256, |t| {
        let db = voice_magnitude_db(voice, note_hz, plot_hz(t));
        (db - PLOT_FLOOR_DB) / (PLOT_CEIL_DB - PLOT_FLOOR_DB)
    });

    let cutoff = voice.cutoff_hz(note_hz);
    paint_marker(
        ui.painter(),
        well,
        &SKIN,
        plot_position(cutoff),
        &format!("{:.1}k", cutoff / 1_000.0),
    );
    if voice.body > 0.04 {
        let reed = voice.character.recipe().resonators[0].freq_hz;
        paint_marker(ui.painter(), well, &SKIN, plot_position(reed), "REED");
    }

    well_caption(
        ui,
        well,
        "VOICE",
        &format!("{:.0}% KEY FOLLOW", voice.key_track() * 100.0),
    );
}

fn draw_output_well(ui: &mut egui::Ui, rect: egui::Rect, scope: &ScopeBuffer) {
    let well = paint_well(ui.painter(), rect, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
    well_caption(ui, well, "OUTPUT", "");
}

fn well_caption(ui: &mut egui::Ui, well: egui::Rect, tag: &str, value: &str) {
    ui.painter().text(
        well.left_bottom() + Vec2::new(3.0, -2.0),
        Align2::LEFT_BOTTOM,
        tag,
        FontId::monospace(8.5),
        cott_plugin_ui::with_alpha(SKIN.legend_dim, 170),
    );
    if !value.is_empty() {
        ui.painter().text(
            well.right_bottom() + Vec2::new(-3.0, -2.0),
            Align2::RIGHT_BOTTOM,
            value,
            FontId::monospace(8.5),
            cott_plugin_ui::with_alpha(SKIN.readout, 190),
        );
    }
}

impl Vst3Plugin for CottWhistle {
    const VST3_CLASS_ID: [u8; 16] = *b"CottWhstlVST3CE!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_vst3!(CottWhistle);

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        CottWhistleParams::default()
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect()
    }

    #[test]
    fn every_parameter_is_versioned() {
        // The class ID is unchanged so hosts keep finding the plugin, which
        // means the only thing separating the rebuilt controls from older
        // whistle saved state is this prefix.
        let ids = ids();
        assert!(!ids.is_empty());
        for id in &ids {
            assert!(id.starts_with("v3-"), "{id} would collide with old state");
        }
    }

    #[test]
    fn parameter_ids_are_unique() {
        let mut ids = ids();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate parameter id");
    }

    #[test]
    fn none_of_the_retired_controls_came_back() {
        // Unison, chorus and the old sine-to-saw morph are gone for good.
        let ids = ids();
        for retired in ["tone", "whistle", "bright", "unison", "chorus"] {
            assert!(
                !ids.iter().any(|id| id.trim_start_matches("v3-") == retired),
                "{retired} is still on the panel"
            );
        }
    }

    #[test]
    fn the_panel_comes_up_on_the_worm() {
        let params = CottWhistleParams::default();
        assert_eq!(params.character.value(), CharacterParam::Worm);
        assert_eq!(
            params.to_dsp(),
            WhistleParams::for_character(Character::Worm)
        );
    }

    #[test]
    fn every_character_maps_onto_the_dsp_side() {
        for choice in CharacterParam::ALL {
            let dsp = choice.dsp();
            assert_eq!(choice.label(), dsp.label());
            // Round-tripping a character's settings through the panel's own
            // parameter ranges must not lose anything, or clicking a character
            // would give a different voice from the one it names.
            let settings = dsp.recipe().defaults;
            let clamped = WhistleParams::for_character(dsp).clamped();
            assert_eq!(clamped.glide_ms, settings.glide_ms);
            assert_eq!(clamped.brilliance, settings.brilliance);
            assert_eq!(clamped.body, settings.body);
            assert_eq!(clamped.vibrato_delay_ms, settings.vibrato_delay_ms);
            assert_eq!(clamped.detune_cents, settings.detune_cents);
        }
    }
}
