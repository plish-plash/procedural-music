//! Counterpoint: the solver node, Split voices, and the Piano roll view.
//!
//! The Counterpoint node needs whole phrases, not 16th-note windows, so it asks the graph
//! for its inputs over the phrase ([`NodeImpl::input_window`]). Solutions are cached by a
//! hash of the problem. During live playback a quick answer is used at once and the full
//! solve runs on a background thread; [`solve_generation`] ticks when one finishes so the
//! editor knows to refresh its views.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use pm_counterpoint::{FixedNote, Problem, RuleSet, Solution, Species, VoiceDef};
use pm_theory::{Chord, Note, Pitch, Scale, Ticks, Tuning};

use super::theory::node;
use super::{Inputs, NodeImpl};
use crate::graph::EvalContext;
use crate::json::Json;
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

const VOICE_NAMES: [&str; 4] = ["Soprano", "Alto", "Tenor", "Bass"];
const MODES: &[&str] = &["Off", "Free", "Fixed"];
const SPECIES: &[&str] = &[
    "1st: note against note",
    "2nd: two notes per measure",
    "3rd: four notes per measure",
    "4th: syncopation",
    "5th: florid",
];

/// Ticks every time a background solve finishes.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Increases whenever a background counterpoint solve finishes; the editor refreshes its
/// views when it changes.
pub fn solve_generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

struct Entry {
    solution: Mutex<Arc<Solution>>,
    full: AtomicBool,
}

struct Cache {
    map: HashMap<u64, Arc<Entry>>,
    order: VecDeque<u64>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            map: HashMap::new(),
            order: VecDeque::new(),
        })
    })
}

/// The solution for a problem: cached, or solved now (sync) or quickly now and fully in the
/// background.
fn obtain(problem: Problem, sync: bool) -> Arc<Entry> {
    let mut h = DefaultHasher::new();
    problem.hash(&mut h);
    let key = h.finish();
    if let Some(e) = cache().lock().unwrap().map.get(&key) {
        let ready = e.full.load(Ordering::Acquire);
        if ready || !sync {
            return e.clone();
        }
    }
    let entry = if sync {
        Arc::new(Entry {
            solution: Mutex::new(Arc::new(pm_counterpoint::solve(&problem))),
            full: AtomicBool::new(true),
        })
    } else {
        let entry = Arc::new(Entry {
            solution: Mutex::new(Arc::new(pm_counterpoint::solve_quick(&problem))),
            full: AtomicBool::new(false),
        });
        let e = entry.clone();
        std::thread::Builder::new()
            .name("counterpoint".into())
            .spawn(move || {
                let full = pm_counterpoint::solve(&problem);
                *e.solution.lock().unwrap() = Arc::new(full);
                e.full.store(true, Ordering::Release);
                GENERATION.fetch_add(1, Ordering::Relaxed);
            })
            .expect("start solver thread");
        entry
    };
    let mut c = cache().lock().unwrap();
    c.map.insert(key, entry.clone());
    c.order.push_back(key);
    while c.order.len() > 48 {
        if let Some(old) = c.order.pop_front() {
            c.map.remove(&old);
        }
    }
    entry
}

// ---- Counterpoint -----------------------------------------------------------------

pub static COUNTERPOINT: NodeSpec = NodeSpec {
    kind: "counterpoint",
    title: "Counterpoint",
    category: "Counterpoint",
    help: "Writes up to four voices (soprano, alto, tenor, bass) that follow the rules of counterpoint. \
           Each voice is Off, Free (written by the solver) or Fixed (the line wired into 'fixed', such as a \
           cantus firmus). With no fixed voice, it writes over the chords of a progression. Species set the \
           rhythm. Broken rules are marked on the piano roll; hover a note to see why it was chosen. \
           Output voice numbers: soprano 1 … bass 4.",
    inputs: &[optional("fixed", PortType::Notes), optional("key", PortType::Scale), optional("chords", PortType::Chord)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "species", label: "Species", kind: ParamKind::Choice { options: SPECIES, default: "1st: note against note" } },
        ParamSpec { name: "soprano", label: "Soprano", kind: ParamKind::Choice { options: MODES, default: "Free" } },
        ParamSpec { name: "soprano_range", label: "  range", kind: ParamKind::Text { default: "C4-G5" } },
        ParamSpec { name: "alto", label: "Alto", kind: ParamKind::Choice { options: MODES, default: "Off" } },
        ParamSpec { name: "alto_range", label: "  range", kind: ParamKind::Text { default: "G3-C5" } },
        ParamSpec { name: "tenor", label: "Tenor", kind: ParamKind::Choice { options: MODES, default: "Off" } },
        ParamSpec { name: "tenor_range", label: "  range", kind: ParamKind::Text { default: "C3-G4" } },
        ParamSpec { name: "bass", label: "Bass", kind: ParamKind::Choice { options: MODES, default: "Fixed" } },
        ParamSpec { name: "bass_range", label: "  range", kind: ParamKind::Text { default: "E2-C4" } },
        ParamSpec { name: "bars", label: "Phrase bars", kind: ParamKind::Number { min: 1.0, max: 32.0, step: 1.0, default: 8.0 } },
        ParamSpec { name: "parallels", label: "Parallels", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "dissonance", label: "Dissonance", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "melody", label: "Melody", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "spacing", label: "Spacing", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "tendency", label: "Tendency", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "harmony", label: "Harmony", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "cadence", label: "Cadence", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "variety", label: "Variety", kind: ParamKind::Number { min: 0.0, max: 100.0, step: 1.0, default: 0.0 } },
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: true } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
        ParamSpec { name: "beam", label: "Search width", kind: ParamKind::Number { min: 8.0, max: 256.0, step: 8.0, default: 64.0 } },
        ParamSpec { name: "velocity", label: "Velocity", kind: ParamKind::Number { min: 1.0, max: 127.0, step: 1.0, default: 90.0 } },
        ParamSpec { name: "marks", label: "Rule marks", kind: ParamKind::Bool { default: true } },
    ],
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Off,
    Free,
    Fixed,
}

struct ViewState {
    start: Ticks,
    length: Ticks,
    bar: Ticks,
    entry: Arc<Entry>,
    /// SATB index of each solver voice.
    voice_map: Vec<usize>,
    key: Scale,
}

struct CounterpointNode {
    voices: [(Mode, i32, i32); 4],
    species: Species,
    rules: RuleSet,
    bars: u32,
    beam: usize,
    variety: u32,
    seed: u64,
    lock: bool,
    velocity: u8,
    marks: bool,
    view: Mutex<Option<ViewState>>,
}

fn parse_range(text: &str, voice: &str) -> Result<(i32, i32), String> {
    let bad = || format!("{voice} range '{text}' should look like C4-G5");
    // Split on the dash between the two pitches (not a minus sign of octave -1).
    let split = text
        .char_indices()
        .skip(1)
        .find(|&(i, c)| c == '-' && text[..i].chars().last().is_some_and(|p| p.is_ascii_digit()));
    let (a, b) = match split {
        Some((i, _)) => (&text[..i], &text[i + 1..]),
        None => return Err(bad()),
    };
    let (lo, hi) = (
        Pitch::parse(a).ok_or_else(bad)?,
        Pitch::parse(b).ok_or_else(bad)?,
    );
    if hi.step() - lo.step() < 7 {
        return Err(format!("{voice} range needs at least a fifth"));
    }
    Ok((lo.step(), hi.step()))
}

pub fn build_counterpoint(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let mut voices = [(Mode::Off, 0, 0); 4];
    let mut fixed = 0;
    for (i, name) in ["soprano", "alto", "tenor", "bass"].iter().enumerate() {
        let mode = match p.text(name).as_str() {
            "Free" => Mode::Free,
            "Fixed" => Mode::Fixed,
            _ => Mode::Off,
        };
        fixed += (mode == Mode::Fixed) as usize;
        let (lo, hi) = parse_range(&p.text(&format!("{name}_range")), VOICE_NAMES[i])?;
        voices[i] = (mode, lo, hi);
    }
    if fixed > 1 {
        return Err("only one voice can be Fixed".into());
    }
    if voices.iter().filter(|v| v.0 != Mode::Off).count() < 2 {
        return Err("turn on at least two voices".into());
    }
    if !voices.iter().any(|v| v.0 == Mode::Free) {
        return Err("set at least one voice to Free".into());
    }
    let species = match SPECIES
        .iter()
        .position(|s| *s == p.text("species"))
        .unwrap_or(0)
    {
        1 => Species::Second,
        2 => Species::Third,
        3 => Species::Fourth,
        4 => Species::Fifth,
        _ => Species::First,
    };
    Ok(Box::new(CounterpointNode {
        voices,
        species,
        rules: RuleSet {
            parallels: p.flag("parallels"),
            dissonance: p.flag("dissonance"),
            melody: p.flag("melody"),
            spacing: p.flag("spacing"),
            tendency: p.flag("tendency"),
            harmony: p.flag("harmony"),
            cadence: p.flag("cadence"),
        },
        bars: p.int("bars").max(1) as u32,
        beam: p.int("beam").max(8) as usize,
        variety: p.int("variety").max(0) as u32,
        seed: p.number("seed") as u64,
        lock: p.flag("lock"),
        velocity: p.int("velocity").clamp(1, 127) as u8,
        marks: p.flag("marks"),
        view: Mutex::new(None),
    }))
}

/// Contiguous (start, end, value) segments of a timeline covering `[from, to)`.
fn segments<T: Clone>(tl: &Timeline<T>, from: Ticks, to: Ticks) -> Vec<(Ticks, Ticks, T)> {
    let mut out = Vec::new();
    let mut t = from;
    while t < to {
        let seg = tl.at(t);
        let end = seg.end.min(to);
        out.push((t, end, seg.value.clone()));
        t = end;
    }
    out
}

impl CounterpointNode {
    fn phrase(&self, ctx: &EvalContext) -> Ticks {
        ctx.meter.ticks_per_bar() * self.bars
    }

    fn problem(
        &self,
        ins: &Inputs,
        start: Ticks,
        length: Ticks,
        ctx: &EvalContext,
    ) -> Result<(Problem, Vec<usize>, Scale), String> {
        let end = start + length;
        let bar = ctx.meter.ticks_per_bar();
        // The fixed line: one note at a time, inside the phrase.
        let mut line: Vec<Note> = ins
            .notes(0)
            .iter()
            .copied()
            .filter(|n| n.start >= start && n.start < end)
            .collect();
        line.sort_by_key(|n| (n.start, std::cmp::Reverse(n.pitch.step())));
        line.dedup_by_key(|n| n.start);
        if line.iter().any(|n| n.pitch.tuning() != Tuning::TWELVE) {
            return Err("counterpoint works in 12-EDO only".into());
        }
        let fixed_notes: Vec<FixedNote> = line
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let next = line.get(i + 1).map_or(end, |m| m.start);
                FixedNote {
                    start: n.start,
                    duration: next.min(end).saturating_sub(n.start).max(1),
                    pitch: n.pitch.step(),
                }
            })
            .collect();

        let mut voices = Vec::new();
        let mut voice_map = Vec::new();
        let mut has_fixed = false;
        for (i, &(mode, low, high)) in self.voices.iter().enumerate() {
            let fixed = match mode {
                Mode::Off => continue,
                Mode::Free => None,
                Mode::Fixed => {
                    if fixed_notes.is_empty() {
                        return Err(format!(
                            "the {} is Fixed: wire a line into 'fixed'",
                            VOICE_NAMES[i].to_lowercase()
                        ));
                    }
                    has_fixed = true;
                    Some(fixed_notes.clone())
                }
            };
            voices.push(VoiceDef {
                name: VOICE_NAMES[i].into(),
                low,
                high,
                fixed,
            });
            voice_map.push(i);
        }

        let chords: Vec<(Ticks, Ticks, Chord)> = ins
            .chord(2)
            .map(|c| segments(c, start, end))
            .unwrap_or_default();
        let keys: Vec<(Ticks, Ticks, Scale)> = match ins.scale(1) {
            Some(k) => segments(k, start, end),
            None => vec![(start, end, Scale::parse("C major").expect("valid"))],
        };
        let measures: Vec<(Ticks, Ticks)> = if has_fixed {
            fixed_notes.iter().map(|n| (n.start, n.duration)).collect()
        } else if !chords.is_empty() {
            chords.iter().map(|c| (c.0, c.1 - c.0)).collect()
        } else {
            (0..self.bars).map(|b| (start + b * bar, bar)).collect()
        };
        // Variety draws from the node's seed (fresh on each pass unless locked), per phrase.
        let seed = if self.variety == 0 {
            0
        } else {
            let stream = crate::rng::stream(self.lock, self.seed, ctx.salt);
            crate::rng::hash(&[stream, (start / length.max(1)) as u64])
        };
        let key = keys.first().map(|k| k.2).expect("at least one key segment");
        Ok((
            Problem {
                voices,
                measures,
                keys,
                chords,
                species: self.species,
                rules: self.rules,
                beam: self.beam,
                variety: self.variety,
                seed,
            },
            voice_map,
            key,
        ))
    }
}

impl NodeImpl for CounterpointNode {
    fn input_window(&self, ctx: &EvalContext) -> Option<(Ticks, Ticks)> {
        let l = self.phrase(ctx);
        let first = ctx.start / l * l;
        let last = ctx.end.saturating_sub(1) / l * l + l;
        Some((first, last))
    }

    fn eval(&self, ins: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let l = self.phrase(ctx);
        let mut out = Vec::new();
        let mut t = ctx.start / l * l;
        while t < ctx.end {
            let (problem, voice_map, key) = self.problem(ins, t, l, ctx)?;
            let entry = obtain(problem, ctx.sync);
            let sol = entry.solution.lock().unwrap().clone();
            for (slot, &p) in sol.slots.iter().zip(&sol.pitches) {
                if slot.start >= ctx.start && slot.start < ctx.end {
                    let pitch = Pitch::midi(p);
                    out.push(Note {
                        start: slot.start,
                        duration: slot.duration,
                        pitch: pitch.with_spelling(key.spell_pc(pitch.pitch_class())),
                        velocity: self.velocity,
                        voice: voice_map[slot.voice] as u8,
                    });
                }
            }
            if ctx.preview && t == 0 {
                *self.view.lock().unwrap() = Some(ViewState {
                    start: t,
                    length: l,
                    bar: ctx.meter.ticks_per_bar(),
                    entry,
                    voice_map,
                    key,
                });
            }
            t += l;
        }
        out.sort_by_key(|n| n.start);
        Ok(vec![Some(Value::Notes(out))])
    }

    fn view(&self) -> Option<Json> {
        let guard = self.view.lock().unwrap();
        let v = guard.as_ref()?;
        let sol = v.entry.solution.lock().unwrap().clone();
        let name = |p: i32| {
            let pitch = Pitch::midi(p);
            pitch
                .with_spelling(v.key.spell_pc(pitch.pitch_class()))
                .to_string()
        };
        let notes = sol
            .slots
            .iter()
            .zip(&sol.pitches)
            .map(|(s, &p)| {
                Json::obj(vec![
                    ("t", Json::Num((s.start - v.start) as f64)),
                    ("d", Json::Num(s.duration as f64)),
                    ("p", Json::Num(p as f64)),
                    ("v", Json::Num(v.voice_map[s.voice] as f64)),
                    ("fixed", Json::Bool(s.fixed.is_some())),
                    ("name", Json::str(name(p))),
                ])
            })
            .collect();
        let marks = if self.marks {
            sol.issues()
                .map(|x| {
                    Json::obj(vec![
                        ("n", Json::Num(x.slot as f64)),
                        ("o", x.other.map_or(Json::Null, |o| Json::Num(o as f64))),
                        ("rule", Json::str(x.rule.name())),
                        ("cost", Json::Num((x.cost * 10.0).round() as f64 / 10.0)),
                        ("hard", Json::Bool(x.cost >= pm_counterpoint::HARD)),
                    ])
                })
                .collect()
        } else {
            Vec::new()
        };
        let alts = sol
            .alternatives
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let (p, delta) = (*a)?;
                Some(Json::obj(vec![
                    ("n", Json::Num(i as f64)),
                    ("name", Json::str(name(p))),
                    ("delta", Json::Num((delta * 10.0).round() as f64 / 10.0)),
                ]))
            })
            .collect();
        Some(Json::obj(vec![
            ("kind", Json::str("roll")),
            ("length", Json::Num(v.length as f64)),
            ("bar", Json::Num(v.bar as f64)),
            (
                "voices",
                Json::Arr(VOICE_NAMES.iter().map(|n| Json::str(*n)).collect()),
            ),
            ("notes", Json::Arr(notes)),
            ("marks", Json::Arr(marks)),
            ("alts", Json::Arr(alts)),
            ("cost", Json::Num((sol.cost * 10.0).round() as f64 / 10.0)),
            ("pending", Json::Bool(!v.entry.full.load(Ordering::Acquire))),
        ]))
    }
}

// ---- Split voices -----------------------------------------------------------------

pub static SPLIT: NodeSpec = NodeSpec {
    kind: "split_voices",
    title: "Split voices",
    category: "Notes",
    help: "Separates notes by voice number, for sending each voice to its own instrument. Counterpoint numbers \
           voices from the top (soprano = 1); Play chords from the bottom note up.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[port("v1", PortType::Notes), port("v2", PortType::Notes), port("v3", PortType::Notes), port("v4", PortType::Notes)],
    params: &[],
};

pub fn build_split(_: &Params) -> Result<Box<dyn NodeImpl>, String> {
    node(|ins, _| {
        let all = ins.notes(0);
        Ok((0..4u8)
            .map(|v| {
                Some(Value::Notes(
                    all.iter().copied().filter(|n| n.voice == v).collect(),
                ))
            })
            .collect())
    })
}

// ---- Piano roll -------------------------------------------------------------------

pub static PIANO_ROLL: NodeSpec = NodeSpec {
    kind: "piano_roll",
    title: "Piano roll",
    category: "Analysis",
    help: "Shows the notes on a wire as a piano roll, coloured by voice. Display only.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[],
    params: &[ParamSpec {
        name: "bars",
        label: "Bars",
        kind: ParamKind::Number {
            min: 1.0,
            max: 16.0,
            step: 1.0,
            default: 4.0,
        },
    }],
};

struct PianoRoll {
    bars: u32,
    /// The notes shown, the bar length, and where the shown bars start.
    shown: Mutex<Option<(Vec<Note>, Ticks, Ticks)>>,
}

impl PianoRoll {
    /// The bars shown for a window: the block of `bars` bars holding its start.
    fn block(&self, ctx: &EvalContext) -> (Ticks, Ticks) {
        let len = ctx.meter.ticks_per_bar() * self.bars;
        let start = ctx.start - ctx.start % len;
        (start, start + len)
    }
}

pub fn build_piano_roll(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    Ok(Box::new(PianoRoll {
        bars: p.int("bars").max(1) as u32,
        shown: Mutex::new(None),
    }))
}

impl NodeImpl for PianoRoll {
    fn view_only(&self) -> bool {
        true
    }

    fn input_window(&self, ctx: &EvalContext) -> Option<(Ticks, Ticks)> {
        Some(self.block(ctx))
    }

    fn eval(&self, ins: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let (start, _) = self.block(ctx);
        *self.shown.lock().unwrap() = Some((ins.notes(0).to_vec(), ctx.meter.ticks_per_bar(), start));
        Ok(Vec::new())
    }

    fn view(&self) -> Option<Json> {
        let guard = self.shown.lock().unwrap();
        let (notes, bar, start) = guard.as_ref()?;
        let notes = notes
            .iter()
            .filter(|n| n.start >= *start)
            .map(|n| {
                let (key, _) = n.pitch.tuning().to_midi(n.pitch.step());
                Json::obj(vec![
                    ("t", Json::Num((n.start - start) as f64)),
                    ("d", Json::Num(n.duration as f64)),
                    ("p", Json::Num(key as f64)),
                    ("v", Json::Num(n.voice as f64)),
                    ("fixed", Json::Bool(false)),
                    ("name", Json::str(n.pitch.to_string())),
                ])
            })
            .collect();
        Some(Json::obj(vec![
            ("kind", Json::str("roll")),
            ("length", Json::Num((*bar * self.bars) as f64)),
            ("bar", Json::Num(*bar as f64)),
            (
                "voices",
                Json::Arr((1..=8).map(|i| Json::str(format!("Voice {i}"))).collect()),
            ),
            ("notes", Json::Arr(notes)),
            ("marks", Json::Arr(vec![])),
            ("alts", Json::Arr(vec![])),
        ]))
    }

    fn is_sink(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{handle, parse_graph};
    use crate::{Engine, Graph};
    use pm_theory::Meter;

    fn doc() -> crate::GraphDoc {
        let json = Json::parse(
            r#"{"nodes":[
            {"id":"s","type":"scale","params":{"root":"D","scale":"Dorian"}},
            {"id":"r","type":"pattern","params":{"pattern":"x","step":"1/1"}},
            {"id":"cf","type":"sequencer","params":{"degrees":"1 3 2 1 4 3 5 4 3 2 1","octave":3}},
            {"id":"cp","type":"counterpoint","params":{"bars":11}},
            {"id":"sp","type":"split_voices","params":{}},
            {"id":"roll","type":"piano_roll","params":{"bars":4}}],
          "wires":[
            {"from":{"node":"s","port":"scale"},"to":{"node":"cf","port":"scale"}},
            {"from":{"node":"r","port":"rhythm"},"to":{"node":"cf","port":"rhythm"}},
            {"from":{"node":"cf","port":"notes"},"to":{"node":"cp","port":"fixed"}},
            {"from":{"node":"s","port":"scale"},"to":{"node":"cp","port":"key"}},
            {"from":{"node":"cp","port":"notes"},"to":{"node":"sp","port":"notes"}},
            {"from":{"node":"sp","port":"v1"},"to":{"node":"roll","port":"notes"}}]}"#,
        )
        .unwrap();
        parse_graph(&json).unwrap()
    }

    #[test]
    fn solves_a_cantus_firmus_in_windows() {
        let g = Graph::compile(&doc());
        assert!(g.issues().is_empty(), "{:?}", g.issues());
        let bar = Meter::COMMON.ticks_per_bar();
        // Evaluate bar 9 (the penultimate) on its own: the node still sees the whole phrase.
        let ctx = EvalContext {
            sync: true,
            ..EvalContext::new(9 * bar, 10 * bar, Meter::COMMON, 0)
        };
        let report = g.report(Meter::COMMON, 4, true);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let view = report
            .views
            .iter()
            .find(|(id, _)| id == "cp")
            .expect("counterpoint view")
            .1
            .clone();
        assert_eq!(view.get("notes").unwrap().as_array().len(), 22);
        assert_eq!(view.get("pending"), Some(&Json::Bool(false)));
        let roll = report
            .views
            .iter()
            .find(|(id, _)| id == "roll")
            .expect("piano roll view")
            .1
            .clone();
        assert_eq!(
            roll.get("notes").unwrap().as_array().len(),
            4,
            "soprano, bars 1-4"
        );
        let _ = ctx;
    }

    #[test]
    fn plays_through_the_engine_with_background_solving() {
        let mut engine = Engine::new();
        let mut d = doc();
        // Send the soprano to a MIDI sink so notes come out of the engine.
        d.nodes.push(crate::NodeDoc {
            id: "out".into(),
            kind: "midi_out".into(),
            params: Default::default(),
        });
        d.wires.push(crate::WireDoc {
            from_node: "sp".into(),
            from_port: "v1".into(),
            to_node: "out".into(),
            to_port: "notes".into(),
        });
        let report = engine.set_graph(&d);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let gen0 = solve_generation();
        handle(&mut engine, 0.0, 1, "play", &Json::Null).unwrap();
        let mut ons = 0;
        let mut t = 0.0;
        while t < 3.0 {
            ons += engine
                .poll(t)
                .iter()
                .filter(|m| m.bytes[0] & 0xf0 == 0x90)
                .count();
            t += 0.01;
        }
        assert!(ons >= 1, "notes play while the full solve runs");
        // The background solve finishes and bumps the generation.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while solve_generation() == gen0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(solve_generation() > gen0);
    }

    #[test]
    fn bad_settings_are_reported() {
        let mut d = doc();
        d.nodes[3]
            .params
            .insert("alto".into(), crate::ParamValue::Text("Fixed".into()));
        let g = Graph::compile(&d);
        assert_eq!(g.issues()[0].message, "only one voice can be Fixed");
        assert!(parse_range("C4-C4", "Soprano").is_err());
        assert_eq!(parse_range("C-1-C1", "Bass").unwrap(), (0, 24));
    }
}
