//! Operations on note streams.

use pm_theory::{Note, Pitch, Rounding, Ticks, PPQ};

use super::theory::node;
use super::{scale_velocity, step_ticks, Inputs, NodeImpl, STEP_OPTIONS};
use crate::graph::EvalContext;
use crate::rng;
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;

pub static OCTAVE: NodeSpec = NodeSpec {
    kind: "octave",
    title: "Octave",
    category: "Notes",
    help: "Moves every note up or down by whole octaves.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[ParamSpec {
        name: "shift",
        label: "Shift",
        kind: ParamKind::Number {
            min: -4.0,
            max: 4.0,
            step: 1.0,
            default: -1.0,
        },
    }],
};

pub fn build_octave(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let shift = p.int("shift");
    node(move |ins, _| {
        let mut notes = ins.notes(0).to_vec();
        for n in &mut notes {
            n.pitch = n
                .pitch
                .offset(shift * n.pitch.tuning().steps_per_octave())
                .with_spelling(n.pitch.spelling());
        }
        Ok(vec![Some(Value::Notes(notes))])
    })
}

pub static QUANTIZE: NodeSpec = NodeSpec {
    kind: "quantize",
    title: "Quantize to scale",
    category: "Notes",
    help: "Snaps every note to the scale sounding when it starts. Wire a chord in to snap to chord tones.",
    inputs: &[port("notes", PortType::Notes), port("scale", PortType::Scale)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[ParamSpec {
        name: "rounding",
        label: "Rounding",
        kind: ParamKind::Choice { options: &["Nearest", "Up", "Down"], default: "Nearest" },
    }],
};

pub fn build_quantize(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let rounding = match p.text("rounding").as_str() {
        "Up" => Rounding::Up,
        "Down" => Rounding::Down,
        _ => Rounding::Nearest,
    };
    node(move |ins, _| {
        let Some(scales) = ins.scale(1) else {
            return Ok(vec![Some(Value::Notes(ins.notes(0).to_vec()))]);
        };
        let mut notes = ins.notes(0).to_vec();
        for n in &mut notes {
            let scale = scales.at(n.start).value;
            if scale.tuning() == n.pitch.tuning() {
                n.pitch = scale.quantize(n.pitch, rounding);
            }
        }
        Ok(vec![Some(Value::Notes(notes))])
    })
}

pub static VELOCITY: NodeSpec = NodeSpec {
    kind: "velocity",
    title: "Velocity",
    category: "Notes",
    help: "Scales every note's velocity (loudness), then shapes it with a curve into a range: \
           Soft lifts quiet notes (evening out the dynamics), Hard pushes them down (exaggerating them).",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "amount",
            label: "Amount %",
            kind: ParamKind::Number {
                min: 0.0,
                max: 200.0,
                step: 5.0,
                default: 80.0,
            },
        },
        ParamSpec {
            name: "curve",
            label: "Curve",
            kind: ParamKind::Choice { options: &["Linear", "Soft", "Hard"], default: "Linear" },
        },
        ParamSpec { name: "min", label: "Min", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 1.0 } },
        ParamSpec { name: "max", label: "Max", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 127.0 } },
    ],
};

/// A velocity (1–127) through a curve into `[min, max]`; Linear over the full range
/// leaves it unchanged.
fn shape_velocity(v: u8, curve: &str, min: f64, max: f64) -> u8 {
    let x = (v.max(1) - 1) as f64 / 126.0;
    let y = match curve {
        "Soft" => x.sqrt(),
        "Hard" => x * x,
        _ => x,
    };
    (min + y * (max - min)).round().clamp(1.0, 127.0) as u8
}

pub fn build_velocity(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let amount = p.number("amount");
    let curve = p.text("curve");
    let (min, max) = (p.number("min"), p.number("max"));
    node(move |ins, _| {
        let mut notes = ins.notes(0).to_vec();
        for n in &mut notes {
            n.velocity = shape_velocity(scale_velocity(n.velocity, amount), &curve, min, max);
        }
        Ok(vec![Some(Value::Notes(notes))])
    })
}

/// A node that reads its input over a wider window than it plays (it moves notes in time,
/// or looks ahead), then keeps only the notes that start in the window.
struct Windowed<W, F> {
    window: W,
    f: F,
}

impl<W, F> NodeImpl for Windowed<W, F>
where
    W: Fn(&EvalContext) -> (Ticks, Ticks) + Send + Sync,
    F: Fn(&Inputs, &EvalContext) -> Result<Vec<Note>, String> + Send + Sync,
{
    fn input_window(&self, ctx: &EvalContext) -> Option<(Ticks, Ticks)> {
        Some((self.window)(ctx))
    }

    fn eval(&self, ins: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let mut notes = (self.f)(ins, ctx)?;
        notes.retain(|n| n.start >= ctx.start && n.start < ctx.end);
        notes.sort_by_key(|n| n.start);
        Ok(vec![Some(Value::Notes(notes))])
    }
}

pub(crate) fn windowed<W, F>(window: W, f: F) -> Result<Box<dyn NodeImpl>, String>
where
    W: Fn(&EvalContext) -> (Ticks, Ticks) + Send + Sync + 'static,
    F: Fn(&Inputs, &EvalContext) -> Result<Vec<Note>, String> + Send + Sync + 'static,
{
    Ok(Box::new(Windowed { window, f }))
}

// ---- Humanize ------------------------------------------------------------------------

pub static HUMANIZE: NodeSpec = NodeSpec {
    kind: "humanize",
    title: "Humanize",
    category: "Notes",
    help: "Nudges every note's timing and loudness by a small random amount, so parts sound \
           played rather than programmed. Timing is a share of a sixteenth note, either way.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "timing", label: "Timing %", kind: ParamKind::Number { min: 0.0, max: 50.0, step: 1.0, default: 8.0 } },
        ParamSpec { name: "velocity", label: "Velocity ±", kind: ParamKind::Number { min: 0.0, max: 40.0, step: 1.0, default: 10.0 } },
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: false } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
    ],
};

pub fn build_humanize(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let reach = ((PPQ / 4) as f64 * p.number("timing") / 100.0).round() as i64;
    let vel = p.int("velocity").max(0) as i64;
    let lock = p.flag("lock");
    let seed = p.number("seed") as u64;
    windowed(
        move |ctx| (ctx.start.saturating_sub(reach as Ticks), ctx.end + reach as Ticks),
        move |ins, ctx| {
            let stream = rng::stream(lock, seed, ctx.salt);
            let mut notes = ins.notes(0).to_vec();
            for n in &mut notes {
                let h = rng::hash(&[stream, n.start as u64, n.pitch.step() as u64, n.voice as u64]);
                let dt = rng::below(h, 2 * reach as u64 + 1) as i64 - reach;
                let dv = rng::below(h >> 32, 2 * vel as u64 + 1) as i64 - vel;
                n.start = (n.start as i64 + dt).max(0) as Ticks;
                n.velocity = (n.velocity as i64 + dv).clamp(1, 127) as u8;
            }
            Ok(notes)
        },
    )
}

// ---- Legato --------------------------------------------------------------------------

pub static LEGATO: NodeSpec = NodeSpec {
    kind: "legato",
    title: "Legato",
    category: "Notes",
    help: "Holds each note until the next one in its voice starts (up to a bar), for smooth lines. \
           Over 100% the notes overlap a little, which makes a mono synth glide.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[ParamSpec {
        name: "length",
        label: "Length %",
        kind: ParamKind::Number { min: 50.0, max: 150.0, step: 5.0, default: 100.0 },
    }],
};

pub fn build_legato(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let pct = p.number("length");
    windowed(
        |ctx| (ctx.start, ctx.end + ctx.meter.ticks_per_bar()),
        move |ins, _| {
            let mut notes = ins.notes(0).to_vec();
            notes.sort_by_key(|n| (n.voice, n.start));
            let starts: Vec<(u8, Ticks)> = notes.iter().map(|n| (n.voice, n.start)).collect();
            for (i, n) in notes.iter_mut().enumerate() {
                let next = starts[i + 1..].iter().find(|(v, s)| *v == n.voice && *s > n.start);
                if let Some((_, s)) = next {
                    n.duration = ((((s - n.start) as f64) * pct / 100.0).round() as Ticks).max(1);
                }
            }
            Ok(notes)
        },
    )
}

// ---- Note delay ----------------------------------------------------------------------

pub static NOTE_DELAY: NodeSpec = NodeSpec {
    kind: "note_delay",
    title: "Note delay",
    category: "Notes",
    help: "Moves notes later by some steps. Echoes repeat each note that far apart again, \
           each quieter by Decay. Keep original also plays the notes where they were.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "steps", label: "Delay steps", kind: ParamKind::Number { min: 0.0, max: 16.0, step: 1.0, default: 3.0 } },
        ParamSpec { name: "step", label: "Step", kind: ParamKind::Choice { options: STEP_OPTIONS, default: "1/16" } },
        ParamSpec { name: "echoes", label: "Echoes", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 0.0 } },
        ParamSpec { name: "decay", label: "Decay %", kind: ParamKind::Number { min: 0.0, max: 100.0, step: 5.0, default: 60.0 } },
        ParamSpec { name: "original", label: "Keep original", kind: ParamKind::Bool { default: false } },
    ],
};

pub fn build_note_delay(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let d = p.int("steps").max(0) as Ticks * step_ticks(&p.text("step"));
    let echoes = p.int("echoes").max(0) as Ticks;
    let decay = p.number("decay") / 100.0;
    let first = if p.flag("original") { 0 } else { 1 };
    windowed(
        move |ctx| (ctx.start.saturating_sub(d * (1 + echoes)), ctx.end),
        move |ins, _| {
            let mut out = Vec::new();
            for n in ins.notes(0) {
                for k in first..=1 + echoes {
                    let level = if k <= 1 { 1.0 } else { decay.powi(k as i32 - 1) };
                    let velocity = (n.velocity as f64 * level).round() as u8;
                    if velocity == 0 {
                        break;
                    }
                    out.push(Note { start: n.start + k * d, velocity, ..*n });
                }
            }
            Ok(out)
        },
    )
}

// ---- Repeat bars ---------------------------------------------------------------------

pub static REPEAT_BARS: NodeSpec = NodeSpec {
    kind: "repeat_bars",
    title: "Repeat bars",
    category: "Notes",
    help: "Loops the first bars of every block: with 1 of every 4, bar 1 plays four times, then \
           bar 5 four times, and so on. Turns a wandering line into a repeating riff.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "bars", label: "Repeat bars", kind: ParamKind::Number { min: 1.0, max: 8.0, step: 1.0, default: 1.0 } },
        ParamSpec { name: "every", label: "Of every", kind: ParamKind::Number { min: 1.0, max: 32.0, step: 1.0, default: 4.0 } },
    ],
};

pub fn build_repeat_bars(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let bars = p.int("bars").max(1) as Ticks;
    let every = (p.int("every").max(1) as Ticks).max(bars);
    windowed(
        move |ctx| {
            let (keep, block) = (bars * ctx.meter.ticks_per_bar(), every * ctx.meter.ticks_per_bar());
            let first = ctx.start / block * block;
            let last = ctx.end.saturating_sub(1) / block * block;
            (first, last + keep)
        },
        move |ins, ctx| {
            let (keep, block) = (bars * ctx.meter.ticks_per_bar(), every * ctx.meter.ticks_per_bar());
            let mut out = Vec::new();
            for n in ins.notes(0) {
                let within = n.start % block;
                if within >= keep {
                    continue;
                }
                let mut start = n.start;
                while start - n.start + within < block {
                    out.push(Note { start, ..*n });
                    start += keep;
                }
            }
            Ok(out)
        },
    )
}

// ---- Filter and split ------------------------------------------------------------------

pub static FILTER: NodeSpec = NodeSpec {
    kind: "note_filter",
    title: "Note filter",
    category: "Notes",
    help: "Keeps the notes inside a pitch range and a velocity range (or, inverted, the ones outside).",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "low", label: "Lowest", kind: ParamKind::Text { default: "C0" } },
        ParamSpec { name: "high", label: "Highest", kind: ParamKind::Text { default: "G9" } },
        ParamSpec { name: "min_vel", label: "Min velocity", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 1.0 } },
        ParamSpec { name: "max_vel", label: "Max velocity", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 127.0 } },
        ParamSpec { name: "invert", label: "Invert", kind: ParamKind::Bool { default: false } },
    ],
};

fn pitch_param(text: &str, what: &str) -> Result<Pitch, String> {
    Pitch::parse(text).ok_or_else(|| format!("{what} '{text}' is not a pitch like C3 or F#4"))
}

pub fn build_filter(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let low = pitch_param(&p.text("low"), "lowest note")?.step();
    let high = pitch_param(&p.text("high"), "highest note")?.step();
    let (min_vel, max_vel) = (p.int("min_vel") as u8, p.int("max_vel") as u8);
    let invert = p.flag("invert");
    node(move |ins, _| {
        let keep = |n: &Note| {
            let inside = (low..=high).contains(&n.pitch.step()) && (min_vel..=max_vel).contains(&n.velocity);
            inside != invert
        };
        Ok(vec![Some(Value::Notes(ins.notes(0).iter().copied().filter(keep).collect()))])
    })
}

pub static RHYTHM_GATE: NodeSpec = NodeSpec {
    kind: "rhythm_gate",
    title: "Rhythm gate",
    category: "Notes",
    help: "Combines notes with a rhythm, like Combine does two rhythms: And keeps the notes that \
           start on a hit, Drop the ones that don't. Or adds a note on every hit that has none, \
           repeating the latest notes (from up to a bar before); Xor does both of the last two. \
           Replace plays the latest notes on every hit instead, giving the notes the rhythm's timing.",
    inputs: &[port("notes", PortType::Notes), port("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[ParamSpec {
        name: "op",
        label: "Operation",
        kind: ParamKind::Choice { options: &["And", "Or", "Xor", "Drop", "Replace"], default: "And" },
    }],
};

pub fn build_rhythm_gate(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let op = p.text("op");
    let keep_on = matches!(op.as_str(), "And" | "Or");
    let keep_off = matches!(op.as_str(), "Or" | "Xor" | "Drop");
    let fill = matches!(op.as_str(), "Or" | "Xor" | "Replace");
    // Replace plays every hit; the others only the hits no note starts on.
    let every_hit = op == "Replace";
    windowed(
        // A bar back, for the notes that empty hits repeat.
        |ctx| (ctx.start.saturating_sub(ctx.meter.ticks_per_bar()), ctx.end),
        move |ins, ctx| {
            let input = ins.notes(0);
            let Some(rhythm) = ins.rhythm(1) else {
                return Ok(input.to_vec());
            };
            let mut hits = Vec::new();
            rhythm.for_each_in(ctx.start, ctx.end, |t, o| hits.push((t, *o)));
            let on_hit = |t: Ticks| hits.binary_search_by_key(&t, |h| h.0).is_ok();
            let mut out: Vec<Note> = input
                .iter()
                .filter(|n| if on_hit(n.start) { keep_on } else { keep_off })
                .copied()
                .collect();
            if fill {
                for (t, onset) in &hits {
                    if !every_hit && input.iter().any(|n| n.start == *t) {
                        continue;
                    }
                    let Some(latest) = input.iter().filter(|n| n.start <= *t).map(|n| n.start).max() else {
                        continue;
                    };
                    for n in input.iter().filter(|n| n.start == latest) {
                        out.push(Note { start: *t, duration: onset.duration, velocity: onset.velocity, ..*n });
                    }
                }
            }
            Ok(out)
        },
    )
}

pub static SPLIT_RANGE: NodeSpec = NodeSpec {
    kind: "split_range",
    title: "Split by range",
    category: "Notes",
    help: "Sends notes at or above the split note to one output and the rest to the other, \
           like a keyboard split.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("above", PortType::Notes), port("below", PortType::Notes)],
    params: &[ParamSpec { name: "split", label: "Split at", kind: ParamKind::Text { default: "C4" } }],
};

pub fn build_split_range(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let split = pitch_param(&p.text("split"), "split note")?.step();
    node(move |ins, _| {
        let (above, below): (Vec<Note>, Vec<Note>) = ins.notes(0).iter().copied().partition(|n| n.pitch.step() >= split);
        Ok(vec![Some(Value::Notes(above)), Some(Value::Notes(below))])
    })
}

// ---- Harmonizer ------------------------------------------------------------------------

pub static HARMONIZER: NodeSpec = NodeSpec {
    kind: "harmonizer",
    title: "Harmonizer",
    category: "Counterpoint",
    help: "Adds a parallel line a third or sixth away, in the key, so it stays diatonic (a mix of \
           major and minor intervals). The harmony gets the next voice number, so Split voices can \
           send it elsewhere.",
    inputs: &[port("notes", PortType::Notes), port("key", PortType::Scale)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "interval",
            label: "Interval",
            kind: ParamKind::Choice {
                options: &["3rd above", "6th above", "3rd below", "6th below", "10th above"],
                default: "3rd above",
            },
        },
        ParamSpec { name: "original", label: "Keep original", kind: ParamKind::Bool { default: true } },
    ],
};

pub fn build_harmonizer(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let steps = match p.text("interval").as_str() {
        "6th above" => 5,
        "3rd below" => -2,
        "6th below" => -5,
        "10th above" => 9,
        _ => 2,
    };
    let original = p.flag("original");
    node(move |ins, _| {
        let input = ins.notes(0);
        let Some(keys) = ins.scale(1) else {
            return Ok(vec![Some(Value::Notes(input.to_vec()))]);
        };
        let mut out = Vec::with_capacity(input.len() * 2);
        for n in input {
            if original {
                out.push(*n);
            }
            let key = keys.at(n.start).value;
            if key.tuning() != n.pitch.tuning() || key.is_empty() {
                continue;
            }
            let base = key.quantize(n.pitch, Rounding::Nearest);
            if let Some(pitch) = key.transpose_diatonic(base, steps) {
                out.push(Note { pitch, voice: n.voice.saturating_add(1), ..*n });
            }
        }
        Ok(vec![Some(Value::Notes(out))])
    })
}

pub static MERGE: NodeSpec = NodeSpec {
    kind: "merge",
    title: "Merge",
    category: "Notes",
    help: "Combines up to four note streams into one.",
    inputs: &[
        optional("a", PortType::Notes),
        optional("b", PortType::Notes),
        optional("c", PortType::Notes),
        optional("d", PortType::Notes),
    ],
    outputs: &[port("notes", PortType::Notes)],
    params: &[],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::tests::{node as doc_node, wire};
    use crate::graph::{Graph, GraphDoc};
    use crate::nodes::{build, spec_for};
    use crate::spec::ParamValue;
    use crate::timeline::Timeline;
    use pm_theory::{Meter, Scale};
    use std::collections::HashMap;

    const BAR: Ticks = PPQ * 4;

    fn note(start: Ticks, pitch: &str) -> Note {
        Note { start, duration: 100, pitch: Pitch::parse(pitch).unwrap(), velocity: 100, voice: 0 }
    }

    fn run(kind: &str, params: &[(&str, ParamValue)], ins: Vec<Option<Value>>, start: Ticks, end: Ticks) -> Vec<Note> {
        let values: HashMap<String, ParamValue> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let n = build(spec_for(kind).unwrap(), &values).unwrap();
        match n.eval(&Inputs { values: ins }, &EvalContext::new(start, end, Meter::COMMON, 3)).unwrap().remove(0) {
            Some(Value::Notes(n)) => n,
            other => panic!("{other:?}"),
        }
    }

    fn num(v: f64) -> ParamValue {
        ParamValue::Number(v)
    }

    #[test]
    fn time_moves() {
        let line = || Some(Value::Notes(vec![note(0, "C4"), note(480, "D4"), note(4000, "E4")]));
        let legato = run("legato", &[], vec![line()], 0, BAR);
        assert_eq!(legato.iter().map(|n| n.duration).collect::<Vec<_>>(), [480, 3520]);

        let delayed = run("note_delay", &[("steps", num(3.0))], vec![line()], 0, BAR);
        assert_eq!(delayed.iter().map(|n| n.start).collect::<Vec<_>>(), [720, 1200]);
        let echoed = run(
            "note_delay",
            &[("steps", num(2.0)), ("echoes", num(1.0)), ("original", ParamValue::Bool(true))],
            vec![Some(Value::Notes(vec![note(0, "C4")]))],
            0,
            BAR,
        );
        assert_eq!(echoed.iter().map(|n| (n.start, n.velocity)).collect::<Vec<_>>(), [(0, 100), (480, 100), (960, 60)]);

        let riff = run("repeat_bars", &[], vec![Some(Value::Notes(vec![note(100, "C4"), note(BAR + 5, "D4")]))], 0, 4 * BAR);
        assert_eq!(riff.iter().map(|n| n.start).collect::<Vec<_>>(), [100, BAR + 100, 2 * BAR + 100, 3 * BAR + 100]);

        let lock = [("lock", ParamValue::Bool(true)), ("timing", num(50.0))];
        let a = run("humanize", &lock, vec![line()], 0, BAR);
        let b = run("humanize", &lock, vec![line()], 0, BAR);
        assert_eq!(a, b);
        assert!(a.iter().zip([0, 480]).all(|(n, t)| (n.start as i64 - t).abs() <= 120));
    }

    #[test]
    fn pitch_and_loudness() {
        let key = Some(Value::Scale(Timeline::constant(Scale::parse("C major").unwrap())));
        let line = Some(Value::Notes(vec![note(0, "C4"), note(240, "E4")]));
        let h = run("harmonizer", &[], vec![line.clone(), key], 0, BAR);
        let names: Vec<(String, u8)> = h.iter().map(|n| (n.pitch.to_string(), n.voice)).collect();
        assert_eq!(names, [("C4".into(), 0), ("E4".into(), 1), ("E4".into(), 0), ("G4".into(), 1)]);

        let high = run("note_filter", &[("low", ParamValue::Text("D4".into()))], vec![line.clone()], 0, BAR);
        assert_eq!(high.len(), 1);
        assert_eq!(shape_velocity(90, "Linear", 1.0, 127.0), 90);
        assert!(shape_velocity(40, "Soft", 1.0, 127.0) > 40);
        assert!(shape_velocity(40, "Hard", 1.0, 127.0) < 40);
    }

    #[test]
    fn rhythm_gate_combines_notes_with_hits() {
        // Hits on beats 1 and 3; notes on beat 1 (a two-note chord) and beat 2.
        let rhythm = || Some(Value::Rhythm(Box::new(pm_theory::Rhythm::from_grid("X.x.", PPQ).unwrap())));
        let line = || Some(Value::Notes(vec![note(0, "C4"), note(0, "E4"), note(PPQ, "D4")]));
        let gate = |op: &str| {
            let out = run("rhythm_gate", &[("op", ParamValue::Text(op.into()))], vec![line(), rhythm()], 0, BAR);
            out.iter().map(|n| (n.start, n.pitch.to_string())).collect::<Vec<_>>()
        };
        let at = |t: Ticks, p: &str| (t, p.to_string());
        assert_eq!(gate("And"), [at(0, "C4"), at(0, "E4")]);
        assert_eq!(gate("Drop"), [at(PPQ, "D4")]);
        // Replace: the latest notes on every hit, and nothing else.
        assert_eq!(gate("Replace"), [at(0, "C4"), at(0, "E4"), at(2 * PPQ, "D4")]);
        let replaced = run("rhythm_gate", &[("op", ParamValue::Text("Replace".into()))], vec![line(), rhythm()], 0, BAR);
        assert!(replaced.iter().all(|n| n.duration == PPQ));
        assert_eq!(replaced[0].velocity, pm_theory::Onset::ACCENT);
        // The empty hit on beat 3 repeats the latest notes, D4, with the hit's length and loudness.
        assert_eq!(gate("Or"), [at(0, "C4"), at(0, "E4"), at(PPQ, "D4"), at(2 * PPQ, "D4")]);
        assert_eq!(gate("Xor"), [at(PPQ, "D4"), at(2 * PPQ, "D4")]);
        let or = run("rhythm_gate", &[("op", ParamValue::Text("Or".into()))], vec![line(), rhythm()], 0, BAR);
        assert_eq!((or[3].duration, or[3].velocity), (PPQ, pm_theory::Onset::NORMAL));
        // In the next bar, with no notes of its own, the hits repeat the bar before's last note.
        let next = run("rhythm_gate", &[("op", ParamValue::Text("Or".into()))], vec![line(), rhythm()], BAR, 2 * BAR);
        assert_eq!(next.iter().map(|n| (n.start, n.pitch.to_string())).collect::<Vec<_>>(), [at(BAR, "D4"), at(BAR + 2 * PPQ, "D4")]);
    }

    #[test]
    fn delay_reaches_back_in_a_graph() {
        // A note at the end of bar 1, delayed into bar 2, plays when bar 2 is evaluated alone.
        let doc = GraphDoc {
            nodes: vec![
                doc_node("s", "scale", &[]),
                doc_node("seq", "sequencer", &[("degrees", ParamValue::Text("1".into()))]),
                doc_node("p", "pattern", &[("pattern", ParamValue::Text("...............x".into()))]),
                doc_node("d", "note_delay", &[("steps", num(2.0))]),
                doc_node("out", "midi_out", &[]),
            ],
            wires: vec![
                wire("s", "scale", "seq", "scale"),
                wire("p", "rhythm", "seq", "rhythm"),
                wire("seq", "notes", "d", "notes"),
                wire("d", "notes", "out", "notes"),
            ],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let out = g.evaluate(&EvalContext::new(BAR, BAR + PPQ, Meter::COMMON, 0));
        assert_eq!(out[0].notes.iter().map(|n| n.start).collect::<Vec<_>>(), [BAR + 240]);
    }
}

pub fn build_merge(_: &Params) -> Result<Box<dyn NodeImpl>, String> {
    node(move |ins, _| {
        let mut notes = Vec::new();
        for i in 0..4 {
            notes.extend_from_slice(ins.notes(i));
        }
        notes.sort_by_key(|n| n.start);
        Ok(vec![Some(Value::Notes(notes))])
    })
}
