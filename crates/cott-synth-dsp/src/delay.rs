/// Echo time for the single Delay knob (mix only). ~dotted-eighth at 120 BPM.
pub const DELAY_TIME_SEC: f32 = 0.375;
const FEEDBACK: f32 = 0.4;
const MAX_TIME_SEC: f32 = 1.0;

/// Mono feedback delay. Mix is applied by the caller.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    write: usize,
    sample_rate: f32,
}

impl DelayLine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            buf: vec![0.0; buffer_len(sr)],
            write: 0,
            sample_rate: sr,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        if (sr - self.sample_rate).abs() < 0.5 && self.buf.len() == buffer_len(sr) {
            return;
        }
        *self = Self::new(sr);
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
    }

    /// Write `input` into the line (with feedback) and return the delayed sample.
    pub fn tick(&mut self, input: f32) -> f32 {
        let n = self.buf.len();
        if n < 2 {
            return 0.0;
        }
        let input = if input.is_finite() { input } else { 0.0 };
        let delay_samples = (DELAY_TIME_SEC * self.sample_rate).clamp(1.0, (n - 1) as f32);
        let read = self.write as f32 - delay_samples;
        let read = if read < 0.0 { read + n as f32 } else { read };
        let i0 = (read.floor() as usize) % n;
        let i1 = (i0 + 1) % n;
        let frac = read.fract();
        let mut delayed = self.buf[i0] + (self.buf[i1] - self.buf[i0]) * frac;
        if !delayed.is_finite() {
            delayed = 0.0;
        }
        let written = input + delayed * FEEDBACK;
        self.buf[self.write] = if written.is_finite() { written } else { 0.0 };
        self.write += 1;
        if self.write >= n {
            self.write = 0;
        }
        delayed
    }
}

fn buffer_len(sample_rate: f32) -> usize {
    ((MAX_TIME_SEC * sample_rate).ceil() as usize).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stays_silent() {
        let mut delay = DelayLine::new(48_000.0);
        for _ in 0..100 {
            assert_eq!(delay.tick(0.0), 0.0);
        }
    }

    #[test]
    fn impulse_returns_after_delay() {
        let sr = 1_000.0;
        let mut delay = DelayLine::new(sr);
        let expected = (DELAY_TIME_SEC * sr).round() as usize;
        assert!(delay.tick(1.0).abs() < 1e-6);
        let mut peak_at = 0usize;
        let mut peak = 0.0f32;
        for i in 1..expected + 8 {
            let y = delay.tick(0.0);
            if y.abs() > peak {
                peak = y.abs();
                peak_at = i;
            }
        }
        assert!(peak > 0.5, "peak {peak}");
        assert!(
            (peak_at as i32 - expected as i32).abs() <= 2,
            "peak at {peak_at}, expected ~{expected}"
        );
    }

    #[test]
    fn nan_input_does_not_poison() {
        let mut delay = DelayLine::new(1_000.0);
        assert!(delay.tick(f32::NAN).is_finite());
        for _ in 0..800 {
            assert!(delay.tick(0.0).is_finite());
        }
    }
}
