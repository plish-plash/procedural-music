//! Song structure: choosing what plays, and the section being played.

use super::{Inputs, NodeImpl};
use crate::control::Control;
use crate::graph::{section_key, EvalContext};
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;

const SELECT_HELP: &str = "Passes on one of its inputs, chosen by number (0 for none). A Section \
    node can choose by section.";

const SELECT_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "input",
    label: "Input",
    kind: ParamKind::Number { min: 0.0, max: 4.0, step: 1.0, default: 1.0 },
}];

macro_rules! select_spec {
    ($name:ident, $kind:literal, $title:literal, $ty:expr) => {
        pub static $name: NodeSpec = NodeSpec {
            kind: $kind,
            title: $title,
            category: "Structure",
            help: SELECT_HELP,
            inputs: &[
                optional("1", $ty),
                optional("2", $ty),
                optional("3", $ty),
                optional("4", $ty),
            ],
            outputs: &[port("out", $ty)],
            params: SELECT_PARAMS,
        };
    };
}

select_spec!(SELECT_NOTES, "select_notes", "Select (notes)", PortType::Notes);
select_spec!(SELECT_RHYTHM, "select_rhythm", "Select (rhythm)", PortType::Rhythm);
select_spec!(SELECT_CHORD, "select_chord", "Select (chords)", PortType::Chord);
select_spec!(SELECT_SCALE, "select_scale", "Select (scale)", PortType::Scale);

pub fn build_select(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let input = p.int("input");
    super::theory::node(move |ins, _| {
        let chosen = usize::try_from(input - 1).ok().and_then(|i| ins.values.get(i).cloned().flatten());
        Ok(vec![chosen])
    })
}

// ---- Section --------------------------------------------------------------------

pub static SECTION: NodeSpec = NodeSpec {
    kind: "section",
    title: "Section",
    category: "Control",
    help: "A value for each section of the arrangement: 'A = 1, B = 2, Outro = 0'. '*' means any \
           section, and 'A#2' only the second time A comes round; the first entry that matches \
           counts. Default is used for sections not listed, and without an arrangement.",
    inputs: &[],
    outputs: &[port("control", PortType::Control)],
    params: &[
        ParamSpec { name: "values", label: "Values", kind: ParamKind::Text { default: "A = 1, B = 2" } },
        ParamSpec {
            name: "default",
            label: "Default",
            kind: ParamKind::Number { min: -20_000.0, max: 20_000.0, step: 0.01, default: 0.0 },
        },
    ],
};

/// Which sections an entry applies to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pattern {
    Any,
    Section { key: u64, repeat: Option<u32> },
}

impl Pattern {
    fn parse(t: &str) -> Result<Pattern, String> {
        if t == "*" {
            return Ok(Pattern::Any);
        }
        match t.rsplit_once('#') {
            Some((name, n)) => {
                let n: u32 = n
                    .trim()
                    .parse()
                    .map_err(|_| format!("'{t}': expected a number after #"))?;
                if n == 0 {
                    return Err(format!("'{t}': repeats count from 1"));
                }
                Ok(Pattern::Section {
                    key: section_key(name),
                    repeat: Some(n),
                })
            }
            None => Ok(Pattern::Section {
                key: section_key(t),
                repeat: None,
            }),
        }
    }

    fn matches(self, ctx: &EvalContext) -> bool {
        match self {
            Pattern::Any => true,
            Pattern::Section { key, repeat } => {
                key == ctx.section && repeat.is_none_or(|r| r == ctx.repeat)
            }
        }
    }
}

/// Entries like "A = 1, B#2 = 3, * = 0" ('=' or ':').
fn parse_values(text: &str) -> Result<Vec<(Pattern, f64)>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|entry| {
            let (name, value) = entry
                .split_once(['=', ':'])
                .ok_or_else(|| format!("'{entry}': expected a section and a value, like A = 1"))?;
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("'{entry}': which section?"));
            }
            let value: f64 = value
                .trim()
                .parse()
                .map_err(|_| format!("'{entry}': '{}' is not a number", value.trim()))?;
            Ok((Pattern::parse(name)?, value))
        })
        .collect()
}

struct Section {
    values: Vec<(Pattern, f64)>,
    default: f64,
}

pub fn build_section(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    Ok(Box::new(Section {
        values: parse_values(&p.text("values"))?,
        default: p.number("default"),
    }))
}

impl Section {
    /// The value of the first entry matching the section in `ctx`, or the default.
    fn value(&self, ctx: &EvalContext) -> f64 {
        if ctx.section == 0 {
            return self.default;
        }
        self.values
            .iter()
            .find(|(p, _)| p.matches(ctx))
            .map_or(self.default, |&(_, v)| v)
    }
}

impl NodeImpl for Section {
    fn eval(&self, _: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        Ok(vec![Some(Value::Control(Control::Constant(self.value(ctx))))])
    }
}

// ---- Harmonic context -------------------------------------------------------------

pub static CONTEXT: NodeSpec = NodeSpec {
    kind: "context",
    title: "Harmonic context",
    category: "Structure",
    help: "The song's key and chords. Every scale or chord input left unwired anywhere in the \
           patch (inside groups too) reads them from here, so the parts follow one harmony \
           without wires to each; a wire always wins. Without a key, scale inputs get the chords' \
           own notes. Use one per patch.",
    inputs: &[optional("key", PortType::Scale), optional("chords", PortType::Chord)],
    outputs: &[port("key", PortType::Scale), port("chords", PortType::Chord)],
    params: &[],
};

pub fn build_context(_: &Params) -> Result<Box<dyn NodeImpl>, String> {
    super::theory::node(|ins, _| {
        let chords = ins.chord(1).cloned().map(Value::Chord);
        let key = match ins.scale(0) {
            Some(k) => Some(Value::Scale(k.clone())),
            None => chords.as_ref().and_then(|c| c.convert(PortType::Scale)),
        };
        Ok(vec![key, chords])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::tests::{node, wire};
    use crate::graph::{Graph, GraphDoc};
    use crate::spec::ParamValue;
    use pm_theory::Meter;

    #[test]
    fn values_by_section() {
        let section = |values: &str, default| Section { values: parse_values(values).unwrap(), default };
        let ctx = |name: &str, repeat| EvalContext {
            section: section_key(name),
            repeat,
            ..EvalContext::new(0, 1, Meter::COMMON, 0)
        };
        let s = section("A#2 = 3, A = 1, B: -2.5, * = 7", 9.0);
        assert_eq!(s.value(&EvalContext::new(0, 1, Meter::COMMON, 0)), 9.0, "no arrangement: the default");
        assert_eq!(s.value(&ctx("a", 1)), 1.0);
        assert_eq!(s.value(&ctx("A", 2)), 3.0, "the first match wins");
        assert_eq!(s.value(&ctx(" B ", 1)), -2.5);
        assert_eq!(s.value(&ctx("Outro", 1)), 7.0);
        assert_eq!(section("A = 1", 0.0).value(&ctx("Outro", 1)), 0.0, "unlisted: the default");
        for bad in ["A#x = 1", "A#0 = 1", "A", "A = one", "= 2"] {
            assert!(parse_values(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn section_drives_select() {
        let text = |s: &str| ParamValue::Text(s.into());
        let doc = GraphDoc {
            nodes: vec![
                node("c", "chord", &[("symbol", text("C"))]),
                node("g", "chord", &[("symbol", text("G"))]),
                node("sec", "section", &[("values", text("A = 1, B = 2"))]),
                node("sel", "select_chord", &[]),
            ],
            wires: vec![
                wire("c", "chord", "sel", "1"),
                wire("g", "chord", "sel", "2"),
                wire("sec", "control", "sel", "param.input"),
            ],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let root = |section: &str| {
            let ctx = EvalContext {
                section: section_key(section),
                repeat: 1,
                ..EvalContext::new(0, 240, Meter::COMMON, 0)
            };
            match g.output_value("sel", None, &ctx, 0) {
                Ok(Value::Chord(tl)) => Some(tl.at(0).value.notes()[0].step()),
                _ => None,
            }
        };
        assert_eq!([root("A"), root("B"), root("Outro")], [Some(60), Some(67), None]);
    }
}
