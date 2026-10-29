//! Node and port descriptions shared with the UI.

use std::collections::HashMap;

/// Wire types. Colors and shapes are chosen by the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortType {
    Scale,
    Chord,
    Rhythm,
    Notes,
    /// A stereo audio stream inside the built-in sound engine.
    Audio,
    /// A number over time, which drives a parameter.
    Control,
}

/// Prefix of the input port that automates a parameter: `param.cutoff`.
pub const PARAM_PORT: &str = "param.";

impl PortType {
    pub fn name(self) -> &'static str {
        match self {
            PortType::Scale => "scale",
            PortType::Chord => "chord",
            PortType::Rhythm => "rhythm",
            PortType::Notes => "notes",
            PortType::Audio => "audio",
            PortType::Control => "control",
        }
    }

    pub fn from_name(name: &str) -> Option<PortType> {
        Some(match name {
            "scale" => PortType::Scale,
            "chord" => PortType::Chord,
            "rhythm" => PortType::Rhythm,
            "notes" => PortType::Notes,
            "audio" => PortType::Audio,
            "control" => PortType::Control,
            _ => return None,
        })
    }

    /// Whether an output of this type may feed an input of type `to`.
    pub fn connects_to(self, to: PortType) -> bool {
        self == to || (self == PortType::Chord && to == PortType::Scale)
    }

    pub fn all() -> [PortType; 6] {
        [
            PortType::Scale,
            PortType::Chord,
            PortType::Rhythm,
            PortType::Notes,
            PortType::Audio,
            PortType::Control,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PortSpec {
    pub name: &'static str,
    pub ty: PortType,
    /// Inputs only: whether the node works without it.
    pub optional: bool,
}

pub const fn port(name: &'static str, ty: PortType) -> PortSpec {
    PortSpec {
        name,
        ty,
        optional: false,
    }
}

pub const fn optional(name: &'static str, ty: PortType) -> PortSpec {
    PortSpec {
        name,
        ty,
        optional: true,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ParamKind {
    Number {
        min: f64,
        max: f64,
        step: f64,
        default: f64,
    },
    /// A number shown on a logarithmic slider (frequencies, times). `min` must be > 0.
    LogNumber {
        min: f64,
        max: f64,
        default: f64,
    },
    Text {
        default: &'static str,
    },
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
    Bool {
        default: bool,
    },
    /// A MIDI output port name, chosen from the ports the app finds.
    MidiPort,
    /// A set of pitch classes as note names ("C D Eb G A"), picked on a one-octave keyboard.
    PitchSet {
        default: &'static str,
    },
}

/// Settings shown only while another setting of the same node has one of some values:
/// (node kind, setting, the setting it depends on, the values that show it).
static CONDITIONS: &[(&str, &str, &str, &[&str])] = &[
    ("scale", "notes", "scale", &["Custom"]),
    ("progression", "size", "notation", &["Degrees"]),
    ("transpose", "semitones", "by", &["Semitones"]),
    ("transpose", "steps", "by", &["Scale steps"]),
    ("midi_cc", "cc", "message", &["Control change"]),
];

/// When `param` of `node` is shown only for some values of another setting: that setting
/// and those values.
pub fn condition(node: &str, param: &str) -> Option<(&'static str, &'static [&'static str])> {
    CONDITIONS
        .iter()
        .find(|(k, p, _, _)| *k == node && *p == param)
        .map(|&(_, _, on, values)| (on, values))
}

/// Nodes whose preview shows another output than their first: (node kind, output).
static PREVIEW_OUTPUTS: &[(&str, &str)] = &[("polyrhythm", "both")];

impl NodeSpec {
    /// The output the node's preview shows (and plays).
    pub fn preview_output(&self) -> usize {
        PREVIEW_OUTPUTS
            .iter()
            .find(|(k, _)| *k == self.kind)
            .and_then(|(_, name)| self.outputs.iter().position(|o| o.name == *name))
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
}

impl ParamSpec {
    /// Whether a Control input can drive this parameter of `node`: numbers, except seeds
    /// (which pick a stream, not an amount), on nodes that keep state between evaluations
    /// (the counterpoint solver, piano rolls) and on MIDI outputs, whose channel and
    /// program aren't meant to move.
    pub fn automatable(&self, node: &NodeSpec) -> bool {
        matches!(self.kind, ParamKind::Number { .. } | ParamKind::LogNumber { .. })
            && self.name != "seed"
            && !matches!(node.kind, "counterpoint" | "piano_roll" | "midi_out" | "midi_cc")
    }

    /// `value` snapped to this parameter's grid (its step, or three significant figures
    /// on a log scale) and range, so automation builds a bounded number of node variants.
    pub fn snap(&self, value: f64) -> f64 {
        match self.kind {
            ParamKind::Number { min, max, step, .. } => {
                let v = if step > 0.0 { (value / step).round() * step } else { value };
                v.clamp(min, max)
            }
            ParamKind::LogNumber { min, max, .. } => {
                let v = value.clamp(min, max);
                let m = 10f64.powi(2 - v.abs().log10().floor() as i32);
                (v * m).round() / m
            }
            _ => value,
        }
    }

    pub fn default_value(&self) -> ParamValue {
        match self.kind {
            ParamKind::Number { default, .. } | ParamKind::LogNumber { default, .. } => {
                ParamValue::Number(default)
            }
            ParamKind::Text { default } | ParamKind::PitchSet { default } => {
                ParamValue::Text(default.to_string())
            }
            ParamKind::Choice { default, .. } => ParamValue::Text(default.to_string()),
            ParamKind::Bool { default } => ParamValue::Bool(default),
            ParamKind::MidiPort => ParamValue::Text(String::new()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Number(f64),
    Text(String),
    Bool(bool),
}

#[derive(Debug)]
pub struct NodeSpec {
    pub kind: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub help: &'static str,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    pub params: &'static [ParamSpec],
}

/// Parameter values for one node, read with defaults from its spec.
pub struct Params<'a> {
    spec: &'static NodeSpec,
    values: &'a HashMap<String, ParamValue>,
}

impl<'a> Params<'a> {
    pub fn new(spec: &'static NodeSpec, values: &'a HashMap<String, ParamValue>) -> Self {
        Params { spec, values }
    }

    fn get(&self, name: &str) -> ParamValue {
        let spec = self
            .spec
            .params
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("node {} has no param {name}", self.spec.kind));
        match (self.values.get(name), spec.kind) {
            (
                Some(ParamValue::Number(n)),
                ParamKind::Number { min, max, .. } | ParamKind::LogNumber { min, max, .. },
            ) => ParamValue::Number(n.clamp(min, max)),
            (Some(v @ ParamValue::Number(_)), _) => v.clone(),
            (
                Some(v @ ParamValue::Text(_)),
                ParamKind::Text { .. } | ParamKind::MidiPort | ParamKind::PitchSet { .. },
            ) => {
                v.clone()
            }
            (Some(ParamValue::Text(t)), ParamKind::Choice { options, default }) => {
                ParamValue::Text(if options.contains(&t.as_str()) {
                    t.clone()
                } else {
                    default.to_string()
                })
            }
            (Some(v @ ParamValue::Bool(_)), ParamKind::Bool { .. }) => v.clone(),
            _ => spec.default_value(),
        }
    }

    pub fn number(&self, name: &str) -> f64 {
        match self.get(name) {
            ParamValue::Number(n) => n,
            _ => 0.0,
        }
    }

    pub fn int(&self, name: &str) -> i32 {
        self.number(name).round() as i32
    }

    pub fn text(&self, name: &str) -> String {
        match self.get(name) {
            ParamValue::Text(t) => t,
            _ => String::new(),
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        matches!(self.get(name), ParamValue::Bool(true))
    }
}
