//! Theory sources and transforms: scales, chords, progressions, voicings.

use pm_theory::{voicing, Chord, Interval, Meter, Pitch, Progression, Scale, ScaleKind, Spelling};

use super::{Inputs, NodeImpl};
use crate::graph::EvalContext;
use crate::spec::{port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
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
    help: "A root and a named scale. The notes repeat in every octave.",
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
    ],
};

pub fn build_scale(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let root = Spelling::parse(&p.text("root")).ok_or("bad root")?;
    let name = p.text("scale");
    let kind = ScaleKind::all()
        .find(|k| k.name() == name)
        .ok_or_else(|| format!("unknown scale '{name}'"))?;
    let scale = Scale::preset(kind, root);
    node(move |_, _| Ok(vec![Some(Value::Scale(Timeline::constant(scale)))]))
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
    help: "Chords over time, as Roman numerals in a key (ii7 V7 Imaj7) or chord symbols (Dm7 G7 Cmaj7). \
           '-' holds the previous chord one more step; '|' is ignored.",
    inputs: &[],
    outputs: &[port("chords", PortType::Chord), port("key", PortType::Scale)],
    params: &[
        ParamSpec { name: "chords", label: "Chords", kind: ParamKind::Text { default: "I vi IV V" } },
        ParamSpec {
            name: "notation",
            label: "Notation",
            kind: ParamKind::Choice { options: &["Roman numerals", "Chord symbols"], default: "Roman numerals" },
        },
        ParamSpec { name: "key", label: "Key", kind: ParamKind::Text { default: "C major" } },
        ParamSpec {
            name: "bars",
            label: "Bars each",
            kind: ParamKind::Number { min: 0.25, max: 8.0, step: 0.25, default: 1.0 },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 1.0, max: 7.0, step: 1.0, default: 4.0 } },
    ],
};

pub fn build_progression(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let text = p.text("chords");
    let key_text = p.text("key");
    let roman = p.text("notation") == "Roman numerals";
    let octave = p.int("octave");
    let bar = Meter::COMMON.ticks_per_bar();
    let duration = ((p.number("bars") * bar as f64).round() as u32).max(1);
    let key = if key_text.trim().is_empty() {
        None
    } else {
        Some(
            Scale::parse(&key_text)
                .ok_or_else(|| format!("unknown key '{key_text}' (try 'C major' or 'A minor')"))?,
        )
    };
    let prog = if roman {
        let key = key.ok_or("Roman numerals need a key")?;
        Progression::from_roman(&text, &key, duration, octave).ok_or_else(|| {
            bad_token(&text, |t| pm_theory::parse_roman(t, &key, octave).is_some())
        })?
    } else {
        Progression::from_symbols(&text, duration, octave)
            .ok_or_else(|| bad_token(&text, |t| Chord::from_symbol(t, octave).is_some()))?
    };
    if prog.is_empty() {
        return Err("no chords".into());
    }
    let chords = Timeline::from_steps(prog.steps().iter().map(|s| (s.duration, s.chord)).collect())
        .expect("non-empty");
    let scales = Timeline::from_steps(
        prog.steps()
            .iter()
            .map(|s| {
                let scale = s
                    .scale
                    .or(key)
                    .or_else(|| s.chord.to_scale())
                    .unwrap_or_else(|| Scale::parse("C").unwrap());
                (s.duration, scale)
            })
            .collect(),
    )
    .expect("non-empty");
    node(move |_, _| {
        Ok(vec![
            Some(Value::Chord(chords.clone())),
            Some(Value::Scale(scales.clone())),
        ])
    })
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
    help: "Moves every chord by a number of semitones.",
    inputs: &[port("chords", PortType::Chord)],
    outputs: &[port("chords", PortType::Chord)],
    params: &[ParamSpec {
        name: "semitones",
        label: "Semitones",
        kind: ParamKind::Number {
            min: -24.0,
            max: 24.0,
            step: 1.0,
            default: 0.0,
        },
    }],
};

pub fn build_transpose(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let n = p.int("semitones");
    node(move |ins, _| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        Ok(vec![Some(Value::Chord(
            chords.map(|c| c.transpose(Interval::new(c.tuning(), n))),
        ))])
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
