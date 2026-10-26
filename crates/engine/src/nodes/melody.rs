//! Note generators: arpeggios, sequences, random walks, chords, bass and drums.

use pm_theory::{Chord, Note, Onset, Pitch, Rhythm, Scale, Ticks};

use super::theory::node;
use super::{default_pulse, gate, onsets_between, scale_velocity, Inputs, NodeImpl};
use crate::graph::EvalContext;
use crate::rng;
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

const GATE: ParamSpec = ParamSpec {
    name: "gate",
    label: "Gate %",
    kind: ParamKind::Number {
        min: 5.0,
        max: 200.0,
        step: 5.0,
        default: 90.0,
    },
};
const VELOCITY: ParamSpec = ParamSpec {
    name: "velocity",
    label: "Velocity %",
    kind: ParamKind::Number {
        min: 0.0,
        max: 150.0,
        step: 5.0,
        default: 100.0,
    },
};

/// Calls `f(time, onset)` for every onset of the rhythm input (or a 1/16 pulse) in the window.
fn each_onset(
    ins: &Inputs,
    rhythm_port: usize,
    ctx: &EvalContext,
    mut f: impl FnMut(Ticks, &Onset, &Rhythm),
) {
    let pulse;
    let rhythm = match ins.rhythm(rhythm_port) {
        Some(r) => r,
        None => {
            pulse = default_pulse();
            &pulse
        }
    };
    rhythm.for_each_in(ctx.start, ctx.end, |t, o| f(t, o, rhythm));
}

fn notes(v: Vec<Note>) -> Result<Vec<Option<Value>>, String> {
    Ok(vec![Some(Value::Notes(v))])
}

// ---- Arpeggiator -------------------------------------------------------------

pub static ARP: NodeSpec = NodeSpec {
    kind: "arp",
    title: "Arpeggiator",
    category: "Melody",
    help: "Plays each chord's tones one per rhythm hit, from its root in the chosen octave and up \
           through Octaves. The pattern restarts whenever the chord changes. To run up and down a \
           scale, use the Degree sequencer.",
    inputs: &[port("chords", PortType::Chord), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "pattern",
            label: "Pattern",
            kind: ParamKind::Choice { options: &["Up", "Down", "Up-down", "Random"], default: "Up" },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "range", label: "Octaves", kind: ParamKind::Number { min: 1.0, max: 4.0, step: 1.0, default: 1.0 } },
        GATE,
        VELOCITY,
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: false } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
    ],
};

pub fn build_arp(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let pattern = p.text("pattern");
    let octave = p.int("octave");
    let range = p.int("range").max(1) as usize;
    let lock = p.flag("lock");
    let seed = p.number("seed") as u64;
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        // Each chord's tones as a scale from its root (a "chord scale").
        let scales = chords.map(|c| c.to_scale());
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, rhythm| {
            let seg = scales.at(t);
            let Some(scale) = seg.value else {
                return;
            };
            let total = scale.len() * range + 1; // include the top root
            if scale.is_empty() {
                return;
            }
            let k = onsets_between(rhythm, seg.start, t) as usize;
            let i = match pattern.as_str() {
                "Down" => total - 1 - k % total,
                "Up-down" if total > 1 => {
                    let period = 2 * total - 2;
                    let i = k % period;
                    if i < total {
                        i
                    } else {
                        period - i
                    }
                }
                "Random" => {
                    let s = rng::stream(lock, seed, ctx.salt);
                    rng::below(rng::hash(&[s, t as u64]), total as u64) as usize
                }
                _ => k % total,
            };
            out.push(Note {
                start: t,
                duration: gate(onset.duration, gate_pct),
                pitch: scale.pitch(i as i32, octave),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

// ---- Degree sequencer ----------------------------------------------------------

pub static SEQUENCER: NodeSpec = NodeSpec {
    kind: "sequencer",
    title: "Degree sequencer",
    category: "Melody",
    help: "Steps through a list of scale degrees, one per rhythm hit (1 is the root; 8 the root an octave up; \
           negative numbers go below). '-' is a rest.",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "degrees", label: "Degrees", kind: ParamKind::Text { default: "1 3 5 3 6 5 4 2" } },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_sequencer(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let text = p.text("degrees");
    let mut degrees: Vec<Option<i32>> = Vec::new();
    for token in text.split_whitespace() {
        if token == "-" {
            degrees.push(None);
            continue;
        }
        let d: i32 = token
            .parse()
            .map_err(|_| format!("'{token}' is not a degree number"))?;
        if d == 0 {
            return Err("degrees count from 1; there is no degree 0".into());
        }
        // 1-based to 0-based; -1 is the degree just below the root.
        degrees.push(Some(if d > 0 { d - 1 } else { d }));
    }
    if degrees.is_empty() {
        return Err("enter some degrees, like 1 3 5".into());
    }
    let octave = p.int("octave");
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, rhythm| {
            let k = onsets_between(rhythm, 0, t) as usize;
            if let Some(d) = degrees[k % degrees.len()] {
                out.push(Note {
                    start: t,
                    duration: gate(onset.duration, gate_pct),
                    pitch: scales.at(t).value.pitch(d, octave),
                    velocity: scale_velocity(onset.velocity, vel),
                    voice: 0,
                });
            }
        });
        notes(out)
    })
}

// ---- Random walk ---------------------------------------------------------------

pub static RANDOM_WALK: NodeSpec = NodeSpec {
    kind: "random_walk",
    title: "Random walk",
    category: "Melody",
    help: "A melody that wanders through the scale by small random steps, restarting from the root every phrase.",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "max_step", label: "Max step", kind: ParamKind::Number { min: 1.0, max: 5.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "range", label: "Octaves", kind: ParamKind::Number { min: 1.0, max: 3.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "phrase", label: "Phrase bars", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: false } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_random_walk(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let max_step = p.int("max_step").max(1) as u64;
    let octave = p.int("octave");
    let range = p.int("range").max(1);
    let phrase_bars = p.int("phrase").max(1) as u32;
    let lock = p.flag("lock");
    let seed = p.number("seed") as u64;
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let phrase = ctx.meter.ticks_per_bar() * phrase_bars;
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, rhythm| {
            let scale: &Scale = scales.at(t).value;
            let n = scale.len() as i32;
            let (lo, hi) = (-(range * n) / 2, (range * n) / 2);
            let origin = t - t % phrase;
            let phrase_seed = rng::hash(&[rng::stream(lock, seed, ctx.salt), (t / phrase) as u64]);
            let k = onsets_between(rhythm, origin, t);
            let mut pos = 0i32;
            for i in 1..=k {
                let step = rng::below(rng::hash(&[phrase_seed, i]), 2 * max_step + 1) as i32
                    - max_step as i32;
                pos += step;
                // Reflect off the edges of the range.
                if pos > hi {
                    pos = 2 * hi - pos;
                }
                if pos < lo {
                    pos = 2 * lo - pos;
                }
                pos = pos.clamp(lo, hi);
            }
            out.push(Note {
                start: t,
                duration: gate(onset.duration, gate_pct),
                pitch: scale.pitch(pos, octave),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

/// Parses degrees such as "1 3 2 -2" (1-based, negative below the root) to 0-based
/// numbers; '-' and '|' are skipped.
fn parse_degrees(text: &str) -> Result<Vec<i32>, String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        if token == "-" || token == "|" {
            continue;
        }
        let d: i32 = token.parse().map_err(|_| format!("'{token}' is not a degree number"))?;
        if d == 0 {
            return Err("degrees count from 1; there is no degree 0".into());
        }
        out.push(if d > 0 { d - 1 } else { d });
    }
    Ok(out)
}

/// A note's degree in the scale counted from the root of `octave` (0 is that root), after
/// snapping it into the scale.
fn degree_in(scale: &Scale, pitch: Pitch, octave: i32) -> Option<i32> {
    if scale.tuning() != pitch.tuning() || scale.is_empty() {
        return None;
    }
    let (idx, oct) = scale.degree_of(scale.quantize(pitch, pm_theory::Rounding::Nearest))?;
    Some(idx as i32 + scale.len() as i32 * (oct - octave))
}

// ---- Markov ----------------------------------------------------------------------

pub static MARKOV: NodeSpec = NodeSpec {
    kind: "markov",
    title: "Markov",
    category: "Melody",
    help: "Learns which scale degree tends to follow which from a training melody (typed as degrees, or \
           the notes wired into Train, from the first Learn bars), then improvises new phrases in that \
           style, one note per rhythm hit. Order 2 looks at the last two notes, so it copies longer \
           figures.",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm), optional("train", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "degrees", label: "Training", kind: ParamKind::Text { default: "1 2 3 1 3 4 5 5 6 5 4 3 2 1" } },
        ParamSpec { name: "learn", label: "Learn bars", kind: ParamKind::Number { min: 1.0, max: 32.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "order", label: "Order", kind: ParamKind::Number { min: 1.0, max: 2.0, step: 1.0, default: 1.0 } },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "phrase", label: "Phrase bars", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: false } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
        GATE,
        VELOCITY,
    ],
};

/// Transitions learned from a sequence of degrees: for each context of `order` degrees (or
/// one, as a fallback), the degrees that followed it, with repeats for frequency. The
/// sequence is treated as a loop, so every context has a follower.
struct Chain {
    start: i32,
    order: usize,
    next: std::collections::HashMap<Vec<i32>, Vec<i32>>,
}

impl Chain {
    fn learn(seq: &[i32], order: usize) -> Option<Chain> {
        let n = seq.len();
        if n == 0 {
            return None;
        }
        let mut next: std::collections::HashMap<Vec<i32>, Vec<i32>> = std::collections::HashMap::new();
        for i in 0..n {
            for k in 1..=order.min(n) {
                let context: Vec<i32> = (0..k).map(|j| seq[(i + n - k + j) % n]).collect();
                next.entry(context).or_default().push(seq[i]);
            }
        }
        Some(Chain { start: seq[0], order, next })
    }

    /// The `k`-th note of a phrase (0 is the start), walking the chain with `seed`.
    fn walk(&self, seed: u64, k: u64) -> i32 {
        let mut history = vec![self.start];
        for i in 1..=k {
            let follow = (1..=self.order.min(history.len()))
                .rev()
                .find_map(|len| self.next.get(&history[history.len() - len..]));
            let d = match follow {
                Some(options) => options[rng::below(rng::hash(&[seed, i]), options.len() as u64) as usize],
                None => self.start,
            };
            history.push(d);
            if history.len() > self.order {
                history.remove(0);
            }
        }
        *history.last().expect("non-empty")
    }
}

struct MarkovNode {
    typed: Vec<i32>,
    learn_bars: u32,
    order: usize,
    octave: i32,
    phrase_bars: u32,
    lock: bool,
    seed: u64,
    gate_pct: f64,
    vel: f64,
}

impl NodeImpl for MarkovNode {
    fn input_window(&self, ctx: &EvalContext) -> Option<(Ticks, Ticks)> {
        // Only Train needs it; scales and rhythms are the same over any window.
        Some((0, ctx.meter.ticks_per_bar() * self.learn_bars))
    }

    fn eval(&self, ins: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let wired: Vec<i32> = {
            let mut train = ins.notes(2).to_vec();
            train.sort_by_key(|n| (n.start, std::cmp::Reverse(n.pitch)));
            train.dedup_by_key(|n| n.start); // the top note of a chord
            train.iter().filter_map(|n| degree_in(scales.at(n.start).value, n.pitch, self.octave)).collect()
        };
        let seq = if wired.is_empty() { &self.typed } else { &wired };
        let chain = Chain::learn(seq, self.order).ok_or("enter some training degrees, like 1 2 3 1")?;
        let phrase = ctx.meter.ticks_per_bar() * self.phrase_bars;
        let stream = rng::stream(self.lock, self.seed, ctx.salt);
        let mut out = Vec::new();
        // The pulse lives here so it outlives the onsets borrowed from it.
        let pulse = default_pulse();
        let rhythm = ins.rhythm(1).unwrap_or(&pulse);
        // Inputs came over the learning window; play the real one.
        rhythm.for_each_in(ctx.start, ctx.end, |t, onset| {
            let origin = t - t % phrase;
            let k = onsets_between(rhythm, origin, t);
            let d = chain.walk(rng::hash(&[stream, (t / phrase) as u64]), k);
            out.push(Note {
                start: t,
                duration: gate(onset.duration, self.gate_pct),
                pitch: scales.at(t).value.pitch(d, self.octave),
                velocity: scale_velocity(onset.velocity, self.vel),
                voice: 0,
            });
        });
        notes(out)
    }
}

pub fn build_markov(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let typed = parse_degrees(&p.text("degrees"))?;
    Ok(Box::new(MarkovNode {
        typed,
        learn_bars: p.int("learn").max(1) as u32,
        order: p.int("order").clamp(1, 2) as usize,
        octave: p.int("octave"),
        phrase_bars: p.int("phrase").max(1) as u32,
        lock: p.flag("lock"),
        seed: p.number("seed") as u64,
        gate_pct: p.number("gate"),
        vel: p.number("velocity"),
    }))
}

// ---- Contour -----------------------------------------------------------------------

pub static CONTOUR: NodeSpec = NodeSpec {
    kind: "contour",
    title: "Contour",
    category: "Melody",
    help: "A melody that follows a shape over each phrase (rising, falling, an arch…), one note per \
           rhythm hit, spanning Range scale degrees up from the root.",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "shape",
            label: "Shape",
            kind: ParamKind::Choice { options: &["Arch", "Rise", "Fall", "Valley", "Wave"], default: "Arch" },
        },
        ParamSpec { name: "range", label: "Range", kind: ParamKind::Number { min: 1.0, max: 21.0, step: 1.0, default: 7.0 } },
        ParamSpec { name: "phrase", label: "Phrase bars", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        GATE,
        VELOCITY,
    ],
};

/// The height (0 to 1) of a contour shape at `x` (0 to 1) through the phrase.
fn contour_at(shape: &str, x: f64) -> f64 {
    use std::f64::consts::PI;
    match shape {
        "Rise" => x,
        "Fall" => 1.0 - x,
        "Valley" => 1.0 - (PI * x).sin(),
        "Wave" => 0.5 - 0.5 * (4.0 * PI * x).cos(),
        _ => (PI * x).sin(),
    }
}

pub fn build_contour(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let shape = p.text("shape");
    let range = p.number("range");
    let phrase_bars = p.int("phrase").max(1) as u32;
    let octave = p.int("octave");
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let phrase = ctx.meter.ticks_per_bar() * phrase_bars;
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, _| {
            // The shape at the onset's place in the phrase.
            let x = (t % phrase) as f64 / phrase as f64;
            let d = (contour_at(&shape, x) * range).round() as i32;
            out.push(Note {
                start: t,
                duration: gate(onset.duration, gate_pct),
                pitch: scales.at(t).value.pitch(d, octave),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

// ---- Motif transform -------------------------------------------------------------------

pub static MOTIF: NodeSpec = NodeSpec {
    kind: "motif",
    title: "Motif transform",
    category: "Melody",
    help: "Transforms the notes of each phrase: Retrograde plays it backwards, Inversion turns it \
           upside down around its first note (in the key), Augmentation stretches its first half to \
           twice the length, Diminution squeezes it into half and plays it twice.",
    inputs: &[port("notes", PortType::Notes), port("key", PortType::Scale)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "op",
            label: "Transform",
            kind: ParamKind::Choice {
                options: &["Retrograde", "Inversion", "Augmentation", "Diminution"],
                default: "Retrograde",
            },
        },
        ParamSpec { name: "phrase", label: "Phrase bars", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 1.0 } },
    ],
};

/// Each phrase's notes transformed by `op`; `key` (when there is one) keeps inversions diatonic.
fn transform_motif(op: &str, input: &[Note], key: Option<&Timeline<Scale>>, phrase: Ticks) -> Vec<Note> {
    let mut by_phrase: std::collections::BTreeMap<Ticks, Vec<Note>> = std::collections::BTreeMap::new();
    for n in input {
        by_phrase.entry(n.start / phrase * phrase).or_default().push(*n);
    }
    let mut out = Vec::new();
    for (b, mut notes) in by_phrase {
        notes.sort_by_key(|n| (n.start, n.pitch));
        match op {
            "Inversion" => {
                let first = notes[0].pitch;
                for n in &notes {
                    let pitch = match key.map(|k| k.at(n.start).value) {
                        Some(k) if k.tuning() == n.pitch.tuning() && !k.is_empty() => {
                            let (d0, d) = (degree_in(k, first, 0).unwrap_or(0), degree_in(k, n.pitch, 0).unwrap_or(0));
                            k.pitch(2 * d0 - d, 0)
                        }
                        _ => first.offset(first.step() - n.pitch.step()),
                    };
                    out.push(Note { pitch, ..*n });
                }
            }
            "Augmentation" => {
                for n in notes.iter().filter(|n| n.start - b < phrase / 2) {
                    out.push(Note { start: b + 2 * (n.start - b), duration: n.duration * 2, ..*n });
                }
            }
            "Diminution" => {
                for half in [0, phrase / 2] {
                    for n in &notes {
                        out.push(Note { start: b + half + (n.start - b) / 2, duration: (n.duration / 2).max(1), ..*n });
                    }
                }
            }
            _ => {
                for n in &notes {
                    let end = (n.start + n.duration).min(b + phrase);
                    out.push(Note { start: b + (b + phrase - end), ..*n });
                }
            }
        }
    }
    out
}

pub fn build_motif(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let op = p.text("op");
    let bars = p.int("phrase").max(1) as Ticks;
    super::note_ops::windowed(
        move |ctx| {
            let phrase = bars * ctx.meter.ticks_per_bar();
            (ctx.start / phrase * phrase, ctx.end.saturating_sub(1) / phrase * phrase + phrase)
        },
        move |ins, ctx| Ok(transform_motif(&op, ins.notes(0), ins.scale(1), bars * ctx.meter.ticks_per_bar())),
    )
}

// ---- Play chords -----------------------------------------------------------------

pub static PLAY_CHORDS: NodeSpec = NodeSpec {
    kind: "play_chords",
    title: "Play chords",
    category: "Parts",
    help: "Plays each chord's notes together, on every rhythm hit. With no rhythm, each chord is held for its length \
           (a single chord is struck once per bar). Each note's voice number is its position in the chord, low to high.",
    inputs: &[port("chords", PortType::Chord), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[GATE, VELOCITY],
};

pub fn build_play_chords(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        let mut strike = |t: Ticks, duration: Ticks, velocity: u8, chord: &Chord| {
            for (v, &pitch) in chord.notes().iter().enumerate() {
                out.push(Note {
                    start: t,
                    duration,
                    pitch,
                    velocity,
                    voice: v as u8,
                });
            }
        };
        if ins.rhythm(1).is_some() {
            each_onset(ins, 1, ctx, |t, onset, _| {
                strike(
                    t,
                    gate(onset.duration, gate_pct),
                    scale_velocity(onset.velocity, vel),
                    chords.at(t).value,
                )
            });
        } else {
            for seg in chords.starts_in(ctx.start, ctx.end, ctx.meter.ticks_per_bar()) {
                strike(
                    seg.start,
                    gate(seg.end - seg.start, gate_pct),
                    scale_velocity(Onset::NORMAL, vel),
                    seg.value,
                );
            }
        }
        notes(out)
    })
}

// ---- Chord root (bass) -----------------------------------------------------------

pub static CHORD_ROOT: NodeSpec = NodeSpec {
    kind: "chord_root",
    title: "Chord root",
    category: "Parts",
    help: "A bassline: plays one tone of each chord (its root, fifth or lowest note) in a fixed octave, \
           on every rhythm hit, or held for each chord with no rhythm.",
    inputs: &[port("chords", PortType::Chord), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "tone",
            label: "Tone",
            kind: ParamKind::Choice { options: &["Root", "Fifth", "Lowest note"], default: "Root" },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 6.0, step: 1.0, default: 2.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_chord_root(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let tone = p.text("tone");
    let octave = p.int("octave");
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    let pick = move |chord: &Chord| -> Option<Pitch> {
        let root = chord
            .root()
            .or_else(|| chord.bass().map(|b| b.pitch_class()))?;
        let pc = match tone.as_str() {
            "Lowest note" => chord.bass()?.pitch_class() as i32,
            "Fifth" => {
                let tuning = chord.tuning();
                // The chord tone closest to a perfect fifth above the root (works for dim/aug too).
                let fifth = (tuning.steps_per_octave() * 7 + 6) / 12;
                (0..tuning.steps_per_octave())
                    .map(|d| root as i32 + fifth + if d % 2 == 0 { d / 2 } else { -(d / 2 + 1) })
                    .find(|&pc| chord.contains_pc(pc))
                    .unwrap_or(root as i32)
            }
            _ => root as i32,
        };
        let pc = pc.rem_euclid(chord.tuning().steps_per_octave()) as u8;
        Some(Pitch::from_octave(chord.tuning(), octave, pc))
    };
    node(move |ins, ctx| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        if ins.rhythm(1).is_some() {
            each_onset(ins, 1, ctx, |t, onset, _| {
                if let Some(pitch) = pick(chords.at(t).value) {
                    out.push(Note {
                        start: t,
                        duration: gate(onset.duration, gate_pct),
                        pitch,
                        velocity: scale_velocity(onset.velocity, vel),
                        voice: 0,
                    });
                }
            });
        } else {
            for seg in chords.starts_in(ctx.start, ctx.end, ctx.meter.ticks_per_bar()) {
                if let Some(pitch) = pick(seg.value) {
                    out.push(Note {
                        start: seg.start,
                        duration: gate(seg.end - seg.start, gate_pct),
                        pitch,
                        velocity: scale_velocity(Onset::NORMAL, vel),
                        voice: 0,
                    });
                }
            }
        }
        notes(out)
    })
}

// ---- Drum hit --------------------------------------------------------------------

const DRUMS: &[(&str, i32)] = &[
    ("Kick", 36),
    ("Rim", 37),
    ("Snare", 38),
    ("Clap", 39),
    ("Closed hat", 42),
    ("Pedal hat", 44),
    ("Open hat", 46),
    ("Low tom", 45),
    ("Mid tom", 47),
    ("High tom", 50),
    ("Crash", 49),
    ("Ride", 51),
    ("Cowbell", 56),
    ("Shaker", 70),
];
const DRUM_NAMES: &[&str] = &[
    "Kick",
    "Rim",
    "Snare",
    "Clap",
    "Closed hat",
    "Pedal hat",
    "Open hat",
    "Low tom",
    "Mid tom",
    "High tom",
    "Crash",
    "Ride",
    "Cowbell",
    "Shaker",
];

pub static DRUM: NodeSpec = NodeSpec {
    kind: "drum",
    title: "Drum hit",
    category: "Parts",
    help: "Plays one General MIDI drum sound on every rhythm hit. Send it to MIDI channel 10 for GM drums.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "sound", label: "Sound", kind: ParamKind::Choice { options: DRUM_NAMES, default: "Kick" } },
        ParamSpec { name: "note", label: "Note override", kind: ParamKind::Number { min: 0.0, max: 127.0, step: 1.0, default: 0.0 } },
        VELOCITY,
    ],
};

pub fn build_drum(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let sound = p.text("sound");
    let key = match p.int("note") {
        0 => DRUMS.iter().find(|d| d.0 == sound).map_or(36, |d| d.1),
        n => n,
    };
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        if ins.rhythm(0).is_none() {
            return Ok(vec![None]);
        }
        let mut out = Vec::new();
        each_onset(ins, 0, ctx, |t, onset, _| {
            out.push(Note {
                start: t,
                duration: onset.duration.clamp(1, pm_theory::PPQ / 4),
                pitch: Pitch::midi(key),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::build;
    use crate::nodes::spec_for;
    use crate::spec::ParamValue;
    use crate::timeline::Timeline;
    use pm_theory::{Meter, PPQ};
    use std::collections::HashMap;

    fn ctx(start: Ticks, end: Ticks, salt: u64) -> EvalContext {
        EvalContext::new(start, end, Meter::COMMON, salt)
    }

    fn run(
        kind: &str,
        params: &[(&str, ParamValue)],
        inputs: Vec<Option<Value>>,
        c: &EvalContext,
    ) -> Vec<Note> {
        let values: HashMap<String, ParamValue> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let n = build(spec_for(kind).unwrap(), &values).unwrap();
        match n.eval(&Inputs { values: inputs }, c).unwrap().remove(0) {
            Some(Value::Notes(v)) => v,
            other => panic!("expected notes, got {other:?}"),
        }
    }

    fn names(notes: &[Note]) -> Vec<String> {
        notes.iter().map(|n| n.pitch.to_string()).collect()
    }

    fn c_major() -> Option<Value> {
        Some(Value::Scale(Timeline::constant(
            Scale::parse("C major").unwrap(),
        )))
    }

    fn line(pitches: &[&str], step: Ticks) -> Option<Value> {
        Some(Value::Notes(
            pitches
                .iter()
                .enumerate()
                .map(|(i, p)| Note { start: i as Ticks * step, duration: step, pitch: Pitch::parse(p).unwrap(), velocity: 90, voice: 0 })
                .collect(),
        ))
    }

    #[test]
    fn markov_follows_its_training() {
        // A strict alternation can only ever alternate.
        let p = [("degrees", ParamValue::Text("1 5 1 5".into())), ("lock", ParamValue::Bool(true))];
        let v = run("markov", &p, vec![c_major(), None, None], &ctx(0, 4 * PPQ, 0));
        assert_eq!(names(&v)[..4], ["C4", "G4", "C4", "G4"]);
        // Wired training wins over the typed degrees.
        let v = run("markov", &p, vec![c_major(), None, line(&["E4", "F4", "E4", "F4"], PPQ)], &ctx(0, PPQ, 0));
        assert_eq!(names(&v)[..2], ["E4", "F4"]);
        // Unlocked, passes differ; locked, they repeat.
        let free = [("degrees", ParamValue::Text("1 2 3 4 5 4 3 2 1 3 5 2 4".into()))];
        let a = run("markov", &free, vec![c_major(), None, None], &ctx(0, 4 * PPQ, 1));
        let b = run("markov", &free, vec![c_major(), None, None], &ctx(0, 4 * PPQ, 2));
        assert_ne!(names(&a), names(&b));
    }

    #[test]
    fn contour_rises() {
        let v = run("contour", &[("shape", ParamValue::Text("Rise".into()))], vec![c_major(), None], &ctx(0, 8 * PPQ, 0));
        assert_eq!(v.first().map(|n| n.pitch.to_string()).as_deref(), Some("C4"));
        assert!(v.windows(2).all(|w| w[0].pitch <= w[1].pitch));
        assert!(v.last().unwrap().pitch >= Pitch::parse("A4").unwrap());
    }

    #[test]
    fn motif_transforms() {
        let motif = line(&["C4", "D4", "E4", "G4"], PPQ);
        let bar = 4 * PPQ;
        let retro = run("motif", &[], vec![motif.clone(), c_major()], &ctx(0, bar, 0));
        assert_eq!(names(&retro), ["G4", "E4", "D4", "C4"]);
        let inv = run("motif", &[("op", ParamValue::Text("Inversion".into()))], vec![motif.clone(), c_major()], &ctx(0, bar, 0));
        assert_eq!(names(&inv), ["C4", "B3", "A3", "F3"]);
        let aug = run("motif", &[("op", ParamValue::Text("Augmentation".into()))], vec![motif.clone(), c_major()], &ctx(0, bar, 0));
        assert_eq!(aug.iter().map(|n| (n.start, n.duration)).collect::<Vec<_>>(), [(0, 2 * PPQ), (2 * PPQ, 2 * PPQ)]);
        let dim = run("motif", &[("op", ParamValue::Text("Diminution".into()))], vec![motif, c_major()], &ctx(0, bar, 0));
        assert_eq!(dim.len(), 8);
        assert_eq!(dim[4].start, 2 * PPQ);
    }

    #[test]
    fn arp_up_down_over_a_bar() {
        let v = run(
            "arp",
            &[("pattern", ParamValue::Text("Up-down".into()))],
            vec![Some(Value::Chord(Timeline::constant(Chord::from_symbol("C", 4).unwrap()))), None],
            &ctx(0, PPQ * 2, 0),
        );
        assert_eq!(names(&v), ["C4", "E4", "G4", "C5", "G4", "E4", "C4", "E4"]);
        assert_eq!(v[1].start, PPQ / 4);
    }

    #[test]
    fn sequencer_degrees_and_rests() {
        let v = run(
            "sequencer",
            &[("degrees", ParamValue::Text("1 - 8 -1".into()))],
            vec![c_major(), None],
            &ctx(0, PPQ, 0),
        );
        assert_eq!(names(&v), ["C4", "C5", "B3"]);
        assert!(build(
            spec_for("sequencer").unwrap(),
            &HashMap::from([("degrees".into(), ParamValue::Text("1 0".into()))])
        )
        .is_err());
    }

    #[test]
    fn random_arp_locks_like_the_walk() {
        let bar = Meter::COMMON.ticks_per_bar();
        let arp = |params: &[(&str, ParamValue)], salt| {
            let mut all = vec![("pattern", ParamValue::Text("Random".into()))];
            all.extend(params.iter().cloned());
            let cmaj9 = Some(Value::Chord(Timeline::constant(Chord::from_symbol("Cmaj9", 4).unwrap())));
            names(&run("arp", &all, vec![cmaj9, None], &ctx(0, bar, salt)))
        };
        assert_ne!(arp(&[], 1), arp(&[], 2), "fresh on each pass");
        let locked = [("lock", ParamValue::Bool(true))];
        assert_eq!(arp(&locked, 1), arp(&locked, 2), "the same when locked");
        let captured = [
            ("lock", ParamValue::Bool(true)),
            ("seed", ParamValue::Number(rng::fresh_seed(1, 3) as f64)),
        ];
        assert_eq!(arp(&[], 3), arp(&captured, 50), "locking keeps what played");
    }

    #[test]
    fn random_walk_lock_repeats_and_unlocked_varies() {
        let phrase = Meter::COMMON.ticks_per_bar() * 2;
        let locked = [("lock", ParamValue::Bool(true))];
        let walk = |params: &[(&str, ParamValue)], start, salt| {
            names(&run("random_walk", params, vec![c_major(), None], &ctx(start, start + phrase, salt)))
        };
        // Locked: the same on every pass (whatever the salt), different from phrase to phrase.
        assert_eq!(walk(&locked, 0, 1), walk(&locked, 0, 2));
        assert_ne!(walk(&locked, 0, 1), walk(&locked, phrase, 1));
        // Locking with the pass's fresh seed keeps exactly what was playing.
        let captured = [
            ("lock", ParamValue::Bool(true)),
            ("seed", ParamValue::Number(rng::fresh_seed(1, 7) as f64)),
        ];
        for start in [0, phrase, 5 * phrase] {
            assert_eq!(walk(&[], start, 7), walk(&captured, start, 99));
        }
        let c = run(
            "random_walk",
            &[],
            vec![c_major(), None],
            &ctx(0, phrase, 1),
        );
        let d = run(
            "random_walk",
            &[],
            vec![c_major(), None],
            &ctx(0, phrase, 2),
        );
        assert_ne!(names(&c), names(&d));
        // Steps stay within the max step (2 degrees) and the first note is the root.
        assert_eq!(c[0].pitch.to_string(), "C4");
        let major = Scale::parse("C major").unwrap();
        for w in c.windows(2) {
            let (da, _) = major.degree_of(w[0].pitch).unwrap();
            let oa = major.degree_of(w[0].pitch).unwrap().1;
            let (db, ob) = major.degree_of(w[1].pitch).unwrap();
            let pos = |d: usize, o: i32| o * 7 + d as i32;
            assert!((pos(db, ob) - pos(da, oa)).abs() <= 2);
        }
    }

    #[test]
    fn held_chords_and_bass() {
        let prog = Timeline::from_steps(vec![
            (PPQ * 4, Chord::from_symbol("Am", 4).unwrap()),
            (PPQ * 4, Chord::from_symbol("F", 4).unwrap()),
        ])
        .unwrap();
        let c = ctx(0, PPQ * 8, 0);
        let pads = run(
            "play_chords",
            &[],
            vec![Some(Value::Chord(prog.clone())), None],
            &c,
        );
        assert_eq!(pads.len(), 6);
        assert_eq!(pads[3].start, PPQ * 4);
        let bass = run(
            "chord_root",
            &[],
            vec![Some(Value::Chord(prog.clone())), None],
            &c,
        );
        assert_eq!(names(&bass), ["A2", "F2"]);
        let fifths = run(
            "chord_root",
            &[("tone", ParamValue::Text("Fifth".into()))],
            vec![Some(Value::Chord(prog)), None],
            &c,
        );
        assert_eq!(names(&fifths), ["E2", "C2"]);
    }

    #[test]
    fn drums_use_gm_keys() {
        let r = Some(Value::Rhythm(Box::new(
            Rhythm::from_grid("x...x...", PPQ / 4).unwrap(),
        )));
        let v = run(
            "drum",
            &[("sound", ParamValue::Text("Snare".into()))],
            vec![r],
            &ctx(0, PPQ * 4, 0),
        );
        assert_eq!(v.len(), 4, "a half-bar pattern loops twice per bar");
        assert!(v.iter().all(|n| n.pitch.step() == 38));
    }
}
