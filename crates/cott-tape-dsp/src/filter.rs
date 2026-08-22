/// One-pole low-pass for dark tape repeats.

#[derive(Debug, Clone, Copy)]
pub struct OnePoleLp {
    y: f32,
    g: f32,
}

impl Default for OnePoleLp {
    fn default() -> Self {
        Self { y: 0.0, g: 1.0 }
    }
}

impl OnePoleLp {
    pub fn set_cutoff(&mut self, hz: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let hz = hz.clamp(20.0, sr * 0.45);
        self.g = 1.0 - (-std::f32::consts::TAU * hz / sr).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.y += self.g * (x - self.y);
        self.y
    }

    pub fn reset(&mut self) {
        self.y = 0.0;
    }
}
