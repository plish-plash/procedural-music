//! Theory sources and transforms: scales, chords, progressions, voicings.

use pm_theory::{
    harmony, voicing, Chord, Interval, Meter, PcSet, Pitch, Progression, Scale, ScaleKind,
    Spelling, Ticks, Tuning,
};

use super::{Inputs, NodeImpl};
use crate::graph::EvalContext;
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

const ROOTS: &[&str] = &[
    "C", "C#", "Db", "D", "D#", "Eb", "E", "F", "F#", "Gb", "G", "G#", "Ab", "A", "A#", "Bb", "B",
];

const SCALE_NAMES: &[&str] = &[
    "Major",
    "Minor",
    "Harmonic minor",
    "Melodic minor",
    "Dorian",
    "Phrygian",
    "Lydian",
    "Mixolydian",
    "Locrian",
    "Major pentatonic",
    "Minor pentatonic",
    "Blues",
    "Whole tone",
    "Octatonic (half-whole)",
    "Octatonic (whole-half)",
    "Chromatic",
    "Custom",
];

fn parse_pitch(text: &str, what: &str) -> Result<Pitch, String> {
    Pitch::parse(text).ok_or_else(|| format!("{what} '{text}' is not a pitch like C3 or F#4"))
}

/// A boxed node from a closure over its inputs.
struct FnNode<F>(F);

impl<F> NodeImpl for FnNode<F>
where
    F: Fn(&Inputs, &EvalContext) -> Result<Vec<Option<Value>>, String> + Send + Sync,
{
    fn eval(&self, inputs: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        (self.0)(inputs, ctx)
    }
}

pub(crate) fn node<F>(f: F) -> Result<Box<dyn NodeImpl>, String>
where
    F: Fn(&Inputs, &EvalContext) -> Result<Vec<Option<Value>>, String> + Send + Sync + 'static,
{
    Ok(Box::new(FnNode(f)))
}

// ---- Scale ---------------------------------------------------------------

pub static SCALE: NodeSpec = NodeSpec {
    kind: "scale",
    title: "Scale",
    category: "Theory",
    help: "A root and a named scale, or Custom to pick the notes yourself. The notes repeat in every octave.",
    inputs: &[],
    outputs: &[port("scale", PortType::Scale)],
    params: &[
        ParamSpec {
            name: "root",
            label: "Root",
            kind: ParamKind::Choice {
                options: ROOTS,
                default: "C",
            },
        },
        ParamSpec {
            name: "scale",
            label: "Scale",
            kind: ParamKind::Choice {
                options: SCALE_NAMES,
                default: "Major",
            },
        },
        ParamSpec {
            name: "notes",
            label: "Notes",
            kind: ParamKind::PitchSet { default: "C D E G A" },
        },
    ],
};

pub fn build_scale(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let root = Spelling::parse(&p.text("root")).ok_or("bad root")?;
    let name = p.text("scale");
    let scale = if name == "Custom" {
        custom_scale(root, &p.text("notes"))?
    } else {
        let kind = ScaleKind::all()
            .find(|k| k.name() == name)
            .ok_or_else(|| format!("unknown scale '{name}'"))?;
        Scale::preset(kind, root)
    };
    node(move |_, _| Ok(vec![Some(Value::Scale(Timeline::constant(scale)))]))
}

/// A scale on `root` from note names ("C D Eb G A"). The root is always included.
fn custom_scale(root: Spelling, notes: &str) -> Result<Scale, String> {
    let mut set = PcSet::empty(Tuning::TWELVE);
    set.insert(root.pc() as i32);
    for name in notes.split(|c: char| c.is_whitespace() || c == ',') {
        if name.is_empty() {
            continue;
        }
        let sp = Spelling::parse(name).ok_or_else(|| format!("'{name}' is not a note name"))?;
        set.insert(sp.pc() as i32);
    }
    Ok(Scale::new(root.pc(), set).expect("root is in the set").with_root_spelling(root))
}

// ---- Chord ---------------------------------------------------------------

pub static CHORD: NodeSpec = NodeSpec {
    kind: "chord",
    title: "Chord",
    category: "Theory",
    help: "A chord from a symbol such as Cmaj7, F#m7b5 or G7/B.",
    inputs: &[],
    outputs: &[port("chord", PortType::Chord)],
    params: &[
        ParamSpec {
            name: "symbol",
            label: "Symbol",
            kind: ParamKind::Text { default: "Cmaj7" },
        },
        ParamSpec {
            name: "octave",
            label: "Octave",
            kind: ParamKind::Number {
                min: 0.0,
                max: 8.0,
                step: 1.0,
                default: 4.0,
            },
        },
    ],
};

pub fn build_chord(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let symbol = p.text("symbol");
    let chord = Chord::from_symbol(&symbol, p.int("octave"))
        .ok_or_else(|| format!("unknown chord symbol '{symbol}'"))?;
    node(move |_, _| Ok(vec![Some(Value::Chord(Timeline::constant(chord)))]))
}

// ---- Progression -----------------------------------------------------------

pub static PROGRESSION: NodeSpec = NodeSpec {
    kind: "progression",
    title: "Progression",
    category: "Theory",
    help: "Chords over time, as Roman numerals in the key (ii7 V7 Imaj7) or chord symbols (Dm7 G7 Cmaj7). \
           Roman numerals take the key from the Key input (a Scale node, or the Harmonic context); \
           when the key changes, the numerals follow it. '-' holds the previous chord one more step; \
           '|' is ignored.",
    inputs: &[optional("key", PortType::Scale)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec { name: "chords", label: "Chords", kind: ParamKind::Text { default: "I vi IV V" } },
        ParamSpec {
            name: "notation",
            label: "Notation",
            kind: ParamKind::Choice { options: &["Roman numerals", "Chord symbols"], default: "Roman numerals" },
        },
        ParamSpec {
            name: "bars",
            label: "Bars each",
            kind: ParamKind::Number { min: 0.25, max: 8.0, step: 0.25, default: 1.0 },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 1.0, max: 7.0, step: 1.0, default: 4.0 } },
    ],
};

fn chord_timeline(prog: &Progression) -> Timeline<Chord> {
    Timeline::from_steps(prog.steps().iter().map(|s| (s.duration, s.chord)).collect()).expect("non-empty")
}

pub fn build_progression(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let text = p.text("chords");
    let roman = p.text("notation") == "Roman numerals";
    let octave = p.int("octave");
    let bar = Meter::COMMON.ticks_per_bar();
    let duration = ((p.number("bars") * bar as f64).round() as u32).max(1);
    if !roman {
        let prog = Progression::from_symbols(&text, duration, octave)
            .ok_or_else(|| bad_token(&text, |t| Chord::from_symbol(t, octave).is_some()))?;
        if prog.is_empty() {
            return Err("no chords".into());
        }
        let chords = chord_timeline(&prog);
        return node(move |_, _| Ok(vec![Some(Value::Chord(chords.clone()))]));
    }
    // Check the numerals now (in any key); they are read in the real key when it arrives.
    let probe = Scale::parse("C major").expect("valid");
    let shape = Progression::from_roman(&text, &probe, duration, octave)
        .ok_or_else(|| bad_token(&text, |t| pm_theory::parse_roman(t, &probe, octave).is_some()))?;
    if shape.is_empty() {
        return Err("no chords".into());
    }
    node(move |ins, _| {
        let keys = ins
            .scale(0)
            .ok_or("Roman numerals need a key: wire a Scale into Key (or add a Harmonic context)")?;
        Ok(vec![Some(Value::Chord(roman_in_keys(&text, &shape, keys, duration, octave)))])
    })
}

/// The numerals of `text` read in the key sounding at each chord. `shape` is the same
/// progression in any key (for its step lengths). With a changing key, the result runs as
/// long as the progression and the key together (or one pass of the progression, if that
/// would be very long).
fn roman_in_keys(text: &str, shape: &Progression, keys: &Timeline<Scale>, duration: Ticks, octave: i32) -> Timeline<Chord> {
    let mut parsed: Vec<(Scale, Progression)> = Vec::new();
    let mut in_key = |key: &Scale| -> Option<Progression> {
        if let Some((_, p)) = parsed.iter().find(|(k, _)| k == key) {
            return Some(*p);
        }
        let p = Progression::from_roman(text, key, duration, octave)?;
        parsed.push((*key, p));
        Some(p)
    };
    let length = shape.length();
    if keys.is_constant() {
        return chord_timeline(&in_key(keys.at(0).value).unwrap_or(*shape));
    }
    let together = length / gcd(length, keys.length()) * keys.length();
    let total = if together <= 16 * length.max(keys.length()) { together } else { length };
    let mut steps = Vec::new();
    let mut t = 0;
    while t < total {
        for (i, s) in shape.steps().iter().enumerate() {
            let chord = in_key(keys.at(t).value).map_or(s.chord, |p| p.steps()[i].chord);
            steps.push((s.duration, chord));
            t += s.duration;
        }
    }
    Timeline::from_steps(steps).expect("non-empty")
}

fn gcd(a: Ticks, b: Ticks) -> Ticks {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

fn bad_token(text: &str, ok: impl Fn(&str) -> bool) -> String {
    match text
        .split_whitespace()
        .find(|t| *t != "-" && *t != "|" && !ok(t))
    {
        Some(t) => format!("can't read '{t}'"),
        None => "'-' needs a chord before it".into(),
    }
}

// ---- Mode rotate -----------------------------------------------------------

pub static MODE: NodeSpec = NodeSpec {
    kind: "mode",
    title: "Mode rotate",
    category: "Theory",
    help: "Starts the scale on another of its degrees: degree 2 of C major is D Dorian. 1 leaves it unchanged.",
    inputs: &[port("scale", PortType::Scale)],
    outputs: &[port("scale", PortType::Scale)],
    params: &[ParamSpec {
        name: "degree",
        label: "Degree",
        kind: ParamKind::Number { min: 1.0, max: 12.0, step: 1.0, default: 1.0 },
    }],
};

pub fn build_mode(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let degree = p.int("degree") - 1;
    node(move |ins, _| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Scale(scales.map(|s| s.mode(degree))))])
    })
}

// ---- Transpose -------------------------------------------------------------

pub static TRANSPOSE: NodeSpec = NodeSpec {
    kind: "transpose",
    title: "Transpose",
    category: "Theory",
    help: "Moves every chord by a number of semitones, or by steps of the key, so C moves to Dm, \
           then Em, in C major. Scale steps use the key input (or the Harmonic context).",
    inputs: &[port("chords", PortType::Chord), optional("key", PortType::Scale)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec {
            name: "by",
            label: "By",
            kind: ParamKind::Choice { options: &["Semitones", "Scale steps"], default: "Semitones" },
        },
        ParamSpec {
            name: "semitones",
            label: "Semitones",
            kind: ParamKind::Number {
                min: -24.0,
                max: 24.0,
                step: 1.0,
                default: 0.0,
            },
        },
        ParamSpec {
            name: "steps",
            label: "Steps",
            kind: ParamKind::Number { min: -14.0, max: 14.0, step: 1.0, default: 1.0 },
        },
    ],
};

pub fn build_transpose(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let n = p.int("semitones");
    let steps = (p.text("by") == "Scale steps").then(|| p.int("steps"));
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let out = match steps {
            None => chords.map(|c| c.transpose(Interval::new(c.tuning(), n))),
            Some(steps) => {
                let key = ins.scale(1).ok_or("scale steps need a key: wire a scale in or add a Harmonic context")?;
                chords.zip_with(key, |c, k| harmony::transpose_in_key(c, k, steps))
            }
        };
        Ok(vec![Some(Value::Chord(out))])
    })
}

// ---- Transpose key ---------------------------------------------------------------

pub static TRANSPOSE_KEY: NodeSpec = NodeSpec {
    kind: "transpose_key",
    title: "Transpose key",
    category: "Theory",
    help: "Moves a scale or key by a number of semitones: C major up 5 is F major.",
    inputs: &[port("scale", PortType::Scale)],
    outputs: &[port("scale", PortType::Scale)],
    params: &[ParamSpec {
        name: "semitones",
        label: "Semitones",
        kind: ParamKind::Number { min: -12.0, max: 12.0, step: 1.0, default: 0.0 },
    }],
};

pub fn build_transpose_key(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let n = p.int("semitones");
    node(move |ins, _| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Scale(scales.map(|s| s.transpose(Interval::new(s.tuning(), n)))))])
    })
}

// ---- Chord from degree -------------------------------------------------------------

pub static CHORD_DEGREE: NodeSpec = NodeSpec {
    kind: "chord_degree",
    title: "Chord from degree",
    category: "Theory",
    help: "The chord built in thirds on a degree of the key: degree 5 of C major is G (or G7). \
           Pin the degree and drive it with Steps or an LFO to make a progression.",
    inputs: &[port("key", PortType::Scale)],
    outputs: &[port("chord", PortType::Chord)],
    params: &[
        ParamSpec {
            name: "degree",
            label: "Degree",
            kind: ParamKind::Number { min: 1.0, max: 12.0, step: 1.0, default: 1.0 },
        },
        ParamSpec {
            name: "size",
            label: "Size",
            kind: ParamKind::Choice { options: &["Triad", "7th", "9th"], default: "Triad" },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 1.0, max: 7.0, step: 1.0, default: 4.0 } },
    ],
};

pub fn build_chord_degree(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let degree = p.int("degree") - 1;
    let size = match p.text("size").as_str() {
        "7th" => 4,
        "9th" => 5,
        _ => 3,
    };
    let octave = p.int("octave");
    node(move |ins, _| {
        let Some(keys) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(keys.map(|k| k.chord_on_degree(degree, size, octave))))])
    })
}

// ---- Extend ------------------------------------------------------------------------

pub static EXTEND: NodeSpec = NodeSpec {
    kind: "extend",
    title: "Extend",
    category: "Theory",
    help: "Adds the key's notes in thirds above each chord's root: C in C major becomes Cmaj7, \
           Cmaj9… Chords whose root isn't in the key pass through.",
    inputs: &[port("chords", PortType::Chord), port("key", PortType::Scale)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[ParamSpec {
        name: "to",
        label: "Up to",
        kind: ParamKind::Choice { options: &["7th", "9th", "11th", "13th"], default: "7th" },
    }],
};

pub fn build_extend(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let size = match p.text("to").as_str() {
        "9th" => 5,
        "11th" => 6,
        "13th" => 7,
        _ => 4,
    };
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let out = match ins.scale(1) {
            Some(key) => chords.zip_with(key, |c, k| harmony::extend(c, k, size)),
            None => chords.clone(),
        };
        Ok(vec![Some(Value::Chord(out))])
    })
}

// ---- Substitute ----------------------------------------------------------------------

pub static SUBSTITUTE: NodeSpec = NodeSpec {
    kind: "substitute",
    title: "Substitute",
    category: "Theory",
    help: "Swaps chords for related ones. Tritone: a dominant seventh for the one a tritone away \
           (G7 → Db7). Relative: major ↔ relative minor (C → Am). Parallel: major ↔ minor on the \
           same root (C → Cm). 'Only chords' picks which chords of a progression change (\"2 4\"); \
           empty means all.",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec {
            name: "type",
            label: "Type",
            kind: ParamKind::Choice { options: &["Tritone", "Relative", "Parallel"], default: "Tritone" },
        },
        ParamSpec { name: "only", label: "Only chords", kind: ParamKind::Text { default: "" } },
    ],
};

/// Positions or degrees from text such as "2 4" (1-based); empty means all.
fn number_list(text: &str, what: &str) -> Result<Option<Vec<usize>>, String> {
    let mut out = Vec::new();
    for t in text.split(|c: char| c.is_whitespace() || c == ',') {
        if t.is_empty() {
            continue;
        }
        match t.parse::<usize>() {
            Ok(n) if n >= 1 => out.push(n),
            _ => return Err(format!("{what}: '{t}' is not a number from 1 up")),
        }
    }
    Ok((!out.is_empty()).then_some(out))
}

pub fn build_substitute(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let f: fn(&Chord) -> Chord = match p.text("type").as_str() {
        "Relative" => harmony::relative,
        "Parallel" => harmony::parallel,
        _ => harmony::tritone_sub,
    };
    let only = number_list(&p.text("only"), "Only chords")?;
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let mut i = 0;
        let out = chords.map(|c| {
            i += 1;
            if only.as_ref().is_none_or(|o| o.contains(&i)) {
                f(c)
            } else {
                *c
            }
        });
        Ok(vec![Some(Value::Chord(out))])
    })
}

// ---- Borrow --------------------------------------------------------------------------

pub static BORROW: NodeSpec = NodeSpec {
    kind: "borrow",
    title: "Borrow",
    category: "Theory",
    help: "Modal mixture: chords on the listed degrees of the key are rebuilt from a parallel \
           mode on the same root, so IV, vi and vii° in C major become iv, ♭VI and ♭VII from C minor.",
    inputs: &[port("chords", PortType::Chord), port("key", PortType::Scale)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec {
            name: "from",
            label: "From",
            kind: ParamKind::Choice {
                options: &["Minor", "Dorian", "Phrygian", "Lydian", "Mixolydian", "Locrian", "Harmonic minor", "Major"],
                default: "Minor",
            },
        },
        ParamSpec { name: "degrees", label: "Degrees", kind: ParamKind::Text { default: "4 6 7" } },
    ],
};

pub fn build_borrow(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let kind = ScaleKind::from_name(&p.text("from")).ok_or("unknown mode")?;
    let degrees = number_list(&p.text("degrees"), "Degrees")?;
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let Some(keys) = ins.scale(1) else {
            return Ok(vec![Some(Value::Chord(chords.clone()))]);
        };
        let out = chords.zip_with(keys, |c, key| {
            let on = harmony::root_degree(c, key)
                .is_some_and(|d| degrees.as_ref().is_none_or(|ds| ds.contains(&(d + 1))));
            if !on || !key.tuning().is_twelve() {
                return *c;
            }
            let root = key
                .root_spelling()
                .unwrap_or_else(|| Spelling::key_default(key.root()));
            harmony::borrow(c, key, &Scale::preset(kind, root))
        });
        Ok(vec![Some(Value::Chord(out))])
    })
}

// ---- Secondary dominant ------------------------------------------------------------------

pub static SECONDARY: NodeSpec = NodeSpec {
    kind: "secondary_dominant",
    title: "Secondary dominant",
    category: "Theory",
    help: "Puts the dominant seventh of a chord (V7 of it) just before it, taking part of the \
           previous chord's time: before G in C major, D7. Targets are the degrees of the key to \
           lead into.",
    inputs: &[port("chords", PortType::Chord), port("key", PortType::Scale)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec { name: "targets", label: "Targets", kind: ParamKind::Text { default: "2 3 4 5 6" } },
        ParamSpec {
            name: "share",
            label: "Takes",
            kind: ParamKind::Choice { options: &["1/2", "1/4", "All"], default: "1/2" },
        },
    ],
};

pub fn build_secondary(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let targets = number_list(&p.text("targets"), "Targets")?;
    let share = match p.text("share").as_str() {
        "1/4" => 4,
        "All" => 1,
        _ => 2,
    };
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let Some(keys) = ins.scale(1) else {
            return Ok(vec![Some(Value::Chord(chords.clone()))]);
        };
        Ok(vec![Some(Value::Chord(secondary_dominants(chords, keys, targets.as_deref(), share)))])
    })
}

/// Inserts V7 of each target chord at the end of the chord before it (looping, so the first
/// chord's dominant goes at the end of the last). `share` 2 takes half of that chord's time,
/// 4 a quarter, 1 all of it.
fn secondary_dominants(
    chords: &Timeline<Chord>,
    keys: &Timeline<Scale>,
    targets: Option<&[usize]>,
    share: Ticks,
) -> Timeline<Chord> {
    let steps = chords.steps();
    let n = steps.len();
    if chords.is_constant() || n < 2 {
        return chords.clone();
    }
    let mut dominant: Vec<Option<Chord>> = vec![None; n];
    let mut start = 0;
    for (i, (duration, target)) in steps.iter().enumerate() {
        let key = keys.at(start).value;
        start += duration;
        let Some(degree) = harmony::root_degree(target, key) else {
            continue;
        };
        if degree == 0 || targets.is_some_and(|t| !t.contains(&(degree + 1))) {
            continue;
        }
        let (Some(root), Some(near)) = (target.root().or_else(|| target.bass().map(|b| b.pitch_class())), target.bass()) else {
            continue;
        };
        let dom = harmony::dominant_of(root, near);
        let prev = &steps[(i + n - 1) % n].1;
        // Already a major chord on the dominant's root (a plain V → I): leave it.
        let dom_root = dom.root().unwrap_or(0) as i32;
        if prev.root() == dom.root() && prev.contains_pc(dom_root + 4) {
            continue;
        }
        dominant[(i + n - 1) % n] = Some(dom);
    }
    let mut out = Vec::with_capacity(n * 2);
    for ((duration, chord), dom) in steps.iter().zip(dominant) {
        match dom {
            Some(dom) => {
                let cut = duration / share;
                if duration - cut > 0 {
                    out.push((duration - cut, *chord));
                }
                out.push((cut.max(1), dom));
            }
            None => out.push((*duration, *chord)),
        }
    }
    Timeline::from_steps(out).expect("non-empty")
}

// ---- Scale set ---------------------------------------------------------------------------

pub static SCALE_SET: NodeSpec = NodeSpec {
    kind: "scale_set",
    title: "Scale set",
    category: "Theory",
    help: "Combines the notes of two scales: the notes in both (∩), in either (∪), or in A but \
           not B (−). The root is A's, or its next note if A's root is gone.",
    inputs: &[port("a", PortType::Scale), port("b", PortType::Scale)],
    outputs: &[port("scale", PortType::Scale)],
    params: &[ParamSpec {
        name: "op",
        label: "Notes in",
        kind: ParamKind::Choice { options: &["Both (∩)", "Either (∪)", "A not B (−)"], default: "Both (∩)" },
    }],
};

pub fn build_scale_set(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let op = p.text("op");
    let f: fn(PcSet, PcSet) -> PcSet = match op.as_str() {
        "Either (∪)" => PcSet::union,
        "A not B (−)" => PcSet::difference,
        _ => PcSet::intersection,
    };
    node(move |ins, _| {
        let (Some(a), Some(b)) = (ins.scale(0), ins.scale(1)) else {
            return Ok(vec![ins.scale(0).map(|a| Value::Scale(a.clone()))]);
        };
        let mut empty = false;
        let out = a.zip_with(b, |a, b| {
            if a.tuning() != b.tuning() {
                return *a;
            }
            let set = f(a.pc_set(), b.pc_set());
            let Some(root) = set.iter_from(a.root()).next() else {
                empty = true;
                return *a;
            };
            let scale = Scale::new(root, set).expect("root is in the set");
            match a.root_spelling() {
                Some(sp) if root == a.root() => scale.with_root_spelling(sp),
                _ => scale,
            }
        });
        if empty {
            return Err("the two scales have no notes in common".into());
        }
        Ok(vec![Some(Value::Scale(out))])
    })
}

// ---- Voice -----------------------------------------------------------------

pub static VOICE: NodeSpec = NodeSpec {
    kind: "voice",
    title: "Voice",
    category: "Voicing",
    help: "Picks a number of notes from each chord inside a pitch range, root at the bottom where possible. \
           Extra notes double chord tones higher up.",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec { name: "notes", label: "Notes", kind: ParamKind::Number { min: 1.0, max: 8.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "low", label: "Lowest", kind: ParamKind::Text { default: "C3" } },
        ParamSpec { name: "high", label: "Highest", kind: ParamKind::Text { default: "C5" } },
    ],
};

pub fn build_voice(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let count = p.int("notes").max(1) as usize;
    let low = parse_pitch(&p.text("low"), "lowest note")?;
    let high = parse_pitch(&p.text("high"), "highest note")?;
    if high <= low {
        return Err("the highest note must be above the lowest".into());
    }
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(
            chords.map(|c| voice_chord(c, count, low, high)),
        ))])
    })
}

/// `count` notes of the chord within `[low, high]`, starting from the lowest root.
pub(crate) fn voice_chord(chord: &Chord, count: usize, low: Pitch, high: Pitch) -> Chord {
    let pcs = chord.pitch_classes();
    if pcs.is_empty() {
        return *chord;
    }
    let tuning = chord.tuning();
    let low = Pitch::new(tuning, low.step());
    let high = Pitch::new(tuning, high.step());
    let root = chord
        .root()
        .or_else(|| chord.bass().map(|p| p.pitch_class()))
        .unwrap_or(0);
    let spell = |p: Pitch| {
        let sp = chord
            .notes()
            .iter()
            .find(|n| n.pitch_class() == p.pitch_class())
            .and_then(|n| n.spelling());
        p.with_spelling(sp)
    };
    // Every chord tone in range, low to high.
    let candidates: Vec<Pitch> = (low.step()..=high.step())
        .map(|s| Pitch::new(tuning, s))
        .filter(|p| pcs.contains(p.step()))
        .collect();
    let start = candidates
        .iter()
        .position(|p| p.pitch_class() == root)
        .unwrap_or(0);
    let mut picked: Vec<Pitch> = candidates[start..].iter().take(count).copied().collect();
    // Not enough room above the root: take notes below it.
    let mut below = candidates[..start].iter().rev();
    while picked.len() < count {
        match below.next() {
            Some(&p) => picked.insert(0, p),
            None => break,
        }
    }
    let mut out = Chord::empty(tuning);
    for p in picked {
        let _ = out.add(spell(p));
    }
    out.with_root(root)
}

// ---- Voice lead --------------------------------------------------------------

pub static VOICE_LEAD: NodeSpec = NodeSpec {
    kind: "voice_lead",
    title: "Voice lead",
    category: "Voicing",
    help:
        "Revoices each chord so every voice moves as little as possible from the previous chord, \
           keeping the number of voices. Put a Voice node before it to set how many.",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[
        ParamSpec {
            name: "low",
            label: "Lowest",
            kind: ParamKind::Text { default: "C3" },
        },
        ParamSpec {
            name: "high",
            label: "Highest",
            kind: ParamKind::Text { default: "C5" },
        },
    ],
};

pub fn build_voice_lead(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let low = parse_pitch(&p.text("low"), "lowest note")?;
    let high = parse_pitch(&p.text("high"), "highest note")?;
    if high <= low {
        return Err("the highest note must be above the lowest".into());
    }
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(lead_timeline(chords, low, high)))])
    })
}

/// Voice-leads a looping progression. The first chord is led from the last one of a
/// first pass, so the loop point is as smooth as the rest.
pub(crate) fn lead_timeline(chords: &Timeline<Chord>, low: Pitch, high: Pitch) -> Timeline<Chord> {
    if chords.is_constant() {
        return chords.clone();
    }
    let first_pass = chords.map_with_previous(|prev: Option<&Chord>, c| match prev {
        Some(p) => voicing::lead(p, c, low, high),
        None => *c,
    });
    let last = *first_pass.values().last().expect("non-empty");
    chords.map_with_previous(|prev: Option<&Chord>, c| {
        voicing::lead(prev.unwrap_or(&last), c, low, high)
    })
}

// ---- Drop and invert -----------------------------------------------------------

pub static DROP: NodeSpec = NodeSpec {
    kind: "drop",
    title: "Drop voicing",
    category: "Voicing",
    help: "Drop 2 moves the second-highest note down an octave; Drop 3 the third-highest.",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[ParamSpec {
        name: "type",
        label: "Type",
        kind: ParamKind::Choice {
            options: &["Drop 2", "Drop 3"],
            default: "Drop 2",
        },
    }],
};

pub fn build_drop(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let three = p.text("type") == "Drop 3";
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let f = if three {
            voicing::drop3
        } else {
            voicing::drop2
        };
        Ok(vec![Some(Value::Chord(chords.map(f)))])
    })
}

pub static INVERT: NodeSpec = NodeSpec {
    kind: "invert",
    title: "Invert",
    category: "Voicing",
    help: "Moves the lowest note up an octave per step (negative: the highest note down).",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[ParamSpec {
        name: "steps",
        label: "Steps",
        kind: ParamKind::Number {
            min: -4.0,
            max: 4.0,
            step: 1.0,
            default: 1.0,
        },
    }],
};

pub fn build_invert(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let steps = p.int("steps");
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(chords.map(|c| c.invert(steps))))])
    })
}

pub static SPREAD: NodeSpec = NodeSpec {
    kind: "spread",
    title: "Spread",
    category: "Voicing",
    help: "Keeps the bass and spreads the other notes evenly over a number of octaves above it.",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[ParamSpec {
        name: "octaves",
        label: "Octaves",
        kind: ParamKind::Number { min: 1.0, max: 4.0, step: 0.5, default: 2.0 },
    }],
};

pub fn build_spread(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let octaves = p.number("octaves");
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(chords.map(|c| {
            let span = (octaves * c.tuning().steps_per_octave() as f64).round() as i32;
            voicing::spread(c, span)
        })))])
    })
}

pub static CLOSE_OPEN: NodeSpec = NodeSpec {
    kind: "close_open",
    title: "Close / open",
    category: "Voicing",
    help: "Close packs every note into the octave above the bass. Open does the same, then lifts \
           every other note an octave (C E G becomes C G E).",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[ParamSpec {
        name: "position",
        label: "Position",
        kind: ParamKind::Choice { options: &["Close", "Open"], default: "Close" },
    }],
};

pub fn build_close_open(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let f: fn(&Chord) -> Chord = if p.text("position") == "Open" {
        voicing::open
    } else {
        voicing::close_over_bass
    };
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(chords.map(f)))])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_picks_root_first() {
        let c = Chord::from_symbol("G7", 4).unwrap();
        let v = voice_chord(
            &c,
            4,
            Pitch::parse("C3").unwrap(),
            Pitch::parse("C5").unwrap(),
        );
        assert_eq!(format!("{v:#}"), "[G3 B3 D4 F4]");
        let v5 = voice_chord(
            &c,
            5,
            Pitch::parse("C3").unwrap(),
            Pitch::parse("C5").unwrap(),
        );
        assert_eq!(format!("{v5:#}"), "[G3 B3 D4 F4 G4]");
        // Narrow range: notes below the root fill in, and the count can fall short.
        let tight = voice_chord(
            &c,
            4,
            Pitch::parse("C4").unwrap(),
            Pitch::parse("A4").unwrap(),
        );
        assert_eq!(format!("{tight:#}"), "[D4 F4 G4]");
    }

    fn names(t: &Timeline<Chord>) -> Vec<(Ticks, String)> {
        t.steps().iter().map(|(d, c)| (*d, c.name().map(|n| n.to_string()).unwrap_or_default())).collect()
    }

    #[test]
    fn secondary_dominants_lead_into_targets() {
        let key = Scale::parse("C major").unwrap();
        let prog = Progression::from_roman("I ii V I", &key, 960, 4).unwrap();
        let chords =
            Timeline::from_steps(prog.steps().iter().map(|s| (s.duration, s.chord)).collect()).unwrap();
        let out = secondary_dominants(&chords, &Timeline::constant(key), None, 2);
        // V7/ii (A7) before ii, V7/V (D7) before V; V → I and I → I are left alone.
        assert_eq!(
            names(&out),
            [(480, "C".into()), (480, "A7".into()), (480, "Dm".into()), (480, "D7".into()), (960, "G".into()), (960, "C".into())]
        );
        assert_eq!(out.length(), chords.length());
    }

    #[test]
    fn progression_follows_its_key() {
        let spec = crate::nodes::spec_for("progression").unwrap();
        let values: std::collections::HashMap<String, crate::spec::ParamValue> =
            [("chords".to_string(), crate::spec::ParamValue::Text("I V".into()))].into();
        let n = crate::nodes::build(spec, &values).unwrap();
        let ctx = EvalContext::new(0, 960, Meter::COMMON, 0);
        let bar = Meter::COMMON.ticks_per_bar();
        // Two bars of C major, then two of F major: the numerals follow.
        let keys = Timeline::from_steps(vec![(2 * bar, Scale::parse("C major").unwrap()), (2 * bar, Scale::parse("F major").unwrap())]).unwrap();
        let ins = Inputs { values: vec![Some(Value::Scale(keys))] };
        match &n.eval(&ins, &ctx).unwrap()[0] {
            Some(Value::Chord(t)) => assert_eq!(
                names(t).into_iter().map(|(_, n)| n).collect::<Vec<_>>(),
                ["C", "G", "F", "C"]
            ),
            other => panic!("{other:?}"),
        }
        // No key: a problem to fix, not silence.
        assert!(n.eval(&Inputs { values: vec![None] }, &ctx).is_err());
    }

    #[test]
    fn custom_scale_includes_the_root() {
        let s = custom_scale(Spelling::parse("D").unwrap(), "E F A, B").unwrap();
        assert_eq!(s.pc_set().iter().collect::<Vec<_>>(), [2, 4, 5, 9, 11]);
        assert!(custom_scale(Spelling::parse("C").unwrap(), "C H").is_err());
    }

    #[test]
    fn transposes_by_scale_steps_with_the_key() {
        let spec = crate::nodes::spec_for("transpose").unwrap();
        let mut values = std::collections::HashMap::new();
        values.insert("by".to_string(), crate::spec::ParamValue::Text("Scale steps".into()));
        values.insert("steps".to_string(), crate::spec::ParamValue::Number(1.0));
        let n = crate::nodes::build(spec, &values).unwrap();
        let chords = Timeline::constant(Chord::from_symbol("C", 4).unwrap());
        let key = Timeline::constant(Scale::parse("C major").unwrap());
        let ins = Inputs { values: vec![Some(Value::Chord(chords.clone())), Some(Value::Scale(key))] };
        let ctx = EvalContext::new(0, 960, Meter::COMMON, 0);
        match &n.eval(&ins, &ctx).unwrap()[0] {
            Some(Value::Chord(t)) => assert_eq!(names(t)[0].1, "Dm"),
            other => panic!("{other:?}"),
        }
        // Without a key, scale steps report a problem.
        let ins = Inputs { values: vec![Some(Value::Chord(chords)), None] };
        assert!(n.eval(&ins, &ctx).is_err());
    }

    #[test]
    fn lead_loops_smoothly() {
        let key = Scale::parse("C major").unwrap();
        let prog = Progression::from_roman("I vi IV V", &key, 960, 4).unwrap();
        let chords =
            Timeline::from_steps(prog.steps().iter().map(|s| (s.duration, s.chord)).collect())
                .unwrap();
        let low = Pitch::parse("C3").unwrap();
        let high = Pitch::parse("C5").unwrap();
        let voiced = chords.map(|c| voice_chord(c, 4, low, high));
        let led = lead_timeline(&voiced, low, high);
        let all: Vec<&Chord> = led.values().collect();
        for pair in all.windows(2).chain(std::iter::once(&[all[3], all[0]][..])) {
            let motion: i32 = pair[0]
                .notes()
                .iter()
                .zip(pair[1].notes())
                .map(|(a, b)| (a.step() - b.step()).abs())
                .sum();
            assert!(motion <= 7, "{:#} -> {:#}", pair[0], pair[1]);
        }
    }
}
