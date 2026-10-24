//! Operations on note streams.

use pm_theory::Rounding;

use super::theory::node;
use super::{scale_velocity, NodeImpl};
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
    help: "Scales every note's velocity (loudness).",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[ParamSpec {
        name: "amount",
        label: "Amount %",
        kind: ParamKind::Number {
            min: 0.0,
            max: 200.0,
            step: 5.0,
            default: 80.0,
        },
    }],
};

pub fn build_velocity(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let amount = p.number("amount");
    node(move |ins, _| {
        let mut notes = ins.notes(0).to_vec();
        for n in &mut notes {
            n.velocity = scale_velocity(n.velocity, amount);
        }
        Ok(vec![Some(Value::Notes(notes))])
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
