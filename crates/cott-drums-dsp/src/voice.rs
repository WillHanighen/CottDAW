//! One analog voice per pad. No samples.

use crate::engine::{Pad, PadParams};

const TAU: f32 = std::f32::consts::TAU;

/// xorshift noise in `-1..1`.
#[inline]
pub(crate) fn noise_tick(state: &mut u32) -> f32 {
    let mut x = *state;
    if x == 0 {
        x = 0xA341_316C;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as i32 as f32) * (1.0 / 2_147_483_648.0)
}

#[inline]
fn wrap01(phase: &mut f32) {
    if *phase >= 1.0 {
        *phase -= phase.floor();
    }
}

#[inline]
fn exp_coeff(seconds: f32, sample_rate: f32) -> f32 {
    (-1.0 / (seconds.max(1.0e-4) * sample_rate).max(1.0)).exp()
}

#[inline]
fn one_pole_coeff(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let nyquist = sample_rate * 0.49;
    let cutoff = cutoff_hz.clamp(20.0, nyquist);
    1.0 - (-TAU * cutoff / sample_rate).exp()
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Biquad {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn process(&mut self, x: f32, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f32 {
        let y = b0 * x + b1 * self.x1 + b2 * self.x2 - a1 * self.y1 - a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Band-pass (constant-skirt RBJ) around `freq_hz`.
fn bandpass_tick(state: &mut Biquad, x: f32, freq_hz: f32, q: f32, sample_rate: f32) -> f32 {
    let w0 = TAU * (freq_hz / sample_rate).clamp(1.0e-4, 0.49);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q.max(0.1));
    let a0 = 1.0 + alpha;
    let inv = 1.0 / a0;
    let b0 = alpha * inv;
    let b1 = 0.0;
    let b2 = -alpha * inv;
    let a1 = (-2.0 * cos_w0) * inv;
    let a2 = (1.0 - alpha) * inv;
    state.process(x, b0, b1, b2, a1, a2)
}

/// 808-style square-stack frequencies (Hz) before pitch offset.
const HAT_FREQS: [f32; 6] = [203.4, 366.1, 301.8, 521.3, 419.2, 632.4];

#[derive(Debug, Clone)]
pub(crate) struct Voice {
    pub pad: Pad,
    pub env: f32,
    pub click: f32,
    pub vel: f32,
    pub phase: f32,
    pub phase2: f32,
    pub hat_phase: [f32; 6],
    pub noise: u32,
    pub bp: Biquad,
    pub lp: f32,
    pub t: f32,
    pub choke: bool,
}

impl Voice {
    pub(crate) fn new(pad: Pad, seed: u32) -> Self {
        Self {
            pad,
            env: 0.0,
            click: 0.0,
            vel: 0.0,
            phase: 0.0,
            phase2: 0.0,
            hat_phase: [0.0; 6],
            noise: seed | 1,
            bp: Biquad::default(),
            lp: 0.0,
            t: 0.0,
            choke: false,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.env > 1.0e-4
    }

    pub(crate) fn trigger(&mut self, velocity: f32) {
        self.env = 1.0;
        self.click = 1.0;
        self.vel = velocity.clamp(0.0, 1.0);
        self.t = 0.0;
        self.choke = false;
        self.bp.reset();
        self.lp = 0.0;
        // Fresh-ish start without a clicky DC pop on the sine.
        self.phase = 0.0;
        self.phase2 = 0.0;
        for (i, p) in self.hat_phase.iter_mut().enumerate() {
            *p = (i as f32 * 0.13) % 1.0;
        }
    }

    pub(crate) fn choke(&mut self) {
        self.choke = true;
    }

    pub(crate) fn tick(
        &mut self,
        kit: f32,
        pad: &PadParams,
        sample_rate: f32,
    ) -> f32 {
        if !self.is_active() {
            return 0.0;
        }

        let kit = kit.clamp(0.0, 1.0);
        let pitch = pad.pitch.clamp(-12.0, 12.0);
        let pitch_ratio = 2f32.powf(pitch / 12.0);
        let tone = pad.tone.clamp(0.0, 1.0);
        let decay = pad.decay.clamp(0.05, 1.0);
        let level = pad.level.clamp(0.0, 1.0);
        let dt = 1.0 / sample_rate;
        self.t += dt;

        let sample = match self.pad {
            Pad::Kick => self.tick_kick(kit, pitch_ratio, tone, decay, sample_rate),
            Pad::Snare => self.tick_snare(kit, pitch_ratio, tone, decay, sample_rate),
            Pad::Clap => self.tick_clap(kit, tone, decay, sample_rate),
            Pad::ClosedHat => self.tick_hat(false, kit, pitch_ratio, tone, decay, sample_rate),
            Pad::OpenHat => self.tick_hat(true, kit, pitch_ratio, tone, decay, sample_rate),
            Pad::Rim => self.tick_rim(kit, pitch_ratio, tone, decay, sample_rate),
            Pad::Tom => self.tick_tom(kit, pitch_ratio, tone, decay, sample_rate),
        };

        let out = sample * self.vel * level;
        if !out.is_finite() {
            0.0
        } else {
            out.clamp(-1.5, 1.5)
        }
    }

    fn tick_kick(
        &mut self,
        kit: f32,
        pitch_ratio: f32,
        tone: f32,
        decay: f32,
        sample_rate: f32,
    ) -> f32 {
        // Electronic: ~50 Hz sine with a fast pitch drop. Bedroom: thud + membrane partial.
        let base_hz = (48.0 + kit * 14.0) * pitch_ratio;
        let sweep = (1.0 - kit) * 2.4 + 0.35;
        let sweep_hz = base_hz * sweep * (-self.t * (90.0 + kit * 40.0)).exp();
        let freq = (base_hz + sweep_hz).clamp(20.0, 400.0);

        self.phase += freq / sample_rate;
        wrap01(&mut self.phase);
        let body = (self.phase * TAU).sin();

        self.phase2 += (freq * 1.59) / sample_rate;
        wrap01(&mut self.phase2);
        let partial = (self.phase2 * TAU).sin() * (0.08 + kit * 0.28);

        let decay_s = (0.16 + kit * 0.22) * (0.4 + decay * 1.4);
        self.env *= exp_coeff(decay_s, sample_rate);
        self.click *= exp_coeff(0.004 + kit * 0.008, sample_rate);

        let n = noise_tick(&mut self.noise);
        let click_amt = (0.28 - kit * 0.16) * (0.45 + tone * 0.7);
        let click = n * self.click * click_amt;

        (body + partial) * self.env + click
    }

    fn tick_snare(
        &mut self,
        kit: f32,
        pitch_ratio: f32,
        tone: f32,
        decay: f32,
        sample_rate: f32,
    ) -> f32 {
        let shell_hz = (175.0 + kit * 40.0) * pitch_ratio;
        self.phase += shell_hz / sample_rate;
        wrap01(&mut self.phase);
        let shell = (self.phase * TAU).sin();

        let n = noise_tick(&mut self.noise);
        let bp_hz = 1_400.0 + tone * 2_200.0 + kit * 400.0;
        let wires = bandpass_tick(&mut self.bp, n, bp_hz, 1.4, sample_rate);

        let body_s = (0.07 + kit * 0.12) * (0.45 + decay);
        let snap_s = (0.04 + kit * 0.05) * (0.35 + decay * 0.8);
        self.env *= exp_coeff(body_s, sample_rate);
        self.click *= exp_coeff(snap_s, sample_rate);

        let body_mix = 0.22 + kit * 0.38;
        let snap_mix = 0.85 - kit * 0.40;
        shell * self.env * body_mix + wires * self.click * snap_mix
    }

    fn tick_clap(&mut self, kit: f32, tone: f32, decay: f32, sample_rate: f32) -> f32 {
        // Staggered noise bursts, then a muffled tail. Bedroom stretches the gaps a little.
        let stretch = 1.0 + kit * 0.35;
        let onsets = [0.0, 0.011 * stretch, 0.023 * stretch, 0.037 * stretch];
        let mut bursts = 0.0f32;
        for (i, onset) in onsets.iter().enumerate() {
            if self.t >= *onset {
                let local = self.t - *onset;
                let peak = 1.0 - i as f32 * 0.12;
                bursts += (-local * (140.0 - kit * 40.0)).exp() * peak;
            }
        }

        let tail_s = (0.08 + kit * 0.10) * (0.4 + decay);
        self.env *= exp_coeff(tail_s, sample_rate);
        self.click *= exp_coeff(0.06, sample_rate);

        let n = noise_tick(&mut self.noise);
        let raw = n * (bursts + self.env * 0.35);
        let cutoff = 1_800.0 + tone * 2_400.0 - kit * 400.0;
        let c = one_pole_coeff(cutoff.clamp(400.0, 8_000.0), sample_rate);
        self.lp += c * (raw - self.lp);
        self.lp * 1.15
    }

    fn tick_hat(
        &mut self,
        open: bool,
        kit: f32,
        pitch_ratio: f32,
        tone: f32,
        decay: f32,
        sample_rate: f32,
    ) -> f32 {
        let mut mix = 0.0f32;
        for (i, base) in HAT_FREQS.iter().enumerate() {
            let hz = *base * pitch_ratio * (0.92 + kit * 0.08);
            self.hat_phase[i] += hz / sample_rate;
            wrap01(&mut self.hat_phase[i]);
            let s = if self.hat_phase[i] < 0.5 { 1.0 } else { -1.0 };
            mix += s;
        }
        mix /= HAT_FREQS.len() as f32;

        let n = noise_tick(&mut self.noise);
        let noise_amt = 0.18 + kit * 0.28;
        mix = mix * (1.0 - noise_amt) + n * noise_amt;

        let hp_hz = 4_000.0 + tone * 5_000.0 - kit * 1_200.0;
        let c = one_pole_coeff(hp_hz.clamp(800.0, 14_000.0), sample_rate);
        // High-pass via one-pole LP subtracted from input.
        self.lp += c * (mix - self.lp);
        let bright = mix - self.lp;

        let closed_s = (0.045 + kit * 0.03) * (0.4 + decay);
        let open_s = (0.22 + kit * 0.16) * (0.45 + decay);
        let mut seconds = if open { open_s } else { closed_s };
        if self.choke {
            seconds = 0.008;
        }
        self.env *= exp_coeff(seconds, sample_rate);
        bright * self.env * 0.7
    }

    fn tick_rim(
        &mut self,
        kit: f32,
        pitch_ratio: f32,
        tone: f32,
        decay: f32,
        sample_rate: f32,
    ) -> f32 {
        let hz = (780.0 + kit * 180.0) * pitch_ratio * (0.85 + tone * 0.3);
        self.phase += hz / sample_rate;
        wrap01(&mut self.phase);
        let ping = (self.phase * TAU).sin();
        let n = noise_tick(&mut self.noise);

        let ping_s = (0.018 + kit * 0.02) * (0.4 + decay);
        self.env *= exp_coeff(ping_s, sample_rate);
        self.click *= exp_coeff(0.006, sample_rate);
        ping * self.env * 0.7 + n * self.click * 0.35
    }

    fn tick_tom(
        &mut self,
        kit: f32,
        pitch_ratio: f32,
        tone: f32,
        decay: f32,
        sample_rate: f32,
    ) -> f32 {
        let base_hz = (118.0 - kit * 18.0) * pitch_ratio;
        let sweep = (1.0 - kit * 0.5) * 1.4;
        let freq = base_hz * (1.0 + sweep * (-self.t * 70.0).exp());
        self.phase += freq.clamp(40.0, 500.0) / sample_rate;
        wrap01(&mut self.phase);
        let body = (self.phase * TAU).sin();

        self.phase2 += (freq * 1.5) / sample_rate;
        wrap01(&mut self.phase2);
        let partial = (self.phase2 * TAU).sin() * (0.12 + kit * 0.2) * (0.4 + tone * 0.5);

        let decay_s = (0.18 + kit * 0.16) * (0.4 + decay * 1.3);
        self.env *= exp_coeff(decay_s, sample_rate);
        self.click *= exp_coeff(0.008, sample_rate);
        let n = noise_tick(&mut self.noise);
        (body + partial) * self.env + n * self.click * 0.12
    }
}
