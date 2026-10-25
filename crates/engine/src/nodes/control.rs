//! Control sources: numbers over time that automate other nodes' parameters (wired into a
//! pinned parameter's input).

use super::theory::node;
use super::{step_ticks, NodeImpl, STEP_OPTIONS};
use crate::control::{Control, Shape};
use crate::spec::{port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

/// A value in the units of whatever parameter it drives (so any range).
const fn value(name: &'static str, label: &'static str, default: f64) -> ParamSpec {
    ParamSpec {
        name,
        label,
        kind: ParamKind::Number {
            min: -20_000.0,
            max: 20_000.0,
            step: 0.01,
            default,
        },
    }
}

const fn bars(name: &'static str, label: &'static str, default: f64) -> ParamSpec {
    ParamSpec {
        name,
        label,
        kind: ParamKind::Number {
            min: 0.25,
            max: 64.0,
            step: 0.25,
            default,
        },
    }
}

const CONTROL_OUT: &[crate::spec::PortSpec] = &[port("control", PortType::Control)];

pub static LFO: NodeSpec = NodeSpec {
    kind: "lfo",
    title: "LFO",
    category: "Control",
    help: "A repeating wave between Min and Max, one cycle every so many bars. Wire it into a \
           pinned parameter; values are in that parameter's units.",
    inputs: &[],
    outputs: CONTROL_OUT,
    params: &[
        ParamSpec {
            name: "shape",
            label: "Shape",
            kind: ParamKind::Choice {
                options: &["Sine", "Triangle", "Saw", "Square"],
                default: "Sine",
            },
        },
        bars("cycle", "Cycle bars", 4.0),
        value("min", "Min", 0.0),
        value("max", "Max", 12.0),
        ParamSpec {
            name: "phase",
            label: "Phase %",
            kind: ParamKind::Number {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                default: 0.0,
            },
        },
    ],
};

pub fn build_lfo(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let shape = Shape::from_name(&p.text("shape"));
    let cycle = p.number("cycle");
    let (min, max) = (p.number("min"), p.number("max"));
    let phase = p.number("phase") / 100.0;
    node(move |_, ctx| {
        let cycle = (cycle * ctx.meter.ticks_per_bar() as f64).round() as u32;
        Ok(vec![Some(Value::Control(Control::Lfo {
            shape,
            cycle,
            min,
            max,
            phase,
        }))])
    })
}

pub static RAMP: NodeSpec = NodeSpec {
    kind: "ramp",
    title: "Ramp",
    category: "Control",
    help: "Moves in a straight line from one value to another over so many bars, then holds \
           (or starts again, with Loop). Each section starts it from the beginning.",
    inputs: &[],
    outputs: CONTROL_OUT,
    params: &[
        value("from", "From", 0.0),
        value("to", "To", 1.0),
        bars("bars", "Bars", 8.0),
        ParamSpec {
            name: "loop",
            label: "Loop",
            kind: ParamKind::Bool { default: false },
        },
    ],
};

pub fn build_ramp(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let (from, to) = (p.number("from"), p.number("to"));
    let bars = p.number("bars");
    let looped = p.flag("loop");
    node(move |_, ctx| {
        let length = (bars * ctx.meter.ticks_per_bar() as f64).round() as u32;
        Ok(vec![Some(Value::Control(Control::Ramp {
            from,
            to,
            length,
            looped,
        }))])
    })
}

pub static STEPS: NodeSpec = NodeSpec {
    kind: "steps",
    title: "Steps",
    category: "Control",
    help: "A looping sequence of values, each held for one step: \"0 5 7 5\" with a 1/1 step \
           changes once a bar.",
    inputs: &[],
    outputs: CONTROL_OUT,
    params: &[
        ParamSpec {
            name: "values",
            label: "Values",
            kind: ParamKind::Text { default: "0 5 7 5" },
        },
        ParamSpec {
            name: "step",
            label: "Step",
            kind: ParamKind::Choice {
                options: STEP_OPTIONS,
                default: "1/1",
            },
        },
    ],
};

pub fn build_steps(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let step = step_ticks(&p.text("step"));
    let values = p
        .text("values")
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>().map_err(|_| format!("'{s}' is not a number")))
        .collect::<Result<Vec<f64>, String>>()?;
    if values.len() > 64 {
        return Err("at most 64 values".into());
    }
    let timeline = Timeline::from_steps(values.into_iter().map(|v| (step, v)).collect())
        .ok_or("enter some values")?;
    node(move |_, _| Ok(vec![Some(Value::Control(Control::Steps(timeline.clone())))]))
}
