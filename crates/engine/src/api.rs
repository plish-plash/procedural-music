//! The UI's command interface, as JSON in and JSON out.
//!
//! Both the desktop app and the development server route UI calls through
//! [`handle`], so the protocol lives in one place.

use std::collections::HashMap;

use crate::engine::{Engine, MidiMessage};
use crate::graph::{CompileReport, GraphDoc, NodeDoc, WireDoc};
use crate::json::Json;
use crate::nodes::specs;
use crate::spec::{ParamKind, ParamValue, PortSpec, PortType};

/// Result of one command: the JSON reply and any MIDI to send right away.
pub struct Reply {
    pub json: Json,
    pub midi: Vec<MidiMessage>,
}

impl Reply {
    fn json(json: Json) -> Reply {
        Reply {
            json,
            midi: Vec::new(),
        }
    }
}

/// Runs a UI command. `now` is the caller's clock in seconds; `salt` should be
/// different on every call (used when playback starts).
pub fn handle(
    engine: &mut Engine,
    now: f64,
    salt: u64,
    command: &str,
    args: &Json,
) -> Result<Reply, String> {
    match command {
        "specs" => Ok(Reply::json(specs_json())),
        "set_graph" => {
            let doc = args.get("doc").ok_or("missing doc")?;
            let doc = match doc {
                Json::Str(text) => Json::parse(text)?,
                other => other.clone(),
            };
            let report = engine.set_graph(&parse_graph(&doc)?);
            Ok(Reply::json(report_json(&report)))
        }
        "play" => {
            engine.play(now, salt);
            Ok(Reply::json(status_json(engine, now)))
        }
        "stop" => {
            let midi = engine.stop();
            Ok(Reply {
                json: status_json(engine, now),
                midi,
            })
        }
        "set_bpm" => {
            let bpm = args
                .get("bpm")
                .and_then(Json::as_f64)
                .ok_or("missing bpm")?;
            engine.set_bpm(now, bpm);
            Ok(Reply::json(status_json(engine, now)))
        }
        "set_clock_ports" => {
            let ports = args.get("ports").map(Json::as_array).unwrap_or(&[]);
            engine.set_clock_ports(
                ports
                    .iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect(),
            );
            Ok(Reply::json(status_json(engine, now)))
        }
        "status" => Ok(Reply::json(status_json(engine, now))),
        other => Err(format!("unknown command '{other}'")),
    }
}

pub fn status_json(engine: &Engine, now: f64) -> Json {
    let tick = engine.playhead(now);
    let meter = engine.meter();
    let (bar, beat, _) = meter.position(tick as u32);
    Json::obj(vec![
        ("playing", Json::Bool(engine.is_playing())),
        ("bpm", Json::Num(engine.bpm())),
        ("tick", Json::Num(tick.floor())),
        ("bar", Json::Num(bar as f64 + 1.0)),
        ("beat", Json::Num(beat as f64 + 1.0)),
        (
            "meter",
            Json::Arr(vec![
                Json::Num(meter.beats as f64),
                Json::Num(meter.unit as f64),
            ]),
        ),
        (
            "clockPorts",
            Json::Arr(
                engine
                    .clock_ports()
                    .iter()
                    .map(|p| Json::str(p.as_str()))
                    .collect(),
            ),
        ),
    ])
}

fn port_json(p: &PortSpec) -> Json {
    Json::obj(vec![
        ("name", Json::str(p.name)),
        ("type", Json::str(p.ty.name())),
        ("optional", Json::Bool(p.optional)),
    ])
}

/// Every node spec plus the port types and which outputs feed which inputs.
pub fn specs_json() -> Json {
    let nodes = specs()
        .iter()
        .map(|s| {
            let params = s
                .params
                .iter()
                .map(|p| {
                    let mut f = vec![("name", Json::str(p.name)), ("label", Json::str(p.label))];
                    match p.kind {
                        ParamKind::Number {
                            min,
                            max,
                            step,
                            default,
                        } => f.extend([
                            ("kind", Json::str("number")),
                            ("min", Json::Num(min)),
                            ("max", Json::Num(max)),
                            ("step", Json::Num(step)),
                            ("default", Json::Num(default)),
                        ]),
                        ParamKind::Text { default } => {
                            f.extend([("kind", Json::str("text")), ("default", Json::str(default))])
                        }
                        ParamKind::Choice { options, default } => f.extend([
                            ("kind", Json::str("choice")),
                            (
                                "options",
                                Json::Arr(options.iter().map(|o| Json::str(*o)).collect()),
                            ),
                            ("default", Json::str(default)),
                        ]),
                        ParamKind::Bool { default } => f.extend([
                            ("kind", Json::str("bool")),
                            ("default", Json::Bool(default)),
                        ]),
                        ParamKind::MidiPort => {
                            f.extend([("kind", Json::str("midi_port")), ("default", Json::str(""))])
                        }
                    }
                    Json::obj(f)
                })
                .collect();
            Json::obj(vec![
                ("kind", Json::str(s.kind)),
                ("title", Json::str(s.title)),
                ("category", Json::str(s.category)),
                ("help", Json::str(s.help)),
                (
                    "inputs",
                    Json::Arr(s.inputs.iter().map(port_json).collect()),
                ),
                (
                    "outputs",
                    Json::Arr(s.outputs.iter().map(port_json).collect()),
                ),
                ("params", Json::Arr(params)),
            ])
        })
        .collect();
    Json::obj(vec![
        (
            "types",
            Json::Arr(
                PortType::all()
                    .iter()
                    .map(|t| Json::str(t.name()))
                    .collect(),
            ),
        ),
        (
            "compat",
            Json::Arr(
                PortType::compatibility()
                    .into_iter()
                    .map(|(a, b)| Json::Arr(vec![Json::str(a.name()), Json::str(b.name())]))
                    .collect(),
            ),
        ),
        ("nodes", Json::Arr(nodes)),
    ])
}

/// Reads a graph document: `{nodes: [{id, type, params}], wires: [{from: {node, port}, to: {node, port}}]}`.
/// Other fields (positions, notes) are ignored.
pub fn parse_graph(doc: &Json) -> Result<GraphDoc, String> {
    let mut nodes = Vec::new();
    for n in doc.get("nodes").map(Json::as_array).unwrap_or(&[]) {
        let id = n
            .get("id")
            .and_then(Json::as_str)
            .ok_or("node without an id")?
            .to_string();
        let kind = n
            .get("type")
            .and_then(Json::as_str)
            .ok_or("node without a type")?
            .to_string();
        let mut params = HashMap::new();
        for (k, v) in n.get("params").map(Json::as_object).unwrap_or(&[]) {
            let value = match v {
                Json::Num(x) => ParamValue::Number(*x),
                Json::Str(s) => ParamValue::Text(s.clone()),
                Json::Bool(b) => ParamValue::Bool(*b),
                _ => continue,
            };
            params.insert(k.clone(), value);
        }
        nodes.push(NodeDoc { id, kind, params });
    }
    let mut wires = Vec::new();
    for w in doc.get("wires").map(Json::as_array).unwrap_or(&[]) {
        let end = |key: &str| -> Option<(String, String)> {
            let e = w.get(key)?;
            Some((
                e.get("node")?.as_str()?.to_string(),
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
    Ok(GraphDoc { nodes, wires })
}

pub fn report_json(report: &CompileReport) -> Json {
    Json::obj(vec![
        (
            "issues",
            Json::Arr(
                report
                    .issues
                    .iter()
                    .map(|i| {
                        Json::obj(vec![
                            ("node", Json::str(i.node.as_str())),
                            ("message", Json::str(i.message.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "previews",
            Json::Obj(
                report
                    .previews
                    .iter()
                    .map(|(id, text)| (id.clone(), Json::str(text.as_str())))
                    .collect(),
            ),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_describe_every_node() {
        let s = specs_json();
        let nodes = s.get("nodes").unwrap().as_array();
        assert_eq!(nodes.len(), specs().len());
        let arp = nodes
            .iter()
            .find(|n| n.get("kind").unwrap().as_str() == Some("arp"))
            .unwrap();
        assert_eq!(
            arp.get("inputs").unwrap().as_array()[1].get("optional"),
            Some(&Json::Bool(true))
        );
        let compat = s.get("compat").unwrap().to_string();
        assert!(compat.contains(r#"["chord","scale"]"#));
        assert!(!compat.contains(r#"["scale","chord"]"#));
        // The whole thing round-trips through text.
        assert_eq!(Json::parse(&s.to_string()).unwrap(), s);
    }

    #[test]
    fn graph_commands() {
        let doc = r#"{"nodes":[
            {"id":"s","type":"scale","x":10,"y":20,"params":{"root":"D","scale":"Dorian"}},
            {"id":"a","type":"arp","params":{"pattern":"Up"}},
            {"id":"o","type":"midi_out","params":{"channel":2,"mute":false}}],
          "wires":[{"from":{"node":"s","port":"scale"},"to":{"node":"a","port":"scale"}},
                   {"from":{"node":"a","port":"notes"},"to":{"node":"o","port":"notes"}}]}"#;
        let mut engine = Engine::new();
        let args = Json::obj(vec![("doc", Json::str(doc))]);
        let reply = handle(&mut engine, 0.0, 1, "set_graph", &args).unwrap();
        assert_eq!(
            reply.json.get("issues").unwrap().as_array().len(),
            0,
            "{}",
            reply.json
        );
        let previews = reply.json.get("previews").unwrap();
        assert_eq!(
            previews.get("s").unwrap().as_str(),
            Some("D Dorian · D E F G A B C")
        );
        assert!(previews
            .get("o")
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("Default port · ch 2"));

        handle(&mut engine, 0.0, 1, "play", &Json::Null).unwrap();
        let status = handle(&mut engine, 1.0, 1, "status", &Json::Null)
            .unwrap()
            .json;
        assert_eq!(status.get("playing"), Some(&Json::Bool(true)));
        let mut sent = 0;
        let mut t = 0.0;
        while t < 1.0 {
            sent += engine.poll(t).len();
            t += 0.005;
        }
        assert!(sent > 0);
        let stop = handle(&mut engine, 1.0, 1, "stop", &Json::Null).unwrap();
        assert!(stop.midi.iter().all(|m| m.bytes[0] == 0x81));
        assert!(handle(&mut engine, 0.0, 1, "nope", &Json::Null).is_err());
    }
}
