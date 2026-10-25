//! Song structure: nodes that change what plays from section to section.

use super::{Inputs, NodeImpl};
use crate::graph::{section_key, EvalContext};
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;

const HELP: &str = "Passes on one of its inputs depending on the section of the arrangement. \
    List the sections for each input, separated by commas: 'A, Outro'. '*' means any section, and \
    'A#2' only the second time A comes round. The first input whose list matches plays; if none \
    matches, nothing does. Without an arrangement, input a plays.";

const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "a",
        label: "a in sections",
        kind: ParamKind::Text { default: "*" },
    },
    ParamSpec {
        name: "b",
        label: "b in sections",
        kind: ParamKind::Text { default: "" },
    },
    ParamSpec {
        name: "c",
        label: "c in sections",
        kind: ParamKind::Text { default: "" },
    },
    ParamSpec {
        name: "d",
        label: "d in sections",
        kind: ParamKind::Text { default: "" },
    },
];

macro_rules! switch_spec {
    ($name:ident, $kind:literal, $title:literal, $ty:expr) => {
        pub static $name: NodeSpec = NodeSpec {
            kind: $kind,
            title: $title,
            category: "Structure",
            help: HELP,
            inputs: &[
                optional("a", $ty),
                optional("b", $ty),
                optional("c", $ty),
                optional("d", $ty),
            ],
            outputs: &[port("out", $ty)],
            params: PARAMS,
        };
    };
}

switch_spec!(
    SWITCH_NOTES,
    "switch_notes",
    "Section switch (notes)",
    PortType::Notes
);
switch_spec!(
    SWITCH_RHYTHM,
    "switch_rhythm",
    "Section switch (rhythm)",
    PortType::Rhythm
);
switch_spec!(
    SWITCH_CHORD,
    "switch_chord",
    "Section switch (chords)",
    PortType::Chord
);
switch_spec!(
    SWITCH_SCALE,
    "switch_scale",
    "Section switch (scale)",
    PortType::Scale
);

/// One entry of a section list.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pattern {
    Any,
    Section { key: u64, repeat: Option<u32> },
}

fn parse_list(text: &str) -> Result<Vec<Pattern>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
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
        })
        .collect()
}

struct Switch {
    lists: Vec<Vec<Pattern>>,
}

pub fn build_switch(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let lists = ["a", "b", "c", "d"]
        .iter()
        .map(|name| parse_list(&p.text(name)))
        .collect::<Result<_, _>>()?;
    Ok(Box::new(Switch { lists }))
}

impl Switch {
    fn choose(&self, ctx: &EvalContext) -> Option<usize> {
        if ctx.section == 0 {
            return Some(0);
        }
        self.lists.iter().position(|list| {
            list.iter().any(|p| match *p {
                Pattern::Any => true,
                Pattern::Section { key, repeat } => {
                    key == ctx.section && repeat.is_none_or(|r| r == ctx.repeat)
                }
            })
        })
    }
}

impl NodeImpl for Switch {
    fn eval(&self, ins: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let chosen = self
            .choose(ctx)
            .and_then(|i| ins.values.get(i).cloned().flatten());
        Ok(vec![chosen])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_theory::Meter;

    #[test]
    fn picks_inputs_by_section() {
        let s = Switch {
            lists: vec![
                parse_list("A").unwrap(),
                parse_list("B, A#2").unwrap(),
                parse_list("*").unwrap(),
                vec![],
            ],
        };
        let ctx = |name: &str, repeat| EvalContext {
            section: section_key(name),
            repeat,
            ..EvalContext::new(0, 1, Meter::COMMON, 0)
        };
        assert_eq!(s.choose(&EvalContext::new(0, 1, Meter::COMMON, 0)), Some(0));
        assert_eq!(s.choose(&ctx("a", 1)), Some(0));
        assert_eq!(s.choose(&ctx(" B ", 1)), Some(1));
        assert_eq!(s.choose(&ctx("Outro", 1)), Some(2));
        let only_second = Switch {
            lists: vec![parse_list("A#2").unwrap()],
        };
        assert_eq!(only_second.choose(&ctx("A", 1)), None);
        assert_eq!(only_second.choose(&ctx("A", 2)), Some(0));
        assert!(parse_list("A#x").is_err());
    }
}
