//! The filter half of the voice: a four-pole ladder plus a fixed resonator
//! bank.
//!
//! The Pro Soloist did not shape its reed voices with the VCF alone. Its pulse
//! output could be routed through banks of fixed band-pass "resonators" whose
//! peaks sat where an acoustic instrument's formants sit, and the result went
//! either on into the VCF or straight to the VCA. That parallel path is what
//! makes the worm sound like a squeezed reed rather than a filter sweep, so it
//! is modelled here alongside the ladder that the later Moog leads leaned on.

/// Resonators in the bank. The hardware had ten fixed circuits and switched up
/// to five in at once; three is enough to place a voice's formants.
pub const BANK_SIZE: usize = 3;

/// Four-pole transistor-ladder low-pass, topology preserving with the feedback
/// path solved rather than delayed, so it stays stable up to self-oscillation.
///
/// ARP's 4034 was a copy of the Moog ladder until Moog's lawyers intervened, so
/// the same model serves both halves of this instrument's lineage.
#[derive(Debug, Clone, Copy, Default)]
pub struct LadderLp {
    /// One-pole TPT coefficient, `tan(w/2) / (1 + tan(w/2))`.
    g: f32,
    /// Feedback depth, 0..4. Four self-oscillates.
    k: f32,
    s: [f32; 4],
}

impl LadderLp {
    pub fn new(cutoff_hz: f32, emphasis: f32, sample_rate: f32) -> Self {
        let mut f = Self::default();
        f.set(cutoff_hz, emphasis, sample_rate);
        f
    }

    /// `emphasis` is 0..1; 1 sits just under self-oscillation.
    pub fn set(&mut self, cutoff_hz: f32, emphasis: f32, sample_rate: f32) {
        let sample_rate = sample_rate.max(1.0);
        let cutoff = cutoff_hz.clamp(20.0, sample_rate * 0.45);
        let t = (std::f32::consts::PI * cutoff / sample_rate)
            .tan()
            .clamp(1e-5, 12.0);
        self.g = t / (1.0 + t);
        self.k = emphasis.clamp(0.0, 1.0) * 3.85;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let g = self.g;
        let one_minus = 1.0 - g;
        // Propagate the stored states through the cascade, then solve for the
        // output the feedback loop settles on this sample.
        let state =
            one_minus * (g * g * g * self.s[0] + g * g * self.s[1] + g * self.s[2] + self.s[3]);
        let g4 = g * g * g * g;
        let y4 = (g4 * x + state) / (1.0 + self.k * g4);
        // Saturating the feedback node is what makes a hard-driven ladder growl
        // instead of scream.
        let mut u = soft_clip(x - self.k * y4);

        for s in self.s.iter_mut() {
            let v = (u - *s) * g;
            let y = v + *s;
            *s = y + v;
            u = y;
        }

        if u.is_finite() {
            // Feedback drains the passband; hand some of it back.
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

/// Bounded saturator for the ladder's input stage.
///
/// Kept gentle on purpose. A narrow pulse is nearly all crest, and a hard
/// transfer curve here flattens the very spikes that carry its harmonics — the
/// filter ends up duller with the drive up than with it down, which is backwards.
fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-8.0, 8.0);
    x / (1.0 + 0.08 * x * x).sqrt()
}

/// One fixed resonator: a two-pole band-pass with unity gain at its peak.
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Constant peak gain band-pass (RBJ cookbook).
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

/// Centre frequency, Q and level of one resonator slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResonatorSpec {
    pub freq_hz: f32,
    pub q: f32,
    pub gain: f32,
}

impl ResonatorSpec {
    pub const fn new(freq_hz: f32, q: f32, gain: f32) -> Self {
        Self { freq_hz, q, gain }
    }
}

/// The parallel bank. Every slot sees the raw oscillator mix; their outputs sum.
#[derive(Debug, Clone, Copy)]
pub struct ResonatorBank {
    slots: [Resonator; BANK_SIZE],
    gains: [f32; BANK_SIZE],
    /// Sum of the slot gains, used to keep the bank at a sane level.
    norm: f32,
}

impl Default for ResonatorBank {
    fn default() -> Self {
        Self::new()
    }
}

impl ResonatorBank {
    pub fn new() -> Self {
        Self {
            slots: [Resonator::new(); BANK_SIZE],
            gains: [0.0; BANK_SIZE],
            norm: 1.0,
        }
    }

    pub fn set(&mut self, specs: &[ResonatorSpec; BANK_SIZE], sample_rate: f32) {
        let mut total = 0.0;
        for (i, spec) in specs.iter().enumerate() {
            self.slots[i].set(spec.freq_hz, spec.q, sample_rate);
            self.gains[i] = spec.gain.max(0.0);
            total += self.gains[i];
        }
        self.norm = if total > 1e-6 { 1.0 / total } else { 0.0 };
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let mut sum = 0.0;
        for (slot, gain) in self.slots.iter_mut().zip(self.gains) {
            sum += slot.process(x) * gain;
        }
        // Band-passes throw most of the signal away; the bank is brought back up
        // to something comparable with the direct path so the Body control is a
        // blend rather than a fade to nothing.
        sum * self.norm * BANK_MAKEUP
    }

    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.reset();
        }
    }
}

/// Band-passes at these Qs pass a fraction of a broadband source; this puts the
/// bank back in the same neighbourhood as the direct path.
pub const BANK_MAKEUP: f32 = 3.2;

/// One-pole high-pass. The Pro Soloist had four fixed settings ahead of its
/// VCF; each voice here picks its own corner.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnePoleHp {
    coeff: f32,
    prev_in: f32,
    prev_out: f32,
}

impl OnePoleHp {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let mut f = Self::default();
        f.set(cutoff_hz, sample_rate);
        f
    }

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

/// Fixed 12 Hz blocker on the output, so saturation asymmetry cannot walk the
/// signal off centre.
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
