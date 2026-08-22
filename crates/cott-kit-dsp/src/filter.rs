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

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.y = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OnePoleHp {
    x1: f32,
    y: f32,
    g: f32,
}

impl Default for OnePoleHp {
    fn default() -> Self {
        Self {
            x1: 0.0,
            y: 0.0,
            g: 0.0,
        }
    }
}

impl OnePoleHp {
    pub fn set_cutoff(&mut self, hz: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let hz = hz.clamp(20.0, sr * 0.45);
        self.g = (-std::f32::consts::TAU * hz / sr).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.y = self.g * (self.y + x - self.x1);
        self.x1 = x;
        self.y
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y = 0.0;
    }
}
