//! Monophonic low-note-priority engine. One circuit, thirty recipes.

use crate::GROWL_HZ;
use crate::envelope::{AdsrState, ArState};
use crate::filter::{
    BANK_SIZE, DcBlocker, HPF_HZ, LadderLp, OnePoleHp, RESONANCE_MAX, ResonatorBank, curve_bank,
};
use crate::oscillator::Oscillator;
use crate::recipe::WhistleParams;

pub const OVERSAMPLE: i32 = 2;

/// Sample-accurate MIDI note event relative to the current block.
#[derive(Debug, Clone, Copy)]
pub struct MidiNoteEvent {
    pub sample_offset: u32,
    pub note: u8,
    pub velocity: u8,
    pub channel: u8,
    pub on: bool,
}

const MAX_HELD: usize = 16;
/// Aftertouch deadband so ordinary playing does not fire the strip.
const PRESSURE_DEADBAND: f32 = 0.08;
/// Pitch-bend from the strip is about a whole tone, up only.
const TOUCH_BEND_SEMITONES: f32 = 2.0;

#[derive(Debug, Clone, Copy)]
struct Held {
    note: u8,
    channel: u8,
}

#[derive(Debug, Clone)]
pub struct WhistleEngine {
    sample_rate: f32,
    oversample_rate: f32,
    held: [Option<Held>; MAX_HELD],
    sounding: Option<u8>,
    current_hz: f32,
    target_hz: f32,
    osc: Oscillator,
    hp: [OnePoleHp; 4],
    ladder: LadderLp,
    bank: ResonatorBank,
    adsr: AdsrState,
    ar: ArState,
    lfo_phase: f32,
    growl_phase: f32,
    lfo_age: f32,
    pressure_raw: f32,
    pitch_bend: f32,
    dc: DcBlocker,
    last_repeat: bool,
    last_hp_key: u32,
    last_sr_key: u32,
}

impl Default for WhistleEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl WhistleEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            oversample_rate: sr * OVERSAMPLE as f32,
            held: [None; MAX_HELD],
            sounding: None,
            current_hz: 440.0,
            target_hz: 440.0,
            osc: Oscillator::default(),
            hp: [OnePoleHp::default(); 4],
            ladder: LadderLp::default(),
            bank: ResonatorBank::default(),
            adsr: AdsrState::default(),
            ar: ArState::default(),
            lfo_phase: 0.0,
            growl_phase: 0.0,
            lfo_age: 0.0,
            pressure_raw: 0.0,
            pitch_bend: 0.0,
            dc: DcBlocker::new(sr),
            last_repeat: false,
            last_hp_key: u32::MAX,
            last_sr_key: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.sample_rate = sr;
        self.oversample_rate = sr * OVERSAMPLE as f32;
        self.dc.set_sample_rate(sr);
        self.last_hp_key = u32::MAX;
        self.last_sr_key = 0;
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn sounding_note(&self) -> Option<u8> {
        self.sounding
    }

    pub fn set_pitch_bend(&mut self, semitones: f32) {
        self.pitch_bend = semitones.clamp(-12.0, 12.0);
    }

    pub fn set_pressure(&mut self, pressure: f32) {
        self.pressure_raw = pressure.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        self.held = [None; MAX_HELD];
        self.sounding = None;
        self.osc.reset(0.0);
        for hp in &mut self.hp {
            hp.reset();
        }
        self.ladder.reset();
        self.bank.reset();
        self.adsr = AdsrState::default();
        self.ar = ArState::default();
        self.lfo_phase = 0.0;
        self.growl_phase = 0.0;
        self.lfo_age = 0.0;
        self.dc.reset();
        self.last_repeat = false;
        self.last_hp_key = u32::MAX;
        self.last_sr_key = 0;
    }

    pub fn all_notes_off(&mut self) {
        self.held = [None; MAX_HELD];
        self.sounding = None;
        let dummy = WhistleParams::default();
        self.adsr.note_off(&dummy.adsr, self.oversample_rate);
        self.ar.note_off(
            dummy.ar_attack_ms,
            dummy.ar_release_ms,
            self.oversample_rate,
        );
    }

    pub fn process_block(
        &mut self,
        params: &WhistleParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let p = params.clamped();
        let n = left.len().min(right.len());
        self.prepare_filters(&p);

        let mut ev_i = 0;
        for i in 0..n {
            while ev_i < events.len() && events[ev_i].sample_offset as usize <= i {
                self.handle_event(&p, events[ev_i]);
                ev_i += 1;
            }
            let mut acc = 0.0;
            for _ in 0..OVERSAMPLE {
                acc += self.tick(&p);
            }
            let y = self.dc.process(acc / OVERSAMPLE as f32);
            let y = if y.is_finite() { y } else { 0.0 };
            left[i] = y;
            right[i] = y;
        }
    }

    fn prepare_filters(&mut self, p: &WhistleParams) {
        let sr = self.oversample_rate;
        let hp_key = (p.hp_mask as u32) ^ ((p.hp_hz * 4.0) as u32);
        if hp_key != self.last_hp_key {
            if p.hp_mask == 0 {
                self.hp[0].set(p.hp_hz, sr);
            } else {
                for i in 0..4 {
                    self.hp[i].set(HPF_HZ[i], sr);
                }
            }
            self.last_hp_key = hp_key;
        }

        let key = p.resonators.iter().fold(0u32, |h, s| {
            h.wrapping_mul(16777619)
                ^ (s.enabled as u32)
                ^ ((s.curve as u32) << 1)
                ^ ((s.to_vcf * 100.0) as u32)
                ^ ((s.to_vca * 100.0) as u32) << 8
        });
        if key != self.last_sr_key {
            // Same Board C bank, two nets on: one interpolated peak.
            let mut logf = [0.0f32; BANK_SIZE];
            let mut qacc = [0.0f32; BANK_SIZE];
            let mut vcf = [0.0f32; BANK_SIZE];
            let mut vca = [0.0f32; BANK_SIZE];
            let mut n = [0u32; BANK_SIZE];
            for slot in &p.resonators {
                if !slot.enabled || (slot.to_vcf <= 0.0 && slot.to_vca <= 0.0) {
                    continue;
                }
                let b = curve_bank(slot.curve) as usize;
                logf[b] += slot.freq_hz().max(1.0).ln();
                qacc[b] += slot.q();
                vcf[b] = vcf[b].max(slot.to_vcf);
                vca[b] = vca[b].max(slot.to_vca);
                n[b] += 1;
            }
            for i in 0..BANK_SIZE {
                if n[i] == 0 {
                    self.bank.disable_slot(i);
                } else {
                    let freq = (logf[i] / n[i] as f32).exp();
                    let q = qacc[i] / n[i] as f32;
                    self.bank.set_slot(i, freq, q, vcf[i], vca[i], sr);
                }
            }
            self.last_sr_key = key;
        }
    }

    fn handle_event(&mut self, p: &WhistleParams, ev: MidiNoteEvent) {
        if ev.on && ev.velocity > 0 {
            self.note_on(p, ev.note.min(127), ev.channel);
        } else {
            self.note_off(p, ev.note.min(127), ev.channel);
        }
    }

    fn note_on(&mut self, p: &WhistleParams, note: u8, channel: u8) {
        if let Some(slot) = self.held.iter_mut().find(|s| s.is_none()) {
            *slot = Some(Held { note, channel });
        } else {
            self.held[0] = Some(Held { note, channel });
        }
        self.reassign(p, true);
    }

    fn note_off(&mut self, p: &WhistleParams, note: u8, channel: u8) {
        for slot in &mut self.held {
            if let Some(h) = slot {
                if h.note == note && h.channel == channel {
                    *slot = None;
                }
            }
        }
        if self.lowest_held().is_none() {
            self.sounding = None;
            self.adsr.note_off(&p.adsr, self.oversample_rate);
            self.ar
                .note_off(p.ar_attack_ms, p.ar_release_ms, self.oversample_rate);
        } else {
            self.reassign(p, true);
        }
    }

    fn lowest_held(&self) -> Option<u8> {
        self.held.iter().flatten().map(|h| h.note).min()
    }

    /// Low-note priority. GATE stays high while any key is down. TRIGGER fires
    /// on first press and on a key-change that actually takes the sounding note.
    fn reassign(&mut self, p: &WhistleParams, retrigger_on_change: bool) {
        let Some(low) = self.lowest_held() else {
            return;
        };
        let changed = self.sounding != Some(low);
        self.sounding = Some(low);
        let midi = (low as i32 + (p.octave + p.rom_octave) * 12).clamp(0, 127) as u8;
        self.target_hz = crate::midi_note_to_hz(midi);
        if self.current_hz < 1.0 {
            self.current_hz = self.target_hz;
        }
        if changed && retrigger_on_change {
            self.trigger(p);
        }
    }

    fn trigger(&mut self, p: &WhistleParams) {
        self.adsr.note_on(&p.adsr, self.oversample_rate);
        self.ar
            .note_on(p.ar_attack_ms, p.ar_release_ms, self.oversample_rate);
        self.lfo_age = 0.0;
    }

    fn tick(&mut self, p: &WhistleParams) -> f32 {
        let sr = self.oversample_rate;
        self.glide_toward(p, sr);

        let pressure = touch_amount(self.pressure_raw, p.touch_sens);
        let adsr = self.adsr.next_sample(&p.adsr, sr);
        let ar = self.ar.next_sample(p.ar_attack_ms, p.ar_release_ms, sr);

        self.lfo_phase = wrap(self.lfo_phase + p.lfo_hz / sr);
        self.growl_phase = wrap(self.growl_phase + GROWL_HZ / sr);
        if self.sounding.is_some() {
            self.lfo_age += 1.0 / sr;
        }

        if p.repeat && self.sounding.is_some() {
            let hi = self.lfo_phase < 0.5;
            if hi && !self.last_repeat {
                self.trigger(p);
            }
            self.last_repeat = hi;
        } else {
            self.last_repeat = false;
        }

        let lfo = (self.lfo_phase * std::f32::consts::TAU).sin();
        let growl_wave = growl_shape(self.growl_phase);

        let vib = if p.touch_vibrato {
            lfo * pressure * 0.35
        } else {
            let delay = p.lfo_delay_ms * 0.001;
            let fade = if delay <= 0.0 {
                1.0
            } else {
                ((self.lfo_age - delay) / delay.max(0.05)).clamp(0.0, 1.0)
            };
            lfo * p.lfo_fm * fade
        };

        let bend = if p.touch_bend {
            pressure * TOUCH_BEND_SEMITONES
        } else {
            0.0
        };
        let hz = self.current_hz * 2f32.powf((vib + bend + self.pitch_bend) / 12.0);
        let dt = (hz / sr).clamp(0.0, 0.49);

        let pwm =
            (p.adsr_pwm * adsr + p.ar_pwm * ar + p.lfo_pwm * (0.5 + 0.5 * lfo)).clamp(0.0, 0.95);
        // Dynamic converter: ADSR into a ramp-to-pulse. Starts at 1/2, narrows.
        let dyn_width = (0.5 * (1.0 - pwm)).clamp(0.03, 0.5);
        let osc = self.osc.next(dt, p.pulse_bits, dyn_width);

        // Mixer (saw and/or pulse) through the four Board C series-RC HPs, then 4034.
        let mix = osc.pulse * p.pulse_level + osc.saw * p.saw_level;
        let hp_out = self.cascade_hp(mix, p.hp_mask);
        // Resonators see pulse only (factory resonator_mix is 0).
        let res_in = osc.pulse * (1.0 - p.resonator_mix) + osc.saw * p.resonator_mix;
        let (res_vcf, res_vca) = self.bank.process(res_in);

        let vcf_out = if p.vcf_enable {
            let brilliance = if p.touch_brilliance {
                (p.brilliance + pressure * 0.45).clamp(0.0, 1.0)
            } else {
                p.brilliance
            };
            // Barton: brilliance attenuates the envelope mix into the VCF,
            // about 100% down to 30%, plus a DC offset so the slider still
            // does something when envelopes rest.
            let env_mix = p.adsr_to_vcf * adsr + p.ar_to_vcf * ar;
            let brill_gain = 0.30 + 0.70 * brilliance;
            let env_term = env_mix * brill_gain + 0.12 * brilliance;

            let growl_amt = if p.touch_growl {
                (p.growl + pressure * 0.7).clamp(0.0, 1.0)
            } else {
                p.growl
            };
            let growl_cv = growl_wave * growl_amt * 0.35;

            let wow = if p.touch_wow { pressure * 0.55 } else { 0.0 };
            let note = self.sounding.unwrap_or(60) as f32;
            let key = ((note - 60.0) / 48.0) * p.vcf_keytrack;

            let cutoff_n = (p.vcf_cutoff + env_term * 0.55 + key + growl_cv + wow).clamp(0.0, 1.0);
            let cutoff_hz = 80.0 * 100f32.powf(cutoff_n);

            let resonance = if p.touch_wow {
                p.vcf_resonance + (RESONANCE_MAX - p.vcf_resonance).max(0.0) * pressure
            } else {
                p.vcf_resonance
            };
            self.ladder.set(cutoff_hz, resonance.min(RESONANCE_MAX), sr);
            self.ladder.process(hp_out + res_vcf)
        } else {
            0.0
        };

        let vca_env = p.adsr_to_vca * adsr + p.ar_to_vca * ar;
        let trem = 1.0 - p.lfo_to_vca * (0.5 + 0.5 * lfo);
        let touch_vol = if p.touch_volume {
            0.25 + 0.75 * pressure
        } else {
            1.0
        };
        let audio = (vcf_out + res_vca) * vca_env * trem;
        audio * p.volume * p.gain * touch_vol * 0.45
    }

    fn cascade_hp(&mut self, x: f32, mask: u8) -> f32 {
        if mask == 0 {
            return self.hp[0].process(x);
        }
        let mut y = x;
        for i in 0..4 {
            if mask & (1 << i) != 0 {
                y = self.hp[i].process(y);
            }
        }
        y
    }

    fn glide_toward(&mut self, p: &WhistleParams, sr: f32) {
        if p.portamento_ms <= 0.5 {
            self.current_hz = self.target_hz;
            return;
        }
        let tau = p.portamento_ms * 0.001;
        let coeff = 1.0 - (-1.0 / (tau * sr)).exp();
        self.current_hz += (self.target_hz - self.current_hz) * coeff.clamp(0.0, 1.0);
    }
}

fn touch_amount(raw: f32, sens: f32) -> f32 {
    if raw <= PRESSURE_DEADBAND {
        0.0
    } else {
        let spanned = (raw - PRESSURE_DEADBAND) / (1.0 - PRESSURE_DEADBAND);
        (spanned * sens).clamp(0.0, 1.0)
    }
}

/// Board D 32 Hz astable into a bandpass, into VCF CV. A sine is what that
/// bandpass leaves of a square.
fn growl_shape(phase: f32) -> f32 {
    (phase * std::f32::consts::TAU).sin()
}

fn wrap(x: f32) -> f32 {
    x - x.floor()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::Voice;

    fn render(params: &WhistleParams, note: u8, n: usize) -> Vec<f32> {
        let mut eng = WhistleEngine::new(48_000.0);
        let events = [MidiNoteEvent {
            sample_offset: 0,
            note,
            velocity: 100,
            channel: 0,
            on: true,
        }];
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        eng.process_block(params, &events, &mut left, &mut right);
        left
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn oboe_makes_sound() {
        let p = WhistleParams::from_voice(Voice::Oboe);
        let buf = render(&p, 69, 8_000);
        assert!(rms(&buf[1_000..]) > 1e-4, "oboe was silent");
    }

    #[test]
    fn low_note_priority_keeps_the_bottom() {
        let p = WhistleParams::from_voice(Voice::Oboe);
        let mut eng = WhistleEngine::new(48_000.0);
        let events = [
            MidiNoteEvent {
                sample_offset: 0,
                note: 60,
                velocity: 100,
                channel: 0,
                on: true,
            },
            MidiNoteEvent {
                sample_offset: 64,
                note: 64,
                velocity: 100,
                channel: 0,
                on: true,
            },
        ];
        let mut left = vec![0.0f32; 128];
        let mut right = vec![0.0f32; 128];
        eng.process_block(&p, &events, &mut left, &mut right);
        assert_eq!(eng.sounding_note(), Some(60));
    }

    #[test]
    fn a_lower_key_steals() {
        let p = WhistleParams::from_voice(Voice::Oboe);
        let mut eng = WhistleEngine::new(48_000.0);
        let events = [
            MidiNoteEvent {
                sample_offset: 0,
                note: 64,
                velocity: 100,
                channel: 0,
                on: true,
            },
            MidiNoteEvent {
                sample_offset: 32,
                note: 55,
                velocity: 100,
                channel: 0,
                on: true,
            },
        ];
        let mut left = vec![0.0f32; 64];
        let mut right = vec![0.0f32; 64];
        eng.process_block(&p, &events, &mut left, &mut right);
        assert_eq!(eng.sounding_note(), Some(55));
    }

    #[test]
    fn brilliance_does_nothing_when_the_ladder_is_out() {
        let mut dark = WhistleParams::from_voice(Voice::Oboe);
        dark.vcf_enable = false;
        dark.brilliance = 0.0;
        let mut bright = dark;
        bright.brilliance = 1.0;
        let a = render(&dark, 72, 6_000);
        let b = render(&bright, 72, 6_000);
        let err: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(err < 1e-4, "brilliance moved a bypassed voice by {err}");
    }

    #[test]
    fn growl_runs_near_thirty_two_hertz() {
        let sr = 48_000.0;
        let mut phase = 0.0f32;
        let mut crossings = 0u32;
        let mut prev = 0.0f32;
        for _ in 0..(sr as i32) {
            phase = wrap(phase + GROWL_HZ / sr);
            let y = growl_shape(phase);
            if prev >= 0.0 && y < 0.0 {
                crossings += 1;
            }
            prev = y;
        }
        assert!(
            (30..=34).contains(&crossings),
            "growl crossings in 1s: {crossings}"
        );
    }

    #[test]
    fn aftertouch_deadband_ignores_a_light_press() {
        assert_eq!(touch_amount(0.05, 1.0), 0.0);
        assert!(touch_amount(0.5, 1.0) > 0.3);
        assert!(touch_amount(0.5, 0.2) < touch_amount(0.5, 1.0));
    }

    #[test]
    fn touch_volume_makes_pressure_the_vca() {
        let mut p = WhistleParams::from_voice(Voice::Oboe);
        p.touch_volume = true;
        p.touch_sens = 1.0;
        let quiet = {
            let mut eng = WhistleEngine::new(48_000.0);
            eng.set_pressure(0.0);
            let events = [MidiNoteEvent {
                sample_offset: 0,
                note: 69,
                velocity: 100,
                channel: 0,
                on: true,
            }];
            let mut left = vec![0.0f32; 6_000];
            let mut right = vec![0.0f32; 6_000];
            eng.process_block(&p, &events, &mut left, &mut right);
            rms(&left[1_000..])
        };
        let loud = {
            let mut eng = WhistleEngine::new(48_000.0);
            eng.set_pressure(1.0);
            let events = [MidiNoteEvent {
                sample_offset: 0,
                note: 69,
                velocity: 100,
                channel: 0,
                on: true,
            }];
            let mut left = vec![0.0f32; 6_000];
            let mut right = vec![0.0f32; 6_000];
            eng.process_block(&p, &events, &mut left, &mut right);
            rms(&left[1_000..])
        };
        assert!(
            loud > quiet * 1.5,
            "pressure should open the VCA, quiet={quiet} loud={loud}"
        );
    }

    #[test]
    fn wow_overrides_resonance() {
        let mut p = WhistleParams::from_voice(Voice::Tuba);
        p.touch_wow = true;
        p.touch_sens = 1.0;
        p.vcf_resonance = 0.05;
        p.growl = 0.0;
        let dull = {
            let mut eng = WhistleEngine::new(48_000.0);
            eng.set_pressure(0.0);
            let events = [MidiNoteEvent {
                sample_offset: 0,
                note: 48,
                velocity: 100,
                channel: 0,
                on: true,
            }];
            let mut left = vec![0.0f32; 8_000];
            let mut right = vec![0.0f32; 8_000];
            eng.process_block(&p, &events, &mut left, &mut right);
            rms(&left[2_000..])
        };
        let wow = {
            let mut eng = WhistleEngine::new(48_000.0);
            eng.set_pressure(1.0);
            let events = [MidiNoteEvent {
                sample_offset: 0,
                note: 48,
                velocity: 100,
                channel: 0,
                on: true,
            }];
            let mut left = vec![0.0f32; 8_000];
            let mut right = vec![0.0f32; 8_000];
            eng.process_block(&p, &events, &mut left, &mut right);
            rms(&left[2_000..])
        };
        assert!(
            (wow - dull).abs() > 1e-4,
            "wow should move the ladder, dull={dull} wow={wow}"
        );
    }
}
