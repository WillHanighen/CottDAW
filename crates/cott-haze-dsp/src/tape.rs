use std::f32::consts::TAU;

const DELAY_LEN: usize = 2048;
const WOW_HZ: f32 = 0.32;
const FLUTTER_HZ: f32 = 6.4;
const BASE_DELAY_L: f32 = 0.0032;
const BASE_DELAY_R: f32 = 0.0041;

/// Bus tape: pitch wobble, a short walked delay, and a soft saturate.

#[derive(Debug, Clone)]
pub struct Tape {
    sample_rate: f32,
    wow_phase: f32,
    flutter_phase: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
}

impl Tape {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            wow_phase: 0.0,
            flutter_phase: 0.0,
            buf_l: vec![0.0; DELAY_LEN],
            buf_r: vec![0.0; DELAY_LEN],
            write: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub fn reset(&mut self) {
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write = 0;
    }

    /// Pitch scale for the voices this sample. Exactly `1.0` when `flutter` is 0.
    pub fn pitch_ratio(&self, flutter: f32) -> f32 {
        if flutter <= 0.0 {
            return 1.0;
        }
        let wow = (self.wow_phase * TAU).sin();
        let fast = (self.flutter_phase * TAU).sin();
        let cents = flutter * (wow * 8.0 + fast * 3.5);
        2f32.powf(cents / 1200.0)
    }

    pub fn process(&mut self, left: f32, right: f32, flutter: f32, warmth: f32) -> (f32, f32) {
        self.wow_phase = wrap(self.wow_phase + WOW_HZ / self.sample_rate);
        self.flutter_phase = wrap(self.flutter_phase + FLUTTER_HZ / self.sample_rate);

        let (left, right) = saturate(left, right, warmth);
        if flutter <= 0.0 {
            return (left, right);
        }

        self.buf_l[self.write] = left;
        self.buf_r[self.write] = right;

        let wander = (self.wow_phase * TAU).sin() * 0.0011 * flutter;
        let left_out = read(
            &self.buf_l,
            self.write,
            (BASE_DELAY_L + wander) * self.sample_rate,
        );
        let right_out = read(
            &self.buf_r,
            self.write,
            (BASE_DELAY_R - wander * 0.7) * self.sample_rate,
        );

        self.write += 1;
        if self.write >= DELAY_LEN {
            self.write = 0;
        }
        (left_out, right_out)
    }
}

fn saturate(left: f32, right: f32, warmth: f32) -> (f32, f32) {
    if warmth <= 0.0 {
        return (left, right);
    }
    let drive = 1.0 + warmth * 2.8;
    let norm = 1.0 / drive.tanh();
    ((left * drive).tanh() * norm, (right * drive).tanh() * norm)
}

fn read(buf: &[f32], write: usize, delay_samples: f32) -> f32 {
    let delay = delay_samples.clamp(1.0, (DELAY_LEN - 2) as f32);
    let pos = write as f32 - delay;
    let i = pos.floor();
    let frac = pos - i;
    let i0 = wrap_index(i as i32);
    let i1 = wrap_index(i as i32 + 1);
    buf[i0] * (1.0 - frac) + buf[i1] * frac
}

fn wrap_index(i: i32) -> usize {
    let n = DELAY_LEN as i32;
    (((i % n) + n) % n) as usize
}

#[inline]
fn wrap(phase: f32) -> f32 {
    phase - phase.floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_flutter_is_unity_pitch() {
        let tape = Tape::new(48_000.0);
        assert_eq!(tape.pitch_ratio(0.0), 1.0);
    }

    #[test]
    fn zero_flutter_is_dry() {
        let mut tape = Tape::new(48_000.0);
        let (l, r) = tape.process(0.25, -0.25, 0.0, 0.0);
        assert_eq!(l, 0.25);
        assert_eq!(r, -0.25);
    }
}
