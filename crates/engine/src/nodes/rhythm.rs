//! Rhythm sources and transforms.

use super::theory::node;
use super::{step_ticks, NodeImpl, STEP_OPTIONS};
use crate::rng;
use crate::spec::{port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;
use pm_theory::{Meter, Onset, Rhythm, Ticks};

const STEP_PARAM: ParamSpec = ParamSpec {
    name: "step",
    label: "Step",
    kind: ParamKind::Choice {
        options: STEP_OPTIONS,
        default: "1/16",
    },
};

pub static PATTERN: NodeSpec = NodeSpec {
    kind: "pattern",
    title: "Pattern",
    category: "Rhythm",
    help: "A step pattern: x hit, X accent, . rest, _ hold the previous hit. Spaces and | are ignored.",
    inputs: &[],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[ParamSpec { name: "pattern", label: "Pattern", kind: ParamKind::Text { default: "x.x.x.xx" } }, STEP_PARAM],
};

pub fn build_pattern(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let text = p.text("pattern");
    let rhythm = Rhythm::from_grid(&text, step_ticks(&p.text("step")))
        .ok_or("patterns use x X . - _ | and spaces, with at most 64 hits")?;
    if text.chars().all(|c| c == ' ' || c == '|') {
        return Err("the pattern is empty".into());
    }
    node(move |_, _| Ok(vec![Some(Value::Rhythm(Box::new(rhythm)))]))
}

pub static EUCLID: NodeSpec = NodeSpec {
    kind: "euclid",
    title: "Euclidean",
    category: "Rhythm",
    help: "Spreads a number of hits as evenly as possible over a number of steps. E(3,8) is the tresillo.",
    inputs: &[],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[
        ParamSpec { name: "hits", label: "Hits", kind: ParamKind::Number { min: 0.0, max: 32.0, step: 1.0, default: 5.0 } },
        ParamSpec { name: "steps", label: "Steps", kind: ParamKind::Number { min: 1.0, max: 32.0, step: 1.0, default: 8.0 } },
        ParamSpec { name: "rotate", label: "Rotate", kind: ParamKind::Number { min: 0.0, max: 31.0, step: 1.0, default: 0.0 } },
        STEP_PARAM,
    ],
};

pub fn build_euclid(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let steps = p.int("steps").max(1) as u32;
    let hits = (p.int("hits").max(0) as u32).min(steps);
    let step = step_ticks(&p.text("step"));
    let rotate = p.int("rotate") as i64 % steps as i64;
    let rhythm = Rhythm::euclidean(hits, steps, step)
        .ok_or("bad euclidean rhythm")?
        .rotate(-rotate * step as i64);
    node(move |_, _| Ok(vec![Some(Value::Rhythm(Box::new(rhythm)))]))
}

pub static COMBINE: NodeSpec = NodeSpec {
    kind: "combine",
    title: "Combine",
    category: "Rhythm",
    help: "Combines two rhythms of the same length: Or (either hits), And (both), Xor (exactly one), A not B.",
    inputs: &[port("a", PortType::Rhythm), port("b", PortType::Rhythm)],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[ParamSpec {
        name: "op",
        label: "Operation",
        kind: ParamKind::Choice { options: &["Or", "And", "Xor", "A not B"], default: "Or" },
    }],
};

pub fn build_combine(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let op = p.text("op");
    node(move |ins, _| {
        let (Some(a), Some(b)) = (ins.rhythm(0), ins.rhythm(1)) else {
            return Ok(vec![None]);
        };
        let out = match op.as_str() {
            "And" => a.intersection(b),
            "Xor" => a.xor(b),
            "A not B" => a.difference(b),
            _ => a.union(b),
        }
        .ok_or_else(|| {
            format!(
                "the rhythms have different lengths ({} vs {} ticks)",
                a.length(),
                b.length()
            )
        })?;
        Ok(vec![Some(Value::Rhythm(Box::new(out)))])
    })
}

pub static SWING: NodeSpec = NodeSpec {
    kind: "swing",
    title: "Swing",
    category: "Rhythm",
    help: "Delays every second step. 50% is straight, 67% a triplet feel, 75% dotted.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[
        ParamSpec {
            name: "amount",
            label: "Amount %",
            kind: ParamKind::Number {
                min: 50.0,
                max: 75.0,
                step: 1.0,
                default: 60.0,
            },
        },
        ParamSpec {
            name: "step",
            label: "Grid",
            kind: ParamKind::Choice {
                options: STEP_OPTIONS,
                default: "1/16",
            },
        },
    ],
};

pub fn build_swing(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let amount = p.int("amount") as u8;
    let grid = step_ticks(&p.text("step"));
    node(move |ins, _| {
        let Some(r) = ins.rhythm(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Rhythm(Box::new(r.swing(grid, amount))))])
    })
}

fn rhythm_out(r: Rhythm) -> Result<Vec<Option<Value>>, String> {
    Ok(vec![Some(Value::Rhythm(Box::new(r)))])
}

// ---- Probability -------------------------------------------------------------------

pub static PROBABILITY: NodeSpec = NodeSpec {
    kind: "probability",
    title: "Probability",
    category: "Rhythm",
    help: "Gives every hit a chance of playing, rolled again on each pass, so a pattern thins out \
           and varies. Accents can be kept. Locked, the same hits play every time.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[
        ParamSpec {
            name: "chance",
            label: "Chance %",
            kind: ParamKind::Number { min: 0.0, max: 100.0, step: 1.0, default: 70.0 },
        },
        ParamSpec { name: "accents", label: "Keep accents", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: false } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
    ],
};

pub fn build_probability(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let chance = p.int("chance").clamp(0, 100) as u8;
    let accents = p.flag("accents");
    let lock = p.flag("lock");
    let seed = p.number("seed") as u64;
    node(move |ins, ctx| {
        let Some(r) = ins.rhythm(0) else {
            return Ok(vec![None]);
        };
        let stream = rng::stream(lock, seed, ctx.salt);
        rhythm_out(r.with_chance(chance, accents, rng::hash(&[stream, 0x9e0b])))
    })
}

// ---- Accent ------------------------------------------------------------------------

pub static ACCENT: NodeSpec = NodeSpec {
    kind: "accent",
    title: "Accent",
    category: "Rhythm",
    help: "Sets loudness from an accent pattern laid over the rhythm, one character per step: \
           x or X accents hits on that step, . leaves them soft. The pattern repeats within the rhythm.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[
        ParamSpec { name: "pattern", label: "Accents", kind: ParamKind::Text { default: "x..." } },
        STEP_PARAM,
        ParamSpec { name: "accent", label: "Accent vel", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 127.0 } },
        ParamSpec { name: "soft", label: "Other vel", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 80.0 } },
    ],
};

pub fn build_accent(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let marks: Vec<bool> = p
        .text("pattern")
        .chars()
        .filter(|c| !matches!(c, ' ' | '|'))
        .map(|c| match c {
            'x' | 'X' => Ok(true),
            '.' | '-' => Ok(false),
            _ => Err(format!("accent patterns use x and . (not '{c}')")),
        })
        .collect::<Result<_, _>>()?;
    if marks.is_empty() {
        return Err("the accent pattern is empty".into());
    }
    let step = step_ticks(&p.text("step"));
    let accent = p.int("accent").clamp(1, 127) as u8;
    let soft = p.int("soft").clamp(1, 127) as u8;
    node(move |ins, _| {
        let Some(r) = ins.rhythm(0) else {
            return Ok(vec![None]);
        };
        rhythm_out(r.with_velocities(|o| {
            if marks[(o.offset / step) as usize % marks.len()] {
                accent
            } else {
                soft
            }
        }))
    })
}

// ---- Rotate ------------------------------------------------------------------------

pub static ROTATE: NodeSpec = NodeSpec {
    kind: "rotate",
    title: "Rotate",
    category: "Rhythm",
    help: "Shifts the rhythm later by a number of steps (negative: earlier); hits that fall off \
           the end wrap around to the start.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[
        ParamSpec { name: "steps", label: "Steps", kind: ParamKind::Number { min: -16.0, max: 16.0, step: 1.0, default: 1.0 } },
        STEP_PARAM,
    ],
};

pub fn build_rotate(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let ticks = p.int("steps") as i64 * step_ticks(&p.text("step")) as i64;
    node(move |ins, _| {
        let Some(r) = ins.rhythm(0) else {
            return Ok(vec![None]);
        };
        rhythm_out(r.rotate(ticks))
    })
}

// ---- Subdivide ---------------------------------------------------------------------

pub static SUBDIVIDE: NodeSpec = NodeSpec {
    kind: "subdivide",
    title: "Subdivide",
    category: "Rhythm",
    help: "Splits every hit into quick repeats (ratchets) that fill its length.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("rhythm", PortType::Rhythm)],
    params: &[ParamSpec {
        name: "into",
        label: "Into",
        kind: ParamKind::Number { min: 1.0, max: 8.0, step: 1.0, default: 2.0 },
    }],
};

pub fn build_subdivide(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let n = p.int("into").max(1) as u32;
    node(move |ins, _| {
        let Some(r) = ins.rhythm(0) else {
            return Ok(vec![None]);
        };
        rhythm_out(r.subdivide(n).ok_or("too many hits: a rhythm holds at most 64")?)
    })
}

// ---- Polyrhythm --------------------------------------------------------------------

pub static POLYRHYTHM: NodeSpec = NodeSpec {
    kind: "polyrhythm",
    title: "Polyrhythm",
    category: "Rhythm",
    help: "Two even pulses over the same span, such as 3 against 4: A's hits, B's hits, and both \
           together (where they meet, the hit is accented).",
    inputs: &[],
    outputs: &[port("a", PortType::Rhythm), port("b", PortType::Rhythm), port("both", PortType::Rhythm)],
    params: &[
        ParamSpec { name: "a", label: "A hits", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 3.0 } },
        ParamSpec { name: "b", label: "B hits", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "bars", label: "Over bars", kind: ParamKind::Number { min: 0.25, max: 4.0, step: 0.25, default: 1.0 } },
    ],
};

/// `hits` even onsets over `length`, on the given display grid.
fn pulse(hits: u32, length: Ticks, grid: Ticks) -> Rhythm {
    let mut r = Rhythm::new(length).with_grid(grid);
    for i in 0..hits {
        let offset = (i as u64 * length as u64 / hits as u64) as Ticks;
        let _ = r.add(Onset { offset, duration: (length / hits).max(1), ..Onset::default() });
    }
    r
}

pub fn build_polyrhythm(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let (a, b) = (p.int("a").max(1) as u32, p.int("b").max(1) as u32);
    let length = ((p.number("bars") * Meter::COMMON.ticks_per_bar() as f64).round() as Ticks).max(1);
    let grid = gcd(length / a, length / b).max(1);
    let ra = pulse(a, length, grid);
    let rb = pulse(b, length, grid);
    let mut both = ra.union(&rb).expect("same length");
    for o in ra.onsets() {
        if rb.hits_at(o.offset) {
            let _ = both.add(Onset { velocity: Onset::ACCENT, ..*o });
        }
    }
    node(move |_, _| {
        Ok(vec![
            Some(Value::Rhythm(Box::new(ra))),
            Some(Value::Rhythm(Box::new(rb))),
            Some(Value::Rhythm(Box::new(both))),
        ])
    })
}

fn gcd(a: Ticks, b: Ticks) -> Ticks {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::EvalContext;
    use crate::nodes::{build, spec_for, Inputs};
    use crate::spec::ParamValue;
    use std::collections::HashMap;

    fn run(kind: &str, params: &[(&str, ParamValue)], input: Option<Rhythm>) -> Vec<Rhythm> {
        let values: HashMap<String, ParamValue> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let n = build(spec_for(kind).unwrap(), &values).unwrap();
        let ins = Inputs { values: vec![input.map(|r| Value::Rhythm(Box::new(r)))] };
        n.eval(&ins, &EvalContext::new(0, 960, Meter::COMMON, 5))
            .unwrap()
            .into_iter()
            .map(|v| match v {
                Some(Value::Rhythm(r)) => *r,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn rhythm_transforms() {
        let four = Rhythm::from_grid("xxxx", 240).unwrap();
        let acc = &run("accent", &[("pattern", ParamValue::Text("x.".into()))], Some(four))[0];
        let vels: Vec<u8> = acc.onsets().iter().map(|o| o.velocity).collect();
        assert_eq!(vels, [127, 80, 127, 80]);
        let rot = &run("rotate", &[("steps", ParamValue::Number(1.0))], Some(Rhythm::from_grid("x...", 240).unwrap()))[0];
        assert_eq!(rot.to_string(), ".x..");
        let poly = run("polyrhythm", &[], None);
        assert_eq!(poly[0].len(), 3);
        assert_eq!(poly[1].len(), 4);
        assert_eq!(poly[2].len(), 6); // 3 + 4, sharing the downbeat
        assert_eq!(poly[2].onsets()[0].velocity, Onset::ACCENT);
    }

    #[test]
    fn probability_follows_the_lock() {
        let r = Rhythm::from_grid("xxxxxxxxxxxxxxxx", 240).unwrap();
        let hits = |salt: u64, lock: bool| {
            let values: HashMap<String, ParamValue> =
                [("chance".to_string(), ParamValue::Number(50.0)), ("lock".to_string(), ParamValue::Bool(lock))].into();
            let n = build(spec_for("probability").unwrap(), &values).unwrap();
            let ins = Inputs { values: vec![Some(Value::Rhythm(Box::new(r)))] };
            let Some(Value::Rhythm(out)) = n.eval(&ins, &EvalContext::new(0, 960, Meter::COMMON, salt)).unwrap().remove(0) else {
                panic!()
            };
            let mut t = Vec::new();
            out.for_each_in(0, 3840, |x, _| t.push(x));
            t
        };
        assert_ne!(hits(1, false), hits(2, false));
        assert_eq!(hits(1, true), hits(2, true));
    }
}
