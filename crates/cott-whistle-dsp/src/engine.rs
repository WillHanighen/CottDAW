//! The monophonic voice: one note at a time, always gliding.
//!
//! Two oscillators — a pulse at the character's programmed duty and either a
//! Minimoog ramp or an ARP divider staircase — feed a fixed high-pass, then
//! split between the resonator bank and the direct path before reaching the
//! ladder. One note sounds at a time with last-note priority, and overlapping
//! notes slide rather than retrigger, which is the entire trick behind the sound.

use serde::{Deserialize, Serialize};

use crate::character::{Character, Settings};
use crate::filter::{DcBlocker, LadderLp, OnePoleHp, ResonatorBank};
use crate::oscillator::{Oscillator, Shape};

/// Held notes tracked for last-note priority.
pub const MAX_HELD_NOTES: usize = 16;

/// Ladder coefficients are refreshed this often, in samples.
const COEFF_INTERVAL: usize = 32;
/// Vibrato fades in over this long once its delay has run out.
const VIBRATO_SWELL_MS: f32 = 140.0;
/// Panel controls reach a new value in about this long.
const SMOOTH_MS: f32 = 12.0;
/// Level going into the filter. A 1/14 pulse peaks far above its own RMS, so
/// the mixer is padded rather than left to slam the ladder's input stage.
const INPUT_TRIM: f32 = 0.62;
/// Makeup for the filter and the envelope, so Output at halfway already sits in
/// a mix and Output wide open still leaves headroom.
const OUTPUT_TRIM: f32 = 1.9;
/// The mod wheel is worth this much extra vibrato on top of the panel depth.
const MOD_WHEEL_CENTS: f32 = 55.0;
/// C5, the register these leads live in. Cutoff is calibrated here and the
/// keyboard-follow pivots around it.
const KEY_TRACK_REF_HZ: f32 = 523.251_1;

/// Everything the panel can reach, plus the character that wires the rest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WhistleParams {
    pub character: Character,
    /// Portamento time between notes, milliseconds.
    pub glide_ms: f32,
    /// Transpose in octaves.
    pub octave: i32,
    /// Oscillator mixer: 0 is all pulse, 1 is all saw.
    pub blend: f32,
    /// Spread between the two oscillators, cents.
    pub detune_cents: f32,
    /// VCF cutoff within the character's range, 0..1.
    pub brilliance: f32,
    /// VCF feedback, 0..1.
    pub emphasis: f32,
    /// How much of the voice comes through the resonator bank, 0..1.
    pub body: f32,
    pub vibrato_hz: f32,
    /// Vibrato depth in cents; the mod wheel adds on top.
    pub vibrato_cents: f32,
    /// How long a fresh note waits before the vibrato swells in.
    pub vibrato_delay_ms: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// Saturation after the amplifier, 0..1.
    pub drive: f32,
    pub gain: f32,
}

impl Default for WhistleParams {
    fn default() -> Self {
        Self::for_character(Character::default())
    }
}

impl WhistleParams {
    /// The calibrated settings for a character.
    pub fn for_character(character: Character) -> Self {
        let Settings {
            glide_ms,
            octave,
            blend,
            detune_cents,
            brilliance,
            emphasis,
            body,
            vibrato_hz,
            vibrato_cents,
            vibrato_delay_ms,
            attack_ms,
            release_ms,
            drive,
            gain,
        } = character.recipe().defaults;
        Self {
            character,
            glide_ms,
            octave,
            blend,
            detune_cents,
            brilliance,
            emphasis,
            body,
            vibrato_hz,
            vibrato_cents,
            vibrato_delay_ms,
            attack_ms,
            release_ms,
            drive,
            gain,
        }
    }

    pub fn clamped(self) -> Self {
        Self {
            character: self.character,
            glide_ms: self.glide_ms.clamp(0.0, 4_000.0),
            octave: self.octave.clamp(-2, 3),
            blend: self.blend.clamp(0.0, 1.0),
            detune_cents: self.detune_cents.clamp(0.0, 50.0),
            brilliance: self.brilliance.clamp(0.0, 1.0),
            emphasis: self.emphasis.clamp(0.0, 1.0),
            body: self.body.clamp(0.0, 1.0),
            vibrato_hz: self.vibrato_hz.clamp(0.05, 20.0),
            vibrato_cents: self.vibrato_cents.clamp(0.0, 200.0),
            vibrato_delay_ms: self.vibrato_delay_ms.clamp(0.0, 3_000.0),
            attack_ms: self.attack_ms.clamp(0.1, 5_000.0),
            release_ms: self.release_ms.clamp(1.0, 8_000.0),
            drive: self.drive.clamp(0.0, 1.0),
            gain: self.gain.clamp(0.0, 1.0),
        }
    }

    /// Duty cycle the character's oscillator runs at.
    pub fn pulse_width(&self) -> f32 {
        self.character.recipe().pulse_width
    }

    /// Octaves above C5 the VCF sits when playing in the lead's own register.
    pub fn cutoff_octaves(&self) -> f32 {
        let (lo, hi) = self.character.recipe().cutoff_octaves;
        lo + (hi - lo) * self.brilliance.clamp(0.0, 1.0)
    }

    /// How far the VCF follows the keyboard, 0..1.
    pub fn key_track(&self) -> f32 {
        self.character.recipe().key_track
    }

    /// VCF cutoff for a given played frequency.
    ///
    /// The filter follows the keyboard, but not one for one — these machines
    /// had a keyboard-follow trim rather than a hard link, and with the
    /// resonator bank standing still it would be wrong for the VCF to move with
    /// the note anyway. Low notes stay dark, high ones open up.
    pub fn cutoff_hz(&self, note_hz: f32) -> f32 {
        let anchor = KEY_TRACK_REF_HZ * 2f32.powf(self.cutoff_octaves());
        anchor * (note_hz.max(1.0) / KEY_TRACK_REF_HZ).powf(self.key_track())
    }
}

/// Sample-accurate MIDI note event relative to the current block.
#[derive(Debug, Clone, Copy)]
pub struct MidiNoteEvent {
    pub sample_offset: u32,
    pub note: u8,
    pub velocity: u8,
    pub channel: u8,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Idle,
    Attack,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct HeldNote {
    note: u8,
    channel: u8,
    velocity: f32,
}

/// One gliding voice and the circuit behind it.
#[derive(Debug, Clone)]
pub struct WhistleEngine {
    sample_rate: f32,

    held: [HeldNote; MAX_HELD_NOTES],
    held_len: usize,

    /// Fractional MIDI note the glide is heading for.
    target_note: f32,
    /// Fractional MIDI note actually sounding.
    current_note: f32,
    velocity: f32,

    stage: EnvStage,
    env: f32,

    saw: Oscillator,
    pulse: Oscillator,
    lfo_phase: f32,
    /// Seconds since the last fresh note, for the vibrato delay.
    since_attack: f32,

    pitch_bend_semitones: f32,
    mod_wheel: f32,

    blend_smoothed: f32,
    body_smoothed: f32,
    drive_smoothed: f32,
    gain_smoothed: f32,
    brilliance_smoothed: f32,
    emphasis_smoothed: f32,

    /// Character the static coefficients were built for.
    wired_for: Option<Character>,
    coeff_countdown: usize,

    hp: OnePoleHp,
    bank: ResonatorBank,
    ladder: LadderLp,
    dc: DcBlocker,
}

impl Default for WhistleEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl WhistleEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let defaults = WhistleParams::default();
        Self {
            sample_rate,
            held: [HeldNote {
                note: 0,
                channel: 0,
                velocity: 0.0,
            }; MAX_HELD_NOTES],
            held_len: 0,
            target_note: 69.0,
            current_note: 69.0,
            velocity: 0.0,
            stage: EnvStage::Idle,
            env: 0.0,
            saw: Oscillator::new(0.0),
            pulse: Oscillator::new(0.25),
            lfo_phase: 0.0,
            since_attack: 0.0,
            pitch_bend_semitones: 0.0,
            mod_wheel: 0.0,
            blend_smoothed: defaults.blend,
            body_smoothed: defaults.body,
            drive_smoothed: defaults.drive,
            gain_smoothed: defaults.gain,
            brilliance_smoothed: defaults.brilliance,
            emphasis_smoothed: defaults.emphasis,
            wired_for: None,
            coeff_countdown: 0,
            hp: OnePoleHp::new(90.0, sample_rate),
            bank: ResonatorBank::new(),
            ladder: LadderLp::new(4_000.0, defaults.emphasis, sample_rate),
            dc: DcBlocker::new(sample_rate),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Call from `initialize`, not from the audio thread.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.dc.set_sample_rate(self.sample_rate);
        self.wired_for = None;
        self.coeff_countdown = 0;
    }

    pub fn reset(&mut self) {
        self.held_len = 0;
        self.stage = EnvStage::Idle;
        self.env = 0.0;
        self.velocity = 0.0;
        self.saw.reset(0.0);
        self.pulse.reset(0.25);
        self.lfo_phase = 0.0;
        self.since_attack = 0.0;
        self.pitch_bend_semitones = 0.0;
        self.mod_wheel = 0.0;
        self.hp.reset();
        self.bank.reset();
        self.ladder.reset();
        self.dc.reset();
    }

    /// Pitch bend in semitones; the wrapper applies the host's bend range.
    pub fn set_pitch_bend(&mut self, semitones: f32) {
        self.pitch_bend_semitones = semitones.clamp(-48.0, 48.0);
    }

    /// Mod wheel, 0..1. Deepens the vibrato.
    pub fn set_mod_wheel(&mut self, amount: f32) {
        self.mod_wheel = amount.clamp(0.0, 1.0);
    }

    /// Frequency currently sounding, before the octave switch and vibrato.
    pub fn current_hz(&self) -> f32 {
        midi_note_to_hz_f32(self.current_note)
    }

    pub fn envelope_level(&self) -> f32 {
        self.env
    }

    pub fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    pub fn held_count(&self) -> usize {
        self.held_len
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        if velocity == 0 {
            self.note_off(note, channel);
            return;
        }
        let legato = self.held_len > 0 && self.stage != EnvStage::Idle;
        self.push_held(HeldNote {
            note: note.min(127),
            channel: channel & 0x0f,
            velocity: velocity.min(127) as f32 / 127.0,
        });
        self.retarget();

        if !legato {
            // A fresh entry: land on the pitch, restart the envelope, and hold
            // the vibrato back until the note has had a moment to speak.
            if self.stage == EnvStage::Idle {
                self.current_note = self.target_note;
            }
            self.since_attack = 0.0;
            self.stage = EnvStage::Attack;
        }
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        let note = note.min(127);
        let channel = channel & 0x0f;
        let mut i = 0;
        while i < self.held_len {
            if self.held[i].note == note && self.held[i].channel == channel {
                for j in i..self.held_len - 1 {
                    self.held[j] = self.held[j + 1];
                }
                self.held_len -= 1;
            } else {
                i += 1;
            }
        }
        if self.held_len == 0 {
            if self.stage != EnvStage::Idle {
                self.stage = EnvStage::Release;
            }
        } else {
            self.retarget();
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held_len = 0;
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    fn push_held(&mut self, note: HeldNote) {
        // Re-pressing a held note moves it back to the top of the stack.
        let mut i = 0;
        while i < self.held_len {
            if self.held[i].note == note.note && self.held[i].channel == note.channel {
                for j in i..self.held_len - 1 {
                    self.held[j] = self.held[j + 1];
                }
                self.held_len -= 1;
            } else {
                i += 1;
            }
        }
        if self.held_len == MAX_HELD_NOTES {
            for j in 0..MAX_HELD_NOTES - 1 {
                self.held[j] = self.held[j + 1];
            }
            self.held_len -= 1;
        }
        self.held[self.held_len] = note;
        self.held_len += 1;
    }

    /// Last-note priority: the newest key wins.
    fn retarget(&mut self) {
        if self.held_len == 0 {
            return;
        }
        let top = self.held[self.held_len - 1];
        self.target_note = top.note as f32;
        self.velocity = top.velocity;
    }

    /// Rebuild the parts of the circuit that only move when the character does.
    fn rewire(&mut self, character: Character) {
        let recipe = character.recipe();
        self.hp.set(recipe.hp_hz, self.sample_rate);
        self.bank.set(&recipe.resonators, self.sample_rate);
        self.wired_for = Some(character);
    }

    /// Render a block, applying note events at their sample offsets.
    ///
    /// The voice is mono; both channels get the same signal.
    pub fn process_block(
        &mut self,
        params: &WhistleParams,
        events: &[MidiNoteEvent],
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let params = params.clamped();
        let frames = left.len().min(right.len());

        if self.wired_for != Some(params.character) {
            self.rewire(params.character);
        }
        let recipe = params.character.recipe();

        let glide_coeff = glide_coefficient(params.glide_ms, self.sample_rate);
        let attack_step = 1.0 / (params.attack_ms * 0.001 * self.sample_rate).max(1.0);
        // Fall by 40 dB over the release time, then stop.
        let release_coeff = (-4.6 / (params.release_ms * 0.001 * self.sample_rate).max(1.0)).exp();
        let vib_step = params.vibrato_hz / self.sample_rate;
        let vib_delay_s = params.vibrato_delay_ms * 0.001;
        let vib_swell = 1.0 / (VIBRATO_SWELL_MS * 0.001).max(1e-4);
        let dt_seconds = 1.0 / self.sample_rate;
        let smooth = 1.0 - (-1.0 / (SMOOTH_MS * 0.001 * self.sample_rate).max(1.0)).exp();
        let cutoff_ceiling = (self.sample_rate * 0.45).min(18_000.0);
        let detune = params.detune_cents * 0.5;
        let saw_ratio = cents_ratio(detune);
        let pulse_ratio = cents_ratio(-detune);
        let nyquist_guard = self.sample_rate * 0.48;

        let mut event_i = 0;
        for frame in 0..frames {
            while event_i < events.len() && events[event_i].sample_offset as usize <= frame {
                let ev = events[event_i];
                if ev.on {
                    self.note_on(ev.note, ev.velocity, ev.channel);
                } else {
                    self.note_off(ev.note, ev.channel);
                }
                event_i += 1;
            }

            match self.stage {
                EnvStage::Idle => self.env = 0.0,
                EnvStage::Attack => {
                    self.env += attack_step;
                    if self.env >= 1.0 {
                        self.env = 1.0;
                        self.stage = EnvStage::Sustain;
                    }
                }
                EnvStage::Sustain => self.env = 1.0,
                EnvStage::Release => {
                    self.env *= release_coeff;
                    if self.env <= 1e-4 {
                        self.env = 0.0;
                        self.stage = EnvStage::Idle;
                    }
                }
            }

            if self.stage == EnvStage::Idle {
                left[frame] = 0.0;
                right[frame] = 0.0;
                continue;
            }

            self.blend_smoothed += (params.blend - self.blend_smoothed) * smooth;
            self.body_smoothed += (params.body - self.body_smoothed) * smooth;
            self.drive_smoothed += (params.drive - self.drive_smoothed) * smooth;
            self.gain_smoothed += (params.gain - self.gain_smoothed) * smooth;

            // Glide towards the target note.
            self.current_note += (self.target_note - self.current_note) * glide_coeff;

            // Vibrato waits out its delay, then swells in.
            self.since_attack += dt_seconds;
            let vib_depth = (((self.since_attack - vib_delay_s) * vib_swell).clamp(0.0, 1.0))
                * (params.vibrato_cents + self.mod_wheel * MOD_WHEEL_CENTS);
            self.lfo_phase += vib_step;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= self.lfo_phase.floor();
            }
            let vib_cents = vib_depth * (std::f32::consts::TAU * self.lfo_phase).sin();

            let note = self.current_note
                + params.octave as f32 * 12.0
                + self.pitch_bend_semitones
                + vib_cents / 100.0;
            let base_hz = midi_note_to_hz_f32(note).clamp(1.0, nyquist_guard);

            if self.coeff_countdown == 0 {
                self.brilliance_smoothed += (params.brilliance - self.brilliance_smoothed) * 0.3;
                self.emphasis_smoothed += (params.emphasis - self.emphasis_smoothed) * 0.3;
                let probe = WhistleParams {
                    brilliance: self.brilliance_smoothed,
                    ..params
                };
                let cutoff = probe.cutoff_hz(base_hz).clamp(60.0, cutoff_ceiling);
                self.ladder
                    .set(cutoff, self.emphasis_smoothed, self.sample_rate);
                self.coeff_countdown = COEFF_INTERVAL;
            }
            self.coeff_countdown -= 1;

            // Oscillator mixer. Moog characters run a real ramp; the Worm keeps
            // the ARP divider staircase in the saw slot in case the blend is
            // opened, but its factory mix is pulse only.
            let saw_shape = if recipe.staircase {
                Shape::Staircase
            } else {
                Shape::Saw
            };
            let saw = self.saw.next(
                (base_hz * saw_ratio).min(nyquist_guard) / self.sample_rate,
                saw_shape,
                0.5,
            );
            let pulse = self.pulse.next(
                (base_hz * pulse_ratio).min(nyquist_guard) / self.sample_rate,
                Shape::Pulse,
                recipe.pulse_width,
            );
            let blend = self.blend_smoothed;
            let mix = self.hp.process(saw * blend + pulse * (1.0 - blend)) * INPUT_TRIM;

            // Split between the resonator bank and the direct path. Most of the
            // hardware's resonator voices went straight to the VCA; this
            // character says how much of the bank carries on into the VCF.
            let body = self.body_smoothed;
            let banked = self.bank.process(mix) * body;
            let to_vcf = mix * (1.0 - body) + banked * recipe.resonator_to_vcf;
            let bypass = banked * (1.0 - recipe.resonator_to_vcf);

            let voice = self.ladder.process(to_vcf) + bypass;

            // Amplifier, then a little analog rounding. Kept gentle: a hard
            // tanh is what turned the last voicing into a mid-range fuzz.
            let amp = self.env * (0.4 + 0.6 * self.velocity);
            let drive_gain = 1.0 + self.drive_smoothed * 3.0;
            let driven = (voice * amp * drive_gain).tanh() / drive_gain.powf(0.55);

            let out = (self.dc.process(driven) * self.gain_smoothed * OUTPUT_TRIM).clamp(-1.0, 1.0);
            left[frame] = out;
            right[frame] = out;
        }

        while event_i < events.len() {
            let ev = events[event_i];
            if ev.on {
                self.note_on(ev.note, ev.velocity, ev.channel);
            } else {
                self.note_off(ev.note, ev.channel);
            }
            event_i += 1;
        }
    }
}

/// Per-sample approach coefficient for the requested glide time.
///
/// The time is read as three time constants, so a note is about 95% of the way
/// there after `glide_ms`.
fn glide_coefficient(glide_ms: f32, sample_rate: f32) -> f32 {
    if glide_ms <= 0.5 {
        return 1.0;
    }
    let tau_samples = (glide_ms * 0.001 * sample_rate / 3.0).max(1.0);
    1.0 - (-1.0 / tau_samples).exp()
}

fn cents_ratio(cents: f32) -> f32 {
    2f32.powf(cents / 1_200.0)
}

fn midi_note_to_hz_f32(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn on(sample_offset: u32, note: u8) -> MidiNoteEvent {
        MidiNoteEvent {
            sample_offset,
            note,
            velocity: 100,
            channel: 0,
            on: true,
        }
    }

    fn off(sample_offset: u32, note: u8) -> MidiNoteEvent {
        MidiNoteEvent {
            sample_offset,
            note,
            velocity: 0,
            channel: 0,
            on: false,
        }
    }

    /// Steady params: no glide, no vibrato, so a rendered tone is analysable.
    fn steady(character: Character) -> WhistleParams {
        WhistleParams {
            glide_ms: 0.0,
            octave: 0,
            vibrato_cents: 0.0,
            attack_ms: 1.0,
            ..WhistleParams::for_character(character)
        }
    }

    fn render(
        engine: &mut WhistleEngine,
        params: &WhistleParams,
        events: &[MidiNoteEvent],
        frames: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.process_block(params, events, &mut left, &mut right);
        (left, right)
    }

    /// Hold a note and return the second half of the render, once the attack and
    /// the filter states have settled.
    fn steady_tone(params: &WhistleParams, note: u8, sample_rate: f32) -> Vec<f32> {
        let mut engine = WhistleEngine::new(sample_rate);
        let frames = (sample_rate * 0.5) as usize;
        let (left, _) = render(&mut engine, params, &[on(0, note)], frames);
        left[frames / 2..].to_vec()
    }

    /// Windowed single-bin DFT.
    fn magnitude_at(samples: &[f32], sample_rate: f32, hz: f32) -> f32 {
        let n = samples.len();
        let w = std::f64::consts::TAU * hz as f64 / sample_rate as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in samples.iter().enumerate() {
            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let a = w * i as f64;
            re += *s as f64 * win * a.cos();
            im += *s as f64 * win * a.sin();
        }
        ((re * re + im * im).sqrt() * 4.0 / n as f64) as f32
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    // ---- voice behaviour ----------------------------------------------------

    #[test]
    fn nothing_sounds_until_a_note_arrives() {
        let mut engine = WhistleEngine::new(SR);
        let (left, right) = render(&mut engine, &WhistleParams::default(), &[], 512);
        assert!(left.iter().all(|s| *s == 0.0));
        assert!(right.iter().all(|s| *s == 0.0));
        assert!(!engine.is_active());
    }

    #[test]
    fn the_release_runs_down_to_silence_and_stops() {
        let params = WhistleParams {
            release_ms: 60.0,
            ..steady(Character::WestCoast)
        };
        let mut engine = WhistleEngine::new(SR);
        render(&mut engine, &params, &[on(0, 72)], 4_800);
        render(&mut engine, &params, &[off(0, 72)], 12_000);

        assert!(!engine.is_active(), "the voice never went idle");
        assert_eq!(engine.envelope_level(), 0.0);
        let (tail, _) = render(&mut engine, &params, &[], 512);
        assert!(tail.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_newest_key_takes_the_voice_and_the_older_one_gets_it_back() {
        let params = steady(Character::WestCoast);
        let mut engine = WhistleEngine::new(SR);

        render(&mut engine, &params, &[on(0, 60)], 480);
        assert!((engine.current_hz() - crate::midi_note_to_hz(60)).abs() < 1.0);

        render(&mut engine, &params, &[on(0, 67)], 480);
        assert_eq!(engine.held_count(), 2);
        assert!((engine.current_hz() - crate::midi_note_to_hz(67)).abs() < 1.0);

        // Letting the top note go hands the voice back to the one still held.
        render(&mut engine, &params, &[off(0, 67)], 480);
        assert_eq!(engine.held_count(), 1);
        assert!((engine.current_hz() - crate::midi_note_to_hz(60)).abs() < 1.0);
        assert!(engine.is_active(), "the held note should still be sounding");
    }

    #[test]
    fn overlapping_notes_slide_instead_of_retriggering() {
        // The whole sound depends on this: a legato note must not restart the
        // envelope, or the glide turns into two separate stabs.
        let params = WhistleParams {
            attack_ms: 200.0,
            ..steady(Character::Silk)
        };
        let mut engine = WhistleEngine::new(SR);

        render(&mut engine, &params, &[on(0, 60)], 4_800);
        let before = engine.envelope_level();
        assert!(before > 0.4 && before < 1.0, "mid-attack level {before}");

        render(&mut engine, &params, &[on(0, 67)], 1);
        assert!(
            engine.envelope_level() >= before,
            "envelope restarted: {before} -> {}",
            engine.envelope_level()
        );
    }

    #[test]
    fn a_note_after_silence_does_retrigger() {
        let params = WhistleParams {
            release_ms: 20.0,
            ..steady(Character::Silk)
        };
        let mut engine = WhistleEngine::new(SR);
        render(&mut engine, &params, &[on(0, 60), off(2_400, 60)], 9_600);
        assert!(!engine.is_active());

        render(&mut engine, &params, &[on(0, 67)], 1);
        assert!(engine.is_active());
        assert!(
            engine.envelope_level() < 0.2,
            "should start from the bottom"
        );
    }

    #[test]
    fn portamento_climbs_smoothly_and_arrives() {
        let params = WhistleParams {
            glide_ms: 200.0,
            ..steady(Character::Worm)
        };
        let mut engine = WhistleEngine::new(SR);
        render(&mut engine, &params, &[on(0, 48)], 480);

        let mut trace = Vec::new();
        render(&mut engine, &params, &[on(0, 72)], 1);
        // Glide time is read as three time constants, so give it a few more to
        // settle before checking that it actually landed.
        for _ in 0..600 {
            render(&mut engine, &params, &[], 48);
            trace.push(engine.current_hz());
        }

        for pair in trace.windows(2) {
            assert!(pair[1] >= pair[0], "glide reversed: {pair:?}");
        }
        assert!(trace[0] < crate::midi_note_to_hz(60), "it teleported");
        let target = crate::midi_note_to_hz(72);
        let arrived = *trace.last().unwrap();
        assert!(
            (arrived - target).abs() / target < 0.01,
            "never arrived: {arrived} vs {target}"
        );
    }

    #[test]
    fn zero_glide_lands_on_the_note_immediately() {
        let params = steady(Character::SanAndreas);
        let mut engine = WhistleEngine::new(SR);
        render(&mut engine, &params, &[on(0, 48)], 480);
        render(&mut engine, &params, &[on(0, 72)], 2);
        let target = crate::midi_note_to_hz(72);
        assert!((engine.current_hz() - target).abs() / target < 1e-3);
    }

    #[test]
    fn events_take_effect_at_their_sample_offset() {
        let params = steady(Character::WestCoast);
        let mut engine = WhistleEngine::new(SR);
        let (left, _) = render(&mut engine, &params, &[on(200, 72)], 1_024);

        assert!(
            left[..200].iter().all(|s| *s == 0.0),
            "sound leaked in before the note-on"
        );
        assert!(
            peak(&left[300..]) > 1e-4,
            "the note never started: {}",
            peak(&left[300..])
        );
    }

    #[test]
    fn events_past_the_end_of_the_block_still_register() {
        let params = steady(Character::WestCoast);
        let mut engine = WhistleEngine::new(SR);
        render(&mut engine, &params, &[on(4_000, 72)], 128);
        assert!(engine.is_active());
    }

    #[test]
    fn a_note_off_for_a_key_that_was_never_down_is_ignored() {
        let params = steady(Character::WestCoast);
        let mut engine = WhistleEngine::new(SR);
        render(&mut engine, &params, &[on(0, 72), off(64, 60)], 512);
        assert!(engine.is_active());
        assert_eq!(engine.held_count(), 1);
    }

    #[test]
    fn the_held_stack_survives_more_keys_than_it_can_track() {
        let params = steady(Character::WestCoast);
        let mut engine = WhistleEngine::new(SR);
        let events: Vec<_> = (0..MAX_HELD_NOTES as u8 + 6)
            .map(|i| on(i as u32, 40 + i))
            .collect();
        render(&mut engine, &params, &events, 512);
        assert!(engine.held_count() <= MAX_HELD_NOTES);
        let newest = crate::midi_note_to_hz(40 + MAX_HELD_NOTES as u8 + 5);
        assert!((engine.current_hz() - newest).abs() < 1.0);
    }

    #[test]
    fn vibrato_holds_off_until_its_delay_has_run() {
        let params = WhistleParams {
            glide_ms: 0.0,
            octave: 0,
            vibrato_cents: 100.0,
            vibrato_hz: 6.0,
            vibrato_delay_ms: 300.0,
            ..WhistleParams::for_character(Character::Worm)
        };
        let mut engine = WhistleEngine::new(SR);
        let mut early = Vec::new();
        render(&mut engine, &params, &[on(0, 72)], 1);
        for _ in 0..20 {
            render(&mut engine, &params, &[], 480);
            early.push(engine.current_hz());
        }
        // current_hz is the glide target only; the vibrato rides on top of it,
        // so measure the pitch that reaches the oscillators via the spectrum.
        let flat = crate::midi_note_to_hz(72);
        assert!(early.iter().all(|hz| (hz - flat).abs() < 0.5));

        let short = WhistleParams {
            vibrato_delay_ms: 0.0,
            ..params
        };
        let with_vib = steady_tone(&short, 72, SR);
        let without = steady_tone(
            &WhistleParams {
                vibrato_cents: 0.0,
                ..short
            },
            72,
            SR,
        );
        // A semitone of vibrato smears the fundamental; a flat tone does not.
        let smeared = magnitude_at(&with_vib, SR, flat);
        let sharp = magnitude_at(&without, SR, flat);
        assert!(
            smeared < sharp * 0.8,
            "vibrato did not modulate the pitch: {smeared} vs {sharp}"
        );
    }

    #[test]
    fn the_mod_wheel_deepens_the_vibrato() {
        let params = WhistleParams {
            vibrato_cents: 0.0,
            vibrato_delay_ms: 0.0,
            vibrato_hz: 6.0,
            ..steady(Character::Worm)
        };
        let flat = crate::midi_note_to_hz(72);

        let mut dry = WhistleEngine::new(SR);
        let (a, _) = render(&mut dry, &params, &[on(0, 72)], 24_000);

        let mut wet = WhistleEngine::new(SR);
        wet.set_mod_wheel(1.0);
        let (b, _) = render(&mut wet, &params, &[on(0, 72)], 24_000);

        let steady_bin = magnitude_at(&a[12_000..], SR, flat);
        let wobbled = magnitude_at(&b[12_000..], SR, flat);
        assert!(
            wobbled < steady_bin * 0.9,
            "mod wheel did nothing: {wobbled} vs {steady_bin}"
        );
    }

    #[test]
    fn both_channels_carry_the_same_mono_voice() {
        let params = steady(Character::Silk);
        let mut engine = WhistleEngine::new(SR);
        let (left, right) = render(&mut engine, &params, &[on(0, 72)], 4_096);
        assert_eq!(left, right);
    }

    // ---- stability ----------------------------------------------------------

    #[test]
    fn output_stays_finite_and_bounded_at_every_sample_rate() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
            for character in Character::ALL {
                // Everything pushed to the stops at once.
                let params = WhistleParams {
                    character,
                    glide_ms: 0.0,
                    octave: 2,
                    blend: 0.5,
                    detune_cents: 50.0,
                    brilliance: 1.0,
                    emphasis: 1.0,
                    body: 1.0,
                    vibrato_hz: 20.0,
                    vibrato_cents: 200.0,
                    vibrato_delay_ms: 0.0,
                    attack_ms: 0.1,
                    release_ms: 1.0,
                    drive: 1.0,
                    gain: 1.0,
                };
                let mut engine = WhistleEngine::new(sample_rate);
                for note in [24u8, 48, 72, 96, 120] {
                    let (left, _) = render(
                        &mut engine,
                        &params,
                        &[on(0, note)],
                        (sample_rate * 0.05) as usize,
                    );
                    assert!(
                        left.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
                        "{character:?} at {sample_rate} Hz, note {note}: peak {}",
                        peak(&left)
                    );
                }
            }
        }
    }

    #[test]
    fn self_oscillation_does_not_run_away() {
        let params = WhistleParams {
            emphasis: 1.0,
            body: 0.0,
            drive: 1.0,
            gain: 1.0,
            ..steady(Character::WestCoast)
        };
        let mut engine = WhistleEngine::new(SR);
        let (left, _) = render(&mut engine, &params, &[on(0, 36)], 96_000);
        assert!(left.iter().all(|s| s.is_finite()));
        assert!(peak(&left[48_000..]) <= 1.0);
    }

    #[test]
    fn changing_character_mid_note_does_not_click() {
        let mut engine = WhistleEngine::new(SR);
        let mut previous = 0.0f32;
        let mut worst = 0.0f32;
        for character in Character::ALL {
            let params = WhistleParams {
                glide_ms: 0.0,
                octave: 0,
                ..WhistleParams::for_character(character)
            };
            let (left, _) = render(&mut engine, &params, &[on(0, 72)], 9_600);
            for s in left {
                worst = worst.max((s - previous).abs());
                previous = s;
            }
        }
        assert!(worst < 0.6, "step of {worst} between samples");
    }

    #[test]
    fn output_level_lands_somewhere_usable() {
        for character in Character::ALL {
            let tone = steady_tone(&steady(character), 72, SR);
            let p = peak(&tone);
            assert!(
                (0.1..=1.0).contains(&p),
                "{character:?} peaks at {p}, rms {}",
                rms(&tone)
            );
        }
    }

    // ---- spectrum -----------------------------------------------------------

    /// Amplitudes of the first eight partials of a held note.
    fn partials(params: &WhistleParams, note: u8) -> Vec<f32> {
        let tone = steady_tone(params, note, SR);
        let f0 = crate::midi_note_to_hz(note) * 2f32.powi(params.octave);
        (1..=8)
            .map(|h| magnitude_at(&tone, SR, f0 * h as f32))
            .collect()
    }

    #[test]
    fn no_character_is_anything_like_a_sine() {
        // Source is pulse/saw/square, then a tight filter. A whistle still has
        // a 2nd harmonic; a sine does not. Requiring a bright analog stack here
        // is what pushed the last voicing into a muted mid-range lead.
        for character in Character::ALL {
            let p = partials(&steady(character), 72);
            let strongest = p.iter().cloned().fold(0.0f32, f32::max);
            assert!(strongest > 1e-3, "{character:?} produced nothing");
            let significant = p.iter().filter(|m| **m > strongest * 0.08).count();
            assert!(significant >= 2, "{character:?} collapsed to a sine: {p:?}");
        }
    }

    #[test]
    fn a_c4_with_the_factory_voice_whistles_near_c6() {
        // The piano roll sits around middle C. These leads are a Minimoog 2'
        // (two octaves up), so that key has to come out around 1 kHz, not 260.
        for character in Character::ALL {
            let params = WhistleParams::for_character(character);
            assert_eq!(params.octave, 2, "{character:?} is not at 2'");
            let tone = steady_tone(&params, 60, SR);
            let f0 = crate::midi_note_to_hz(60) * 4.0; // C6
            let concert = crate::midi_note_to_hz(60);
            let whistle = magnitude_at(&tone, SR, f0);
            let low = magnitude_at(&tone, SR, concert);
            assert!(
                whistle > low * 4.0,
                "{character:?} is still in the bass: 261 Hz {low} vs 1047 Hz {whistle}"
            );
            assert!(whistle > 1e-3, "{character:?} has no energy at C6");
        }
    }

    #[test]
    fn the_worm_leans_on_its_reed_resonators() {
        // The reed bank sits at a fixed 1.15 kHz and does not follow the
        // keyboard, so on a C5 it lands on the second partial and leaves the
        // fundamental behind. That tilt is the whole voice.
        let character = Character::Worm;
        let reed_hz = character.recipe().resonators[0].freq_hz;
        let note = 72u8;
        let f0 = crate::midi_note_to_hz(note);
        let nearest = (reed_hz / f0).round().max(1.0);
        assert!(
            nearest > 1.0,
            "pick a note whose fundamental is off the reed"
        );

        let banked = steady(character);
        let bare = WhistleParams {
            body: 0.0,
            ..banked
        };

        let tilt = |params: &WhistleParams| {
            let tone = steady_tone(params, note, SR);
            magnitude_at(&tone, SR, f0 * nearest) / magnitude_at(&tone, SR, f0).max(1e-9)
        };
        let with_bank = tilt(&banked);
        let without = tilt(&bare);
        assert!(
            with_bank > without * 1.5,
            "the bank is not shaping the voice: {with_bank} with, {without} without"
        );
    }

    #[test]
    fn the_worm_runs_on_the_arp_pulse_and_not_a_ramp() {
        // A 1/14 pulse has no null until its fourteenth harmonic, and its even
        // partials are strong; a square would null every other one.
        let params = steady(Character::Worm);
        assert_eq!(params.blend, 0.0, "the Worm should be pulse only");
        assert!((params.pulse_width() - 1.0 / 14.0).abs() < 1e-6);

        let p = partials(&params, 60);
        let odd: f32 = p.iter().step_by(2).sum();
        let even: f32 = p.iter().skip(1).step_by(2).sum();
        assert!(
            even > odd * 0.3,
            "even partials missing, that is a square not a 1/14 pulse: {p:?}"
        );
    }

    #[test]
    fn the_characters_do_not_sound_alike() {
        // Compare normalised partial profiles pairwise.
        let profiles: Vec<Vec<f32>> = Character::ALL
            .iter()
            .map(|c| {
                let p = partials(&steady(*c), 72);
                let total: f32 = p.iter().sum::<f32>().max(1e-9);
                p.into_iter().map(|m| m / total).collect()
            })
            .collect();

        for i in 0..profiles.len() {
            for j in i + 1..profiles.len() {
                let distance: f32 = profiles[i]
                    .iter()
                    .zip(&profiles[j])
                    .map(|(a, b)| (a - b).abs())
                    .sum();
                assert!(
                    distance > 0.12,
                    "{:?} and {:?} are only {distance} apart",
                    Character::ALL[i],
                    Character::ALL[j]
                );
            }
        }
    }

    #[test]
    fn high_notes_do_not_fold_aliases_back_under_the_fundamental() {
        for character in Character::ALL {
            let params = WhistleParams {
                body: 0.0,
                brilliance: 1.0,
                drive: 0.0,
                ..steady(character)
            };
            let note = 96u8; // C7, above anything these leads actually play.
            let f0 = crate::midi_note_to_hz(note);
            let tone = steady_tone(&params, note, SR);
            let fundamental = magnitude_at(&tone, SR, f0);
            assert!(fundamental > 1e-4, "{character:?} produced no tone");

            // Nothing harmonic lives below f0, so anything down here folded over.
            for probe in [370.0, 611.0, 977.0, 1_453.0] {
                let junk = magnitude_at(&tone, SR, probe);
                assert!(
                    junk < fundamental * 0.06,
                    "{character:?} aliased {junk} to {probe} Hz against {fundamental}"
                );
            }
        }
    }
}
