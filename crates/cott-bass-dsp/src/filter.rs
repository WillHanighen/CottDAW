/// Cascaded one-pole pair. About 12 dB/oct.

#[derive(Debug, Clone, Copy, Default)]
pub struct Lowpass12 {
    a: f32,
    b: f32,
    g: f32,
}

impl Lowpass12 {
    pub fn set_cutoff(&mut self, hz: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let hz = hz.clamp(20.0, sr * 0.45);
        self.g = 1.0 - (-std::f32::consts::TAU * hz / sr).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.a += self.g * (x - self.a);
        self.b += self.g * (self.a - self.b);
        self.b
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.a = 0.0;
        self.b = 0.0;
    }
}
