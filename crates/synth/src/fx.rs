//! Stereo effects and the mixer. All process in place.

use crate::dsp::{pan, FilterMode, Smooth, Svf, TAU};

/// A delay line with linear-interpolated reads.
#[derive(Clone)]
struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    fn new(len: usize) -> Self {
        DelayLine {
            buf: vec![0.0; len.max(2)],
            pos: 0,
        }
    }

    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 2) as f32);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        let a = self.buf[(self.pos + len - i) % len];
        let b = self.buf[(self.pos + len - i - 1) % len];
        a + (b - a) * frac
    }

    #[inline]
    fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

// ---- Delay -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayParams {
    /// Delay time in beats (0.5 = an eighth note).
    pub beats: f32,
    pub feedback: f32,
    pub mix: f32,
    pub ping_pong: bool,
    /// Low-pass in the feedback path, Hz.
    pub tone: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        DelayParams {
            beats: 0.75,
            feedback: 0.35,
            mix: 0.25,
            ping_pong: true,
            tone: 4000.0,
        }
    }
}

pub struct Delay {
    pub params: DelayParams,
    sr: f32,
    left: DelayLine,
    right: DelayLine,
    time: Smooth,
    tone_l: Svf,
    tone_r: Svf,
}

const MAX_DELAY_SECONDS: f32 = 3.0;

impl Delay {
    pub fn new(params: DelayParams, sr: f32) -> Self {
        let len = (MAX_DELAY_SECONDS * sr) as usize;
        let mut d = Delay {
            params,
            sr,
            left: DelayLine::new(len),
            right: DelayLine::new(len),
            time: Smooth::new(0.0, 0.08, sr),
            tone_l: Svf::default(),
            tone_r: Svf::default(),
        };
        d.set_params(params);
        d
    }

    pub fn set_params(&mut self, params: DelayParams) {
        self.params = params;
        self.tone_l.set(params.tone, 0.0, self.sr);
        self.tone_r.set(params.tone, 0.0, self.sr);
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], bpm: f64) {
        let p = self.params;
        let target =
            (p.beats * 60.0 / bpm as f32 * self.sr).clamp(1.0, MAX_DELAY_SECONDS * self.sr - 4.0);
        self.time.target = target;
        if self.time.value == 0.0 {
            self.time.value = target;
        }
        let fb = p.feedback.clamp(0.0, 0.95);
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let t = self.time.next();
            let dl = self.left.read(t);
            let dr = self.right.read(t);
            let (in_l, in_r) = (*xl, *xr);
            if p.ping_pong {
                let mono = (in_l + in_r) * 0.5;
                self.left
                    .write(mono + self.tone_l.process(dr, FilterMode::Low) * fb);
                self.right
                    .write(self.tone_r.process(dl, FilterMode::Low) * fb);
            } else {
                self.left
                    .write(in_l + self.tone_l.process(dl, FilterMode::Low) * fb);
                self.right
                    .write(in_r + self.tone_r.process(dr, FilterMode::Low) * fb);
            }
            *xl = in_l + dl * p.mix;
            *xr = in_r + dr * p.mix;
        }
    }

    pub fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
    }
}

// ---- Reverb (Freeverb) ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbParams {
    /// 0..1
    pub size: f32,
    /// 0..1
    pub damping: f32,
    /// 0..1
    pub mix: f32,
    /// 0..1
    pub width: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        ReverbParams {
            size: 0.75,
            damping: 0.4,
            mix: 0.25,
            width: 1.0,
        }
    }
}

#[derive(Clone)]
struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    #[inline]
    fn tick(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

#[derive(Clone)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        self.buf[self.pos] = x + b * 0.5;
        self.pos = (self.pos + 1) % self.buf.len();
        b - x
    }
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

pub struct Reverb {
    pub params: ReverbParams,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    pub fn new(params: ReverbParams, sr: f32) -> Self {
        let scale = sr / 44_100.0;
        let make = |spread: usize| {
            let combs = COMBS
                .iter()
                .map(|&n| Comb {
                    buf: vec![0.0; ((n + spread) as f32 * scale) as usize],
                    pos: 0,
                    store: 0.0,
                })
                .collect();
            let allpasses = ALLPASSES
                .iter()
                .map(|&n| Allpass {
                    buf: vec![0.0; ((n + spread) as f32 * scale) as usize],
                    pos: 0,
                })
                .collect();
            (combs, allpasses)
        };
        let (cl, al) = make(0);
        let (cr, ar) = make(SPREAD);
        Reverb {
            params,
            combs: [cl, cr],
            allpasses: [al, ar],
        }
    }

    pub fn set_params(&mut self, params: ReverbParams) {
        self.params = params;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let feedback = 0.7 + p.size.clamp(0.0, 1.0) * 0.28;
        let damp = p.damping.clamp(0.0, 1.0) * 0.4;
        let wet1 = p.mix * (p.width / 2.0 + 0.5) * 3.0;
        let wet2 = p.mix * ((1.0 - p.width) / 2.0) * 3.0;
        let dry = 1.0 - p.mix * 0.5;
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let input = (*xl + *xr) * 0.015;
            let mut out = [0.0f32; 2];
            for (o, (combs, allpasses)) in out
                .iter_mut()
                .zip(self.combs.iter_mut().zip(self.allpasses.iter_mut()))
            {
                let mut acc = 0.0;
                for c in combs.iter_mut() {
                    acc += c.tick(input, feedback, damp);
                }
                for a in allpasses.iter_mut() {
                    acc = a.tick(acc);
                }
                *o = acc;
            }
            let (dl, dr) = (*xl, *xr);
            *xl = dl * dry + out[0] * wet1 + out[1] * wet2;
            *xr = dr * dry + out[1] * wet1 + out[0] * wet2;
        }
    }
}

// ---- Chorus -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChorusParams {
    pub rate: f32,
    /// 0..1
    pub depth: f32,
    /// 0..1
    pub mix: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        ChorusParams {
            rate: 0.6,
            depth: 0.5,
            mix: 0.4,
        }
    }
}

pub struct Chorus {
    pub params: ChorusParams,
    sr: f32,
    left: DelayLine,
    right: DelayLine,
    phase: f32,
}

impl Chorus {
    pub fn new(params: ChorusParams, sr: f32) -> Self {
        let len = (0.05 * sr) as usize;
        Chorus {
            params,
            sr,
            left: DelayLine::new(len),
            right: DelayLine::new(len),
            phase: 0.0,
        }
    }

    pub fn set_params(&mut self, params: ChorusParams) {
        self.params = params;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let base = 0.012 * self.sr;
        let depth = p.depth.clamp(0.0, 1.0) * 0.006 * self.sr;
        let inc = p.rate / self.sr;
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            self.phase = (self.phase + inc).fract();
            let dl = base + depth * (TAU * self.phase).sin();
            let dr = base + depth * (TAU * self.phase + TAU / 4.0).sin();
            self.left.write(*xl);
            self.right.write(*xr);
            let wl = self.left.read(dl);
            let wr = self.right.read(dr);
            *xl = *xl * (1.0 - p.mix * 0.5) + wl * p.mix;
            *xr = *xr * (1.0 - p.mix * 0.5) + wr * p.mix;
        }
    }
}

// ---- Filter -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterParams {
    pub mode: FilterMode,
    pub cutoff: f32,
    /// 0..1
    pub resonance: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams {
            mode: FilterMode::Low,
            cutoff: 2000.0,
            resonance: 0.2,
        }
    }
}

pub struct FilterFx {
    pub params: FilterParams,
    sr: f32,
    left: Svf,
    right: Svf,
    cutoff: Smooth,
    counter: u32,
}

impl FilterFx {
    pub fn new(params: FilterParams, sr: f32) -> Self {
        FilterFx {
            params,
            sr,
            left: Svf::default(),
            right: Svf::default(),
            cutoff: Smooth::new(params.cutoff, 0.03, sr),
            counter: 0,
        }
    }

    pub fn set_params(&mut self, params: FilterParams) {
        self.params = params;
        self.cutoff.target = params.cutoff;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let c = self.cutoff.next();
            if self.counter.is_multiple_of(16) {
                self.left.set(c, p.resonance, self.sr);
                self.right.set(c, p.resonance, self.sr);
            }
            self.counter = self.counter.wrapping_add(1);
            *xl = self.left.process(*xl, p.mode);
            *xr = self.right.process(*xr, p.mode);
        }
    }
}

// ---- Drive -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveParams {
    /// 0..1
    pub amount: f32,
    /// 0..1
    pub mix: f32,
}

impl Default for DriveParams {
    fn default() -> Self {
        DriveParams {
            amount: 0.4,
            mix: 1.0,
        }
    }
}

pub fn drive(l: &mut [f32], r: &mut [f32], p: DriveParams) {
    let gain = 1.0 + p.amount.clamp(0.0, 1.0) * 24.0;
    let makeup = 1.0 / gain.sqrt();
    for x in l.iter_mut().chain(r.iter_mut()) {
        let wet = (*x * gain).tanh() * makeup;
        *x = *x * (1.0 - p.mix) + wet * p.mix;
    }
}

// ---- Mixer -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelParams {
    /// Linear gain.
    pub level: f32,
    /// -1..1
    pub pan: f32,
}

impl Default for ChannelParams {
    fn default() -> Self {
        ChannelParams {
            level: 1.0,
            pan: 0.0,
        }
    }
}

/// Applies a channel's level and pan to a stereo signal and adds it to the output.
pub fn mix_into(
    out_l: &mut [f32],
    out_r: &mut [f32],
    in_l: &[f32],
    in_r: &[f32],
    ch: ChannelParams,
) {
    let (gl, gr) = pan(ch.pan);
    // Equal-power pan is -3 dB in the centre; compensate so centre is unity.
    let (gl, gr) = (
        gl * ch.level * std::f32::consts::SQRT_2,
        gr * ch.level * std::f32::consts::SQRT_2,
    );
    if ch.pan.abs() < 1e-3 {
        for i in 0..out_l.len() {
            out_l[i] += in_l[i] * ch.level;
            out_r[i] += in_r[i] * ch.level;
        }
        return;
    }
    for i in 0..out_l.len() {
        // A balance control: each side keeps its own signal.
        out_l[i] += in_l[i] * gl.min(ch.level);
        out_r[i] += in_r[i] * gr.min(ch.level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn impulse(n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        l[0] = 1.0;
        r[0] = 1.0;
        (l, r)
    }

    #[test]
    fn delay_echoes_on_the_beat() {
        let params = DelayParams {
            beats: 1.0,
            feedback: 0.0,
            mix: 1.0,
            ping_pong: false,
            tone: 20_000.0,
        };
        let mut d = Delay::new(params, SR);
        let (mut l, mut r) = impulse(48_000);
        d.process(&mut l, &mut r, 120.0);
        // One beat at 120 bpm = 0.5 s = 24000 samples.
        let echo = l[1..]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0
            + 1;
        assert!((echo as i64 - 24_000).abs() <= 2, "{echo}");
    }

    #[test]
    fn reverb_tail_decays_and_stays_finite() {
        let mut rv = Reverb::new(
            ReverbParams {
                mix: 1.0,
                ..Default::default()
            },
            SR,
        );
        let (mut l, mut r) = impulse(48_000 * 4);
        rv.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        let energy = |s: &[f32]| s.iter().map(|x| x * x).sum::<f32>();
        let early = energy(&l[4_800..24_000]);
        let late = energy(&l[150_000..]);
        assert!(early > 0.01 && late < early * 0.05, "{early} {late}");
    }

    #[test]
    fn chorus_and_drive_are_bounded() {
        let mut c = Chorus::new(ChorusParams::default(), SR);
        let mut l: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        drive(
            &mut l,
            &mut r,
            DriveParams {
                amount: 1.0,
                mix: 1.0,
            },
        );
        assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 1.5));
    }

    #[test]
    fn centred_channel_is_unity() {
        let mut ol = vec![0.0; 4];
        let mut or = vec![0.0; 4];
        mix_into(
            &mut ol,
            &mut or,
            &[1.0; 4],
            &[1.0; 4],
            ChannelParams {
                level: 0.5,
                pan: 0.0,
            },
        );
        assert_eq!(ol, vec![0.5; 4]);
        let mut ol = vec![0.0; 4];
        let mut or = vec![0.0; 4];
        mix_into(
            &mut ol,
            &mut or,
            &[1.0; 4],
            &[1.0; 4],
            ChannelParams {
                level: 1.0,
                pan: -1.0,
            },
        );
        assert!(or[0].abs() < 1e-6 && (ol[0] - 1.0).abs() < 1e-6);
    }
}
