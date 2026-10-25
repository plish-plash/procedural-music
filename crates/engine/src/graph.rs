//! Graph documents, compilation and evaluation.

use std::collections::HashMap;
use std::fmt::Write as _;

use pm_theory::{Chord, Meter, Note, Pitch, Scale, Ticks};

use pm_synth::{ModuleDesc, RackDesc};

use crate::json::Json;
use crate::nodes::{self, Inputs, NodeImpl, Sink};
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
    /// The arrangement, in order. Empty means the graph simply plays from bar 1.
    pub sections: Vec<SectionDoc>,
    /// Part name (the title of the top-level group) for nodes inside groups.
    pub parts: HashMap<String, String>,
}

/// One section of the arrangement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionDoc {
    pub name: String,
    pub bars: u32,
    /// Tempo for this section; `None` uses the song tempo.
    pub bpm: Option<f64>,
    /// Parameter values that differ in this section: (node id, parameter, value), sorted.
    pub overrides: Vec<(String, String, ParamValue)>,
}

impl GraphDoc {
    /// The graph as it plays in one section: its parameter overrides applied.
    pub fn with_overrides(&self, overrides: &[(String, String, ParamValue)]) -> GraphDoc {
        let mut doc = GraphDoc {
            nodes: self.nodes.clone(),
            wires: self.wires.clone(),
            ..Default::default()
        };
        for (node, param, value) in overrides {
            if let Some(n) = doc.nodes.iter_mut().find(|n| &n.id == node) {
                n.params.insert(param.clone(), value.clone());
            }
        }
        doc
    }
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
    /// True when evaluating for the editor's previews (display-only nodes run).
    pub preview: bool,
    /// True when slow work must finish now (offline rendering, tests) rather than
    /// continue in the background.
    pub sync: bool,
    /// The section being played, as [`section_key`] of its name (0 without an arrangement).
    pub section: u64,
    /// Which time this section name comes round in the arrangement (1 for the first).
    pub repeat: u32,
}

/// A section name as nodes compare it: case and surrounding spaces don't matter.
pub fn section_key(name: &str) -> u64 {
    let k = name
        .trim()
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
        });
    k.max(1)
}

impl EvalContext {
    /// A live-playback context.
    pub fn new(start: Ticks, end: Ticks, meter: Meter, salt: u64) -> Self {
        EvalContext {
            start,
            end,
            meter,
            salt,
            preview: false,
            sync: false,
            section: 0,
            repeat: 0,
        }
    }

    /// The context the editor's previews use: the first `bars` bars, as heard in a
    /// section (see [`EvalContext::section`]).
    pub fn preview(meter: Meter, bars: u32, sync: bool, section: u64, repeat: u32) -> Self {
        EvalContext {
            preview: true,
            sync,
            section,
            repeat,
            ..EvalContext::new(0, meter.ticks_per_bar() * bars.max(1), meter, 0)
        }
    }
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
    /// (node id, structured view such as a piano roll) for nodes that draw one.
    pub views: Vec<(String, Json)>,
}

/// Notes leaving the graph through one sink node (MIDI out or a built-in instrument).
#[derive(Clone, Debug)]
pub struct SinkOutput {
    pub node: String,
    /// The node's id in the audio rack (see [`module_id`]).
    pub module: u64,
    pub sink: Sink,
    pub notes: Vec<Note>,
}

/// A node's stable id in the audio rack: a hash of its node id, so sound state and
/// in-flight notes follow the node across edits.
pub fn module_id(node_id: &str) -> u64 {
    node_id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
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
    /// For each node, its upstream nodes in evaluation order (for phrase-length inputs).
    ancestors: Vec<Vec<usize>>,
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

        // Ancestors of each node, in evaluation order.
        let mut upstream = vec![vec![false; nodes.len()]; nodes.len()];
        for &i in &order {
            for &(src, _) in nodes[i].inputs.iter().flatten() {
                let from = upstream[src].clone();
                for (k, flag) in from.into_iter().enumerate() {
                    upstream[i][k] |= flag;
                }
                upstream[i][src] = true;
            }
        }
        let ancestors = (0..nodes.len())
            .map(|i| order.iter().copied().filter(|&k| upstream[i][k]).collect())
            .collect();

        Graph {
            nodes,
            order,
            ancestors,
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

    /// The sound nodes as an audio rack: instruments, effects, mixers and outputs.
    pub fn rack_desc(&self) -> RackDesc {
        let mut modules = Vec::new();
        for &i in &self.order {
            let node = &self.nodes[i];
            let Some(kind) = node.imp.as_ref().and_then(|imp| imp.module()) else {
                continue;
            };
            let inputs = node
                .spec
                .inputs
                .iter()
                .zip(&node.inputs)
                .filter(|(spec, _)| spec.ty == PortType::Audio)
                .map(|(_, slot)| slot.map(|(src, _)| module_id(&self.nodes[src].id)))
                .collect();
            modules.push(ModuleDesc {
                id: module_id(&node.id),
                kind,
                inputs,
            });
        }
        RackDesc { modules }
    }

    /// Issues found while compiling.
    pub fn issues(&self) -> &[NodeIssue] {
        &self.issues
    }

    fn run(&self, ctx: &EvalContext) -> Evaluation {
        self.run_subset(&self.order, ctx)
    }

    /// Evaluates `subset` (in order). A node that needs its inputs over a longer window
    /// (a whole phrase) gets them from a separate evaluation of its ancestors.
    fn run_subset(&self, subset: &[usize], ctx: &EvalContext) -> Evaluation {
        let mut outputs: Vec<Vec<Option<Value>>> = self
            .nodes
            .iter()
            .map(|n| vec![None; n.spec.outputs.len()])
            .collect();
        let mut sinks = Vec::new();
        let mut issues = Vec::new();
        for &i in subset {
            let node = &self.nodes[i];
            let Some(imp) = &node.imp else { continue };
            // Effects, mixers and audio outputs only process audio, in the rack; display
            // nodes only run for previews.
            if (!imp.is_sink()
                && !node.spec.outputs.is_empty()
                && node.spec.outputs.iter().all(|o| o.ty == PortType::Audio))
                || (imp.view_only() && !ctx.preview)
                || (node.spec.outputs.is_empty() && !imp.is_sink() && !imp.view_only())
            {
                continue;
            }
            let wider = imp.input_window(ctx).filter(|&w| w != (ctx.start, ctx.end));
            let sub = wider.map(|(start, end)| {
                self.run_subset(&self.ancestors[i], &EvalContext { start, end, ..*ctx })
            });
            let source = sub.as_ref().map_or(&outputs, |s| &s.outputs);
            let mut values = Vec::with_capacity(node.inputs.len());
            let mut missing = false;
            for (slot, spec) in node.inputs.iter().zip(node.spec.inputs) {
                let v = slot
                    .and_then(|(src, out)| source[src][out].as_ref())
                    .and_then(|v| v.convert(spec.ty));
                missing |= v.is_none() && !spec.optional;
                values.push(v);
            }
            if missing {
                continue;
            }
            let inputs = Inputs { values };
            if imp.is_sink() {
                if let Some(sink) = imp.sink() {
                    sinks.push(SinkOutput {
                        node: node.id.clone(),
                        module: module_id(&node.id),
                        sink,
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

    /// The value on one of a node's outputs (the first when `port` is `None`).
    pub fn output_value(&self, node: &str, port: Option<&str>, ctx: &EvalContext) -> Result<Value, String> {
        let i = self
            .nodes
            .iter()
            .position(|n| n.id == node)
            .ok_or_else(|| format!("no node '{node}'"))?;
        let outputs = self.nodes[i].spec.outputs;
        let out = match port {
            Some(name) => outputs
                .iter()
                .position(|p| p.name == name)
                .ok_or_else(|| format!("no output '{name}'"))?,
            None if outputs.is_empty() => return Err("this node has no output".into()),
            None => 0,
        };
        self.run(ctx).outputs[i][out]
            .take()
            .ok_or_else(|| "no value yet: connect its inputs".into())
    }

    /// Issues plus a short text preview of every node's output, evaluated over the
    /// first `bars` bars.
    pub fn report(&self, meter: Meter, bars: u32, sync: bool) -> CompileReport {
        self.report_section(meter, bars, sync, 0, 0)
    }

    /// Like [`Graph::report`], as heard in a section (see [`EvalContext::section`]).
    pub fn report_section(
        &self,
        meter: Meter,
        bars: u32,
        sync: bool,
        section: u64,
        repeat: u32,
    ) -> CompileReport {
        let ctx = EvalContext::preview(meter, bars, sync, section, repeat);
        let eval = self.run(&ctx);
        let mut issues = self.issues.clone();
        issues.extend(eval.issues);
        let mut previews = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            let text = if let Some(out) = eval.sinks.iter().find(|s| s.node == node.id) {
                match &out.sink {
                    Sink::Midi { port, channel } => {
                        let port = if port.is_empty() {
                            "Default port"
                        } else {
                            port
                        };
                        format!(
                            "{port} · ch {}\n{}",
                            channel + 1,
                            describe_notes(&out.notes, meter)
                        )
                    }
                    Sink::Instrument => describe_notes(&out.notes, meter),
                }
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
        // Nodes that draw their own view keep it; others show their first output.
        let views = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let own = n.imp.as_ref().and_then(|imp| imp.view());
                let view = own.or_else(|| {
                    value_view(eval.outputs[i].iter().flatten().next()?, meter, ctx.end)
                })?;
                Some((n.id.clone(), view))
            })
            .collect();
        CompileReport {
            issues,
            previews,
            views,
        }
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

pub(crate) fn describe(v: &Value, meter: Meter) -> String {
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

// ---- Preview views -----------------------------------------------------------

/// Most timeline steps a view carries (a long progression still draws its start).
const MAX_VIEW_SEGMENTS: usize = 64;

/// A drawable preview of a value: a keyboard for scales and chords, a step grid for
/// rhythms, a piano roll for notes. `None` where only text makes sense (other tunings).
pub(crate) fn value_view(v: &Value, meter: Meter, window: Ticks) -> Option<Json> {
    let bar = meter.ticks_per_bar();
    match v {
        Value::Scale(tl) => {
            if !tl.values().all(|s| s.tuning().is_twelve()) {
                return None;
            }
            Some(keys_view(tl, bar, "scale", |s: &Scale| {
                let pcs = (0..s.len() as i32).map(|d| s.pitch(d, 4).pitch_class());
                (s.to_string(), pcs.map(|pc| pc as i32).collect(), Some(s.root()))
            }))
        }
        Value::Chord(tl) => {
            if !tl.values().all(|c| c.tuning().is_twelve()) {
                return None;
            }
            Some(keys_view(tl, bar, "chord", |c: &Chord| {
                (c.to_string(), c.notes().iter().map(|p| p.step()).collect(), c.root())
            }))
        }
        Value::Rhythm(r) => {
            let onsets = r
                .onsets()
                .iter()
                .map(|o| {
                    Json::obj(vec![
                        ("t", Json::Num(o.offset as f64)),
                        ("d", Json::Num(o.duration as f64)),
                        ("vel", Json::Num(o.velocity as f64)),
                        ("prob", Json::Num(o.probability as f64)),
                    ])
                })
                .collect();
            Some(Json::obj(vec![
                ("kind", Json::str("steps")),
                ("length", Json::Num(r.length() as f64)),
                ("grid", Json::Num(r.grid() as f64)),
                ("bar", Json::Num(bar as f64)),
                ("onsets", Json::Arr(onsets)),
            ]))
        }
        Value::Notes(notes) => {
            if !notes.iter().all(|n| n.pitch.tuning().is_twelve()) {
                return None;
            }
            let notes = notes
                .iter()
                .filter(|n| n.start < window)
                .map(|n| {
                    Json::obj(vec![
                        ("t", Json::Num(n.start as f64)),
                        ("d", Json::Num(n.duration as f64)),
                        ("p", Json::Num(n.pitch.step() as f64)),
                        ("v", Json::Num(n.voice as f64)),
                        ("fixed", Json::Bool(false)),
                        ("name", Json::str(n.pitch.to_string())),
                    ])
                })
                .collect();
            Some(Json::obj(vec![
                ("kind", Json::str("roll")),
                ("compact", Json::Bool(true)),
                ("length", Json::Num(window as f64)),
                ("bar", Json::Num(bar as f64)),
                ("notes", Json::Arr(notes)),
            ]))
        }
    }
}

/// A keyboard view of a scale or chord timeline (`of`). `keys` gives each value's label,
/// its keys (pitch classes for a scale, MIDI notes for a chord) and its root pitch class.
fn keys_view<T>(
    tl: &Timeline<T>,
    bar: Ticks,
    of: &str,
    keys: impl Fn(&T) -> (String, Vec<i32>, Option<u8>),
) -> Json {
    let mut start = 0;
    let mut segs = Vec::new();
    for (duration, v) in tl.steps().iter().take(MAX_VIEW_SEGMENTS) {
        let (label, notes, root) = keys(v);
        segs.push(Json::obj(vec![
            ("t", Json::Num(start as f64)),
            ("d", Json::Num(*duration as f64)),
            ("label", Json::str(label)),
            ("keys", Json::Arr(notes.into_iter().map(|k| Json::Num(k as f64)).collect())),
            ("root", root.map_or(Json::Null, |r| Json::Num(r as f64))),
        ]));
        start += duration;
    }
    Json::obj(vec![
        ("kind", Json::str("keys")),
        ("of", Json::str(of)),
        ("length", Json::Num(tl.length() as f64)),
        ("bar", Json::Num(bar as f64)),
        ("segs", Json::Arr(segs)),
    ])
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
            ..Default::default()
        }
    }

    #[test]
    fn patch_two_plays_voice_led_chords() {
        let g = Graph::compile(&patch_two());
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let bar = Meter::COMMON.ticks_per_bar();
        let out = g.evaluate(&EvalContext::new(0, bar * 4, Meter::COMMON, 0));
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].sink,
            Sink::Midi {
                port: String::new(),
                channel: 0
            }
        );
        let notes = &out[0].notes;
        assert_eq!(notes.len(), 16, "four chords of four voices");
        assert!(notes
            .iter()
            .all(|n| n.pitch >= Pitch::parse("C3").unwrap()
                && n.pitch <= Pitch::parse("C5").unwrap()));
        // A window in the middle returns only the notes that start in it.
        let mid = g.evaluate(&EvalContext::new(bar, bar + PPQ, Meter::COMMON, 0));
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
        let report = g.report(Meter::COMMON, 4, true);
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
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        let report = g.report(Meter::COMMON, 1, true);
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
            ..Default::default()
        };
        let r = Graph::compile(&doc).report(Meter::COMMON, 1, true);
        assert_eq!(r.previews[0].1, "D Dorian · D E F G A B C");
        assert_eq!(r.previews[1].1, "F Lydian · F G A B C D E");
    }

    fn view<'a>(r: &'a CompileReport, id: &str) -> &'a Json {
        &r.views.iter().find(|v| v.0 == id).unwrap_or_else(|| panic!("no view for {id}")).1
    }

    fn nums(j: &Json) -> Vec<f64> {
        j.as_array().iter().filter_map(Json::as_f64).collect()
    }

    #[test]
    fn value_views() {
        let mut doc = patch_two();
        doc.nodes.push(node(
            "s",
            "scale",
            &[("root", text("D")), ("scale", text("Dorian"))],
        ));
        doc.nodes.push(node("e", "euclid", &[]));
        let r = Graph::compile(&doc).report(Meter::COMMON, 4, true);

        // A scale: one constant segment with its seven pitch classes and root.
        let s = view(&r, "s");
        assert_eq!(s.get("kind").unwrap().as_str(), Some("keys"));
        assert_eq!(s.get("of").unwrap().as_str(), Some("scale"));
        let segs = s.get("segs").unwrap().as_array();
        assert_eq!(segs.len(), 1);
        assert_eq!(nums(segs[0].get("keys").unwrap()), [2.0, 4.0, 5.0, 7.0, 9.0, 11.0, 0.0]);
        assert_eq!(segs[0].get("root").unwrap().as_f64(), Some(2.0));

        // A progression: one segment per chord, with absolute notes.
        let p = view(&r, "prog");
        assert_eq!(p.get("of").unwrap().as_str(), Some("chord"));
        let segs = p.get("segs").unwrap().as_array();
        assert_eq!(segs.len(), 4);
        assert_eq!(nums(segs[0].get("keys").unwrap()), [60.0, 64.0, 67.0]);
        assert_eq!(segs[1].get("t").unwrap().as_f64(), segs[0].get("d").unwrap().as_f64());

        // A rhythm: its onsets on a grid.
        let e = view(&r, "e");
        assert_eq!(e.get("kind").unwrap().as_str(), Some("steps"));
        assert!(e.get("grid").unwrap().as_f64().unwrap() > 0.0);
        assert!(!e.get("onsets").unwrap().as_array().is_empty());

        // Notes: a compact roll.
        let n = view(&r, "play");
        assert_eq!(n.get("kind").unwrap().as_str(), Some("roll"));
        assert_eq!(n.get("notes").unwrap().as_array().len(), 16);
    }
}
