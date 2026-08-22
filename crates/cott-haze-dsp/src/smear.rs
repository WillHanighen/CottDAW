use crate::filter::OnePoleLp;

const MAX_DELAY: usize = 16_384;

/// Dark recirculating delay so held chords melt into each other.
#[derive(Debug, Clone)]
pub struct Smear {
    sample_rate: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    lp_l: OnePoleLp,
    lp_r: OnePoleLp,
}

impl Smear {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut smear = Self {
            sample_rate: sr,
            buf_l: vec![0.0; MAX_DELAY],
            buf_r: vec![0.0; MAX_DELAY],
            write: 0,
            lp_l: OnePoleLp::default(),
            lp_r: OnePoleLp::default(),
        };
        smear.apply_rate();
        smear
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.apply_rate();
    }

    fn apply_rate(&mut self) {
        self.lp_l.set_cutoff(2_200.0, self.sample_rate);
        self.lp_r.set_cutoff(1_900.0, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write = 0;
        self.lp_l.reset();
        self.lp_r.reset();
    }

    pub fn process(&mut self, left: f32, right: f32, amount: f32) -> (f32, f32) {
        if amount <= 0.0 {
            return (left, right);
        }

        let amount = amount.clamp(0.0, 1.0);
        let delay_l = (0.030 + amount * 0.060) * self.sample_rate;
        let delay_r = (0.038 + amount * 0.068) * self.sample_rate;
        let cutoff = 2_800.0 * (700.0 / 2_800.0_f32).powf(amount);
        self.lp_l.set_cutoff(cutoff, self.sample_rate);
        self.lp_r.set_cutoff(cutoff * 0.88, self.sample_rate);

        let wet_l = self.lp_l.process(read(&self.buf_l, self.write, delay_l));
        let wet_r = self.lp_r.process(read(&self.buf_r, self.write, delay_r));

        let feedback = 0.32 + amount * 0.42;
        self.buf_l[self.write] = left + wet_l * feedback;
        self.buf_r[self.write] = right + wet_r * feedback;
        self.write += 1;
        if self.write >= MAX_DELAY {
            self.write = 0;
        }

        let mix = amount * 0.62;
        (left + wet_l * mix, right + wet_r * mix)
    }
}

fn read(buf: &[f32], write: usize, delay: f32) -> f32 {
    let delay = delay.clamp(1.0, (MAX_DELAY - 2) as f32);
    let pos = write as f32 - delay;
    let pos = if pos < 0.0 {
        pos + MAX_DELAY as f32
    } else {
        pos
    };
    let i0 = pos as usize % MAX_DELAY;
    let i1 = (i0 + 1) % MAX_DELAY;
    let frac = pos - pos.floor();
    buf[i0] + (buf[i1] - buf[i0]) * frac
}
