use crate::filter::{OnePoleHp, OnePoleLp};
use crate::noise_tick;

/// Vinyl hiss and the occasional click. Silent when `dust` is 0.

#[derive(Debug, Clone)]
pub struct Dust {
    sample_rate: f32,
    hiss: u32,
    crackle: u32,
    hiss_lp: OnePoleLp,
    hiss_hp: OnePoleHp,
    click_env: f32,
    samples_until_click: u32,
}

impl Dust {
    pub fn new(sample_rate: f32) -> Self {
        let mut dust = Self {
            sample_rate: sample_rate.max(1.0),
            hiss: 0xC0FF_EE42,
            crackle: 0xDEAD_BEEF,
            hiss_lp: OnePoleLp::default(),
            hiss_hp: OnePoleHp::default(),
            click_env: 0.0,
            samples_until_click: 0,
        };
        dust.apply_rate();
        dust
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.apply_rate();
    }

    fn apply_rate(&mut self) {
        self.hiss_lp.set_cutoff(2400.0, self.sample_rate);
        self.hiss_hp.set_cutoff(700.0, self.sample_rate);
        self.schedule_click();
    }

    pub fn reset(&mut self) {
        self.hiss = 0xC0FF_EE42;
        self.crackle = 0xDEAD_BEEF;
        self.hiss_lp.reset();
        self.hiss_hp.reset();
        self.click_env = 0.0;
        self.schedule_click();
    }

    pub fn process(&mut self, left: f32, right: f32, dust: f32) -> (f32, f32) {
        if dust <= 0.0 {
            return (left, right);
        }

        let n = noise_tick(&mut self.hiss);
        let hiss = self.hiss_hp.process(self.hiss_lp.process(n)) * dust * 0.035;

        if self.samples_until_click == 0 {
            self.click_env = 0.55 + noise_tick(&mut self.crackle).abs() * 0.45;
            self.schedule_click();
        } else {
            self.samples_until_click -= 1;
        }
        let click = self.click_env * dust * 0.12;
        self.click_env *= 0.86;

        (left + hiss + click, right + hiss * 0.92 + click * 0.8)
    }

    fn schedule_click(&mut self) {
        let span = (self.sample_rate * 1.8) as u32;
        let n = noise_tick(&mut self.crackle).abs();
        self.samples_until_click = ((0.25 + n * 0.75) * span as f32) as u32 + 64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_dust_is_dry() {
        let mut dust = Dust::new(48_000.0);
        let (l, r) = dust.process(0.1, 0.2, 0.0);
        assert_eq!(l, 0.1);
        assert_eq!(r, 0.2);
    }
}
