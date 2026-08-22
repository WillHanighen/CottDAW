use crate::filter::OnePoleLp;

const MAX_DELAY: usize = 192_000;
const WOW_HZ: f32 = 0.28;
const FLUTTER_HZ: f32 = 5.8;

/// Five knobs. Time is milliseconds; the rest are 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapeParams {
    pub time_ms: f32,
    pub feedback: f32,
    pub wow: f32,
    pub drive: f32,
    pub mix: f32,
}

impl Default for TapeParams {
    fn default() -> Self {
        Self {
            time_ms: 320.0,
            feedback: 0.42,
            wow: 0.22,
            drive: 0.28,
            mix: 0.38,
        }
    }
}

impl TapeParams {
    pub fn clamped(self) -> Self {
        Self {
            time_ms: self.time_ms.clamp(20.0, 1_200.0),
            feedback: self.feedback.clamp(0.0, 1.0),
            wow: self.wow.clamp(0.0, 1.0),
            drive: self.drive.clamp(0.0, 1.0),
            mix: self.mix.clamp(0.0, 1.0),
        }
    }
}

/// Stereo tape delay. Runs in place on the host buffers.
#[derive(Debug, Clone)]
pub struct TapeEngine {
    sample_rate: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    lp_l: OnePoleLp,
    lp_r: OnePoleLp,
    wow_phase: f32,
    flutter_phase: f32,
}

impl Default for TapeEngine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl TapeEngine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut engine = Self {
            sample_rate: sr,
            buf_l: vec![0.0; MAX_DELAY],
            buf_r: vec![0.0; MAX_DELAY],
            write: 0,
            lp_l: OnePoleLp::default(),
            lp_r: OnePoleLp::default(),
            wow_phase: 0.0,
            flutter_phase: 0.0,
        };
        engine.apply_rate();
        engine
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.apply_rate();
    }

    fn apply_rate(&mut self) {
        self.lp_l.set_cutoff(3_200.0, self.sample_rate);
        self.lp_r.set_cutoff(2_800.0, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write = 0;
        self.lp_l.reset();
        self.lp_r.reset();
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
    }

    pub fn process_block(&mut self, params: &TapeParams, left: &mut [f32], right: &mut [f32]) {
        let params = params.clamped();
        let frames = left.len().min(right.len());
        let drive = 1.0 + params.drive * 1.8;
        let drive_norm = 1.0 / drive.tanh();
        let feedback = params.feedback * 0.86;
        let cutoff = 4_200.0 * (1_200.0 / 4_200.0_f32).powf(params.drive * 0.45 + 0.2);
        self.lp_l.set_cutoff(cutoff, self.sample_rate);
        self.lp_r.set_cutoff(cutoff * 0.9, self.sample_rate);

        for frame in 0..frames {
            let dry_l = left[frame];
            let dry_r = right[frame];

            self.wow_phase = wrap(self.wow_phase + WOW_HZ / self.sample_rate);
            self.flutter_phase = wrap(self.flutter_phase + FLUTTER_HZ / self.sample_rate);
            let wander = if params.wow <= 0.0 {
                0.0
            } else {
                let wow = (self.wow_phase * std::f32::consts::TAU).sin();
                let flutter = (self.flutter_phase * std::f32::consts::TAU).sin();
                params.wow * (wow * 0.0042 + flutter * 0.0014)
            };

            let delay_l = ((params.time_ms * 0.001 + wander) * self.sample_rate).max(2.0);
            let delay_r =
                ((params.time_ms * 0.001 * 1.017 + wander * 0.82) * self.sample_rate).max(2.0);
            let wet_l = self.lp_l.process(read(&self.buf_l, self.write, delay_l));
            let wet_r = self.lp_r.process(read(&self.buf_r, self.write, delay_r));

            let write_l = (dry_l * drive + wet_l * feedback).tanh() * drive_norm;
            let write_r = (dry_r * drive + wet_r * feedback).tanh() * drive_norm;
            self.buf_l[self.write] = write_l;
            self.buf_r[self.write] = write_r;
            self.write += 1;
            if self.write >= MAX_DELAY {
                self.write = 0;
            }

            let mix = params.mix;
            left[frame] = dry_l * (1.0 - mix) + wet_l * mix;
            right[frame] = dry_r * (1.0 - mix) + wet_r * mix;
        }
    }
}

fn read(buf: &[f32], write: usize, delay: f32) -> f32 {
    let delay = delay.clamp(1.0, (MAX_DELAY - 2) as f32);
    let pos = write as f32 - delay;
    let pos = if pos < 0.0 {
        pos + MAX_DELAY as f32
    } else {
        pos
    };
    let i0 = pos as usize % MAX_DELAY;
    let i1 = (i0 + 1) % MAX_DELAY;
    let frac = pos - pos.floor();
    buf[i0] + (buf[i1] - buf[i0]) * frac
}

fn wrap(phase: f32) -> f32 {
    let p = phase.fract();
    if p < 0.0 {
        p + 1.0
    } else {
        p
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

    #[test]
    fn mix_zero_is_passthrough() {
        let mut engine = TapeEngine::new(48_000.0);
        let params = TapeParams {
            mix: 0.0,
            feedback: 1.0,
            wow: 1.0,
            drive: 1.0,
            time_ms: 200.0,
        };
        let src: Vec<f32> = (0..256).map(|i| (i as f32 * 0.01).sin() * 0.4).collect();
        let mut l = src.clone();
        let mut r = src.clone();
        engine.process_block(&params, &mut l, &mut r);
        for (a, b) in l.iter().zip(src.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn click_comes_back() {
        let mut engine = TapeEngine::new(48_000.0);
        let params = TapeParams {
            time_ms: 20.0,
            feedback: 0.0,
            wow: 0.0,
            drive: 0.0,
            mix: 1.0,
        };
        let mut l = vec![0.0f32; 3_072];
        let mut r = vec![0.0f32; 3_072];
        l[0] = 0.9;
        r[0] = 0.9;
        engine.process_block(&params, &mut l, &mut r);
        let echo = peak(&l[800..1_200]);
        assert!(echo > 0.05, "expected a repeat, peak={echo}");
    }

    #[test]
    fn feedback_leaves_a_tail() {
        let mut engine = TapeEngine::new(48_000.0);
        let params = TapeParams {
            time_ms: 20.0,
            feedback: 0.8,
            wow: 0.0,
            drive: 0.0,
            mix: 1.0,
        };
        let mut l = vec![0.0f32; 8_192];
        let mut r = vec![0.0f32; 8_192];
        l[0] = 0.9;
        r[0] = 0.9;
        engine.process_block(&params, &mut l, &mut r);
        assert!(rms(&l[4_000..]) > 1e-4, "feedback should keep repeating");
    }
}
