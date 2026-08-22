//! Circuit recipe: switch bits and resistor values, not a generic synth preset.

use crate::envelope::AdsrParams;
use crate::filter::CURVES;
use crate::voice::{PulseWidth, Voice};

/// One of five resonator slots. Hardware max is five active at once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResonatorSlot {
    pub enabled: bool,
    /// Index into [`crate::filter::CURVES`].
    pub curve: u8,
    pub to_vcf: f32,
    pub to_vca: f32,
}

impl ResonatorSlot {
    pub const OFF: Self = Self {
        enabled: false,
        curve: 0,
        to_vcf: 0.0,
        to_vca: 0.0,
    };

    pub fn freq_hz(self) -> f32 {
        CURVES[self.curve.min(9) as usize].0
    }

    pub fn q(self) -> f32 {
        CURVES[self.curve.min(9) as usize].1
    }
}

/// ROM wiring for one paddle. Cited against patent figures / SM lines, not a
/// copied schematic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    pub pulse: PulseWidth,
    /// ROM-4 select bits: dyn, 1/14, 1/9, 1/64, 1/2, 2/11.
    pub pulse_bits: u8,
    /// Pulse into the HP/VCF mixer. Factory voices that skip the VCF leave this 0.
    pub pulse_level: f32,
    /// Staircase saw into the HP/VCF mixer. Tuba and Flute live here.
    pub saw_level: f32,
    /// 0 = pulse into resonators, 1 = saw. Hardware cannot send saw through
    /// the bank; factory recipes are 0.
    pub resonator_mix: f32,
    pub hp_hz: f32,
    /// Z7 HPF A/B/C/D enables, bit 0 = A.
    pub hp_mask: u8,
    pub vcf_enable: bool,
    /// 0..1, mapped to ~80 Hz–8 kHz.
    pub vcf_cutoff: f32,
    pub vcf_resonance: f32,
    pub vcf_keytrack: f32,
    pub adsr_to_vcf: f32,
    pub ar_to_vcf: f32,
    pub growl: f32,
    pub adsr: AdsrParams,
    pub ar_attack_ms: f32,
    pub ar_release_ms: f32,
    pub adsr_to_vca: f32,
    pub ar_to_vca: f32,
    /// Dynamic PWM from the envelopes. Fuzz Guitar lives here; there is no
    /// clipper after the VCA.
    pub adsr_pwm: f32,
    pub ar_pwm: f32,
    pub lfo_pwm: f32,
    /// Programmed auto-vibrato depth in semitones. The Vibrato touch switch
    /// *replaces* this so pressure owns it.
    pub lfo_fm: f32,
    pub lfo_delay_ms: f32,
    pub lfo_to_vca: f32,
    pub resonators: [ResonatorSlot; 5],
    /// Unused. Panel transpose lives on [`WhistleParams::octave`].
    pub octave: i32,
    /// Divider octave from Z15 down-1 / down-2. Tuba is -3.
    pub rom_octave: i32,
}

impl Recipe {
    pub fn pulse_width(self) -> f32 {
        self.pulse.ratio()
    }
}

/// Live circuit + performance controls. The engine never branches on Voice;
/// the plugin stamps a [`Recipe`] into these fields when a paddle is thrown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhistleParams {
    pub voice: Voice,
    pub volume: f32,
    pub touch_sens: f32,
    pub brilliance: f32,
    pub portamento_ms: f32,
    pub octave: i32,
    pub lfo_hz: f32,
    pub repeat: bool,
    pub touch_bend: bool,
    pub touch_wow: bool,
    pub touch_growl: bool,
    pub touch_brilliance: bool,
    pub touch_volume: bool,
    pub touch_vibrato: bool,
    pub pulse: PulseWidth,
    pub pulse_bits: u8,
    pub pulse_level: f32,
    pub saw_level: f32,
    pub resonator_mix: f32,
    pub hp_hz: f32,
    pub hp_mask: u8,
    pub vcf_enable: bool,
    pub vcf_cutoff: f32,
    pub vcf_resonance: f32,
    pub vcf_keytrack: f32,
    pub adsr_to_vcf: f32,
    pub ar_to_vcf: f32,
    pub growl: f32,
    pub adsr: AdsrParams,
    pub ar_attack_ms: f32,
    pub ar_release_ms: f32,
    pub adsr_to_vca: f32,
    pub ar_to_vca: f32,
    pub adsr_pwm: f32,
    pub ar_pwm: f32,
    pub lfo_pwm: f32,
    pub lfo_fm: f32,
    pub lfo_delay_ms: f32,
    pub lfo_to_vca: f32,
    pub resonators: [ResonatorSlot; 5],
    pub gain: f32,
    pub rom_octave: i32,
}

impl Default for WhistleParams {
    fn default() -> Self {
        Self::from_voice(Voice::Oboe)
    }
}

impl WhistleParams {
    pub fn from_voice(voice: Voice) -> Self {
        Self::from_recipe(voice, voice.recipe())
    }

    pub fn from_recipe(voice: Voice, r: Recipe) -> Self {
        Self {
            voice,
            volume: 0.72,
            touch_sens: 0.55,
            brilliance: 0.62,
            portamento_ms: 0.0,
            octave: r.octave,
            lfo_hz: 5.5,
            repeat: false,
            touch_bend: false,
            touch_wow: false,
            touch_growl: false,
            touch_brilliance: false,
            touch_volume: false,
            touch_vibrato: false,
            pulse: r.pulse,
            pulse_bits: r.pulse_bits,
            pulse_level: r.pulse_level,
            saw_level: r.saw_level,
            resonator_mix: r.resonator_mix,
            hp_hz: r.hp_hz,
            hp_mask: r.hp_mask,
            vcf_enable: r.vcf_enable,
            vcf_cutoff: r.vcf_cutoff,
            vcf_resonance: r.vcf_resonance,
            vcf_keytrack: r.vcf_keytrack,
            adsr_to_vcf: r.adsr_to_vcf,
            ar_to_vcf: r.ar_to_vcf,
            growl: r.growl,
            adsr: r.adsr,
            ar_attack_ms: r.ar_attack_ms,
            ar_release_ms: r.ar_release_ms,
            adsr_to_vca: r.adsr_to_vca,
            ar_to_vca: r.ar_to_vca,
            adsr_pwm: r.adsr_pwm,
            ar_pwm: r.ar_pwm,
            lfo_pwm: r.lfo_pwm,
            lfo_fm: r.lfo_fm,
            lfo_delay_ms: r.lfo_delay_ms,
            lfo_to_vca: r.lfo_to_vca,
            resonators: r.resonators,
            gain: 0.55,
            rom_octave: r.rom_octave,
        }
    }

    pub fn clamped(self) -> Self {
        let mut p = self;
        p.volume = p.volume.clamp(0.0, 1.0);
        p.touch_sens = p.touch_sens.clamp(0.0, 1.0);
        p.brilliance = p.brilliance.clamp(0.0, 1.0);
        p.portamento_ms = p.portamento_ms.clamp(0.0, 2_500.0);
        p.octave = p.octave.clamp(-1, 1);
        p.rom_octave = p.rom_octave.clamp(-3, 0);
        p.lfo_hz = p.lfo_hz.clamp(0.1, 20.0);
        p.pulse_level = p.pulse_level.clamp(0.0, 1.0);
        p.saw_level = p.saw_level.clamp(0.0, 1.0);
        p.resonator_mix = p.resonator_mix.clamp(0.0, 1.0);
        p.hp_hz = p.hp_hz.clamp(20.0, 4_000.0);
        p.vcf_cutoff = p.vcf_cutoff.clamp(0.0, 1.0);
        p.vcf_resonance = p.vcf_resonance.clamp(0.0, 1.0);
        p.vcf_keytrack = p.vcf_keytrack.clamp(0.0, 1.0);
        p.adsr_to_vcf = p.adsr_to_vcf.clamp(0.0, 1.0);
        p.ar_to_vcf = p.ar_to_vcf.clamp(0.0, 1.0);
        p.growl = p.growl.clamp(0.0, 1.0);
        p.adsr = p.adsr.clamped();
        p.ar_attack_ms = p.ar_attack_ms.clamp(0.0, 5_000.0);
        p.ar_release_ms = p.ar_release_ms.clamp(0.0, 5_000.0);
        p.adsr_to_vca = p.adsr_to_vca.clamp(0.0, 1.0);
        p.ar_to_vca = p.ar_to_vca.clamp(0.0, 1.0);
        p.adsr_pwm = p.adsr_pwm.clamp(0.0, 1.0);
        p.ar_pwm = p.ar_pwm.clamp(0.0, 1.0);
        p.lfo_pwm = p.lfo_pwm.clamp(0.0, 1.0);
        p.lfo_fm = p.lfo_fm.clamp(0.0, 2.0);
        p.lfo_delay_ms = p.lfo_delay_ms.clamp(0.0, 3_000.0);
        p.lfo_to_vca = p.lfo_to_vca.clamp(0.0, 1.0);
        p.gain = p.gain.clamp(0.0, 1.0);
        p
    }
}
