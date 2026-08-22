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
        let hz = hz.clamp(40.0, sr * 0.45);
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

/// Cheap band-pass: high-pass then low-pass around the guitar body.
#[derive(Debug, Clone, Copy, Default)]
pub struct Body {
    hp_x: f32,
    hp_y: f32,
    hp_g: f32,
    lp: OnePoleLp,
}

impl Body {
    pub fn set(&mut self, hz: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let hz = hz.clamp(80.0, 800.0);
        self.hp_g = (-std::f32::consts::TAU * (hz * 0.45) / sr).exp();
        self.lp.set_cutoff(hz * 1.6, sr);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.hp_y = self.hp_g * (self.hp_y + x - self.hp_x);
        self.hp_x = x;
        self.lp.process(self.hp_y)
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.hp_x = 0.0;
        self.hp_y = 0.0;
        self.lp.reset();
    }
}
