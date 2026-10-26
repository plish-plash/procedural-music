//! Theory views: the circle of fifths, the Tonnetz and the interval matrix. Display only:
//! each keeps the chords (and key) on its inputs from the last preview, and the editor
//! draws them, following the playhead. Left unwired, their inputs read the harmonic
//! context, so they show the song's harmony.

use std::sync::Mutex;

use pm_theory::{Chord, Meter, Scale};

use super::{Inputs, NodeImpl};
use crate::graph::{value_view, EvalContext};
use crate::json::Json;
use crate::spec::{optional, NodeSpec, Params, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

pub static CIRCLE: NodeSpec = NodeSpec {
    kind: "circle_of_fifths",
    title: "Circle of fifths",
    category: "Analysis",
    help: "Shows the key and the chords on the circle of fifths: the key's notes around the rim, \
           the sounding chord's notes joined inside, and the path of the chord roots.",
    inputs: &[optional("key", PortType::Scale), optional("chords", PortType::Chord)],
    outputs: &[],
    params: &[],
};

pub static TONNETZ: NodeSpec = NodeSpec {
    kind: "tonnetz",
    title: "Tonnetz",
    category: "Analysis",
    help: "Shows the chords on the Tonnetz, a grid of notes where fifths run across and major \
           thirds up: every major and minor triad is a triangle, and chords that share notes sit \
           side by side, so smooth voice leading looks like short steps.",
    inputs: &[optional("chords", PortType::Chord)],
    outputs: &[],
    params: &[],
};

pub static INTERVALS: NodeSpec = NodeSpec {
    kind: "interval_matrix",
    title: "Interval matrix",
    category: "Analysis",
    help: "Shows the interval between every two notes of the sounding chord, coloured by how they \
           sound (perfect, consonant, dissonant), and its interval-class vector: how many of each \
           interval it holds.",
    inputs: &[optional("chords", PortType::Chord)],
    outputs: &[],
    params: &[],
};

/// What a theory view keeps from its last preview.
#[derive(Clone)]
struct Shown {
    key: Option<Timeline<Scale>>,
    chords: Option<Timeline<Chord>>,
    meter: Meter,
}

struct TheoryView {
    /// The view's kind for the editor ("circle", "tonnetz", "intervals").
    kind: &'static str,
    /// Which input holds the key, if this view has one; the chords are on the last input.
    key_input: Option<usize>,
    shown: Mutex<Option<Shown>>,
}

pub fn build(kind: &str, _: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let (kind, key_input) = match kind {
        "circle_of_fifths" => ("circle", Some(0)),
        "tonnetz" => ("tonnetz", None),
        _ => ("intervals", None),
    };
    Ok(Box::new(TheoryView {
        kind,
        key_input,
        shown: Mutex::new(None),
    }))
}

impl NodeImpl for TheoryView {
    fn view_only(&self) -> bool {
        true
    }

    fn eval(&self, ins: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let chords_input = self.key_input.map_or(0, |k| k + 1);
        *self.shown.lock().unwrap() = Some(Shown {
            key: self.key_input.and_then(|k| ins.scale(k).cloned()),
            chords: ins.chord(chords_input).cloned(),
            meter: ctx.meter,
        });
        Ok(Vec::new())
    }

    /// `{kind, chords, key}`: the chords and key as keyboard views (their timelines of
    /// notes and pitch classes), or null when not wired (and no context).
    fn view(&self) -> Option<Json> {
        let shown = self.shown.lock().unwrap().clone()?;
        let as_view = |v: Option<Value>| {
            v.and_then(|v| value_view(&v, shown.meter, 0, shown.meter.ticks_per_bar()))
                .unwrap_or(Json::Null)
        };
        Some(Json::obj(vec![
            ("kind", Json::str(self.kind)),
            ("chords", as_view(shown.chords.map(Value::Chord))),
            ("key", as_view(shown.key.map(Value::Scale))),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use crate::graph::tests::{node, wire};
    use crate::graph::{Graph, GraphDoc};
    use crate::spec::ParamValue;
    use pm_theory::Meter;

    #[test]
    fn theory_views_show_wired_or_context_harmony() {
        let doc = GraphDoc {
            nodes: vec![
                node("k", "scale", &[]),
                node("p", "progression", &[("chords", ParamValue::Text("ii V I".into()))]),
                node("circle", "circle_of_fifths", &[]),
                node("net", "tonnetz", &[]),
                node("ctx", "context", &[]),
                node("iv", "interval_matrix", &[]),
            ],
            wires: vec![
                wire("k", "scale", "p", "key"),
                wire("p", "chords", "circle", "chords"),
                wire("k", "scale", "circle", "key"),
                wire("p", "chords", "net", "chords"),
                wire("p", "chords", "ctx", "chords"),
            ],
            ..Default::default()
        };
        let r = Graph::compile(&doc).report(Meter::COMMON, 4, true);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        let view = |id: &str| r.views.iter().find(|v| v.0 == id).unwrap().1.clone();
        let circle = view("circle");
        assert_eq!(circle.get("kind").unwrap().as_str(), Some("circle"));
        let segs = circle.get("chords").unwrap().get("segs").unwrap().as_array().len();
        assert_eq!(segs, 3);
        assert_eq!(circle.get("key").unwrap().get("of").unwrap().as_str(), Some("scale"));
        assert_eq!(view("net").get("key"), Some(&crate::json::Json::Null));
        // Unwired, the interval matrix reads the context.
        let iv = view("iv");
        assert_eq!(iv.get("chords").unwrap().get("segs").unwrap().as_array().len(), 3);
    }
}
