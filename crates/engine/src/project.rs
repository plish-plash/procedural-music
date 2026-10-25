//! Reading project documents: groups (subgraphs) are flattened into one graph, and the
//! arrangement's sections are read alongside.
//!
//! A group node carries its own graph: `{id, type: "group", title, graph: {nodes, wires,
//! inputs: [{name, type}], outputs: [{name, type}]}}`. Inside it, wires to and from the
//! pseudo-nodes `@in` and `@out` connect the group's ports. Flattening gives inner nodes
//! ids of the form `group/inner` (nested groups add more levels), so issues, previews,
//! section overrides and audio modules all address inner nodes by that path.

use std::collections::HashMap;

use crate::graph::{GraphDoc, NodeDoc, SectionDoc, WireDoc};
use crate::json::Json;
use crate::spec::ParamValue;

/// The pseudo-node whose outputs are a group's inputs.
pub const GROUP_IN: &str = "@in";
/// The pseudo-node whose inputs are a group's outputs.
pub const GROUP_OUT: &str = "@out";

/// Groups can nest; this guards against runaway documents.
const MAX_DEPTH: usize = 16;

/// Reads a project: `{nodes, wires, arrangement?: {sections: [...]}}`. Positions, titles
/// and other editor fields are ignored.
pub fn parse_project(doc: &Json) -> Result<GraphDoc, String> {
    let mut parts = HashMap::new();
    let level = flatten(doc, "", None, &mut parts, 0)?;
    // Wires left touching @in/@out at the top level lead nowhere.
    let wires = level
        .wires
        .into_iter()
        .filter(|w| !is_io(&w.from_node) && !is_io(&w.to_node))
        .collect();
    let sections = match doc.get("arrangement").and_then(|a| a.get("sections")) {
        Some(list) => list
            .as_array()
            .iter()
            .map(parse_section)
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    Ok(GraphDoc {
        nodes: level.nodes,
        wires,
        sections,
        parts,
    })
}

fn is_io(id: &str) -> bool {
    id == GROUP_IN || id == GROUP_OUT
}

pub(crate) fn param_value(v: &Json) -> Option<ParamValue> {
    match v {
        Json::Num(x) => Some(ParamValue::Number(*x)),
        Json::Str(s) => Some(ParamValue::Text(s.clone())),
        Json::Bool(b) => Some(ParamValue::Bool(*b)),
        _ => None,
    }
}

fn parse_section(s: &Json) -> Result<SectionDoc, String> {
    let name = s
        .get("name")
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let bars = s.get("bars").and_then(Json::as_f64).unwrap_or(4.0).round();
    if !(1.0..=999.0).contains(&bars) {
        return Err(format!("section '{name}' needs 1 to 999 bars"));
    }
    let bpm = s
        .get("bpm")
        .and_then(Json::as_f64)
        .filter(|b| *b > 0.0)
        .map(|b| b.clamp(20.0, 400.0));
    let mut overrides = Vec::new();
    for (node, params) in s.get("overrides").map(Json::as_object).unwrap_or(&[]) {
        for (param, value) in params.as_object() {
            if let Some(v) = param_value(value) {
                overrides.push((node.clone(), param.clone(), v));
            }
        }
    }
    overrides.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    Ok(SectionDoc {
        name,
        bars: bars as u32,
        bpm,
        overrides,
    })
}

/// One level of the document after its groups are spliced in. Wires may still touch
/// `@in`/`@out`, which the level above resolves.
struct Level {
    nodes: Vec<NodeDoc>,
    wires: Vec<WireDoc>,
}

/// Where a group output gets its value.
#[derive(Clone)]
enum Source {
    Node(String, String),
    /// Straight from one of the group's own inputs.
    Input(String),
}

#[derive(Default)]
struct GroupPorts {
    /// Group input → the inner inputs it feeds.
    targets: HashMap<String, Vec<(String, String)>>,
    /// Group output → where its value comes from.
    sources: HashMap<String, Source>,
}

fn flatten(
    g: &Json,
    prefix: &str,
    part: Option<&str>,
    parts: &mut HashMap<String, String>,
    depth: usize,
) -> Result<Level, String> {
    if depth > MAX_DEPTH {
        return Err("groups are nested too deeply".into());
    }
    let mut nodes = Vec::new();
    let mut inner_wires = Vec::new();
    let mut groups: HashMap<String, GroupPorts> = HashMap::new();

    for n in g.get("nodes").map(Json::as_array).unwrap_or(&[]) {
        let local = n
            .get("id")
            .and_then(Json::as_str)
            .ok_or("node without an id")?;
        if local.contains('/') || is_io(local) {
            return Err(format!("node id '{local}' is reserved"));
        }
        let id = format!("{prefix}{local}");
        let kind = n
            .get("type")
            .and_then(Json::as_str)
            .ok_or("node without a type")?;
        if kind == "group" {
            let title = n.get("title").and_then(Json::as_str).unwrap_or("Group");
            let inner = n.get("graph").cloned().unwrap_or(Json::Null);
            let level = flatten(
                &inner,
                &format!("{id}/"),
                part.or(Some(title)),
                parts,
                depth + 1,
            )?;
            let mut ports = GroupPorts::default();
            for w in level.wires {
                match (w.from_node.as_str(), w.to_node.as_str()) {
                    (GROUP_IN, GROUP_OUT) => {
                        ports.sources.insert(w.to_port, Source::Input(w.from_port));
                    }
                    (GROUP_IN, _) => ports
                        .targets
                        .entry(w.from_port)
                        .or_default()
                        .push((w.to_node, w.to_port)),
                    (_, GROUP_OUT) => {
                        ports
                            .sources
                            .insert(w.to_port, Source::Node(w.from_node, w.from_port));
                    }
                    _ => inner_wires.push(w),
                }
            }
            nodes.extend(level.nodes);
            groups.insert(id, ports);
            continue;
        }
        let mut params = HashMap::new();
        for (k, v) in n.get("params").map(Json::as_object).unwrap_or(&[]) {
            if let Some(value) = param_value(v) {
                params.insert(k.clone(), value);
            }
        }
        if let Some(part) = part {
            parts.insert(id.clone(), part.to_string());
        }
        nodes.push(NodeDoc {
            id,
            kind: kind.to_string(),
            params,
        });
    }

    // This level's wires, with node ids prefixed (but not @in/@out).
    let name = |id: &str| {
        if is_io(id) {
            id.to_string()
        } else {
            format!("{prefix}{id}")
        }
    };
    let mut wires = Vec::new();
    for w in g.get("wires").map(Json::as_array).unwrap_or(&[]) {
        let end = |key: &str| -> Option<(String, String)> {
            let e = w.get(key)?;
            Some((
                name(e.get("node")?.as_str()?),
                e.get("port")?.as_str()?.to_string(),
            ))
        };
        let (Some((from_node, from_port)), Some((to_node, to_port))) = (end("from"), end("to"))
        else {
            return Err("malformed wire".into());
        };
        wires.push(WireDoc {
            from_node,
            from_port,
            to_node,
            to_port,
        });
    }

    // Resolve wires that touch groups into wires between real nodes.
    let mut out = inner_wires;
    for w in &wires {
        let sources = resolve(&w.from_node, &w.from_port, &groups, &wires, 0);
        let targets = match groups.get(&w.to_node) {
            Some(ports) => ports.targets.get(&w.to_port).cloned().unwrap_or_default(),
            None => vec![(w.to_node.clone(), w.to_port.clone())],
        };
        for (from_node, from_port) in &sources {
            for (to_node, to_port) in &targets {
                out.push(WireDoc {
                    from_node: from_node.clone(),
                    from_port: from_port.clone(),
                    to_node: to_node.clone(),
                    to_port: to_port.clone(),
                });
            }
        }
    }
    Ok(Level { nodes, wires: out })
}

/// The real output behind `(node, port)`, following group outputs through pass-throughs.
fn resolve(
    node: &str,
    port: &str,
    groups: &HashMap<String, GroupPorts>,
    wires: &[WireDoc],
    depth: usize,
) -> Vec<(String, String)> {
    let Some(ports) = groups.get(node) else {
        return vec![(node.to_string(), port.to_string())];
    };
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    match ports.sources.get(port) {
        Some(Source::Node(n, p)) => vec![(n.clone(), p.clone())],
        Some(Source::Input(input)) => wires
            .iter()
            .filter(|w| w.to_node == node && &w.to_port == input)
            .flat_map(|w| resolve(&w.from_node, &w.from_port, groups, wires, depth + 1))
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EvalContext, Graph};
    use pm_theory::Meter;

    fn parse(text: &str) -> GraphDoc {
        parse_project(&Json::parse(text).unwrap()).unwrap()
    }

    #[test]
    fn groups_flatten_into_one_graph() {
        // Scale → [group: (in) → Arp → (out)] → MIDI out, with a pattern inside the group.
        let doc = parse(
            r#"{"nodes":[
              {"id":"s","type":"scale","params":{"root":"D"}},
              {"id":"g","type":"group","title":"Lead","graph":{
                 "inputs":[{"name":"key","type":"scale"}],"outputs":[{"name":"notes","type":"notes"}],
                 "nodes":[{"id":"p","type":"pattern","params":{"pattern":"x","step":"1/4"}},{"id":"a","type":"arp","params":{}}],
                 "wires":[{"from":{"node":"@in","port":"key"},"to":{"node":"a","port":"scale"}},
                          {"from":{"node":"p","port":"rhythm"},"to":{"node":"a","port":"rhythm"}},
                          {"from":{"node":"a","port":"notes"},"to":{"node":"@out","port":"notes"}}]}},
              {"id":"o","type":"midi_out","params":{}}],
             "wires":[{"from":{"node":"s","port":"scale"},"to":{"node":"g","port":"key"}},
                      {"from":{"node":"g","port":"notes"},"to":{"node":"o","port":"notes"}}]}"#,
        );
        let ids: Vec<&str> = doc.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["s", "g/p", "g/a", "o"]);
        assert_eq!(doc.wires.len(), 3);
        assert_eq!(doc.parts.get("g/a").map(String::as_str), Some("Lead"));
        let g = Graph::compile(&doc);
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let out = g.evaluate(&EvalContext::new(
            0,
            Meter::COMMON.ticks_per_bar(),
            Meter::COMMON,
            0,
        ));
        assert_eq!(out[0].notes.len(), 4);
        assert_eq!(out[0].notes[0].pitch.to_string(), "D4");
    }

    #[test]
    fn nested_groups_and_pass_through() {
        // outer group: (in) → inner group (in → out, a pass-through) → (out).
        let doc = parse(
            r#"{"nodes":[
              {"id":"c","type":"chord","params":{"symbol":"Am"}},
              {"id":"g","type":"group","graph":{"nodes":[
                 {"id":"h","type":"group","graph":{"nodes":[],"wires":[
                    {"from":{"node":"@in","port":"x"},"to":{"node":"@out","port":"y"}}]}}],
                 "wires":[{"from":{"node":"@in","port":"a"},"to":{"node":"h","port":"x"}},
                          {"from":{"node":"h","port":"y"},"to":{"node":"@out","port":"b"}}]}},
              {"id":"pc","type":"play_chords","params":{}}],
             "wires":[{"from":{"node":"c","port":"chord"},"to":{"node":"g","port":"a"}},
                      {"from":{"node":"g","port":"b"},"to":{"node":"pc","port":"chords"}}]}"#,
        );
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.wires.len(), 1);
        assert_eq!(doc.wires[0].from_node, "c");
        assert_eq!(doc.wires[0].to_node, "pc");
    }

    #[test]
    fn sections_and_overrides() {
        let doc = parse(
            r#"{"nodes":[],"wires":[],"arrangement":{"sections":[
              {"name":"Intro","bars":4},
              {"name":"B","bars":8,"bpm":140,"overrides":{"g/p":{"pattern":"xx"},"s":{"root":"G"}}}]}}"#,
        );
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[1].bpm, Some(140.0));
        assert_eq!(doc.sections[1].overrides[0].0, "g/p");
        assert_eq!(doc.sections[1].overrides[1].2, ParamValue::Text("G".into()));
        assert!(parse_project(
            &Json::parse(r#"{"arrangement":{"sections":[{"name":"x","bars":0}]}}"#).unwrap()
        )
        .is_err());
    }
}
