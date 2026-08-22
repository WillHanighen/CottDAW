use crate::filter::{BiquadMode, OnePoleHp, OnePoleLp, StereoBiquad};
use crate::noise_tick;
use crate::smear::Smear;

/// How the record takes the air off. Flip these until one feels like lofi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wear {
    /// Mids stay, sparkle goes. Dusty beat / worn LP.
    #[default]
    Dusty,
    /// Thin and boxy. Cheap radio.
    Radio,
    /// Soft smear, a little grit. Cassette.
    Tape,
}

impl Wear {
    pub const ALL: [Wear; 3] = [Wear::Dusty, Wear::Radio, Wear::Tape];

    pub fn label(self) -> &'static str {
        match self {
            Wear::Dusty => "Dusty",
            Wear::Radio => "Radio",
            Wear::Tape => "Tape",
        }
    }
}

/// Five knobs plus a wear flavor. `0` on the dirt controls is a true bypass of that part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VinylParams {
    pub wear: Wear,
    pub pops: f32,
    pub hiss: f32,
    pub muffle: f32,
    pub rumble: f32,
    pub mix: f32,
}

impl Default for VinylParams {
    fn default() -> Self {
        Self {
            wear: Wear::Dusty,
            pops: 0.28,
            hiss: 0.16,
            muffle: 0.55,
            rumble: 0.14,
            mix: 1.0,
        }
    }
}

impl VinylParams {
    pub fn clamped(self) -> Self {
        Self {
            wear: self.wear,
            pops: self.pops.clamp(0.0, 1.0),
            hiss: self.hiss.clamp(0.0, 1.0),
            muffle: self.muffle.clamp(0.0, 1.0),
            rumble: self.rumble.clamp(0.0, 1.0),
            mix: self.mix.clamp(0.0, 1.0),
        }
    }
}

/// Stereo vinyl wear. Runs in place on the host buffers.
#[derive(Debug, Clone)]
pub struct VinylEngine {
    sample_rate: f32,
    lowpass: StereoBiquad,
    highpass: StereoBiquad,
    hiss_lp: OnePoleLp,
    hiss_hp: OnePoleHp,
    rumble_lp: OnePoleLp,
    hiss_rng: u32,
    pop_rng: u32,
    rumble_rng: u32,
    smear: Smear,
    crackle_env: f32,
    pop_env: f32,
    samples_until_crackle: u32,
    samples_until_pop: u32,
}

impl Default for VinylEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl VinylEngine {
    pub fn new(sample_rate: f32) -> Self {
        let mut engine = Self {
            sample_rate: sample_rate.max(1.0),
            lowpass: StereoBiquad::default(),
            highpass: StereoBiquad::default(),
            hiss_lp: OnePoleLp::default(),
            hiss_hp: OnePoleHp::default(),
            rumble_lp: OnePoleLp::default(),
            hiss_rng: 0xC0FF_EE42,
            pop_rng: 0xDEAD_BEEF,
            rumble_rng: 0x0BAD_F00D,
            smear: Smear::new(sample_rate.max(1.0)),
            crackle_env: 0.0,
            pop_env: 0.0,
            samples_until_crackle: 0,
            samples_until_pop: 0,
        };
        engine.apply_rate();
        engine
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.smear.set_sample_rate(self.sample_rate);
        self.apply_rate();
    }

    fn apply_rate(&mut self) {
        self.hiss_lp.set_cutoff(2800.0, self.sample_rate);
        self.hiss_hp.set_cutoff(800.0, self.sample_rate);
        self.rumble_lp.set_cutoff(55.0, self.sample_rate);
        self.schedule_crackle(0.28);
        self.schedule_pop(0.28);
    }

    pub fn reset(&mut self) {
        self.lowpass.reset();
        self.highpass.reset();
        self.hiss_lp.reset();
        self.hiss_hp.reset();
        self.rumble_lp.reset();
        self.smear.reset();
        self.crackle_env = 0.0;
        self.pop_env = 0.0;
        self.schedule_crackle(0.28);
        self.schedule_pop(0.28);
    }

    pub fn process_block(&mut self, params: &VinylParams, left: &mut [f32], right: &mut [f32]) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        if params.muffle > 0.0 {
            self.set_wear(params.wear, params.muffle);
        }

        for frame in 0..frames {
            let dry_l = left[frame];
            let dry_r = right[frame];
            let (mut wet_l, mut wet_r) = if params.muffle <= 0.0 {
                (dry_l, dry_r)
            } else {
                let (l, r) = self.color(params.wear, params.muffle, dry_l, dry_r);
                self.smear.process(l, r, params.muffle)
            };

            if params.hiss > 0.0 {
                let n = noise_tick(&mut self.hiss_rng);
                let hiss = self.hiss_hp.process(self.hiss_lp.process(n)) * params.hiss * 0.055;
                wet_l += hiss;
                wet_r += hiss * 0.88;
            }

            if params.rumble > 0.0 {
                let n = noise_tick(&mut self.rumble_rng);
                let rumble = self.rumble_lp.process(n) * params.rumble * 0.22;
                wet_l += rumble;
                wet_r += rumble * 0.7;
            }

            if params.pops > 0.0 {
                if self.samples_until_crackle == 0 {
                    self.crackle_env = 0.25 + noise_tick(&mut self.pop_rng).abs() * 0.45;
                    self.schedule_crackle(params.pops);
                } else {
                    self.samples_until_crackle -= 1;
                }
                if self.samples_until_pop == 0 {
                    self.pop_env = 0.7 + noise_tick(&mut self.pop_rng).abs() * 0.5;
                    self.schedule_pop(params.pops);
                } else {
                    self.samples_until_pop -= 1;
                }
                let crackle = self.crackle_env * params.pops * 0.18;
                let pop = self.pop_env * params.pops * 0.42;
                self.crackle_env *= 0.82;
                self.pop_env *= 0.91;
                wet_l += crackle + pop;
                wet_r += crackle * 0.75 + pop * 0.55;
            }

            let mix = params.mix;
            left[frame] = dry_l * (1.0 - mix) + wet_l * mix;
            right[frame] = dry_r * (1.0 - mix) + wet_r * mix;
        }
    }

    fn schedule_crackle(&mut self, pops: f32) {
        let n = noise_tick(&mut self.pop_rng).abs();
        let gap_sec = (0.04 + (1.0 - pops) * 0.55) * (0.35 + n * 1.1);
        self.samples_until_crackle = (gap_sec * self.sample_rate).max(16.0) as u32;
    }

    fn schedule_pop(&mut self, pops: f32) {
        let n = noise_tick(&mut self.pop_rng).abs();
        let gap_sec = (0.55 + (1.0 - pops) * 3.2) * (0.5 + n * 1.4);
        self.samples_until_pop = (gap_sec * self.sample_rate).max(64.0) as u32;
    }

    fn set_wear(&mut self, wear: Wear, muffle: f32) {
        let t = muffle.clamp(0.0, 1.0);
        match wear {
            Wear::Dusty => {
                let lp = 7_200.0 * (1_500.0 / 7_200.0_f32).powf(t);
                self.lowpass.set(BiquadMode::LowPass, lp, self.sample_rate);
            }
            Wear::Radio => {
                let hp = 220.0 + t * 220.0;
                let lp = 3_400.0 * (1_400.0 / 3_400.0_f32).powf(t);
                self.highpass
                    .set(BiquadMode::HighPass, hp, self.sample_rate);
                self.lowpass.set(BiquadMode::LowPass, lp, self.sample_rate);
            }
            Wear::Tape => {
                let lp = 5_400.0 * (1_800.0 / 5_400.0_f32).powf(t);
                self.lowpass.set(BiquadMode::LowPass, lp, self.sample_rate);
            }
        }
    }

    fn color(&mut self, wear: Wear, muffle: f32, left: f32, right: f32) -> (f32, f32) {
        match wear {
            Wear::Dusty => self.lowpass.process(left, right),
            Wear::Radio => {
                let (l, r) = self.highpass.process(left, right);
                self.lowpass.process(l, r)
            }
            Wear::Tape => {
                let (l, r) = self.lowpass.process(left, right);
                let drive = 1.0 + muffle * 1.6;
                let norm = 1.0 / drive.tanh();
                ((l * drive).tanh() * norm, (r * drive).tanh() * norm)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()))
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn sine(n: usize, hz: f32, sr: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / sr).sin() * 0.5)
            .collect()
    }

    #[test]
    fn dry_when_dirt_is_off() {
        let mut engine = VinylEngine::new(48_000.0);
        let params = VinylParams {
            wear: Wear::Dusty,
            pops: 0.0,
            hiss: 0.0,
            muffle: 0.0,
            rumble: 0.0,
            mix: 1.0,
        };
        let src = sine(512, 440.0, 48_000.0);
        let mut l = src.clone();
        let mut r = src.clone();
        engine.process_block(&params, &mut l, &mut r);
        for (a, b) in l.iter().zip(src.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn mix_zero_is_passthrough() {
        let mut engine = VinylEngine::new(48_000.0);
        let params = VinylParams {
            wear: Wear::Dusty,
            pops: 1.0,
            hiss: 1.0,
            muffle: 1.0,
            rumble: 1.0,
            mix: 0.0,
        };
        let src = sine(256, 220.0, 48_000.0);
        let mut l = src.clone();
        let mut r = src.clone();
        engine.process_block(&params, &mut l, &mut r);
        for (a, b) in l.iter().zip(src.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn pops_at_zero_stay_quiet() {
        let mut engine = VinylEngine::new(48_000.0);
        let params = VinylParams {
            wear: Wear::Dusty,
            pops: 0.0,
            hiss: 0.0,
            muffle: 0.0,
            rumble: 0.0,
            mix: 1.0,
        };
        let mut l = vec![0.0f32; 16_384];
        let mut r = vec![0.0f32; 16_384];
        engine.process_block(&params, &mut l, &mut r);
        assert!(peak(&l) < 1e-6);
        assert!(peak(&r) < 1e-6);
    }

    #[test]
    fn pops_make_clicks() {
        let mut engine = VinylEngine::new(48_000.0);
        let params = VinylParams {
            wear: Wear::Dusty,
            pops: 1.0,
            hiss: 0.0,
            muffle: 0.0,
            rumble: 0.0,
            mix: 1.0,
        };
        let mut l = vec![0.0f32; 96_000];
        let mut r = vec![0.0f32; 96_000];
        engine.process_block(&params, &mut l, &mut r);
        assert!(peak(&l) > 0.05, "expected pops, peak={}", peak(&l));
    }

    #[test]
    fn muffle_darkens_brightness() {
        let sr = 48_000.0;
        let src = sine(8192, 8_000.0, sr);
        let open = VinylParams {
            wear: Wear::Dusty,
            pops: 0.0,
            hiss: 0.0,
            muffle: 0.0,
            rumble: 0.0,
            mix: 1.0,
        };
        let dark = VinylParams {
            muffle: 1.0,
            ..open
        };
        let mut l = src.clone();
        let mut r = src.clone();
        VinylEngine::new(sr).process_block(&open, &mut l, &mut r);
        let open_rms = rms(&l[2048..]);

        let mut l = src;
        let mut r = l.clone();
        VinylEngine::new(sr).process_block(&dark, &mut l, &mut r);
        let dark_rms = rms(&l[2048..]);
        assert!(
            dark_rms < open_rms * 0.25,
            "muffle should bury 8 kHz: open={open_rms} dark={dark_rms}"
        );
    }

    #[test]
    fn muffle_washes_a_click() {
        let mut engine = VinylEngine::new(48_000.0);
        let params = VinylParams {
            wear: Wear::Dusty,
            pops: 0.0,
            hiss: 0.0,
            muffle: 1.0,
            rumble: 0.0,
            mix: 1.0,
        };
        let mut l = vec![0.0f32; 4_096];
        let mut r = vec![0.0f32; 4_096];
        l[0] = 1.0;
        r[0] = 1.0;
        engine.process_block(&params, &mut l, &mut r);
        assert!(
            rms(&l[256..]) > 1e-4,
            "muffle smear should leave a wash after a click"
        );
    }
}
