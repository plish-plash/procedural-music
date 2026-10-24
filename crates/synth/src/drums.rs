//! A synthesized drum kit on General MIDI drum keys. No samples needed.

use crate::dsp::{pan, Decay, FilterMode, Noise, Svf, TAU};
use crate::NoteEvent;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrumParams {
    /// Semitones up or down for every drum.
    pub tune: f32,
    /// Multiplies every decay time.
    pub decay: f32,
    /// Linear output gain.
    pub level: f32,
}

impl Default for DrumParams {
    fn default() -> Self {
        DrumParams {
            tune: 0.0,
            decay: 1.0,
            level: 0.7,
        }
    }
}

/// A pitched body with a falling pitch envelope: kicks and toms.
#[derive(Clone, Copy, Default)]
struct Membrane {
    phase: f32,
    amp: Decay,
    pitch: Decay,
    click: Decay,
    base: f32,
    sweep: f32,
    velocity: f32,
}

impl Membrane {
    fn trigger(&mut self, base: f32, sweep: f32, decay: f32, velocity: f32, sr: f32) {
        self.base = base;
        self.sweep = sweep;
        self.velocity = velocity;
        self.phase = 0.0;
        self.amp.trigger(decay, sr);
        self.pitch.trigger(0.035, sr);
        self.click.trigger(0.004, sr);
    }

    #[inline]
    fn next(&mut self, noise: &mut Noise, sr: f32) -> f32 {
        if self.amp.level == 0.0 {
            return 0.0;
        }
        let f = self.base * (1.0 + self.sweep * self.pitch.next());
        self.phase = (self.phase + f / sr).fract();
        let body = (TAU * self.phase).sin() * self.amp.next();
        let click = noise.next() * self.click.next() * 0.25;
        (body + click) * self.velocity
    }
}

#[derive(Clone, Copy, Default)]
struct Snare {
    p1: f32,
    p2: f32,
    tone: Decay,
    rattle: Decay,
    hp: Svf,
    lp: Svf,
    velocity: f32,
    tune: f32,
}

impl Snare {
    fn trigger(&mut self, tune: f32, decay: f32, velocity: f32, sr: f32) {
        self.tune = tune;
        self.velocity = velocity;
        self.tone.trigger(0.12 * decay, sr);
        self.rattle.trigger(0.15 * decay, sr);
        self.hp.set(1500.0, 0.1, sr);
        self.lp.set(8000.0, 0.0, sr);
    }

    #[inline]
    fn next(&mut self, noise: &mut Noise, sr: f32) -> f32 {
        if self.rattle.level == 0.0 && self.tone.level == 0.0 {
            return 0.0;
        }
        self.p1 = (self.p1 + 185.0 * self.tune / sr).fract();
        self.p2 = (self.p2 + 330.0 * self.tune / sr).fract();
        let tone = ((TAU * self.p1).sin() * 0.6 + (TAU * self.p2).sin() * 0.4) * self.tone.next();
        let bright = self.hp.process(noise.next(), FilterMode::High);
        let rattle = self.lp.process(bright, FilterMode::Low) * self.rattle.next();
        (tone * 0.75 + rattle * 0.5) * self.velocity
    }
}

#[derive(Clone, Copy, Default)]
struct Clap {
    t: f32,
    tail: Decay,
    bp: Svf,
    velocity: f32,
    active: bool,
}

impl Clap {
    fn trigger(&mut self, decay: f32, velocity: f32, sr: f32) {
        self.t = 0.0;
        self.velocity = velocity;
        self.active = true;
        self.tail.trigger(0.15 * decay, sr);
        self.bp.set(1150.0, 0.45, sr);
    }

    #[inline]
    fn next(&mut self, noise: &mut Noise, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        self.t += 1.0 / sr;
        // Three quick bursts, then a decaying tail.
        let burst = |start: f32, t: f32| {
            if t >= start {
                (-(t - start) / 0.006).exp()
            } else {
                0.0
            }
        };
        let env = if self.t < 0.03 {
            burst(0.0, self.t)
                .max(burst(0.011, self.t))
                .max(burst(0.022, self.t))
        } else {
            self.tail.next()
        };
        if self.t > 0.03 && self.tail.level == 0.0 {
            self.active = false;
        }
        self.bp.process(noise.next(), FilterMode::Band) * env * 2.2 * self.velocity
    }
}

/// Inharmonic square-wave clusters through a filter: hats, cymbals and cowbell.
#[derive(Clone, Copy, Default)]
struct Metal {
    phases: [f32; 6],
    freqs: [f32; 6],
    count: usize,
    filter: Svf,
    mode: FilterMode,
    env: Decay,
    noise_mix: f32,
    velocity: f32,
}

const HAT_FREQS: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

impl Metal {
    #[allow(clippy::too_many_arguments)]
    fn trigger(
        &mut self,
        freqs: &[f32],
        scale: f32,
        cutoff: f32,
        mode: FilterMode,
        decay: f32,
        noise_mix: f32,
        velocity: f32,
        sr: f32,
    ) {
        self.count = freqs.len().min(6);
        for (i, f) in freqs.iter().take(6).enumerate() {
            self.freqs[i] = f * scale;
        }
        self.filter.set(cutoff, 0.3, sr);
        self.mode = mode;
        self.env.trigger(decay, sr);
        self.noise_mix = noise_mix;
        self.velocity = velocity;
    }

    fn choke(&mut self, sr: f32) {
        if self.env.level > 0.0 {
            let l = self.env.level;
            self.env.trigger(0.01, sr);
            self.env.level = l;
        }
    }

    #[inline]
    fn next(&mut self, noise: &mut Noise, sr: f32) -> f32 {
        if self.env.level == 0.0 {
            return 0.0;
        }
        let mut s = 0.0;
        for i in 0..self.count {
            self.phases[i] = (self.phases[i] + self.freqs[i] / sr).fract();
            s += if self.phases[i] < 0.5 { 1.0 } else { -1.0 };
        }
        s = s / self.count as f32 * (1.0 - self.noise_mix) + noise.next() * self.noise_mix;
        self.filter.process(s, self.mode) * self.env.next() * self.velocity
    }
}

#[derive(Clone, Copy, Default)]
struct Rim {
    phase: f32,
    env: Decay,
    velocity: f32,
}

impl Rim {
    fn trigger(&mut self, velocity: f32, sr: f32) {
        self.env.trigger(0.025, sr);
        self.velocity = velocity;
        self.phase = 0.0;
    }

    #[inline]
    fn next(&mut self, noise: &mut Noise, tune: f32, sr: f32) -> f32 {
        if self.env.level == 0.0 {
            return 0.0;
        }
        self.phase = (self.phase + 1700.0 * tune / sr).fract();
        let e = self.env.next();
        ((TAU * self.phase).sin() * 0.7 + noise.next() * 0.3) * e * self.velocity
    }
}

#[derive(Clone, Copy, Default)]
struct Shaker {
    hp: Svf,
    env: Decay,
    attack: f32,
    velocity: f32,
}

impl Shaker {
    fn trigger(&mut self, decay: f32, velocity: f32, sr: f32) {
        self.hp.set(6000.0, 0.2, sr);
        self.env.trigger(0.09 * decay, sr);
        self.attack = 0.0;
        self.velocity = velocity;
    }

    #[inline]
    fn next(&mut self, noise: &mut Noise, sr: f32) -> f32 {
        if self.env.level == 0.0 {
            return 0.0;
        }
        self.attack = (self.attack + 1.0 / (0.008 * sr)).min(1.0);
        self.hp.process(noise.next(), FilterMode::High)
            * self.env.next()
            * self.attack
            * self.velocity
    }
}

/// The kit. Keys follow General MIDI: 36 kick, 38 snare, 39 clap, 42/44/46 hats, toms on
/// 41–50, 49 crash, 51 ride, 37 rim, 56 cowbell, 70 shaker.
pub struct DrumKit {
    pub params: DrumParams,
    sr: f32,
    noise: Noise,
    kick: Membrane,
    toms: [Membrane; 3],
    snare: Snare,
    clap: Clap,
    hat: Metal,
    open_hat: Metal,
    crash: Metal,
    ride: Metal,
    cowbell: Metal,
    rim: Rim,
    shaker: Shaker,
}

impl DrumKit {
    pub fn new(params: DrumParams, sr: f32) -> Self {
        DrumKit {
            params,
            sr,
            noise: Noise::seeded(0x9e37_79b9),
            kick: Membrane::default(),
            toms: [Membrane::default(); 3],
            snare: Snare::default(),
            clap: Clap::default(),
            hat: Metal::default(),
            open_hat: Metal::default(),
            crash: Metal::default(),
            ride: Metal::default(),
            cowbell: Metal::default(),
            rim: Rim::default(),
            shaker: Shaker::default(),
        }
    }

    pub fn set_params(&mut self, params: DrumParams) {
        self.params = params;
    }

    pub fn note(&mut self, event: &NoteEvent) {
        let NoteEvent::On { key, velocity, .. } = *event else {
            return;
        };
        let sr = self.sr;
        let v = (velocity as f32 / 127.0).powf(1.3);
        let tune = (self.params.tune / 12.0).exp2();
        let d = self.params.decay.clamp(0.1, 8.0);
        match key {
            35 | 36 => self.kick.trigger(48.0 * tune, 3.5, 0.45 * d, v, sr),
            37 => self.rim.trigger(v, sr),
            38 | 40 => self.snare.trigger(tune, d, v, sr),
            39 => self.clap.trigger(d, v, sr),
            41 | 43 | 45 => self.toms[0].trigger(95.0 * tune, 1.2, 0.4 * d, v, sr),
            47 | 48 => self.toms[1].trigger(135.0 * tune, 1.0, 0.35 * d, v, sr),
            50 => self.toms[2].trigger(185.0 * tune, 0.9, 0.3 * d, v, sr),
            42 => {
                self.open_hat.choke(sr);
                self.hat.trigger(
                    &HAT_FREQS,
                    1.6 * tune,
                    7500.0,
                    FilterMode::High,
                    0.06 * d,
                    0.25,
                    v,
                    sr,
                );
            }
            44 => {
                self.open_hat.choke(sr);
                self.hat.trigger(
                    &HAT_FREQS,
                    1.6 * tune,
                    6500.0,
                    FilterMode::High,
                    0.09 * d,
                    0.2,
                    v * 0.8,
                    sr,
                );
            }
            46 => self.open_hat.trigger(
                &HAT_FREQS,
                1.6 * tune,
                7000.0,
                FilterMode::High,
                0.45 * d,
                0.25,
                v,
                sr,
            ),
            49 | 52 | 55 | 57 => self.crash.trigger(
                &HAT_FREQS,
                1.3 * tune,
                5000.0,
                FilterMode::High,
                1.6 * d,
                0.55,
                v,
                sr,
            ),
            51 | 53 | 59 => self.ride.trigger(
                &HAT_FREQS,
                2.1 * tune,
                6000.0,
                FilterMode::Band,
                1.0 * d,
                0.15,
                v * 0.8,
                sr,
            ),
            56 => self.cowbell.trigger(
                &[540.0, 800.0],
                tune,
                900.0,
                FilterMode::Band,
                0.3 * d,
                0.0,
                v * 1.4,
                sr,
            ),
            69 | 70 | 82 => self.shaker.trigger(d, v, sr),
            _ => {}
        }
    }

    pub fn release_all(&mut self) {}

    /// Renders stereo (overwriting the buffers).
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sr;
        let g = self.params.level;
        let tune = (self.params.tune / 12.0).exp2();
        // Fixed stereo placement, like a drummer's view of the kit.
        let (hl, hr) = pan(0.3);
        let (t0l, t0r) = pan(-0.4);
        let (t1l, t1r) = pan(0.0);
        let (t2l, t2r) = pan(0.4);
        let (cl, cr) = pan(-0.35);
        let (rl, rr) = pan(0.35);
        let (sl, sr_) = pan(0.2);
        let n = &mut self.noise;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let centre = self.kick.next(n, sr) * 1.0
                + self.snare.next(n, sr) * 0.8
                + self.clap.next(n, sr) * 0.7
                + self.rim.next(n, tune, sr) * 0.5
                + self.cowbell.next(n, sr) * 0.35;
            let hats = self.hat.next(n, sr) * 0.45 + self.open_hat.next(n, sr) * 0.4;
            let t0 = self.toms[0].next(n, sr) * 0.8;
            let t1 = self.toms[1].next(n, sr) * 0.8;
            let t2 = self.toms[2].next(n, sr) * 0.8;
            let crash = self.crash.next(n, sr) * 0.35;
            let ride = self.ride.next(n, sr) * 0.3;
            let shaker = self.shaker.next(n, sr) * 0.35;
            *l = g
                * (centre * 0.707
                    + hats * hl
                    + t0 * t0l
                    + t1 * t1l
                    + t2 * t2l
                    + crash * cl
                    + ride * rl
                    + shaker * sl);
            *r = g
                * (centre * 0.707
                    + hats * hr
                    + t0 * t0r
                    + t1 * t1r
                    + t2 * t2r
                    + crash * cr
                    + ride * rr
                    + shaker * sr_);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn hit(key: u8) -> (f32, f32, usize) {
        let mut kit = DrumKit::new(DrumParams::default(), SR);
        kit.note(&NoteEvent::On {
            id: 1,
            freq: 0.0,
            key,
            velocity: 127,
        });
        let mut l = vec![0.0; 48_000];
        let mut r = vec![0.0; 48_000];
        kit.render(&mut l, &mut r);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
        let energy: f32 = l.iter().map(|x| x * x).sum::<f32>() / 48_000.0;
        let last_loud = l.iter().rposition(|x| x.abs() > 0.001).unwrap_or(0);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        (peak, energy, last_loud)
    }

    #[test]
    fn every_drum_sounds_and_stays_in_range() {
        for key in [36, 37, 38, 39, 42, 44, 46, 45, 47, 50, 49, 51, 56, 70] {
            let (peak, energy, _) = hit(key);
            assert!(peak > 0.05, "key {key} too quiet: {peak}");
            assert!(peak < 1.0, "key {key} too loud: {peak}");
            assert!(energy > 0.0);
        }
    }

    #[test]
    fn decays_are_ordered() {
        let closed = hit(42).2;
        let open = hit(46).2;
        let crash = hit(49).2;
        assert!(closed < open && open < crash, "{closed} {open} {crash}");
    }

    #[test]
    fn closed_hat_chokes_open() {
        let mut kit = DrumKit::new(DrumParams::default(), SR);
        let mut l = vec![0.0; 4800];
        let mut r = vec![0.0; 4800];
        kit.note(&NoteEvent::On {
            id: 1,
            freq: 0.0,
            key: 46,
            velocity: 127,
        });
        kit.render(&mut l, &mut r);
        kit.note(&NoteEvent::On {
            id: 2,
            freq: 0.0,
            key: 42,
            velocity: 127,
        });
        for _ in 0..3 {
            kit.render(&mut l, &mut r);
        }
        assert_eq!(kit.open_hat.env.level, 0.0);
    }
}
