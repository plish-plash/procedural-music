//! Building blocks: oscillators, filters, envelopes, noise, smoothing.

use std::f32::consts::PI;

pub const TAU: f32 = 2.0 * PI;

/// Decibels to linear gain.
pub fn db(x: f32) -> f32 {
    if x <= -60.0 {
        0.0
    } else {
        10f32.powf(x / 20.0)
    }
}

/// Coefficient for a one-pole smoother/decay reaching ~1/e after `seconds`.
pub fn time_coef(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 {
        0.0
    } else {
        (-1.0 / (seconds * sample_rate)).exp()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Wave {
    #[default]
    Saw,
    Square,
    Triangle,
    Sine,
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// A band-limited (polyBLEP) oscillator.
#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub phase: f32,
}

impl Osc {
    /// One sample; `inc` is frequency / sample rate.
    #[inline]
    pub fn next(&mut self, wave: Wave, inc: f32) -> f32 {
        let t = self.phase;
        let dt = inc.clamp(1e-6, 0.5);
        let v = match wave {
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let naive = if t < 0.5 { 1.0 } else { -1.0 };
                let t2 = if t + 0.5 >= 1.0 { t - 0.5 } else { t + 0.5 };
                naive + poly_blep(t, dt) - poly_blep(t2, dt)
            }
            Wave::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
            Wave::Sine => (TAU * t).sin(),
        };
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    Low,
    High,
    Band,
}

/// Zero-delay-feedback state-variable filter (Simper's form). Stable under fast modulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    /// `resonance` 0..1 (1 is close to self-oscillation).
    pub fn set(&mut self, cutoff: f32, resonance: f32, sample_rate: f32) {
        let fc = cutoff.clamp(10.0, sample_rate * 0.45);
        let g = (PI * fc / sample_rate).tan();
        self.k = 2.0 - 1.95 * resonance.clamp(0.0, 1.0);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Returns (low, band, high).
    #[inline]
    pub fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - self.k * v1 - v2)
    }

    #[inline]
    pub fn process(&mut self, x: f32, mode: FilterMode) -> f32 {
        let (l, b, h) = self.tick(x);
        match mode {
            FilterMode::Low => l,
            FilterMode::Band => b,
            FilterMode::High => h,
        }
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Attack-decay-sustain-release envelope. Times in seconds, sustain 0..1.
/// Attack is linear; decay and release are exponential.
#[derive(Clone, Copy, Debug, Default)]
pub struct Adsr {
    stage: Stage,
    pub level: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    sustain: f32,
}

impl Adsr {
    pub fn set(&mut self, attack: f32, decay: f32, sustain: f32, release: f32, sample_rate: f32) {
        self.attack_step = 1.0 / (attack.max(0.0005) * sample_rate);
        // Seven time constants: within 0.1% (−60 dB) of the target at the stated time.
        self.decay_coef = time_coef(decay.max(0.001) / 7.0, sample_rate);
        self.release_coef = time_coef(release.max(0.001) / 7.0, sample_rate);
        self.sustain = sustain.clamp(0.0, 1.0);
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// A fast fade for stolen or stopped voices.
    pub fn kill(&mut self, sample_rate: f32) {
        self.release_coef = time_coef(0.003, sample_rate);
        self.gate_off();
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn is_released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Idle)
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < 1e-3 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Exponential decay from 1 toward 0, retriggerable.
#[derive(Clone, Copy, Debug, Default)]
pub struct Decay {
    pub level: f32,
    coef: f32,
}

impl Decay {
    pub fn trigger(&mut self, seconds: f32, sample_rate: f32) {
        self.level = 1.0;
        self.coef = time_coef(seconds / 4.0, sample_rate);
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let v = self.level;
        self.level *= self.coef;
        if self.level < 1e-5 {
            self.level = 0.0;
        }
        v
    }
}

/// Fast white noise (xorshift32).
#[derive(Clone, Copy, Debug)]
pub struct Noise(u32);

impl Default for Noise {
    fn default() -> Self {
        Noise(0x1234_5678)
    }
}

impl Noise {
    pub fn seeded(seed: u32) -> Self {
        Noise(seed.max(1))
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole smoother for parameters, to avoid zipper noise.
#[derive(Clone, Copy, Debug, Default)]
pub struct Smooth {
    pub value: f32,
    pub target: f32,
    coef: f32,
}

impl Smooth {
    pub fn new(value: f32, seconds: f32, sample_rate: f32) -> Self {
        Smooth {
            value,
            target: value,
            coef: time_coef(seconds, sample_rate),
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value = self.target + (self.value - self.target) * self.coef;
        self.value
    }
}

/// Equal-power pan: -1 left, 0 centre, 1 right. Returns (left gain, right gain).
pub fn pan(p: f32) -> (f32, f32) {
    let a = (p.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (a.cos(), a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn oscillators_stay_in_range() {
        for wave in [Wave::Saw, Wave::Square, Wave::Triangle, Wave::Sine] {
            let mut o = Osc::default();
            let mut mean = 0.0;
            for _ in 0..48_000 {
                let v = o.next(wave, 440.0 / SR);
                assert!(v.abs() <= 1.2, "{wave:?} {v}");
                mean += v;
            }
            assert!((mean / 48_000.0).abs() < 0.02, "{wave:?} DC {mean}");
        }
    }

    fn tone_gain(filter_cutoff: f32, freq: f32) -> f32 {
        let mut f = Svf::default();
        f.set(filter_cutoff, 0.0, SR);
        let mut o = Osc::default();
        let mut peak: f32 = 0.0;
        for i in 0..24_000 {
            let y = f.process(o.next(Wave::Sine, freq / SR), FilterMode::Low);
            if i > 12_000 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_passes_lows_and_cuts_highs() {
        assert!(tone_gain(1000.0, 100.0) > 0.95);
        assert!(tone_gain(1000.0, 8000.0) < 0.03);
    }

    #[test]
    fn adsr_shape() {
        let mut e = Adsr::default();
        e.set(0.01, 0.1, 0.5, 0.2, SR);
        e.gate_on();
        let mut peak = 0.0f32;
        for _ in 0..(0.01 * SR) as usize + 2 {
            peak = peak.max(e.next());
        }
        assert!((peak - 1.0).abs() < 1e-3);
        for _ in 0..(0.2 * SR) as usize {
            e.next();
        }
        assert!((e.level - 0.5).abs() < 0.02, "{}", e.level);
        e.gate_off();
        for _ in 0..(0.25 * SR) as usize {
            e.next();
        }
        assert!(e.is_idle());
    }

    #[test]
    fn pan_is_equal_power() {
        let (l, r) = pan(0.0);
        assert!((l * l + r * r - 1.0).abs() < 1e-6);
        assert!(pan(-1.0).1.abs() < 1e-6);
    }
}
