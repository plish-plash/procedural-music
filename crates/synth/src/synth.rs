//! Subtractive synthesizers: a 16-voice poly synth and a mono synth with glide.

use crate::dsp::{Adsr, FilterMode, Osc, Svf, Wave};
use crate::NoteEvent;

/// Sound parameters shared by the poly and mono synths. Times in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SynthParams {
    pub wave: Wave,
    /// Second oscillator detune in cents; 0 turns it off.
    pub detune: f32,
    pub cutoff: f32,
    /// 0..1
    pub resonance: f32,
    /// Filter envelope depth in octaves.
    pub env_amount: f32,
    pub filter_decay: f32,
    pub attack: f32,
    pub decay: f32,
    /// 0..1
    pub sustain: f32,
    pub release: f32,
    /// Linear output gain.
    pub level: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        SynthParams {
            wave: Wave::Saw,
            detune: 8.0,
            cutoff: 1800.0,
            resonance: 0.2,
            env_amount: 2.0,
            filter_decay: 0.3,
            attack: 0.005,
            decay: 0.3,
            sustain: 0.7,
            release: 0.4,
            level: 0.5,
        }
    }
}

const CONTROL_RATE: u32 = 16;

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    id: u64,
    age: u64,
    velocity: f32,
    log_freq: f32,
    target_log_freq: f32,
    glide_coef: f32,
    freq: f32,
    osc1: Osc,
    osc2: Osc,
    filter: Svf,
    amp: Adsr,
    fenv: Adsr,
    counter: u32,
}

impl Voice {
    fn configure(&mut self, p: &SynthParams, sr: f32) {
        self.amp.set(p.attack, p.decay, p.sustain, p.release, sr);
        self.fenv
            .set(p.attack.min(0.05), p.filter_decay, 0.0, p.release, sr);
    }

    fn start(
        &mut self,
        id: u64,
        freq: f32,
        velocity: u8,
        p: &SynthParams,
        sr: f32,
        glide: Option<f32>,
    ) {
        self.id = id;
        self.velocity = velocity as f32 / 127.0;
        self.target_log_freq = freq.max(1.0).log2();
        match glide {
            Some(seconds) if seconds > 0.0 && self.freq > 0.0 => {
                self.glide_coef = crate::dsp::time_coef(seconds / 3.0, sr);
            }
            _ => {
                self.log_freq = self.target_log_freq;
                self.freq = freq;
                self.glide_coef = 0.0;
            }
        }
        self.configure(p, sr);
        self.amp.gate_on();
        self.fenv.gate_on();
        self.counter = 0;
    }

    /// Moves to a new pitch without retriggering the envelopes (legato).
    fn glide_to(&mut self, id: u64, freq: f32, seconds: f32, sr: f32) {
        self.id = id;
        self.target_log_freq = freq.max(1.0).log2();
        self.glide_coef = if seconds > 0.0 {
            crate::dsp::time_coef(seconds / 3.0, sr)
        } else {
            0.0
        };
        if seconds <= 0.0 {
            self.log_freq = self.target_log_freq;
            self.freq = freq;
        }
    }

    fn release(&mut self) {
        self.amp.gate_off();
        self.fenv.gate_off();
    }

    fn render_add(&mut self, p: &SynthParams, sr: f32, out: &mut [f32]) {
        if self.amp.is_idle() {
            return;
        }
        let detune = if p.detune > 0.0 {
            2f32.powf(p.detune / 1200.0)
        } else {
            0.0
        };
        let vel_gain = self.velocity * 0.75 + 0.25;
        for sample in out.iter_mut() {
            if self.log_freq != self.target_log_freq {
                self.log_freq =
                    self.target_log_freq + (self.log_freq - self.target_log_freq) * self.glide_coef;
                if (self.log_freq - self.target_log_freq).abs() < 1e-4 {
                    self.log_freq = self.target_log_freq;
                }
                self.freq = self.log_freq.exp2();
            }
            if self.counter.is_multiple_of(CONTROL_RATE) {
                let fe = self.fenv.level;
                let cutoff = p.cutoff * (p.env_amount * fe * (0.4 + 0.6 * self.velocity)).exp2();
                self.filter.set(cutoff, p.resonance, sr);
            }
            self.counter = self.counter.wrapping_add(1);
            let inc = self.freq / sr;
            let mut s = self.osc1.next(p.wave, inc);
            if detune > 0.0 {
                s = 0.5 * (s + self.osc2.next(p.wave, inc * detune));
            }
            let y = self.filter.process(s, FilterMode::Low);
            self.fenv.next();
            let a = self.amp.next();
            *sample += y * a * vel_gain;
        }
    }
}

pub const POLY_VOICES: usize = 16;

/// A 16-voice subtractive synth. Stolen voices restart from their current level, so
/// stealing doesn't click.
pub struct PolySynth {
    pub params: SynthParams,
    sr: f32,
    voices: [Voice; POLY_VOICES],
    age: u64,
}

impl PolySynth {
    pub fn new(params: SynthParams, sr: f32) -> Self {
        PolySynth {
            params,
            sr,
            voices: [Voice::default(); POLY_VOICES],
            age: 0,
        }
    }

    pub fn set_params(&mut self, params: SynthParams) {
        self.params = params;
        for v in self.voices.iter_mut() {
            // Keep each voice's current stage; just update its times and levels.
            let level = v.amp.level;
            let flevel = v.fenv.level;
            v.configure(&params, self.sr);
            v.amp.level = level;
            v.fenv.level = flevel;
        }
    }

    pub fn note(&mut self, event: &NoteEvent) {
        match *event {
            NoteEvent::On {
                id, freq, velocity, ..
            } => {
                self.age += 1;
                let i = self.pick_voice();
                let v = &mut self.voices[i];
                v.age = self.age;
                v.start(id, freq, velocity, &self.params, self.sr, None);
            }
            NoteEvent::Off { id } => {
                for v in self
                    .voices
                    .iter_mut()
                    .filter(|v| v.id == id && !v.amp.is_released())
                {
                    v.release();
                }
            }
        }
    }

    fn pick_voice(&self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| v.amp.is_idle()) {
            return i;
        }
        let released = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.amp.is_released())
            .min_by(|a, b| a.1.amp.level.total_cmp(&b.1.amp.level));
        if let Some((i, _)) = released {
            return i;
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.age)
            .map_or(0, |(i, _)| i)
    }

    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            v.release();
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| !v.amp.is_idle()).count()
    }

    /// Renders mono into `out` (overwriting it).
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let p = self.params;
        for v in self.voices.iter_mut() {
            v.render_add(&p, self.sr, out);
        }
        let gain = p.level * 0.5;
        for s in out.iter_mut() {
            *s *= gain;
        }
    }
}

/// Mono synth parameters beyond the shared ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonoParams {
    /// Glide time in seconds (0 = off).
    pub glide: f32,
    /// When on, overlapping notes glide without restarting the envelopes.
    pub legato: bool,
}

impl Default for MonoParams {
    fn default() -> Self {
        MonoParams {
            glide: 0.06,
            legato: true,
        }
    }
}

/// A one-voice synth with last-note priority and glide, for bass and leads.
pub struct MonoSynth {
    pub params: SynthParams,
    pub mono: MonoParams,
    sr: f32,
    voice: Voice,
    /// Held notes, most recent last: (id, freq, velocity).
    held: Vec<(u64, f32, u8)>,
}

impl MonoSynth {
    pub fn new(params: SynthParams, mono: MonoParams, sr: f32) -> Self {
        MonoSynth {
            params,
            mono,
            sr,
            voice: Voice::default(),
            held: Vec::with_capacity(16),
        }
    }

    pub fn set_params(&mut self, params: SynthParams, mono: MonoParams) {
        self.params = params;
        self.mono = mono;
        let (a, f) = (self.voice.amp.level, self.voice.fenv.level);
        self.voice.configure(&params, self.sr);
        self.voice.amp.level = a;
        self.voice.fenv.level = f;
    }

    pub fn note(&mut self, event: &NoteEvent) {
        match *event {
            NoteEvent::On {
                id, freq, velocity, ..
            } => {
                let sounding = !self.held.is_empty() && !self.voice.amp.is_released();
                if self.held.len() == self.held.capacity() {
                    self.held.remove(0);
                }
                self.held.push((id, freq, velocity));
                if sounding && self.mono.legato {
                    self.voice.glide_to(id, freq, self.mono.glide, self.sr);
                } else {
                    let glide = (!self.voice.amp.is_idle()).then_some(self.mono.glide);
                    self.voice
                        .start(id, freq, velocity, &self.params, self.sr, glide);
                }
            }
            NoteEvent::Off { id } => {
                let was_top = self.held.last().map(|h| h.0) == Some(id);
                self.held.retain(|h| h.0 != id);
                if !was_top {
                    return;
                }
                match self.held.last() {
                    Some(&(prev_id, freq, _)) => {
                        self.voice.glide_to(prev_id, freq, self.mono.glide, self.sr)
                    }
                    None => self.voice.release(),
                }
            }
        }
    }

    pub fn release_all(&mut self) {
        self.held.clear();
        self.voice.release();
    }

    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let p = self.params;
        self.voice.render_add(&p, self.sr, out);
        let gain = p.level * 0.6;
        for s in out.iter_mut() {
            *s *= gain;
        }
    }

    pub fn current_freq(&self) -> f32 {
        self.voice.freq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn on(id: u64, freq: f32) -> NoteEvent {
        NoteEvent::On {
            id,
            freq,
            key: 60,
            velocity: 100,
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn poly_plays_and_releases() {
        let mut s = PolySynth::new(SynthParams::default(), SR);
        let mut buf = vec![0.0; 4800];
        for (i, f) in [261.6, 329.6, 392.0].iter().enumerate() {
            s.note(&on(i as u64, *f));
        }
        s.render(&mut buf);
        assert_eq!(s.active_voices(), 3);
        assert!(rms(&buf) > 0.02, "{}", rms(&buf));
        assert!(buf.iter().all(|x| x.is_finite() && x.abs() < 1.0));
        for i in 0..3 {
            s.note(&NoteEvent::Off { id: i });
        }
        for _ in 0..20 {
            s.render(&mut buf);
        }
        assert_eq!(s.active_voices(), 0);
        assert!(rms(&buf) < 1e-4);
    }

    #[test]
    fn poly_steals_when_full() {
        let mut s = PolySynth::new(SynthParams::default(), SR);
        for i in 0..20 {
            s.note(&on(i, 220.0 + i as f32));
        }
        assert_eq!(s.active_voices(), POLY_VOICES);
        let mut buf = vec![0.0; 1024];
        s.render(&mut buf);
        assert!(buf.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn mono_glides_and_returns_to_held_note() {
        let mut s = MonoSynth::new(
            SynthParams::default(),
            MonoParams {
                glide: 0.05,
                legato: true,
            },
            SR,
        );
        let mut buf = vec![0.0; 480];
        s.note(&on(1, 110.0));
        s.render(&mut buf);
        assert!((s.current_freq() - 110.0).abs() < 0.01);
        s.note(&on(2, 220.0));
        s.render(&mut buf); // 10 ms: partway
        let mid = s.current_freq();
        assert!(mid > 115.0 && mid < 215.0, "{mid}");
        for _ in 0..20 {
            s.render(&mut buf);
        }
        assert!((s.current_freq() - 220.0).abs() < 0.5);
        s.note(&NoteEvent::Off { id: 2 });
        for _ in 0..20 {
            s.render(&mut buf);
        }
        assert!(
            (s.current_freq() - 110.0).abs() < 0.5,
            "back to the held note"
        );
        s.note(&NoteEvent::Off { id: 1 });
        for _ in 0..60 {
            s.render(&mut buf);
        }
        assert!(rms(&buf) < 1e-4);
    }
}
