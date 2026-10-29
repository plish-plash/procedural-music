//! Graph documents, compilation and evaluation.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use pm_theory::{Chord, Meter, Note, Pitch, Scale, Ticks};

use pm_synth::{ModuleDesc, ModuleKind, RackDesc};

use crate::json::Json;
use crate::nodes::{self, Inputs, NodeImpl, Sink};
use crate::spec::{NodeSpec, ParamSpec, ParamValue, PortType, PARAM_PORT};
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
    /// (node id, input port) for inputs the harmonic context feeds.
    pub context: Vec<(String, String)>,
}

/// Notes leaving the graph through one sink node (MIDI out or a built-in instrument).
#[derive(Clone, Debug)]
pub struct SinkOutput {
    pub node: String,
    /// The node's id in the audio rack (see [`module_id`]).
    pub module: u64,
    pub sink: Sink,
    pub notes: Vec<Note>,
    /// For control sinks: the control's value at the start of the window.
    pub control: Option<f64>,
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
    /// For each input port, then each automated parameter (see `controls`):
    /// (source node index, source output index).
    inputs: Vec<Option<(usize, usize)>>,
    /// Parameters driven by a Control input, in the order of their slots in `inputs`
    /// (after the spec's inputs).
    controls: Vec<&'static ParamSpec>,
    /// The parameter values the node was built with, for rebuilding it with automation.
    params: HashMap<String, ParamValue>,
    /// The node built with automated values, by those values (see [`Graph::automated`]).
    variants: Mutex<Vec<(Vec<Option<u64>>, Arc<dyn NodeImpl>)>>,
}

/// How many automated variants of one node are kept before starting over.
const MAX_VARIANTS: usize = 64;

impl CompiledNode {
    /// The slot in `inputs` and the type of an input port. `param.<name>` names the
    /// Control input of an automatable parameter; its slot is made on first use.
    fn input_slot(&mut self, port: &str) -> Option<(usize, PortType)> {
        if let Some(i) = self.spec.inputs.iter().position(|p| p.name == port) {
            return Some((i, self.spec.inputs[i].ty));
        }
        let name = port.strip_prefix(PARAM_PORT)?;
        let param = self
            .spec
            .params
            .iter()
            .find(|p| p.name == name && p.automatable(self.spec))?;
        let k = match self.controls.iter().position(|c| c.name == name) {
            Some(k) => k,
            None => {
                self.controls.push(param);
                self.inputs.push(None);
                self.controls.len() - 1
            }
        };
        Some((self.spec.inputs.len() + k, PortType::Control))
    }
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
    /// Inputs fed by the harmonic context rather than a wire: (node index, input index).
    implicit: Vec<(usize, usize)>,
    /// Whether any parameter is automated (previews then go a slice at a time).
    automated: bool,
}

/// Which nodes feed `target`, directly or through others.
fn feeding(nodes: &[CompiledNode], target: usize) -> Vec<bool> {
    let mut seen = vec![false; nodes.len()];
    let mut stack = vec![target];
    while let Some(i) = stack.pop() {
        for &(src, _) in nodes[i].inputs.iter().flatten() {
            if !seen[src] {
                seen[src] = true;
                stack.push(src);
            }
        }
    }
    seen
}

struct Evaluation {
    outputs: Vec<Vec<Option<Value>>>,
    sinks: Vec<SinkOutput>,
    issues: Vec<NodeIssue>,
    /// Automated rack module settings (see [`Graph::evaluate_window`]).
    modules: Vec<(u64, ModuleKind)>,
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
                controls: Vec::new(),
                params: n.params.clone(),
                variants: Mutex::new(Vec::new()),
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
            let in_port = nodes[to].input_slot(&w.to_port);
            let (Some(op), Some((ip, to_ty))) = (out_port, in_port) else {
                issues.push(NodeIssue {
                    node: w.to_node.clone(),
                    message: format!("no port '{}'", w.to_port),
                });
                continue;
            };
            let from_ty = nodes[from].spec.outputs[op].ty;
            if !from_ty.connects_to(to_ty) {
                issues.push(NodeIssue {
                    node: w.to_node.clone(),
                    message: format!("a {} can't feed a {} input", from_ty.name(), to_ty.name()),
                });
                continue;
            }
            nodes[to].inputs[ip] = Some((from, op));
        }

        // The harmonic context: scale and chord inputs left unwired read it (a wire always
        // wins). Nodes that feed the context can't read it, which also rules out loops;
        // Selects pick among their own inputs, so they don't either.
        let mut implicit = Vec::new();
        let contexts: Vec<usize> = (0..nodes.len())
            .filter(|&i| nodes[i].spec.kind == "context")
            .collect();
        if let Some((&c, rest)) = contexts.split_first() {
            for &other in rest {
                issues.push(NodeIssue {
                    node: nodes[other].id.clone(),
                    message: "only one Harmonic context is used; this one is ignored".into(),
                });
            }
            let feeds = feeding(&nodes, c);
            for i in 0..nodes.len() {
                let kind = nodes[i].spec.kind;
                if feeds[i] || kind == "context" || kind.starts_with("select_") {
                    continue;
                }
                for (k, port) in nodes[i].spec.inputs.iter().enumerate() {
                    let out = match port.ty {
                        PortType::Scale => 0,
                        PortType::Chord => 1,
                        _ => continue,
                    };
                    if nodes[i].inputs[k].is_none() {
                        nodes[i].inputs[k] = Some((c, out));
                        implicit.push((i, k));
                    }
                }
            }
        }
        let automated = nodes
            .iter()
            .any(|n| n.inputs[n.spec.inputs.len()..].iter().any(Option::is_some));

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
            implicit,
            automated,
        }
    }

    /// Whether any parameter is driven by a Control.
    pub fn is_automated(&self) -> bool {
        self.automated
    }

    /// A number parameter of a node as compiled (its default when unset), if it has one.
    pub fn param_number(&self, node: &str, name: &str) -> Option<f64> {
        let n = self.nodes.iter().find(|n| n.id == node)?;
        let spec = n.spec.params.iter().find(|p| p.name == name)?;
        match n.params.get(name).cloned().unwrap_or_else(|| spec.default_value()) {
            ParamValue::Number(v) => Some(v),
            _ => None,
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
        let mut modules = Vec::new();
        for &i in subset {
            let node = &self.nodes[i];
            let Some(base) = &node.imp else { continue };
            // Sound nodes become rack modules: report their automated settings.
            if base.module().is_some() && !node.controls.is_empty() {
                match self.automated(node, &outputs, ctx.start) {
                    Ok(Some(v)) => modules.extend(v.module().map(|k| (module_id(&node.id), k))),
                    Ok(None) => {}
                    Err(message) => issues.push(NodeIssue {
                        node: node.id.clone(),
                        message,
                    }),
                }
            }
            // Effects, mixers and audio outputs only process audio, in the rack; display
            // nodes only run for previews.
            let imp = base;
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
            // With automated parameters, the node as built with their current values.
            let variant = match self.automated(node, source, ctx.start) {
                Ok(v) => v,
                Err(message) => {
                    issues.push(NodeIssue {
                        node: node.id.clone(),
                        message,
                    });
                    continue;
                }
            };
            let imp: &dyn NodeImpl = variant.as_deref().unwrap_or(imp.as_ref());
            let inputs = Inputs { values };
            if imp.is_sink() {
                if let Some(sink) = imp.sink() {
                    sinks.push(SinkOutput {
                        node: node.id.clone(),
                        module: module_id(&node.id),
                        sink,
                        notes: inputs.notes(0).to_vec(),
                        control: match inputs.values.first() {
                            Some(Some(Value::Control(c))) => Some(c.at(ctx.start)),
                            _ => None,
                        },
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
            modules,
        }
    }

    /// The node built with the current values of its automated parameters: each
    /// connected Control is read at tick `t` (from `source`) and snapped to the
    /// parameter's grid. `None` when nothing automates it, so the node runs as compiled.
    /// Variants are cached by their values.
    fn automated(
        &self,
        node: &CompiledNode,
        source: &[Vec<Option<Value>>],
        t: Ticks,
    ) -> Result<Option<Arc<dyn NodeImpl>>, String> {
        if node.controls.is_empty() {
            return Ok(None);
        }
        let first = node.spec.inputs.len();
        let key: Vec<Option<u64>> = node
            .controls
            .iter()
            .enumerate()
            .map(|(k, param)| match node.inputs[first + k].and_then(|(s, o)| source[s][o].as_ref()) {
                Some(Value::Control(c)) => Some(param.snap(c.at(t)).to_bits()),
                _ => None,
            })
            .collect();
        if key.iter().all(Option::is_none) {
            return Ok(None);
        }
        let mut variants = node.variants.lock().unwrap();
        if let Some((_, v)) = variants.iter().find(|(k, _)| *k == key) {
            return Ok(Some(v.clone()));
        }
        let mut params = node.params.clone();
        for (param, value) in node.controls.iter().zip(&key) {
            if let Some(bits) = value {
                params.insert(param.name.to_string(), ParamValue::Number(f64::from_bits(*bits)));
            }
        }
        let built: Arc<dyn NodeImpl> = Arc::from(nodes::build(node.spec, &params)?);
        if variants.len() >= MAX_VARIANTS {
            variants.clear();
        }
        variants.push((key, built.clone()));
        Ok(Some(built))
    }

    /// Notes that start in the window, grouped by output node.
    pub fn evaluate(&self, ctx: &EvalContext) -> Vec<SinkOutput> {
        self.evaluate_window(ctx).0
    }

    /// Like [`Graph::evaluate`], a `slice` at a time when parameters are automated, so
    /// automation moves as it does in playback (which evaluates a slice at a time).
    pub fn evaluate_sliced(&self, ctx: &EvalContext, slice: Ticks) -> Vec<SinkOutput> {
        if !self.automated {
            return self.evaluate(ctx);
        }
        let mut out: Vec<SinkOutput> = Vec::new();
        let mut t = ctx.start;
        while t < ctx.end {
            let end = (t + slice.max(1)).min(ctx.end);
            for sink in self.evaluate(&EvalContext { start: t, end, ..*ctx }) {
                merge_sink(&mut out, sink);
            }
            t = end;
        }
        out
    }

    /// Evaluates a preview window as playback would hear it. Without automation that is
    /// one evaluation. With it, the window goes a slice at a time: notes come from every
    /// slice, other values (scales, chords, rhythms) from the slice holding `focus`. That
    /// slice goes last, so nodes that keep a view of their latest evaluation show it too.
    fn run_preview(&self, ctx: &EvalContext, focus: Ticks) -> Evaluation {
        if !self.automated {
            return self.run(ctx);
        }
        let mut starts: Vec<Ticks> = (ctx.start..ctx.end).step_by(PREVIEW_SLICE as usize).collect();
        if let Some(i) = starts.iter().rposition(|&t| t <= focus) {
            let here = starts.remove(i);
            starts.push(here);
        }
        let mut merged: Option<Evaluation> = None;
        for t in starts {
            let end = (t + PREVIEW_SLICE).min(ctx.end);
            let e = self.run(&EvalContext { start: t, end, ..*ctx });
            let in_slice = |n: &Note| n.start >= t && n.start < end;
            let here = (t..end).contains(&focus);
            let m = merged.get_or_insert_with(|| Evaluation {
                outputs: e.outputs.iter().map(|o| vec![None; o.len()]).collect(),
                sinks: Vec::new(),
                issues: Vec::new(),
                modules: Vec::new(),
            });
            for (outs, acc) in e.outputs.into_iter().zip(&mut m.outputs) {
                for (v, slot) in outs.into_iter().zip(acc.iter_mut()) {
                    match (v, slot) {
                        (Some(Value::Notes(n)), Some(Value::Notes(have))) => {
                            have.extend(n.into_iter().filter(in_slice));
                        }
                        (Some(Value::Notes(n)), slot) => {
                            *slot = Some(Value::Notes(n.into_iter().filter(in_slice).collect()));
                        }
                        (Some(v), slot) if here || slot.is_none() => *slot = Some(v),
                        _ => {}
                    }
                }
            }
            for mut sink in e.sinks {
                sink.notes.retain(in_slice);
                let (node, control) = (sink.node.clone(), sink.control);
                merge_sink(&mut m.sinks, sink);
                if here {
                    if let Some(s) = m.sinks.iter_mut().find(|s| s.node == node) {
                        s.control = control;
                    }
                }
            }
            for issue in e.issues {
                if !m.issues.contains(&issue) {
                    m.issues.push(issue);
                }
            }
        }
        let mut merged = merged.unwrap_or_else(|| self.run(ctx));
        for v in merged.outputs.iter_mut().flatten().flatten() {
            if let Value::Notes(n) = v {
                n.sort_by_key(|n| n.start);
            }
        }
        for s in &mut merged.sinks {
            s.notes.sort_by_key(|n| n.start);
        }
        merged
    }

    /// Like [`Graph::evaluate`], plus the settings of rack modules whose parameters are
    /// automated, as they are at the start of the window.
    pub fn evaluate_window(&self, ctx: &EvalContext) -> (Vec<SinkOutput>, Vec<(u64, ModuleKind)>) {
        let eval = self.run(ctx);
        let mut sinks = eval.sinks;
        for s in &mut sinks {
            s.notes
                .retain(|n| n.start >= ctx.start && n.start < ctx.end);
        }
        (sinks, eval.modules)
    }

    /// The value on one of a node's outputs (the first when `port` is `None`), as the
    /// previews show it (see [`Graph::report_window`]).
    pub fn output_value(
        &self,
        node: &str,
        port: Option<&str>,
        ctx: &EvalContext,
        focus: Ticks,
    ) -> Result<Value, String> {
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
            None => self.nodes[i].spec.preview_output(),
        };
        self.run_preview(ctx, focus).outputs[i][out]
            .take()
            .ok_or_else(|| "no value yet: connect its inputs".into())
    }

    /// Issues plus a short text preview of every node's output, evaluated over the
    /// first `bars` bars.
    pub fn report(&self, meter: Meter, bars: u32, sync: bool) -> CompileReport {
        self.report_window(&EvalContext::preview(meter, bars, sync, 0, 0), 0)
    }

    /// Issues, text previews and views of every node's output over the window of `ctx`
    /// (its section, salt and span), with automation as heard at `focus` (see
    /// [`Graph::run_preview`]). Views are drawn from the start of the window.
    pub fn report_window(&self, ctx: &EvalContext, focus: Ticks) -> CompileReport {
        let meter = ctx.meter;
        let eval = self.run_preview(ctx, focus);
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
                    Sink::Control { port, channel, message } => {
                        let port = if port.is_empty() { "Default port" } else { port };
                        let what = match message {
                            nodes::ControlMessage::Cc(cc) => format!("CC {cc}"),
                            nodes::ControlMessage::PitchBend => "pitch bend".into(),
                            nodes::ControlMessage::Pressure => "channel pressure".into(),
                        };
                        let value = out.control.map_or(String::new(), |v| format!(" = {}", (v * 100.0).round() / 100.0));
                        format!("{port} · ch {} · {what}{value}", channel + 1)
                    }
                }
            } else if let Some(v) = shown(&eval.outputs[i], node.spec) {
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
        // Nodes that draw their own view keep it; others show their preview output.
        let views = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let own = n.imp.as_ref().and_then(|imp| imp.view());
                let view = own.or_else(|| {
                    let v = shown(&eval.outputs[i], n.spec)?;
                    value_view(v, meter, ctx.start, ctx.end - ctx.start)
                })?;
                Some((n.id.clone(), view))
            })
            .collect();
        let context = self
            .implicit
            .iter()
            .map(|&(i, k)| (self.nodes[i].id.clone(), self.nodes[i].spec.inputs[k].name.to_string()))
            .collect();
        CompileReport {
            issues,
            previews,
            views,
            context,
        }
    }
}

/// How finely previews follow automation: as finely as playback does by default.
const PREVIEW_SLICE: Ticks = pm_theory::PPQ / 4;

/// Adds a sink's notes to the list, joining them with an earlier sink of the same node.
/// The value a node's preview shows: its preview output, or failing that its first with a value.
fn shown<'a>(outputs: &'a [Option<Value>], spec: &NodeSpec) -> Option<&'a Value> {
    outputs
        .get(spec.preview_output())
        .and_then(Option::as_ref)
        .or_else(|| outputs.iter().flatten().next())
}

fn merge_sink(out: &mut Vec<SinkOutput>, sink: SinkOutput) {
    match out.iter_mut().find(|o| o.node == sink.node) {
        Some(o) => o.notes.extend(sink.notes),
        None => out.push(sink),
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
        Value::Control(c) => c.describe(meter.ticks_per_bar()),
    }
}

// ---- Preview views -----------------------------------------------------------

/// Most timeline steps a view carries (a long progression still draws its start).
const MAX_VIEW_SEGMENTS: usize = 64;

/// A drawable preview of a value: a keyboard for scales and chords, a step grid for
/// rhythms, a piano roll for notes and a curve for controls. `None` where only text makes
/// sense (other tunings). Notes and curves cover `window` ticks from `start`, drawn from 0.
pub(crate) fn value_view(v: &Value, meter: Meter, start: Ticks, window: Ticks) -> Option<Json> {
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
                        ("prob", Json::Num(o.probability() as f64)),
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
                .filter(|n| n.start >= start && n.start - start < window)
                .map(|n| {
                    Json::obj(vec![
                        ("t", Json::Num((n.start - start) as f64)),
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
        Value::Control(c) => {
            // Sampled finely enough to draw: 16 points a bar.
            let n = (window / bar).max(1) * 16;
            let points: Vec<(f64, f64)> = (0..=n)
                .map(|k| {
                    let t = window * k / n;
                    (t as f64, c.at(start + t))
                })
                .collect();
            let (lo, hi) = points
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
            Some(Json::obj(vec![
                ("kind", Json::str("curve")),
                ("length", Json::Num(window as f64)),
                ("bar", Json::Num(bar as f64)),
                ("min", Json::Num(lo)),
                ("max", Json::Num(hi)),
                (
                    "points",
                    Json::Arr(
                        points
                            .into_iter()
                            .map(|(t, v)| Json::Arr(vec![Json::Num(t), Json::Num(v)]))
                            .collect(),
                    ),
                ),
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
    let first = notes.iter().map(|n| n.start / bar + 1).min().unwrap_or(1);
    let last = notes.iter().map(|n| n.start / bar + 1).max().unwrap_or(1);
    // The window may start past bar 1 (the previews follow the playhead in blocks).
    let block = (first - 1) / 4 * 4 + 1;
    let first_bar: Vec<String> = notes
        .iter()
        .filter(|n| n.start / bar + 1 == first)
        .take(16)
        .map(|n| n.pitch.to_string())
        .collect();
    let mut text = if last > block {
        format!("{} notes in bars {block}–{last}", notes.len())
    } else {
        format!("{} notes in bar {block}", notes.len())
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
pub(crate) mod tests {
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
                node("key", "scale", &[]),
                node("prog", "progression", &[("chords", text("I vi IV V"))]),
                node("voice", "voice", &[]),
                node("lead", "voice_lead", &[]),
                node("play", "play_chords", &[]),
                node("out", "midi_out", &[]),
            ],
            wires: vec![
                wire("key", "scale", "prog", "key"),
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
        assert_eq!(preview("lonely"), "connect chords");
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

    #[test]
    fn harmonic_context_feeds_unwired_inputs() {
        let doc = GraphDoc {
            nodes: vec![
                node("k", "scale", &[]),
                node("p", "progression", &[("chords", text("I IV"))]),
                node("ctx", "context", &[]),
                node("root", "chord_root", &[]),
                node("x", "chord", &[("symbol", text("Dm"))]),
                node("pc", "play_chords", &[]),
                node("sw", "select_chord", &[]),
                node("seq", "sequencer", &[("degrees", text("1 2 3"))]),
            ],
            wires: vec![
                wire("k", "scale", "p", "key"),
                wire("p", "chords", "ctx", "chords"),
                wire("k", "scale", "ctx", "key"),
                wire("x", "chord", "pc", "chords"),
            ],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let r = g.report(Meter::COMMON, 2, true);
        let fed: Vec<(&str, &str)> = r.context.iter().map(|(n, p)| (n.as_str(), p.as_str())).collect();
        // Unwired chord and scale inputs read it; a wire wins; selects don't.
        assert_eq!(fed, [("root", "chords"), ("seq", "scale")]);
        let preview = |id: &str| r.previews.iter().find(|p| p.0 == id).map(|p| p.1.clone()).unwrap_or_default();
        // One held root per chord (no rhythm wired): C, then F.
        assert!(preview("root").starts_with("2 notes in bars 1–2\nC2"), "{}", preview("root"));
        assert!(preview("pc").contains("D4"), "{}", preview("pc"));
        assert!(preview("seq").contains("C4 D4 E4"), "the key, not the chord: {}", preview("seq"));

        // A node that feeds the context doesn't read it (no loop), and a second context is ignored.
        let doc = GraphDoc {
            nodes: vec![
                node("t", "transpose", &[]),
                node("ctx", "context", &[]),
                node("ctx2", "context", &[]),
            ],
            wires: vec![wire("t", "chords", "ctx", "chords")],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.report(Meter::COMMON, 1, true).context.is_empty());
        let issues: Vec<_> = g.issues().iter().map(|i| (i.node.as_str(), i.message.as_str())).collect();
        assert_eq!(issues, [("ctx2", "only one Harmonic context is used; this one is ignored")]);
    }

    #[test]
    fn controls_automate_parameters() {
        let bar = Meter::COMMON.ticks_per_bar();
        let doc = GraphDoc {
            nodes: vec![
                node("c", "chord", &[("symbol", text("C"))]),
                node("t", "transpose", &[]),
                node("k", "steps", &[("values", text("0 5 7"))]),
            ],
            wires: vec![
                wire("c", "chord", "t", "chords"),
                wire("k", "control", "t", "param.semitones"),
            ],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        // One step a bar: the chord moves up 5, then 7, then starts again.
        let lowest = |b: u32| {
            let ctx = EvalContext::new(b * bar, b * bar + PPQ / 4, Meter::COMMON, 0);
            match g.output_value("t", None, &ctx, ctx.start).unwrap() {
                Value::Chord(tl) => tl.at(0).value.notes()[0].step(),
                v => panic!("{v:?}"),
            }
        };
        assert_eq!([lowest(0), lowest(1), lowest(2), lowest(3)], [60, 65, 67, 60]);

        // Only automatable parameters have a port; a control can't feed other inputs.
        let mut bad = doc.clone();
        bad.wires.push(wire("k", "control", "t", "param.nope"));
        bad.wires.push(wire("k", "control", "t", "chords"));
        bad.nodes.push(node("w", "random_walk", &[]));
        bad.wires.push(wire("k", "control", "w", "param.seed"));
        let issues = Graph::compile(&bad).issues().to_vec();
        assert!(issues.iter().any(|i| i.message == "no port 'param.nope'"), "{issues:?}");
        assert!(issues.iter().any(|i| i.message == "a control can't feed a chord input"), "{issues:?}");
        // Seeds pick a stream rather than an amount, so they aren't automated.
        assert!(issues.iter().any(|i| i.node == "w" && i.message == "no port 'param.seed'"), "{issues:?}");

        // The control's own view is a curve over the preview window.
        let r = g.report(Meter::COMMON, 4, true);
        let k = view(&r, "k");
        assert_eq!(k.get("kind").unwrap().as_str(), Some("curve"));
        assert_eq!(k.get("max").unwrap().as_f64(), Some(7.0));
        assert_eq!(
            r.previews.iter().find(|p| p.0 == "k").unwrap().1,
            "steps 0 5 7"
        );
    }

    #[test]
    fn polyrhythm_previews_both() {
        let doc = GraphDoc { nodes: vec![node("p", "polyrhythm", &[])], ..Default::default() };
        let g = Graph::compile(&doc);
        let r = g.report(Meter::COMMON, 1, true);
        // 3 against 4 together: 6 hits (sharing the downbeat), not A's 3.
        let onsets = view(&r, "p").get("onsets").unwrap().to_string();
        assert_eq!(onsets.matches("\"t\"").count(), 6, "{onsets}");
        let ctx = EvalContext::preview(Meter::COMMON, 1, true, 0, 0);
        match g.output_value("p", None, &ctx, 0) {
            Ok(Value::Rhythm(r)) => assert_eq!(r.len(), 6),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn automated_previews_show_the_playhead() {
        let bar = Meter::COMMON.ticks_per_bar();
        let doc = GraphDoc {
            nodes: vec![
                node("key", "scale", &[]),
                node("deg", "chord_degree", &[]),
                node("k", "steps", &[("values", text("1 5 1 6"))]),
                node("circle", "circle_of_fifths", &[]),
                node("cc", "midi_cc", &[]),
            ],
            wires: vec![
                wire("key", "scale", "deg", "key"),
                wire("k", "control", "deg", "param.degree"),
                wire("deg", "chord", "circle", "chords"),
                wire("k", "control", "cc", "value"),
            ],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        // The playhead in bar 2 (G), not the end of the window (Am, bar 4).
        let ctx = EvalContext::preview(Meter::COMMON, 4, true, 0, 0);
        let r = g.report_window(&ctx, bar + PPQ);
        let circle = view(&r, "circle").to_string();
        assert!(circle.contains("\"G\"") && !circle.contains("Am"), "{circle}");
        let cc = &r.previews.iter().find(|p| p.0 == "cc").unwrap().1;
        assert!(cc.ends_with("= 5"), "{cc}");
    }

    #[test]
    fn controls_automate_rack_modules() {
        let doc = GraphDoc {
            nodes: vec![
                node("s", "poly_synth", &[]),
                node(
                    "l",
                    "lfo",
                    &[
                        ("min", ParamValue::Number(200.0)),
                        ("max", ParamValue::Number(4000.0)),
                        ("cycle", ParamValue::Number(1.0)),
                    ],
                ),
            ],
            wires: vec![wire("l", "control", "s", "param.cutoff")],
            ..Default::default()
        };
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let cutoff = |t: Ticks| {
            let (_, modules) = g.evaluate_window(&EvalContext::new(t, t + 240, Meter::COMMON, 0));
            assert_eq!(modules.len(), 1);
            assert_eq!(modules[0].0, module_id("s"));
            match modules[0].1 {
                ModuleKind::Poly(p) => p.cutoff,
                ref k => panic!("{k:?}"),
            }
        };
        let bar = Meter::COMMON.ticks_per_bar();
        assert!((cutoff(0) - 200.0).abs() < 1.0);
        assert!((cutoff(bar / 2) - 4000.0).abs() < 1.0);
        // Without automation, nothing is reported.
        let plain = Graph::compile(&GraphDoc {
            nodes: vec![node("s", "poly_synth", &[])],
            ..Default::default()
        });
        assert!(plain.evaluate_window(&EvalContext::new(0, 240, Meter::COMMON, 0)).1.is_empty());
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
