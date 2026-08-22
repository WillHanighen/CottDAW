//! CottWhistle VST3 — a 1972 Pro Soloist voice, minus the trademark.
//!
//! Performance paddles stamp a ROM recipe into the circuit. The Edit view
//! makes those switches visible. Class ID stays `CottWhstlVST3CE!`; every
//! parameter id is `v4-` so old `v3-` blobs cannot land on the new controls.

use std::sync::Arc;

use cott_plugin_ui::{
    begin_panel, layout, paddle, paint_header, paint_plate, paint_well, param_knob, param_slider,
    plate_legend,
    scope::{paint_waveform, ScopeBuffer, SCOPE_LEN},
    segment_button, PaddleThrow as UiThrow, Skin,
};
use cott_whistle_dsp::{
    MidiNoteEvent, PulseWidth, ResonatorSlot, Voice, WhistleEngine, WhistleParams, CURVE_NAMES,
    PADDLES,
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
const PITCH_BEND_SEMITONES: f32 = 2.0;
/// Hosts that cannot send channel pressure can use breath (CC 2) as the strip.
const AFTERTOUCH_CC: u8 = 2;

struct CottWhistle {
    params: Arc<CottWhistleParams>,
    engine: WhistleEngine,
    events: Vec<MidiNoteEvent>,
    scope: Arc<ScopeBuffer>,
}

#[derive(Params)]
struct CottWhistleParams {
    #[persist = "v4-editor-dpi"]
    editor_state: Arc<EguiState>,

    #[id = "v4-edit"]
    edit_view: BoolParam,

    #[id = "v4-voice"]
    voice: IntParam,

    #[id = "v4-vol"]
    volume: FloatParam,
    #[id = "v4-touch"]
    touch_sens: FloatParam,
    #[id = "v4-bril"]
    brilliance: FloatParam,
    #[id = "v4-porta"]
    portamento_ms: FloatParam,
    #[id = "v4-oct"]
    octave: IntParam,
    #[id = "v4-lfo"]
    lfo_hz: FloatParam,
    #[id = "v4-repeat"]
    repeat: BoolParam,

    #[id = "v4-tbend"]
    touch_bend: BoolParam,
    #[id = "v4-twow"]
    touch_wow: BoolParam,
    #[id = "v4-tgrowl"]
    touch_growl: BoolParam,
    #[id = "v4-tbril"]
    touch_brilliance: BoolParam,
    #[id = "v4-tvol"]
    touch_volume: BoolParam,
    #[id = "v4-tvib"]
    touch_vibrato: BoolParam,

    #[id = "v4-pulse"]
    pulse: IntParam,
    #[id = "v4-plvl"]
    pulse_level: FloatParam,
    #[id = "v4-slvl"]
    saw_level: FloatParam,
    #[id = "v4-rmix"]
    resonator_mix: FloatParam,
    #[id = "v4-hp"]
    hp_hz: FloatParam,
    #[id = "v4-vcfon"]
    vcf_enable: BoolParam,
    #[id = "v4-vcfcut"]
    vcf_cutoff: FloatParam,
    #[id = "v4-vcfres"]
    vcf_resonance: FloatParam,
    #[id = "v4-vcfkbd"]
    vcf_keytrack: FloatParam,
    #[id = "v4-adsrvcf"]
    adsr_to_vcf: FloatParam,
    #[id = "v4-arvcf"]
    ar_to_vcf: FloatParam,
    #[id = "v4-growl"]
    growl: FloatParam,

    #[id = "v4-atk"]
    attack_ms: FloatParam,
    #[id = "v4-dec"]
    decay_ms: FloatParam,
    #[id = "v4-sus"]
    sustain: FloatParam,
    #[id = "v4-rel"]
    release_ms: FloatParam,
    #[id = "v4-aratk"]
    ar_attack_ms: FloatParam,
    #[id = "v4-arrel"]
    ar_release_ms: FloatParam,
    #[id = "v4-adsrvca"]
    adsr_to_vca: FloatParam,
    #[id = "v4-arvca"]
    ar_to_vca: FloatParam,
    #[id = "v4-adsrpwm"]
    adsr_pwm: FloatParam,
    #[id = "v4-arpwm"]
    ar_pwm: FloatParam,
    #[id = "v4-lfopwm"]
    lfo_pwm: FloatParam,
    #[id = "v4-lfofm"]
    lfo_fm: FloatParam,
    #[id = "v4-lfodel"]
    lfo_delay_ms: FloatParam,
    #[id = "v4-lfovca"]
    lfo_to_vca: FloatParam,

    #[id = "v4-r0on"]
    r0_on: BoolParam,
    #[id = "v4-r0c"]
    r0_curve: IntParam,
    #[id = "v4-r0vcf"]
    r0_vcf: FloatParam,
    #[id = "v4-r0vca"]
    r0_vca: FloatParam,
    #[id = "v4-r1on"]
    r1_on: BoolParam,
    #[id = "v4-r1c"]
    r1_curve: IntParam,
    #[id = "v4-r1vcf"]
    r1_vcf: FloatParam,
    #[id = "v4-r1vca"]
    r1_vca: FloatParam,
    #[id = "v4-r2on"]
    r2_on: BoolParam,
    #[id = "v4-r2c"]
    r2_curve: IntParam,
    #[id = "v4-r2vcf"]
    r2_vcf: FloatParam,
    #[id = "v4-r2vca"]
    r2_vca: FloatParam,
    #[id = "v4-r3on"]
    r3_on: BoolParam,
    #[id = "v4-r3c"]
    r3_curve: IntParam,
    #[id = "v4-r3vcf"]
    r3_vcf: FloatParam,
    #[id = "v4-r3vca"]
    r3_vca: FloatParam,
    #[id = "v4-r4on"]
    r4_on: BoolParam,
    #[id = "v4-r4c"]
    r4_curve: IntParam,
    #[id = "v4-r4vcf"]
    r4_vcf: FloatParam,
    #[id = "v4-r4vca"]
    r4_vca: FloatParam,

    #[id = "v4-gain"]
    gain: FloatParam,
}

impl Default for CottWhistle {
    fn default() -> Self {
        Self {
            params: Arc::new(CottWhistleParams::default()),
            engine: WhistleEngine::new(48_000.0),
            events: Vec::with_capacity(64),
            scope: Arc::new(ScopeBuffer::new()),
        }
    }
}

fn percent(name: &'static str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_unit(" %")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

fn ms_skew(name: &'static str, default: f32, max: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min: 0.0,
            max,
            factor: FloatRange::skew_factor(-1.2),
        },
    )
    .with_unit(" ms")
    .with_value_to_string(formatters::v2s_f32_rounded(0))
}

fn curve_param(name: &'static str, default: i32) -> IntParam {
    IntParam::new(name, default, IntRange::Linear { min: 0, max: 9 })
        .with_value_to_string(Arc::new(|v| CURVE_NAMES[(v as usize).min(9)].to_string()))
}

impl Default for CottWhistleParams {
    fn default() -> Self {
        let d = WhistleParams::from_voice(Voice::Oboe);
        Self {
            editor_state: {
                let (w, h) = cott_plugin_ui::physical_size(1200, 720);
                EguiState::from_size(w, h)
            },
            edit_view: BoolParam::new("Edit", false),
            voice: IntParam::new(
                "Voice",
                d.voice.index() as i32,
                IntRange::Linear { min: 0, max: 29 },
            )
            .with_value_to_string(Arc::new(|v| {
                Voice::from_index(v as usize).label().to_string()
            })),
            volume: percent("Volume", d.volume),
            touch_sens: percent("Touch", d.touch_sens),
            brilliance: percent("Brilliance", d.brilliance),
            portamento_ms: ms_skew("Portamento", d.portamento_ms, 2_500.0),
            octave: IntParam::new("Octave", d.octave, IntRange::Linear { min: -1, max: 1 })
                .with_value_to_string(Arc::new(|v| format!("{v:+}"))),
            lfo_hz: FloatParam::new(
                "Rate",
                d.lfo_hz,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-0.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            repeat: BoolParam::new("Repeat", false),
            touch_bend: BoolParam::new("Bend", false),
            touch_wow: BoolParam::new("Wow", false),
            touch_growl: BoolParam::new("Growl", false),
            touch_brilliance: BoolParam::new("TBrilliance", false),
            touch_volume: BoolParam::new("TVolume", false),
            touch_vibrato: BoolParam::new("TVibrato", false),
            pulse: IntParam::new(
                "Pulse",
                d.pulse.index() as i32,
                IntRange::Linear { min: 0, max: 4 },
            )
            .with_value_to_string(Arc::new(|v| {
                PulseWidth::from_index(v as usize).label().to_string()
            })),
            pulse_level: percent("Pulse Lvl", d.pulse_level),
            saw_level: percent("Saw Lvl", d.saw_level),
            resonator_mix: percent("Res Mix", d.resonator_mix),
            hp_hz: FloatParam::new(
                "Highpass",
                d.hp_hz,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 4_000.0,
                    factor: FloatRange::skew_factor(-0.8),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            vcf_enable: BoolParam::new("VCF", d.vcf_enable),
            vcf_cutoff: percent("Cutoff", d.vcf_cutoff),
            vcf_resonance: percent("Resonance", d.vcf_resonance),
            vcf_keytrack: percent("Keytrack", d.vcf_keytrack),
            adsr_to_vcf: percent("ADSR→VCF", d.adsr_to_vcf),
            ar_to_vcf: percent("AR→VCF", d.ar_to_vcf),
            growl: percent("Growl Amt", d.growl),
            attack_ms: ms_skew("Attack", d.adsr.attack_ms, 2_000.0),
            decay_ms: ms_skew("Decay", d.adsr.decay_ms, 3_000.0),
            sustain: percent("Sustain", d.adsr.sustain),
            release_ms: ms_skew("Release", d.adsr.release_ms, 5_000.0),
            ar_attack_ms: ms_skew("AR Atk", d.ar_attack_ms, 2_000.0),
            ar_release_ms: ms_skew("AR Rel", d.ar_release_ms, 5_000.0),
            adsr_to_vca: percent("ADSR→VCA", d.adsr_to_vca),
            ar_to_vca: percent("AR→VCA", d.ar_to_vca),
            adsr_pwm: percent("ADSR PWM", d.adsr_pwm),
            ar_pwm: percent("AR PWM", d.ar_pwm),
            lfo_pwm: percent("LFO PWM", d.lfo_pwm),
            lfo_fm: percent("Auto Vib", (d.lfo_fm / 2.0).clamp(0.0, 1.0)),
            lfo_delay_ms: ms_skew("Vib Delay", d.lfo_delay_ms, 3_000.0),
            lfo_to_vca: percent("Tremolo", d.lfo_to_vca),
            r0_on: BoolParam::new("R1", d.resonators[0].enabled),
            r0_curve: curve_param("R1 Curve", d.resonators[0].curve as i32),
            r0_vcf: percent("R1 VCF", d.resonators[0].to_vcf),
            r0_vca: percent("R1 VCA", d.resonators[0].to_vca),
            r1_on: BoolParam::new("R2", d.resonators[1].enabled),
            r1_curve: curve_param("R2 Curve", d.resonators[1].curve as i32),
            r1_vcf: percent("R2 VCF", d.resonators[1].to_vcf),
            r1_vca: percent("R2 VCA", d.resonators[1].to_vca),
            r2_on: BoolParam::new("R3", d.resonators[2].enabled),
            r2_curve: curve_param("R3 Curve", d.resonators[2].curve as i32),
            r2_vcf: percent("R3 VCF", d.resonators[2].to_vcf),
            r2_vca: percent("R3 VCA", d.resonators[2].to_vca),
            r3_on: BoolParam::new("R4", d.resonators[3].enabled),
            r3_curve: curve_param("R4 Curve", d.resonators[3].curve as i32),
            r3_vcf: percent("R4 VCF", d.resonators[3].to_vcf),
            r3_vca: percent("R4 VCA", d.resonators[3].to_vca),
            r4_on: BoolParam::new("R5", d.resonators[4].enabled),
            r4_curve: curve_param("R5 Curve", d.resonators[4].curve as i32),
            r4_vcf: percent("R5 VCF", d.resonators[4].to_vcf),
            r4_vca: percent("R5 VCA", d.resonators[4].to_vca),
            gain: percent("Output", d.gain),
        }
    }
}

fn slot(on: bool, curve: i32, vcf: f32, vca: f32) -> ResonatorSlot {
    ResonatorSlot {
        enabled: on,
        curve: curve.clamp(0, 9) as u8,
        to_vcf: vcf,
        to_vca: vca,
    }
}

impl CottWhistleParams {
    fn voice(&self) -> Voice {
        Voice::from_index(self.voice.value() as usize)
    }

    fn to_dsp(&self) -> WhistleParams {
        let factory = self.voice().recipe();
        let pulse = PulseWidth::from_index(self.pulse.value() as usize);
        let pulse_bits = if pulse == factory.pulse {
            factory.pulse_bits
        } else {
            pulse.select_bit()
        };
        let hp_hz = self.hp_hz.value();
        let hp_mask = if (hp_hz - factory.hp_hz).abs() > 8.0 {
            0
        } else {
            factory.hp_mask
        };
        WhistleParams {
            voice: self.voice(),
            volume: self.volume.value(),
            touch_sens: self.touch_sens.value(),
            brilliance: self.brilliance.value(),
            portamento_ms: self.portamento_ms.value(),
            octave: self.octave.value(),
            rom_octave: factory.rom_octave,
            lfo_hz: self.lfo_hz.value(),
            repeat: self.repeat.value(),
            touch_bend: self.touch_bend.value(),
            touch_wow: self.touch_wow.value(),
            touch_growl: self.touch_growl.value(),
            touch_brilliance: self.touch_brilliance.value(),
            touch_volume: self.touch_volume.value(),
            touch_vibrato: self.touch_vibrato.value(),
            pulse,
            pulse_bits,
            pulse_level: self.pulse_level.value(),
            saw_level: self.saw_level.value(),
            resonator_mix: self.resonator_mix.value(),
            hp_hz,
            hp_mask,
            vcf_enable: self.vcf_enable.value(),
            vcf_cutoff: self.vcf_cutoff.value(),
            vcf_resonance: self.vcf_resonance.value(),
            vcf_keytrack: self.vcf_keytrack.value(),
            adsr_to_vcf: self.adsr_to_vcf.value(),
            ar_to_vcf: self.ar_to_vcf.value(),
            growl: self.growl.value(),
            adsr: cott_whistle_dsp::AdsrParams {
                attack_ms: self.attack_ms.value(),
                decay_ms: self.decay_ms.value(),
                sustain: self.sustain.value(),
                release_ms: self.release_ms.value(),
            },
            ar_attack_ms: self.ar_attack_ms.value(),
            ar_release_ms: self.ar_release_ms.value(),
            adsr_to_vca: self.adsr_to_vca.value(),
            ar_to_vca: self.ar_to_vca.value(),
            adsr_pwm: self.adsr_pwm.value(),
            ar_pwm: self.ar_pwm.value(),
            lfo_pwm: self.lfo_pwm.value(),
            lfo_fm: self.lfo_fm.value() * 2.0,
            lfo_delay_ms: self.lfo_delay_ms.value(),
            lfo_to_vca: self.lfo_to_vca.value(),
            resonators: [
                slot(
                    self.r0_on.value(),
                    self.r0_curve.value(),
                    self.r0_vcf.value(),
                    self.r0_vca.value(),
                ),
                slot(
                    self.r1_on.value(),
                    self.r1_curve.value(),
                    self.r1_vcf.value(),
                    self.r1_vca.value(),
                ),
                slot(
                    self.r2_on.value(),
                    self.r2_curve.value(),
                    self.r2_vcf.value(),
                    self.r2_vca.value(),
                ),
                slot(
                    self.r3_on.value(),
                    self.r3_curve.value(),
                    self.r3_vcf.value(),
                    self.r3_vca.value(),
                ),
                slot(
                    self.r4_on.value(),
                    self.r4_curve.value(),
                    self.r4_vcf.value(),
                    self.r4_vca.value(),
                ),
            ],
            gain: self.gain.value(),
        }
    }

    fn is_edited(&self) -> bool {
        let live = self.to_dsp();
        let factory = WhistleParams::from_voice(live.voice);
        live.pulse != factory.pulse
            || (live.hp_hz - factory.hp_hz).abs() > 8.0
            || live.vcf_enable != factory.vcf_enable
            || (live.growl - factory.growl).abs() > 0.04
            || live.resonators[0].curve != factory.resonators[0].curve
            || (live.adsr_pwm - factory.adsr_pwm).abs() > 0.04
    }
}

fn set_f(setter: &ParamSetter, param: &FloatParam, value: f32) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

fn set_i(setter: &ParamSetter, param: &IntParam, value: i32) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

fn set_b(setter: &ParamSetter, param: &BoolParam, value: bool) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

fn apply_voice(setter: &ParamSetter, params: &CottWhistleParams, voice: Voice) {
    let r = voice.recipe();
    set_i(setter, &params.voice, voice.index() as i32);
    set_i(setter, &params.pulse, r.pulse.index() as i32);
    set_f(setter, &params.pulse_level, r.pulse_level);
    set_f(setter, &params.saw_level, r.saw_level);
    set_f(setter, &params.resonator_mix, r.resonator_mix);
    set_f(setter, &params.hp_hz, r.hp_hz);
    set_b(setter, &params.vcf_enable, r.vcf_enable);
    set_f(setter, &params.vcf_cutoff, r.vcf_cutoff);
    set_f(setter, &params.vcf_resonance, r.vcf_resonance);
    set_f(setter, &params.vcf_keytrack, r.vcf_keytrack);
    set_f(setter, &params.adsr_to_vcf, r.adsr_to_vcf);
    set_f(setter, &params.ar_to_vcf, r.ar_to_vcf);
    set_f(setter, &params.growl, r.growl);
    set_f(setter, &params.attack_ms, r.adsr.attack_ms);
    set_f(setter, &params.decay_ms, r.adsr.decay_ms);
    set_f(setter, &params.sustain, r.adsr.sustain);
    set_f(setter, &params.release_ms, r.adsr.release_ms);
    set_f(setter, &params.ar_attack_ms, r.ar_attack_ms);
    set_f(setter, &params.ar_release_ms, r.ar_release_ms);
    set_f(setter, &params.adsr_to_vca, r.adsr_to_vca);
    set_f(setter, &params.ar_to_vca, r.ar_to_vca);
    set_f(setter, &params.adsr_pwm, r.adsr_pwm);
    set_f(setter, &params.ar_pwm, r.ar_pwm);
    set_f(setter, &params.lfo_pwm, r.lfo_pwm);
    set_f(setter, &params.lfo_fm, (r.lfo_fm / 2.0).clamp(0.0, 1.0));
    set_f(setter, &params.lfo_delay_ms, r.lfo_delay_ms);
    set_f(setter, &params.lfo_to_vca, r.lfo_to_vca);
    let slots = [
        (
            &params.r0_on,
            &params.r0_curve,
            &params.r0_vcf,
            &params.r0_vca,
        ),
        (
            &params.r1_on,
            &params.r1_curve,
            &params.r1_vcf,
            &params.r1_vca,
        ),
        (
            &params.r2_on,
            &params.r2_curve,
            &params.r2_vcf,
            &params.r2_vca,
        ),
        (
            &params.r3_on,
            &params.r3_curve,
            &params.r3_vcf,
            &params.r3_vca,
        ),
        (
            &params.r4_on,
            &params.r4_curve,
            &params.r4_vcf,
            &params.r4_vca,
        ),
    ];
    for (i, (on, curve, vcf, vca)) in slots.into_iter().enumerate() {
        let s = r.resonators[i];
        set_b(setter, on, s.enabled);
        set_i(setter, curve, s.curve as i32);
        set_f(setter, vcf, s.to_vcf);
        set_f(setter, vca, s.to_vca);
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

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |ctx, _| {
                ctx.set_pixels_per_point(cott_plugin_ui::display_scale());
                cott_plugin_ui::apply_visuals(ctx, &SKIN);
            },
            move |egui_ctx, setter, _state| {
                egui_ctx.set_pixels_per_point(cott_plugin_ui::display_scale());
                egui_ctx.request_repaint();
                ResizableWindow::new("cott_whistle_resize")
                    .min_size(Vec2::new(960.0, 560.0))
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
                NoteEvent::MidiChannelPressure { pressure, .. } => {
                    self.engine.set_pressure(pressure);
                }
                NoteEvent::PolyPressure { pressure, .. } => {
                    self.engine.set_pressure(pressure);
                }
                NoteEvent::MidiCC { cc, value, .. } => match cc {
                    AFTERTOUCH_CC => self.engine.set_pressure(value),
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

        ProcessStatus::Normal
    }
}

fn draw_panel(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    scope: &ScopeBuffer,
) {
    let voice = params.voice();
    let edited = params.is_edited();
    let mut subtitle = format!("{} · {}", voice.group(), voice.label());
    if edited {
        subtitle.push_str(" · edited");
    }

    let content = begin_panel(ui, &SKIN);
    let (header, rest) = layout::split_top(content, 52.0, 8.0);
    paint_header(
        ui.painter(),
        header,
        &SKIN,
        "CottWhistle",
        &subtitle,
        scope.level(),
    );

    let (tab_rect, rest) = layout::split_top(rest, 34.0, 8.0);
    draw_tabs(ui, setter, params, tab_rect);

    if params.edit_view.value() {
        draw_edit(ui, setter, params, rest, scope);
    } else {
        draw_performance(ui, setter, params, rest, scope, voice);
    }
}

fn draw_tabs(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    rect: egui::Rect,
) {
    let inner = paint_plate(ui.painter(), rect, &SKIN);
    let cells = layout::columns(inner, 2, 8.0);
    let edit = params.edit_view.value();
    if segment_button(ui, cells[0], &SKIN, "tab_perf", "Performance", !edit).clicked() && edit {
        set_b(setter, &params.edit_view, false);
    }
    if segment_button(ui, cells[1], &SKIN, "tab_edit", "Edit", edit).clicked() && !edit {
        set_b(setter, &params.edit_view, true);
    }
}

fn draw_performance(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    rest: egui::Rect,
    scope: &ScopeBuffer,
    voice: Voice,
) {
    let well_h = 88.0f32.min(rest.height() * 0.16);
    let (body, well_rect) = layout::split_top(rest, (rest.height() - well_h - 8.0).max(0.0), 8.0);
    let (slider_rect, body) = layout::split_left(body, 168.0, 10.0);
    let (paddle_rect, switch_rect) = layout::split_top(body, body.height() * 0.72, 10.0);

    let inner = paint_plate(ui.painter(), slider_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Touch");
    let cols = layout::columns(inner, 4, 6.0);
    param_slider(ui, cols[0], &SKIN, setter, &params.volume, "Vol");
    param_slider(ui, cols[1], &SKIN, setter, &params.touch_sens, "Sens");
    param_slider(ui, cols[2], &SKIN, setter, &params.brilliance, "Bril");
    param_slider(ui, cols[3], &SKIN, setter, &params.portamento_ms, "Porta");

    let inner = paint_plate(ui.painter(), paddle_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Voices");
    let rows = layout::rows(inner, 3, 8.0);
    for (r, row) in rows.iter().enumerate() {
        let cells = layout::columns(*row, 5, 8.0);
        for (c, cell) in cells.iter().enumerate() {
            let spec = PADDLES[r * 5 + c];
            let throw = match spec.throw_for(voice) {
                cott_whistle_dsp::PaddleThrow::Off => UiThrow::Off,
                cott_whistle_dsp::PaddleThrow::Up => UiThrow::Up,
                cott_whistle_dsp::PaddleThrow::Down => UiThrow::Down,
            };
            if let Some(next) = paddle(
                ui,
                *cell,
                &SKIN,
                spec.up.label(),
                spec.up.label(),
                spec.down.label(),
                throw,
            ) {
                match next {
                    UiThrow::Up => apply_voice(setter, params, spec.up),
                    UiThrow::Down => apply_voice(setter, params, spec.down),
                    UiThrow::Off => {}
                }
            }
        }
    }

    let inner = paint_plate(ui.painter(), switch_rect, &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Touch effects");
    let (switches, extras) = layout::split_left(inner, inner.width() * 0.62, 8.0);
    let sw = layout::columns(switches, 6, 4.0);
    toggle(ui, setter, &params.touch_bend, sw[0], "Bend");
    toggle(ui, setter, &params.touch_wow, sw[1], "Wow");
    toggle(ui, setter, &params.touch_growl, sw[2], "Growl");
    toggle(ui, setter, &params.touch_brilliance, sw[3], "Bril");
    toggle(ui, setter, &params.touch_volume, sw[4], "Vol");
    toggle(ui, setter, &params.touch_vibrato, sw[5], "Vib");

    let extra_cols = layout::columns(extras, 4, 6.0);
    draw_octave(ui, setter, params, extra_cols[0]);
    param_knob(ui, extra_cols[1], &SKIN, setter, &params.lfo_hz, "Rate");
    toggle(ui, setter, &params.repeat, extra_cols[2], "Repeat");
    param_knob(ui, extra_cols[3], &SKIN, setter, &params.gain, "Out");

    draw_output_well(ui, well_rect, scope);
}

fn draw_edit(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    rest: egui::Rect,
    scope: &ScopeBuffer,
) {
    let well_h = 72.0f32.min(rest.height() * 0.18);
    let (body, well_rect) = layout::split_top(rest, (rest.height() - well_h - 8.0).max(0.0), 8.0);
    let rows = layout::rows(body, 3, 8.0);

    let inner = paint_plate(ui.painter(), rows[0], &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Oscillator / VCF");
    let cells = layout::columns(inner, 10, 4.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.pulse, "Pulse");
    param_knob(ui, cells[1], &SKIN, setter, &params.pulse_level, "Pulse");
    param_knob(ui, cells[2], &SKIN, setter, &params.saw_level, "Saw");
    param_knob(
        ui,
        cells[3],
        &SKIN,
        setter,
        &params.resonator_mix,
        "Res Mix",
    );
    param_knob(ui, cells[4], &SKIN, setter, &params.hp_hz, "HP");
    toggle(ui, setter, &params.vcf_enable, cells[5], "VCF");
    param_knob(ui, cells[6], &SKIN, setter, &params.vcf_cutoff, "Cut");
    param_knob(ui, cells[7], &SKIN, setter, &params.vcf_resonance, "Q");
    param_knob(ui, cells[8], &SKIN, setter, &params.vcf_keytrack, "Kbd");
    param_knob(ui, cells[9], &SKIN, setter, &params.growl, "Growl");

    let inner = paint_plate(ui.painter(), rows[1], &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Envelopes");
    let cells = layout::columns(inner, 10, 4.0);
    param_knob(ui, cells[0], &SKIN, setter, &params.attack_ms, "A");
    param_knob(ui, cells[1], &SKIN, setter, &params.decay_ms, "D");
    param_knob(ui, cells[2], &SKIN, setter, &params.sustain, "S");
    param_knob(ui, cells[3], &SKIN, setter, &params.release_ms, "R");
    param_knob(ui, cells[4], &SKIN, setter, &params.adsr_to_vcf, "→VCF");
    param_knob(ui, cells[5], &SKIN, setter, &params.adsr_to_vca, "→VCA");
    param_knob(ui, cells[6], &SKIN, setter, &params.ar_attack_ms, "AR A");
    param_knob(ui, cells[7], &SKIN, setter, &params.ar_release_ms, "AR R");
    param_knob(ui, cells[8], &SKIN, setter, &params.lfo_fm, "AutoVib");
    param_knob(ui, cells[9], &SKIN, setter, &params.lfo_delay_ms, "Delay");

    let inner = paint_plate(ui.painter(), rows[2], &SKIN);
    let inner = plate_legend(ui.painter(), inner, &SKIN, "Resonators");
    let cells = layout::columns(inner, 5, 6.0);
    draw_res_slot(
        ui,
        setter,
        cells[0],
        "1",
        &params.r0_on,
        &params.r0_curve,
        &params.r0_vcf,
        &params.r0_vca,
    );
    draw_res_slot(
        ui,
        setter,
        cells[1],
        "2",
        &params.r1_on,
        &params.r1_curve,
        &params.r1_vcf,
        &params.r1_vca,
    );
    draw_res_slot(
        ui,
        setter,
        cells[2],
        "3",
        &params.r2_on,
        &params.r2_curve,
        &params.r2_vcf,
        &params.r2_vca,
    );
    draw_res_slot(
        ui,
        setter,
        cells[3],
        "4",
        &params.r3_on,
        &params.r3_curve,
        &params.r3_vcf,
        &params.r3_vca,
    );
    draw_res_slot(
        ui,
        setter,
        cells[4],
        "5",
        &params.r4_on,
        &params.r4_curve,
        &params.r4_vcf,
        &params.r4_vca,
    );

    draw_output_well(ui, well_rect, scope);
}

fn draw_res_slot(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    rect: egui::Rect,
    tag: &str,
    on: &BoolParam,
    curve: &IntParam,
    vcf: &FloatParam,
    vca: &FloatParam,
) {
    let rows = layout::rows(rect, 2, 4.0);
    let top = layout::columns(rows[0], 2, 4.0);
    toggle(ui, setter, on, top[0], tag);
    param_knob(ui, top[1], &SKIN, setter, curve, "Curve");
    let bot = layout::columns(rows[1], 2, 4.0);
    param_knob(ui, bot[0], &SKIN, setter, vcf, "VCF");
    param_knob(ui, bot[1], &SKIN, setter, vca, "VCA");
}

fn draw_octave(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &CottWhistleParams,
    rect: egui::Rect,
) {
    let inner = plate_legend(ui.painter(), rect, &SKIN, "Oct");
    let cells = layout::columns(inner, 3, 3.0);
    let cur = params.octave.value();
    for (i, &val) in [-1i32, 0, 1].iter().enumerate() {
        let label = match val {
            -1 => "-1",
            0 => "0",
            _ => "+1",
        };
        if segment_button(
            ui,
            cells[i],
            &SKIN,
            &format!("oct_{val}"),
            label,
            cur == val,
        )
        .clicked()
        {
            set_i(setter, &params.octave, val);
        }
    }
}

fn toggle(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    param: &BoolParam,
    rect: egui::Rect,
    label: &str,
) {
    if segment_button(ui, rect, &SKIN, param.name(), label, param.value()).clicked() {
        set_b(setter, param, !param.value());
    }
}

fn draw_output_well(ui: &mut egui::Ui, rect: egui::Rect, scope: &ScopeBuffer) {
    let well = paint_well(ui.painter(), rect, &SKIN);
    let mut samples = [0.0f32; SCOPE_LEN];
    scope.snapshot(&mut samples);
    paint_waveform(ui.painter(), well, &SKIN, &samples);
    ui.painter().text(
        well.left_bottom() + Vec2::new(4.0, -3.0),
        Align2::LEFT_BOTTOM,
        "OUTPUT",
        FontId::monospace(11.0),
        cott_plugin_ui::with_alpha(SKIN.legend, 210),
    );
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
        let ids = ids();
        assert!(!ids.is_empty());
        for id in &ids {
            assert!(id.starts_with("v4-"), "{id} would collide with old state");
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
    fn none_of_the_g_funk_knobs_came_back() {
        let ids = ids();
        for retired in ["char", "blend", "detune", "emph", "body", "drive"] {
            assert!(
                !ids.iter().any(|id| id.trim_start_matches("v4-") == retired),
                "{retired} is still on the panel"
            );
        }
    }

    #[test]
    fn the_panel_comes_up_on_oboe() {
        let params = CottWhistleParams::default();
        assert_eq!(params.voice(), Voice::Oboe);
        assert_eq!(params.to_dsp().pulse, PulseWidth::One14);
        assert!(!params.edit_view.value());
    }

    #[test]
    fn class_id_did_not_change() {
        assert_eq!(&CottWhistle::VST3_CLASS_ID, b"CottWhstlVST3CE!");
    }
}
