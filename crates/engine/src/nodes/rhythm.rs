//! Rhythm sources and transforms.

use super::theory::node;
use super::{step_ticks, NodeImpl, STEP_OPTIONS};
use crate::spec::{port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;
use pm_theory::Rhythm;

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
