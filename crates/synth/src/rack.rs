//! The rack: a graph of instruments and effects that renders stereo audio, driven by
//! timed note events.
//!
//! The rack is rebuilt from a [`RackDesc`] whenever the patch changes; modules whose id
//! and kind are unchanged keep their state (sounding voices, delay and reverb tails).

use std::collections::VecDeque;
use std::mem::{discriminant, Discriminant};

use crate::drums::{DrumKit, DrumParams};
use crate::fx::{
    self, ChannelParams, Chorus, ChorusParams, Delay, DelayParams, DriveParams, FilterFx,
    FilterParams, Reverb, ReverbParams,
};
use crate::synth::{MonoParams, MonoSynth, PolySynth, SynthParams};
use crate::NoteEvent;

/// Largest number of frames processed between event checks (1.3 ms at 48 kHz).
pub const BLOCK: usize = 64;

/// A note event for one module, at a time in seconds on the shared clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedEvent {
    pub at: f64,
    pub module: u64,
    pub event: NoteEvent,
}

/// What a module is and its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum ModuleKind {
    Poly(SynthParams),
    Mono(SynthParams, MonoParams),
    Drums(DrumParams),
    Mixer([ChannelParams; 4]),
    Filter(FilterParams),
    Delay(DelayParams),
    Reverb(ReverbParams),
    Chorus(ChorusParams),
    Drive(DriveParams),
    /// Sends its input to the speakers, with a linear gain.
    Output {
        level: f32,
    },
}

impl ModuleKind {
    pub fn is_instrument(&self) -> bool {
        matches!(
            self,
            ModuleKind::Poly(_) | ModuleKind::Mono(..) | ModuleKind::Drums(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModuleDesc {
    /// Stable id (a hash of the node id), so events and state follow the node across edits.
    pub id: u64,
    pub kind: ModuleKind,
    /// The module feeding each audio input, by id.
    pub inputs: Vec<Option<u64>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RackDesc {
    pub modules: Vec<ModuleDesc>,
}

/// Messages from the scheduler to the audio thread.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioMsg {
    Rack(RackDesc),
    Tempo(f64),
    Events(Vec<TimedEvent>),
    /// Drop queued events and release every voice.
    Stop,
    /// Drop queued events for these modules and release their voices.
    Silence(Vec<u64>),
}

enum Proc {
    Poly(Box<PolySynth>),
    Mono(Box<MonoSynth>),
    Drums(Box<DrumKit>),
    Mixer([ChannelParams; 4]),
    Filter(Box<FilterFx>),
    Delay(Box<Delay>),
    Reverb(Box<Reverb>),
    Chorus(Box<Chorus>),
    Drive(DriveParams),
    Output(f32),
}

impl Proc {
    fn new(kind: &ModuleKind, sr: f32) -> Proc {
        match kind {
            ModuleKind::Poly(p) => Proc::Poly(Box::new(PolySynth::new(*p, sr))),
            ModuleKind::Mono(p, m) => Proc::Mono(Box::new(MonoSynth::new(*p, *m, sr))),
            ModuleKind::Drums(p) => Proc::Drums(Box::new(DrumKit::new(*p, sr))),
            ModuleKind::Mixer(c) => Proc::Mixer(*c),
            ModuleKind::Filter(p) => Proc::Filter(Box::new(FilterFx::new(*p, sr))),
            ModuleKind::Delay(p) => Proc::Delay(Box::new(Delay::new(*p, sr))),
            ModuleKind::Reverb(p) => Proc::Reverb(Box::new(Reverb::new(*p, sr))),
            ModuleKind::Chorus(p) => Proc::Chorus(Box::new(Chorus::new(*p, sr))),
            ModuleKind::Drive(p) => Proc::Drive(*p),
            ModuleKind::Output { level } => Proc::Output(*level),
        }
    }

    fn update(&mut self, kind: &ModuleKind) {
        match (self, kind) {
            (Proc::Poly(s), ModuleKind::Poly(p)) => s.set_params(*p),
            (Proc::Mono(s), ModuleKind::Mono(p, m)) => s.set_params(*p, *m),
            (Proc::Drums(s), ModuleKind::Drums(p)) => s.set_params(*p),
            (Proc::Mixer(c), ModuleKind::Mixer(n)) => *c = *n,
            (Proc::Filter(f), ModuleKind::Filter(p)) => f.set_params(*p),
            (Proc::Delay(d), ModuleKind::Delay(p)) => d.set_params(*p),
            (Proc::Reverb(r), ModuleKind::Reverb(p)) => r.set_params(*p),
            (Proc::Chorus(c), ModuleKind::Chorus(p)) => c.set_params(*p),
            (Proc::Drive(d), ModuleKind::Drive(p)) => *d = *p,
            (Proc::Output(l), ModuleKind::Output { level }) => *l = *level,
            _ => {}
        }
    }

    fn note(&mut self, e: &NoteEvent) {
        match self {
            Proc::Poly(s) => s.note(e),
            Proc::Mono(s) => s.note(e),
            Proc::Drums(s) => s.note(e),
            _ => {}
        }
    }

    fn release_all(&mut self) {
        match self {
            Proc::Poly(s) => s.release_all(),
            Proc::Mono(s) => s.release_all(),
            Proc::Drums(s) => s.release_all(),
            _ => {}
        }
    }
}

struct Slot {
    id: u64,
    kind: Discriminant<ModuleKind>,
    proc: Proc,
    inputs: Vec<Option<usize>>,
}

#[derive(Clone)]
struct Stereo {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl Stereo {
    fn new() -> Self {
        Stereo {
            l: vec![0.0; BLOCK],
            r: vec![0.0; BLOCK],
        }
    }
}

pub struct Rack {
    sr: f32,
    bpm: f64,
    slots: Vec<Slot>,
    order: Vec<usize>,
    bufs: Vec<Stereo>,
    queue: VecDeque<TimedEvent>,
    scratch: Vec<f32>,
}

impl Rack {
    pub fn new(sample_rate: f32) -> Rack {
        Rack {
            sr: sample_rate,
            bpm: 120.0,
            slots: Vec::new(),
            order: Vec::new(),
            bufs: Vec::new(),
            queue: VecDeque::with_capacity(1024),
            scratch: vec![0.0; BLOCK],
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    pub fn module_count(&self) -> usize {
        self.slots.len()
    }

    pub fn queued_events(&self) -> usize {
        self.queue.len()
    }

    pub fn handle(&mut self, msg: AudioMsg) {
        match msg {
            AudioMsg::Rack(desc) => self.update(&desc),
            AudioMsg::Tempo(bpm) => self.bpm = bpm.clamp(20.0, 400.0),
            AudioMsg::Events(events) => {
                let sorted = self
                    .queue
                    .back()
                    .is_none_or(|last| events.first().is_none_or(|e| e.at >= last.at));
                self.queue.extend(events);
                if !sorted
                    || !self
                        .queue
                        .iter()
                        .zip(self.queue.iter().skip(1))
                        .all(|(a, b)| a.at <= b.at)
                {
                    self.queue
                        .make_contiguous()
                        .sort_by(|a, b| a.at.total_cmp(&b.at));
                }
            }
            AudioMsg::Stop => {
                self.queue.clear();
                for s in self.slots.iter_mut() {
                    s.proc.release_all();
                }
            }
            AudioMsg::Silence(ids) => {
                self.queue.retain(|e| !ids.contains(&e.module));
                for s in self.slots.iter_mut().filter(|s| ids.contains(&s.id)) {
                    s.proc.release_all();
                }
            }
        }
    }

    fn update(&mut self, desc: &RackDesc) {
        let mut old: Vec<Option<Slot>> = std::mem::take(&mut self.slots)
            .into_iter()
            .map(Some)
            .collect();
        let mut slots = Vec::with_capacity(desc.modules.len());
        for m in &desc.modules {
            let d = discriminant(&m.kind);
            let reused = old
                .iter_mut()
                .find(|s| s.as_ref().is_some_and(|s| s.id == m.id && s.kind == d))
                .and_then(Option::take);
            let proc = match reused {
                Some(mut s) => {
                    s.proc.update(&m.kind);
                    s.proc
                }
                None => Proc::new(&m.kind, self.sr),
            };
            slots.push(Slot {
                id: m.id,
                kind: d,
                proc,
                inputs: Vec::new(),
            });
        }
        for (slot, m) in slots.iter_mut().zip(&desc.modules) {
            slot.inputs = m
                .inputs
                .iter()
                .map(|i| i.and_then(|id| desc.modules.iter().position(|o| o.id == id)))
                .collect();
        }
        // Topological order; modules in a feedback loop are left out.
        let n = slots.len();
        let mut indegree: Vec<usize> = slots
            .iter()
            .map(|s| s.inputs.iter().flatten().count())
            .collect();
        let mut order = Vec::with_capacity(n);
        let mut ready: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
        while let Some(i) = ready.pop() {
            order.push(i);
            for (j, s) in slots.iter().enumerate() {
                for &src in s.inputs.iter().flatten() {
                    if src == i {
                        indegree[j] -= 1;
                        if indegree[j] == 0 {
                            ready.push(j);
                        }
                    }
                }
            }
        }
        self.slots = slots;
        self.order = order;
        self.bufs = vec![Stereo::new(); n];
    }

    /// Renders interleaved stereo starting at `start` seconds on the shared clock.
    /// Events due before the end of each 64-frame block are applied at its start.
    pub fn render(&mut self, start: f64, out: &mut [f32]) {
        let frames = out.len() / 2;
        let mut pos = 0;
        while pos < frames {
            let n = BLOCK.min(frames - pos);
            let block_end = start + (pos + n) as f64 / self.sr as f64;
            while self.queue.front().is_some_and(|e| e.at < block_end) {
                let e = self.queue.pop_front().expect("peeked");
                if let Some(slot) = self.slots.iter_mut().find(|s| s.id == e.module) {
                    slot.proc.note(&e.event);
                }
            }
            self.process_block(n);
            let mut l = [0.0f32; BLOCK];
            let mut r = [0.0f32; BLOCK];
            for (i, slot) in self.slots.iter().enumerate() {
                if matches!(slot.proc, Proc::Output(_)) && self.order.contains(&i) {
                    for k in 0..n {
                        l[k] += self.bufs[i].l[k];
                        r[k] += self.bufs[i].r[k];
                    }
                }
            }
            for k in 0..n {
                // Gentle saturation keeps overs from clipping hard.
                out[2 * (pos + k)] = soft_clip(l[k]);
                out[2 * (pos + k) + 1] = soft_clip(r[k]);
            }
            pos += n;
        }
    }

    fn process_block(&mut self, n: usize) {
        for oi in 0..self.order.len() {
            let i = self.order[oi];
            let mut buf = std::mem::replace(
                &mut self.bufs[i],
                Stereo {
                    l: Vec::new(),
                    r: Vec::new(),
                },
            );
            let (l, r) = (&mut buf.l[..n], &mut buf.r[..n]);
            let slot = &mut self.slots[i];
            match &mut slot.proc {
                Proc::Poly(s) => {
                    s.render(&mut self.scratch[..n]);
                    l.copy_from_slice(&self.scratch[..n]);
                    r.copy_from_slice(&self.scratch[..n]);
                }
                Proc::Mono(s) => {
                    s.render(&mut self.scratch[..n]);
                    l.copy_from_slice(&self.scratch[..n]);
                    r.copy_from_slice(&self.scratch[..n]);
                }
                Proc::Drums(s) => s.render(l, r),
                Proc::Mixer(channels) => {
                    l.fill(0.0);
                    r.fill(0.0);
                    for (k, input) in slot.inputs.iter().enumerate().take(4) {
                        if let Some(src) = *input {
                            let b = &self.bufs[src];
                            fx::mix_into(l, r, &b.l[..n], &b.r[..n], channels[k]);
                        }
                    }
                }
                other => {
                    l.fill(0.0);
                    r.fill(0.0);
                    for src in slot.inputs.iter().flatten() {
                        let b = &self.bufs[*src];
                        for k in 0..n {
                            l[k] += b.l[k];
                            r[k] += b.r[k];
                        }
                    }
                    match other {
                        Proc::Filter(f) => f.process(l, r),
                        Proc::Delay(d) => d.process(l, r, self.bpm),
                        Proc::Reverb(rv) => rv.process(l, r),
                        Proc::Chorus(c) => c.process(l, r),
                        Proc::Drive(p) => fx::drive(l, r, *p),
                        Proc::Output(level) => {
                            for x in l.iter_mut().chain(r.iter_mut()) {
                                *x *= *level;
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.bufs[i] = buf;
        }
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    // Linear up to 0.8, then smoothly into ±1.
    let a = x.abs();
    if a <= 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn desc() -> RackDesc {
        RackDesc {
            modules: vec![
                ModuleDesc {
                    id: 1,
                    kind: ModuleKind::Poly(SynthParams::default()),
                    inputs: vec![],
                },
                ModuleDesc {
                    id: 2,
                    kind: ModuleKind::Reverb(ReverbParams::default()),
                    inputs: vec![Some(1)],
                },
                ModuleDesc {
                    id: 3,
                    kind: ModuleKind::Output { level: 1.0 },
                    inputs: vec![Some(2)],
                },
            ],
        }
    }

    fn on(at: f64, id: u64) -> TimedEvent {
        TimedEvent {
            at,
            module: 1,
            event: NoteEvent::On {
                id,
                freq: 220.0,
                key: 57,
                velocity: 110,
            },
        }
    }

    fn first_sound(buf: &[f32]) -> Option<usize> {
        buf.chunks(2).position(|f| f[0].abs() > 1e-4)
    }

    #[test]
    fn events_land_within_a_block() {
        let mut rack = Rack::new(SR);
        rack.handle(AudioMsg::Rack(desc()));
        rack.handle(AudioMsg::Events(vec![on(0.1, 1)]));
        let mut out = vec![0.0; 2 * 9600];
        rack.render(0.0, &mut out);
        let at = first_sound(&out).expect("sound");
        assert!((at as i64 - 4800).abs() <= BLOCK as i64, "{at}");
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }

    #[test]
    fn state_survives_param_edits_but_not_kind_changes() {
        let mut rack = Rack::new(SR);
        rack.handle(AudioMsg::Rack(desc()));
        rack.handle(AudioMsg::Events(vec![on(0.0, 1)]));
        let mut out = vec![0.0; 2 * 4800];
        rack.render(0.0, &mut out);
        // Change the synth's cutoff: the note keeps sounding.
        let mut d = desc();
        d.modules[0].kind = ModuleKind::Poly(SynthParams {
            cutoff: 600.0,
            ..SynthParams::default()
        });
        rack.handle(AudioMsg::Rack(d.clone()));
        rack.render(0.1, &mut out);
        assert!(out.iter().any(|x| x.abs() > 1e-3));
        // Replace it with a mono synth: a fresh instrument, so (after the reverb tail) silence.
        d.modules[0].kind = ModuleKind::Mono(SynthParams::default(), MonoParams::default());
        d.modules[1].kind = ModuleKind::Filter(FilterParams::default());
        rack.handle(AudioMsg::Rack(d));
        rack.render(0.2, &mut out);
        assert!(out[2 * 2000..].iter().all(|x| x.abs() < 1e-3));
    }

    #[test]
    fn stop_clears_queue_and_releases() {
        let mut rack = Rack::new(SR);
        rack.handle(AudioMsg::Rack(desc()));
        rack.handle(AudioMsg::Events(vec![on(0.0, 1), on(5.0, 2)]));
        let mut out = vec![0.0; 2 * 4800];
        rack.render(0.0, &mut out);
        rack.handle(AudioMsg::Stop);
        assert_eq!(rack.queued_events(), 0);
    }

    #[test]
    fn silence_drops_only_its_modules() {
        let mut rack = Rack::new(SR);
        rack.handle(AudioMsg::Rack(desc()));
        let mut other = on(5.0, 3);
        other.module = 9;
        rack.handle(AudioMsg::Events(vec![on(4.0, 1), other, on(6.0, 2)]));
        rack.handle(AudioMsg::Silence(vec![1]));
        assert_eq!(rack.queued_events(), 1);
    }

    #[test]
    fn mixer_levels_and_unsorted_events() {
        let d = RackDesc {
            modules: vec![
                ModuleDesc {
                    id: 1,
                    kind: ModuleKind::Drums(DrumParams::default()),
                    inputs: vec![],
                },
                ModuleDesc {
                    id: 2,
                    kind: ModuleKind::Mixer(
                        [ChannelParams {
                            level: 0.0,
                            pan: 0.0,
                        }; 4],
                    ),
                    inputs: vec![Some(1), None, None, None],
                },
                ModuleDesc {
                    id: 3,
                    kind: ModuleKind::Output { level: 1.0 },
                    inputs: vec![Some(2)],
                },
            ],
        };
        let mut rack = Rack::new(SR);
        rack.handle(AudioMsg::Rack(d));
        let kick = |at| TimedEvent {
            at,
            module: 1,
            event: NoteEvent::On {
                id: 0,
                freq: 0.0,
                key: 36,
                velocity: 127,
            },
        };
        rack.handle(AudioMsg::Events(vec![kick(0.05)]));
        rack.handle(AudioMsg::Events(vec![kick(0.01)]));
        let mut out = vec![0.0; 2 * 4800];
        rack.render(0.0, &mut out);
        assert!(
            out.iter().all(|x| *x == 0.0),
            "a muted mixer channel passes nothing"
        );
    }

    #[test]
    fn soft_clip_is_continuous_and_bounded() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert!((soft_clip(0.8) - 0.8).abs() < 1e-6);
        assert!(soft_clip(10.0) <= 1.0 && soft_clip(-10.0) >= -1.0);
    }
}
