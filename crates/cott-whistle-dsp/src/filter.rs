//! 4034 ladder, one-pole HP, and the three Board C resonator banks.
//!
//! Ten named Twin-T nets, frequencies from schematic R/C (notes/2701.md).
//! Same bank, two nets on: one interpolated peak into one op-amp. Banks 1 and
//! 2 feed the VCA. Bank 3 feeds the VCF, the VCA, or both (Z8).

/// Twin-T f0 = 1/(2 pi R sqrt(C1 C2)), Board C schematic. Q is the bootstrapped
/// op-amp around each bank, not a vocal formant.
///
/// Index: Cello2, Violin2, E.Horn, Cello1, Violin3, Violin1, Cello3, E.Piano,
/// E.Bass, Oboe. Banks 1, 1, 2, 2, 2, 3, 3, 3, 3, 3.
pub const CURVES: [(f32, f32); 10] = [
    (1_453.0, 2.2),
    (1_299.0, 2.2),
    (988.0, 2.2),
    (293.0, 2.2),
    (3_308.0, 2.2),
    (585.0, 2.2),
    (1_937.0, 2.2),
    (374.0, 2.2),
    (115.0, 2.2),
    (1_715.0, 2.2),
];

pub const CURVE_NAMES: [&str; 10] = [
    "Cello 2", "Violin 2", "E. Horn", "Cello 1", "Violin 3", "Violin 1", "Cello 3", "E. Piano",
    "E. Bass", "Oboe",
];

/// Hardware bank for a named curve: 0 and 1 to VCA, 2 is bank 3.
pub fn curve_bank(curve: u8) -> u8 {
    match curve.min(9) {
        0 | 1 => 0,
        2 | 3 | 4 => 1,
        _ => 2,
    }
}

/// Three hardware banks (Board C). Edit-panel slots collapse into these.
pub const BANK_SIZE: usize = 3;

/// ROM "maximum" resonance (Z6). Wow forces this. Not self-oscillation.
pub const RESONANCE_MAX: f32 = 0.42;

/// Four-pole transistor-ladder low-pass. ARP's 4034 was a Moog copy until the
/// lawyers arrived; resonance is capped below self-oscillation because none of
/// the factory paddles go there.
#[derive(Debug, Clone, Copy, Default)]
pub struct LadderLp {
    g: f32,
    k: f32,
    s: [f32; 4],
}

impl LadderLp {
    pub fn set(&mut self, cutoff_hz: f32, emphasis: f32, sample_rate: f32) {
        let sample_rate = sample_rate.max(1.0);
        let cutoff = cutoff_hz.clamp(20.0, sample_rate * 0.45);
        let t = (std::f32::consts::PI * cutoff / sample_rate)
            .tan()
            .clamp(1e-5, 12.0);
        self.g = t / (1.0 + t);
        self.k = emphasis.clamp(0.0, 1.0) * 3.2;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let g = self.g;
        let one_minus = 1.0 - g;
        let state =
            one_minus * (g * g * g * self.s[0] + g * g * self.s[1] + g * self.s[2] + self.s[3]);
        let g4 = g * g * g * g;
        let y4 = (g4 * x + state) / (1.0 + self.k * g4);
        let mut u = (x - self.k * y4).clamp(-8.0, 8.0).tanh();

        for s in self.s.iter_mut() {
            let v = (u - *s) * g;
            let y = v + *s;
            *s = y + v;
            u = y;
        }

        if u.is_finite() {
            u * (1.0 + self.k * 0.3)
        } else {
            self.reset();
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Resonator {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    pub fn set(&mut self, freq_hz: f32, q: f32, sample_rate: f32) {
        let sample_rate = sample_rate.max(1.0);
        let freq = freq_hz.clamp(20.0, sample_rate * 0.45);
        let q = q.clamp(0.2, 24.0);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let a0 = 1.0 + alpha;
        let inv = 1.0 / a0;
        self.b0 = alpha * inv;
        self.b2 = -alpha * inv;
        self.a1 = -2.0 * cos_w0 * inv;
        self.a2 = (1.0 - alpha) * inv;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = if y.is_finite() { y } else { 0.0 };
        self.y1
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Three Board C banks. Each can feed the VCF, the VCA, or both.
#[derive(Debug, Clone, Copy)]
pub struct ResonatorBank {
    slots: [Resonator; BANK_SIZE],
    to_vcf: [f32; BANK_SIZE],
    to_vca: [f32; BANK_SIZE],
}

impl Default for ResonatorBank {
    fn default() -> Self {
        Self {
            slots: [Resonator::default(); BANK_SIZE],
            to_vcf: [0.0; BANK_SIZE],
            to_vca: [0.0; BANK_SIZE],
        }
    }
}

impl ResonatorBank {
    pub fn set_slot(&mut self, i: usize, freq_hz: f32, q: f32, to_vcf: f32, to_vca: f32, sr: f32) {
        self.slots[i].set(freq_hz, q, sr);
        self.to_vcf[i] = to_vcf.max(0.0);
        self.to_vca[i] = to_vca.max(0.0);
    }

    pub fn disable_slot(&mut self, i: usize) {
        self.to_vcf[i] = 0.0;
        self.to_vca[i] = 0.0;
    }

    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let mut vcf = 0.0;
        let mut vca = 0.0;
        for i in 0..BANK_SIZE {
            if self.to_vcf[i] <= 0.0 && self.to_vca[i] <= 0.0 {
                continue;
            }
            let y = self.slots[i].process(x);
            vcf += y * self.to_vcf[i];
            vca += y * self.to_vca[i];
        }
        (vcf, vca)
    }

    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.reset();
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OnePoleHp {
    coeff: f32,
    prev_in: f32,
    prev_out: f32,
}

impl OnePoleHp {
    pub fn set(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let sample_rate = sample_rate.max(1.0);
        let rc = 1.0 / (std::f32::consts::TAU * cutoff_hz.clamp(1.0, sample_rate * 0.45));
        let dt = 1.0 / sample_rate;
        self.coeff = rc / (rc + dt);
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.coeff * (self.prev_out + x - self.prev_in);
        self.prev_in = x;
        self.prev_out = if y.is_finite() { y } else { 0.0 };
        self.prev_out
    }

    pub fn reset(&mut self) {
        self.prev_in = 0.0;
        self.prev_out = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DcBlocker {
    r: f32,
    prev_in: f32,
    prev_out: f32,
}

impl Default for DcBlocker {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl DcBlocker {
    pub fn new(sample_rate: f32) -> Self {
        let mut f = Self {
            r: 0.999,
            prev_in: 0.0,
            prev_out: 0.0,
        };
        f.set_sample_rate(sample_rate);
        f
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sample_rate = sample_rate.max(1.0);
        self.r = 1.0 - (std::f32::consts::TAU * 12.0 / sample_rate).min(0.5);
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.prev_in + self.r * self.prev_out;
        self.prev_in = x;
        self.prev_out = if y.is_finite() { y } else { 0.0 };
        self.prev_out
    }

    pub fn reset(&mut self) {
        self.prev_in = 0.0;
        self.prev_out = 0.0;
    }
}

/// Board C series-RC highpasses, R = 12k. A is the brightest.
pub const HPF_HZ: [f32; 4] = [2_653.0, 603.0, 282.0, 60.0];

/// Four hardware HP switch points. Index 0 = A.
pub fn hp_from_index(index: u8) -> f32 {
    HPF_HZ[index.min(3) as usize]
}

/// Highest enabled section, or 20 Hz if the ROM left them all off.
pub fn hp_from_mask(mask: u8) -> f32 {
    let mut hz: f32 = 20.0;
    for i in 0..4u8 {
        if mask & (1 << i) != 0 {
            hz = hz.max(HPF_HZ[i as usize]);
        }
    }
    hz
}
