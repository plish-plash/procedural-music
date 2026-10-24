//! Graph documents, compilation and evaluation.

use std::collections::HashMap;
use std::fmt::Write as _;

use pm_theory::{Chord, Meter, Note, Pitch, Scale, Ticks};

use crate::nodes::{self, Inputs, NodeImpl};
use crate::spec::{NodeSpec, ParamValue, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

/// A node as saved in a project file.
#[derive(Clone, Debug, Default)]
pub struct NodeDoc {
    pub id: String,
    pub kind: String,
    pub params: HashMap<String, ParamValue>,
}

/// A wire as saved in a project file. Ports are named, so files survive reordering.
#[derive(Clone, Debug, Default)]
pub struct WireDoc {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

#[derive(Clone, Debug, Default)]
pub struct GraphDoc {
    pub nodes: Vec<NodeDoc>,
    pub wires: Vec<WireDoc>,
}

/// The time window and settings for one evaluation.
#[derive(Clone, Copy, Debug)]
pub struct EvalContext {
    /// Inclusive start tick.
    pub start: Ticks,
    /// Exclusive end tick.
    pub end: Ticks,
    pub meter: Meter,
    /// Changes every time playback starts; unlocked random nodes mix it in.
    pub salt: u64,
}

/// A problem with one node (or with the graph, when `node` is empty).
#[derive(Clone, Debug, PartialEq)]
pub struct NodeIssue {
    pub node: String,
    pub message: String,
}

/// What the UI shows after the graph changes.
#[derive(Clone, Debug, Default)]
pub struct CompileReport {
    pub issues: Vec<NodeIssue>,
    /// (node id, multi-line text) for every node that has something to show.
    pub previews: Vec<(String, String)>,
}

/// Notes leaving the graph through one output node.
#[derive(Clone, Debug)]
pub struct SinkOutput {
    pub node: String,
    pub port: String,
    pub channel: u8,
    pub notes: Vec<Note>,
}

struct CompiledNode {
    id: String,
    spec: &'static NodeSpec,
    imp: Option<Box<dyn NodeImpl>>,
    /// For each input port: (source node index, source output index).
    inputs: Vec<Option<(usize, usize)>>,
}

/// A compiled graph, ready to evaluate.
#[derive(Default)]
pub struct Graph {
    nodes: Vec<CompiledNode>,
    /// Evaluation order (sources first). Nodes in cycles are left out.
    order: Vec<usize>,
    issues: Vec<NodeIssue>,
}

struct Evaluation {
    outputs: Vec<Vec<Option<Value>>>,
    sinks: Vec<SinkOutput>,
    issues: Vec<NodeIssue>,
}

impl Graph {
    /// Builds a graph. Problems (unknown nodes, bad parameters, mismatched wires,
    /// cycles) are recorded as issues and the affected parts are left out.
    pub fn compile(doc: &GraphDoc) -> Graph {
        let mut issues = Vec::new();
        let mut nodes: Vec<CompiledNode> = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();

        for n in &doc.nodes {
            let Some(spec) = nodes::spec_for(&n.kind) else {
                issues.push(NodeIssue {
                    node: n.id.clone(),
                    message: format!("unknown node type '{}'", n.kind),
                });
                continue;
            };
            let imp = match nodes::build(spec, &n.params) {
                Ok(imp) => Some(imp),
                Err(message) => {
                    issues.push(NodeIssue {
                        node: n.id.clone(),
                        message,
                    });
                    None
                }
            };
            if index.insert(&n.id, nodes.len()).is_some() {
                issues.push(NodeIssue {
                    node: n.id.clone(),
                    message: "duplicate node id".into(),
                });
                continue;
            }
            nodes.push(CompiledNode {
                id: n.id.clone(),
                spec,
                imp,
                inputs: vec![None; spec.inputs.len()],
            });
        }

        for w in &doc.wires {
            let (Some(&from), Some(&to)) = (
                index.get(w.from_node.as_str()),
                index.get(w.to_node.as_str()),
            ) else {
                continue; // wire to a node that failed to load
            };
            let out_port = nodes[from]
                .spec
                .outputs
                .iter()
                .position(|p| p.name == w.from_port);
            let in_port = nodes[to]
                .spec
                .inputs
                .iter()
                .position(|p| p.name == w.to_port);
            let (Some(op), Some(ip)) = (out_port, in_port) else {
                issues.push(NodeIssue {
                    node: w.to_node.clone(),
                    message: format!("no port '{}'", w.to_port),
                });
                continue;
            };
            let from_ty = nodes[from].spec.outputs[op].ty;
            let to_ty = nodes[to].spec.inputs[ip].ty;
            if !from_ty.connects_to(to_ty) {
                issues.push(NodeIssue {
                    node: w.to_node.clone(),
                    message: format!("a {} can't feed a {} input", from_ty.name(), to_ty.name()),
                });
                continue;
            }
            nodes[to].inputs[ip] = Some((from, op));
        }

        // Topological order (Kahn). Whatever is left is in a cycle.
        let mut indegree: Vec<usize> = nodes
            .iter()
            .map(|n| n.inputs.iter().flatten().count())
            .collect();
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            for &(src, _) in n.inputs.iter().flatten() {
                dependents[src].push(i);
            }
        }
        let mut ready: Vec<usize> = (0..nodes.len()).filter(|&i| indegree[i] == 0).collect();
        let mut order = Vec::with_capacity(nodes.len());
        while let Some(i) = ready.pop() {
            order.push(i);
            for &d in &dependents[i] {
                indegree[d] -= 1;
                if indegree[d] == 0 {
                    ready.push(d);
                }
            }
        }
        for (i, n) in nodes.iter().enumerate() {
            if !order.contains(&i) {
                issues.push(NodeIssue {
                    node: n.id.clone(),
                    message: "part of a loop; loops are not allowed".into(),
                });
            }
        }

        Graph {
            nodes,
            order,
            issues,
        }
    }

    /// Program changes requested by output nodes: (port, channel, program).
    pub fn programs(&self) -> Vec<(String, u8, u8)> {
        self.nodes
            .iter()
            .filter_map(|n| n.imp.as_ref()?.program())
            .collect()
    }

    /// Issues found while compiling.
    pub fn issues(&self) -> &[NodeIssue] {
        &self.issues
    }

    fn run(&self, ctx: &EvalContext) -> Evaluation {
        let mut outputs: Vec<Vec<Option<Value>>> = self
            .nodes
            .iter()
            .map(|n| vec![None; n.spec.outputs.len()])
            .collect();
        let mut sinks = Vec::new();
        let mut issues = Vec::new();
        for &i in &self.order {
            let node = &self.nodes[i];
            let Some(imp) = &node.imp else { continue };
            let mut values = Vec::with_capacity(node.inputs.len());
            let mut missing = false;
            for (slot, spec) in node.inputs.iter().zip(node.spec.inputs) {
                let v = slot
                    .and_then(|(src, out)| outputs[src][out].as_ref())
                    .and_then(|v| v.convert(spec.ty));
                missing |= v.is_none() && !spec.optional;
                values.push(v);
            }
            if missing {
                continue;
            }
            let inputs = Inputs { values };
            if imp.is_sink() {
                if let Some((port, channel)) = imp.sink() {
                    sinks.push(SinkOutput {
                        node: node.id.clone(),
                        port,
                        channel,
                        notes: inputs.notes(0).to_vec(),
                    });
                }
                continue;
            }
            match imp.eval(&inputs, ctx) {
                Ok(mut out) => {
                    out.resize(node.spec.outputs.len(), None);
                    outputs[i] = out;
                }
                Err(message) => issues.push(NodeIssue {
                    node: node.id.clone(),
                    message,
                }),
            }
        }
        Evaluation {
            outputs,
            sinks,
            issues,
        }
    }

    /// Notes that start in the window, grouped by output node.
    pub fn evaluate(&self, ctx: &EvalContext) -> Vec<SinkOutput> {
        let mut sinks = self.run(ctx).sinks;
        for s in &mut sinks {
            s.notes
                .retain(|n| n.start >= ctx.start && n.start < ctx.end);
        }
        sinks
    }

    /// Issues plus a short text preview of every node's output, evaluated over the
    /// first `bars` bars.
    pub fn report(&self, meter: Meter, bars: u32) -> CompileReport {
        let ctx = EvalContext {
            start: 0,
            end: meter.ticks_per_bar() * bars.max(1),
            meter,
            salt: 0,
        };
        let eval = self.run(&ctx);
        let mut issues = self.issues.clone();
        issues.extend(eval.issues);
        let mut previews = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            let text = if let Some(sink) = eval.sinks.iter().find(|s| s.node == node.id) {
                let port = if sink.port.is_empty() {
                    "Default port"
                } else {
                    &sink.port
                };
                format!(
                    "{port} · ch {}\n{}",
                    sink.channel + 1,
                    describe_notes(&sink.notes, meter)
                )
            } else if let Some(v) = eval.outputs[i].iter().flatten().next() {
                describe(v, meter)
            } else {
                match node
                    .spec
                    .inputs
                    .iter()
                    .zip(&node.inputs)
                    .find(|(s, slot)| !s.optional && slot.is_none())
                {
                    Some((spec, _)) => format!("connect {}", spec.name),
                    None => continue,
                }
            };
            previews.push((node.id.clone(), text));
        }
        CompileReport { issues, previews }
    }
}

// ---- Preview text ------------------------------------------------------------

fn pc_names(scale: &Scale) -> String {
    (0..scale.len() as i32)
        .map(|d| strip_octave(&scale.pitch(d, 4).to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_octave(name: &str) -> String {
    if name.contains('\\') {
        return name.to_string();
    }
    name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
        .to_string()
}

fn bar_label(start: Ticks, meter: Meter) -> String {
    let (bar, beat, tick) = meter.position(start);
    if beat == 0 && tick == 0 {
        format!("{}", bar + 1)
    } else {
        format!("{}.{}", bar + 1, beat + 1)
    }
}

fn describe_timeline<T>(tl: &Timeline<T>, meter: Meter, line: impl Fn(&T) -> String) -> String {
    if tl.is_constant() {
        return line(tl.values().next().expect("non-empty"));
    }
    let mut out = String::new();
    let mut start = 0;
    for (i, (duration, v)) in tl.steps().iter().enumerate() {
        if i == 12 {
            let _ = write!(out, "… {} more", tl.steps().len() - 12);
            break;
        }
        let _ = writeln!(out, "{:>4}  {}", bar_label(start, meter), line(v));
        start += duration;
    }
    out.trim_end().to_string()
}

fn describe(v: &Value, meter: Meter) -> String {
    match v {
        Value::Scale(tl) => {
            describe_timeline(tl, meter, |s: &Scale| format!("{s} · {}", pc_names(s)))
        }
        Value::Chord(tl) => describe_timeline(tl, meter, |c: &Chord| {
            let notes: Vec<String> = c.notes().iter().map(Pitch::to_string).collect();
            format!("{c}  {}", notes.join(" "))
        }),
        Value::Rhythm(r) => format!("{r}"),
        Value::Notes(n) => describe_notes(n, meter),
    }
}

fn describe_notes(notes: &[Note], meter: Meter) -> String {
    if notes.is_empty() {
        return "no notes".into();
    }
    let bar = meter.ticks_per_bar();
    let first_bar: Vec<String> = notes
        .iter()
        .filter(|n| n.start < bar)
        .take(16)
        .map(|n| n.pitch.to_string())
        .collect();
    let last_bar = notes.iter().map(|n| n.start / bar + 1).max().unwrap_or(1);
    let mut text = if last_bar > 1 {
        format!("{} notes in bars 1–{last_bar}", notes.len())
    } else {
        format!("{} notes in bar 1", notes.len())
    };
    if !first_bar.is_empty() {
        let _ = write!(text, "\n{}", first_bar.join(" "));
    }
    text
}

impl PortType {
    /// Port types as the UI lists them, with which outputs feed which inputs.
    pub fn compatibility() -> Vec<(PortType, PortType)> {
        let all = PortType::all();
        all.iter()
            .flat_map(|&a| {
                all.iter()
                    .filter(move |&&b| a.connects_to(b))
                    .map(move |&b| (a, b))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_theory::PPQ;

    pub(crate) fn node(id: &str, kind: &str, params: &[(&str, ParamValue)]) -> NodeDoc {
        NodeDoc {
            id: id.into(),
            kind: kind.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    pub(crate) fn wire(from: &str, fp: &str, to: &str, tp: &str) -> WireDoc {
        WireDoc {
            from_node: from.into(),
            from_port: fp.into(),
            to_node: to.into(),
            to_port: tp.into(),
        }
    }

    fn text(s: &str) -> ParamValue {
        ParamValue::Text(s.into())
    }

    /// Patch 2 from the design document: Progression → Voice → Voice lead → Play chords → MIDI out.
    pub(crate) fn patch_two() -> GraphDoc {
        GraphDoc {
            nodes: vec![
                node("prog", "progression", &[("chords", text("I vi IV V"))]),
                node("voice", "voice", &[]),
                node("lead", "voice_lead", &[]),
                node("play", "play_chords", &[]),
                node("out", "midi_out", &[]),
            ],
            wires: vec![
                wire("prog", "chords", "voice", "chords"),
                wire("voice", "chords", "lead", "chords"),
                wire("lead", "chords", "play", "chords"),
                wire("play", "notes", "out", "notes"),
            ],
        }
    }

    #[test]
    fn patch_two_plays_voice_led_chords() {
        let g = Graph::compile(&patch_two());
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let bar = Meter::COMMON.ticks_per_bar();
        let out = g.evaluate(&EvalContext {
            start: 0,
            end: bar * 4,
            meter: Meter::COMMON,
            salt: 0,
        });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, 0);
        let notes = &out[0].notes;
        assert_eq!(notes.len(), 16, "four chords of four voices");
        assert!(notes
            .iter()
            .all(|n| n.pitch >= Pitch::parse("C3").unwrap()
                && n.pitch <= Pitch::parse("C5").unwrap()));
        // A window in the middle returns only the notes that start in it.
        let mid = g.evaluate(&EvalContext {
            start: bar,
            end: bar + PPQ,
            meter: Meter::COMMON,
            salt: 0,
        });
        assert_eq!(mid[0].notes.len(), 4);
        assert!(mid[0].notes.iter().all(|n| n.start == bar));
    }

    #[test]
    fn previews_and_issues() {
        let mut doc = patch_two();
        doc.nodes
            .push(node("bad", "chord", &[("symbol", text("Cfoo"))]));
        doc.nodes.push(node("lonely", "arp", &[]));
        doc.wires.push(wire("prog", "chords", "lonely", "rhythm")); // wrong type
        let g = Graph::compile(&doc);
        let report = g.report(Meter::COMMON, 4);
        let issue = |id: &str| {
            report
                .issues
                .iter()
                .find(|i| i.node == id)
                .map(|i| i.message.clone())
        };
        assert_eq!(issue("bad").unwrap(), "unknown chord symbol 'Cfoo'");
        assert_eq!(
            issue("lonely").unwrap(),
            "a chord can't feed a rhythm input"
        );
        let preview = |id: &str| {
            report
                .previews
                .iter()
                .find(|p| p.0 == id)
                .map(|p| p.1.clone())
                .unwrap_or_default()
        };
        assert!(
            preview("prog").starts_with("   1  C  C4 E4 G4"),
            "{}",
            preview("prog")
        );
        assert!(preview("lead").lines().count() == 4);
        assert_eq!(preview("lonely"), "connect scale");
        assert!(
            preview("out").starts_with("Default port · ch 1\n16 notes in bars 1–4"),
            "{}",
            preview("out")
        );
    }

    #[test]
    fn implicit_chord_to_scale_and_cycles() {
        let doc = GraphDoc {
            nodes: vec![
                node("c", "chord", &[("symbol", text("Am7"))]),
                node("m", "mode", &[]),
                node("t1", "transpose", &[]),
                node("t2", "transpose", &[]),
            ],
            wires: vec![
                wire("c", "chord", "m", "scale"),
                wire("t1", "chords", "t2", "chords"),
                wire("t2", "chords", "t1", "chords"),
            ],
        };
        let g = Graph::compile(&doc);
        let report = g.report(Meter::COMMON, 1);
        assert!(
            report
                .previews
                .iter()
                .any(|(id, t)| id == "m" && t.contains("A C E G")),
            "{:?}",
            report.previews
        );
        assert_eq!(
            report
                .issues
                .iter()
                .filter(|i| i.message.contains("loop"))
                .count(),
            2
        );
    }

    #[test]
    fn scale_preview() {
        let doc = GraphDoc {
            nodes: vec![
                node(
                    "s",
                    "scale",
                    &[("root", text("D")), ("scale", text("Dorian"))],
                ),
                node("m", "mode", &[("degree", ParamValue::Number(3.0))]),
            ],
            wires: vec![wire("s", "scale", "m", "scale")],
        };
        let r = Graph::compile(&doc).report(Meter::COMMON, 1);
        assert_eq!(r.previews[0].1, "D Dorian · D E F G A B C");
        assert_eq!(r.previews[1].1, "F Lydian · F G A B C D E");
    }
}
