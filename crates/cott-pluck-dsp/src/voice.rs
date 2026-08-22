use crate::filter::{Body, OnePoleLp};
use crate::midi_note_to_hz;
use crate::noise_tick;
use crate::PluckParams;

const MAX_LEN: usize = 2_048;

#[derive(Debug, Clone)]
pub struct Voice {
    #[allow(dead_code)]
    pub note: u8,
    #[allow(dead_code)]
    pub channel: u8,
    pub age: u64,
    buf: Vec<f32>,
    len: usize,
    read: usize,
    last: f32,
    decay: f32,
    remaining: u32,
    tone: OnePoleLp,
    body: Body,
    vel: f32,
}

impl Voice {
    pub fn start(
        note: u8,
        velocity: u8,
        channel: u8,
        age: u64,
        params: &PluckParams,
        sample_rate: f32,
        rng: &mut u32,
    ) -> Self {
        let hz = midi_note_to_hz(note).clamp(40.0, 2_000.0);
        let len = (sample_rate / hz).round().clamp(8.0, MAX_LEN as f32) as usize;
        let vel = velocity as f32 / 127.0;
        let mut buf = vec![0.0f32; MAX_LEN];
        let brightness = 0.25 + vel * 0.75 * (1.0 - params.mute * 0.55);
        for i in 0..len {
            let n = noise_tick(rng);
            buf[i] = n * brightness;
        }
        // One pass of averaging so the burst is a string, not a click.
        let mut prev = buf[len - 1];
        for i in 0..len {
            let x = buf[i];
            buf[i] = 0.5 * (x + prev);
            prev = x;
        }

        let mute = params.mute.clamp(0.0, 1.0);
        let decay = 0.9975 - mute * 0.018 - (hz / 2_000.0) * 0.004;
        let mut tone = OnePoleLp::default();
        tone.set_cutoff(
            1_800.0 * (420.0 / 1_800.0_f32).powf(mute * 0.65 + (1.0 - params.tone) * 0.5),
            sample_rate,
        );
        let mut body = Body::default();
        body.set(180.0 + params.body * 220.0, sample_rate);

        Self {
            note,
            channel,
            age,
            buf,
            len,
            read: 0,
            last: 0.0,
            decay: decay.clamp(0.90, 0.9994),
            remaining: (sample_rate * (1.2 + (1.0 - mute) * 2.4)) as u32,
            tone,
            body,
            vel,
        }
    }

    pub fn tick(&mut self, params: &PluckParams) -> f32 {
        if self.remaining == 0 {
            return 0.0;
        }
        self.remaining -= 1;
        let x = self.buf[self.read];
        let avg = 0.5 * (x + self.last);
        self.last = x;
        let damped = avg * self.decay;
        self.buf[self.read] = damped;
        self.read += 1;
        if self.read >= self.len {
            self.read = 0;
        }
        let string = self.tone.process(damped);
        let body = self.body.process(string);
        let mix = string * (1.0 - params.body * 0.55) + body * params.body * 0.9;
        mix * self.vel * 0.55
    }

    pub fn is_active(&self) -> bool {
        self.remaining > 0
    }
}
