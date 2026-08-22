/// One-pole low-pass.

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

/// One-pole high-pass.

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

/// RBJ low-pass or high-pass. 12 dB/oct, so muffle actually takes the air off.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiquadMode {
    LowPass,
    HighPass,
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    fn process(&mut self, x: f32, c: &BiquadCoeffs) -> f32 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
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

#[derive(Debug, Clone, Copy)]
pub struct StereoBiquad {
    coeffs: BiquadCoeffs,
    left: BiquadState,
    right: BiquadState,
}

impl Default for StereoBiquad {
    fn default() -> Self {
        Self {
            coeffs: BiquadCoeffs {
                b0: 1.0,
                ..BiquadCoeffs::default()
            },
            left: BiquadState::default(),
            right: BiquadState::default(),
        }
    }
}

impl StereoBiquad {
    pub fn set(&mut self, mode: BiquadMode, hz: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let hz = hz.clamp(20.0, sr * 0.45);
        let w0 = std::f32::consts::TAU * (hz / sr);
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * 0.707);
        let (b0, b1, b2, a0, a1, a2) = match mode {
            BiquadMode::LowPass => {
                let b1 = 1.0 - cos_w0;
                let b0 = b1 * 0.5;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            BiquadMode::HighPass => {
                let b0 = (1.0 + cos_w0) * 0.5;
                let b1 = -(1.0 + cos_w0);
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
        };
        self.coeffs = BiquadCoeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        };
    }

    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        (
            self.left.process(left, &self.coeffs),
            self.right.process(right, &self.coeffs),
        )
    }

    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

impl OnePoleHp {
    pub fn set_cutoff(&mut self, hz: f32, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        let hz = hz.clamp(10.0, sr * 0.45);
        self.g = (-std::f32::consts::TAU * hz / sr).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.y = self.g * (self.y + x - self.x1);
        self.x1 = x;
        self.y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y = 0.0;
    }
}
