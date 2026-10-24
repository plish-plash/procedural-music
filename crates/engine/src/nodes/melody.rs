//! Note generators: arpeggios, sequences, random walks, chords, bass and drums.

use pm_theory::{Chord, Note, Onset, Pitch, Rhythm, Scale, Ticks};

use super::theory::node;
use super::{default_pulse, gate, onsets_between, scale_velocity, Inputs, NodeImpl};
use crate::graph::EvalContext;
use crate::rng;
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;

const GATE: ParamSpec = ParamSpec {
    name: "gate",
    label: "Gate %",
    kind: ParamKind::Number {
        min: 5.0,
        max: 200.0,
        step: 5.0,
        default: 90.0,
    },
};
const VELOCITY: ParamSpec = ParamSpec {
    name: "velocity",
    label: "Velocity %",
    kind: ParamKind::Number {
        min: 0.0,
        max: 150.0,
        step: 5.0,
        default: 100.0,
    },
};

/// Calls `f(time, onset)` for every onset of the rhythm input (or a 1/16 pulse) in the window.
fn each_onset(
    ins: &Inputs,
    rhythm_port: usize,
    ctx: &EvalContext,
    mut f: impl FnMut(Ticks, &Onset, &Rhythm),
) {
    let pulse;
    let rhythm = match ins.rhythm(rhythm_port) {
        Some(r) => r,
        None => {
            pulse = default_pulse();
            &pulse
        }
    };
    rhythm.for_each_in(ctx.start, ctx.end, |t, o| f(t, o, rhythm));
}

fn notes(v: Vec<Note>) -> Result<Vec<Option<Value>>, String> {
    Ok(vec![Some(Value::Notes(v))])
}

// ---- Arpeggiator -------------------------------------------------------------

pub static ARP: NodeSpec = NodeSpec {
    kind: "arp",
    title: "Arpeggiator",
    category: "Melody",
    help: "Plays the notes of a scale (or a chord's tones) one per rhythm hit. \
           The pattern restarts whenever the scale or chord changes. With no rhythm, it plays sixteenths.",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "pattern",
            label: "Pattern",
            kind: ParamKind::Choice { options: &["Up", "Down", "Up-down", "Random"], default: "Up" },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "range", label: "Octaves", kind: ParamKind::Number { min: 1.0, max: 4.0, step: 1.0, default: 1.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_arp(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let pattern = p.text("pattern");
    let octave = p.int("octave");
    let range = p.int("range").max(1) as usize;
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, rhythm| {
            let seg = scales.at(t);
            let scale = seg.value;
            let total = scale.len() * range + 1; // include the top root
            if scale.is_empty() {
                return;
            }
            let k = onsets_between(rhythm, seg.start, t) as usize;
            let i = match pattern.as_str() {
                "Down" => total - 1 - k % total,
                "Up-down" if total > 1 => {
                    let period = 2 * total - 2;
                    let i = k % period;
                    if i < total {
                        i
                    } else {
                        period - i
                    }
                }
                "Random" => rng::below(rng::hash(&[ctx.salt, t as u64]), total as u64) as usize,
                _ => k % total,
            };
            out.push(Note {
                start: t,
                duration: gate(onset.duration, gate_pct),
                pitch: scale.pitch(i as i32, octave),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

// ---- Degree sequencer ----------------------------------------------------------

pub static SEQUENCER: NodeSpec = NodeSpec {
    kind: "sequencer",
    title: "Degree sequencer",
    category: "Melody",
    help: "Steps through a list of scale degrees, one per rhythm hit (1 is the root; 8 the root an octave up; \
           negative numbers go below). '-' is a rest.",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "degrees", label: "Degrees", kind: ParamKind::Text { default: "1 3 5 3 6 5 4 2" } },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_sequencer(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let text = p.text("degrees");
    let mut degrees: Vec<Option<i32>> = Vec::new();
    for token in text.split_whitespace() {
        if token == "-" {
            degrees.push(None);
            continue;
        }
        let d: i32 = token
            .parse()
            .map_err(|_| format!("'{token}' is not a degree number"))?;
        if d == 0 {
            return Err("degrees count from 1; there is no degree 0".into());
        }
        // 1-based to 0-based; -1 is the degree just below the root.
        degrees.push(Some(if d > 0 { d - 1 } else { d }));
    }
    if degrees.is_empty() {
        return Err("enter some degrees, like 1 3 5".into());
    }
    let octave = p.int("octave");
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, rhythm| {
            let k = onsets_between(rhythm, 0, t) as usize;
            if let Some(d) = degrees[k % degrees.len()] {
                out.push(Note {
                    start: t,
                    duration: gate(onset.duration, gate_pct),
                    pitch: scales.at(t).value.pitch(d, octave),
                    velocity: scale_velocity(onset.velocity, vel),
                    voice: 0,
                });
            }
        });
        notes(out)
    })
}

// ---- Random walk ---------------------------------------------------------------

pub static RANDOM_WALK: NodeSpec = NodeSpec {
    kind: "random_walk",
    title: "Random walk",
    category: "Melody",
    help: "A melody that wanders through the scale by small random steps, restarting from the root every phrase. \
           Fresh each time unless Lock is on; locked phrases repeat exactly (change Seed for another one).",
    inputs: &[port("scale", PortType::Scale), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "max_step", label: "Max step", kind: ParamKind::Number { min: 1.0, max: 5.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 8.0, step: 1.0, default: 4.0 } },
        ParamSpec { name: "range", label: "Octaves", kind: ParamKind::Number { min: 1.0, max: 3.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "phrase", label: "Phrase bars", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 2.0 } },
        ParamSpec { name: "lock", label: "Lock", kind: ParamKind::Bool { default: false } },
        ParamSpec { name: "seed", label: "Seed", kind: ParamKind::Number { min: 0.0, max: 9999.0, step: 1.0, default: 1.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_random_walk(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let max_step = p.int("max_step").max(1) as u64;
    let octave = p.int("octave");
    let range = p.int("range").max(1);
    let phrase_bars = p.int("phrase").max(1) as u32;
    let lock = p.flag("lock");
    let seed = p.number("seed") as u64;
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(scales) = ins.scale(0) else {
            return Ok(vec![None]);
        };
        let phrase = ctx.meter.ticks_per_bar() * phrase_bars;
        let mut out = Vec::new();
        each_onset(ins, 1, ctx, |t, onset, rhythm| {
            let scale: &Scale = scales.at(t).value;
            let n = scale.len() as i32;
            let (lo, hi) = (-(range * n) / 2, (range * n) / 2);
            let origin = t - t % phrase;
            let phrase_seed = if lock {
                rng::hash(&[seed])
            } else {
                rng::hash(&[seed, ctx.salt, (t / phrase) as u64])
            };
            let k = onsets_between(rhythm, origin, t);
            let mut pos = 0i32;
            for i in 1..=k {
                let step = rng::below(rng::hash(&[phrase_seed, i]), 2 * max_step + 1) as i32
                    - max_step as i32;
                pos += step;
                // Reflect off the edges of the range.
                if pos > hi {
                    pos = 2 * hi - pos;
                }
                if pos < lo {
                    pos = 2 * lo - pos;
                }
                pos = pos.clamp(lo, hi);
            }
            out.push(Note {
                start: t,
                duration: gate(onset.duration, gate_pct),
                pitch: scale.pitch(pos, octave),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

// ---- Play chords -----------------------------------------------------------------

pub static PLAY_CHORDS: NodeSpec = NodeSpec {
    kind: "play_chords",
    title: "Play chords",
    category: "Parts",
    help: "Plays each chord's notes together, on every rhythm hit. With no rhythm, each chord is held for its length \
           (a single chord is struck once per bar). Each note's voice number is its position in the chord, low to high.",
    inputs: &[port("chords", PortType::Chord), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[GATE, VELOCITY],
};

pub fn build_play_chords(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        let mut strike = |t: Ticks, duration: Ticks, velocity: u8, chord: &Chord| {
            for (v, &pitch) in chord.notes().iter().enumerate() {
                out.push(Note {
                    start: t,
                    duration,
                    pitch,
                    velocity,
                    voice: v as u8,
                });
            }
        };
        if ins.rhythm(1).is_some() {
            each_onset(ins, 1, ctx, |t, onset, _| {
                strike(
                    t,
                    gate(onset.duration, gate_pct),
                    scale_velocity(onset.velocity, vel),
                    chords.at(t).value,
                )
            });
        } else {
            for seg in chords.starts_in(ctx.start, ctx.end, ctx.meter.ticks_per_bar()) {
                strike(
                    seg.start,
                    gate(seg.end - seg.start, gate_pct),
                    scale_velocity(Onset::NORMAL, vel),
                    seg.value,
                );
            }
        }
        notes(out)
    })
}

// ---- Chord root (bass) -----------------------------------------------------------

pub static CHORD_ROOT: NodeSpec = NodeSpec {
    kind: "chord_root",
    title: "Chord root",
    category: "Parts",
    help: "A bassline: plays one tone of each chord (its root, fifth or lowest note) in a fixed octave, \
           on every rhythm hit, or held for each chord with no rhythm.",
    inputs: &[port("chords", PortType::Chord), optional("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec {
            name: "tone",
            label: "Tone",
            kind: ParamKind::Choice { options: &["Root", "Fifth", "Lowest note"], default: "Root" },
        },
        ParamSpec { name: "octave", label: "Octave", kind: ParamKind::Number { min: 0.0, max: 6.0, step: 1.0, default: 2.0 } },
        GATE,
        VELOCITY,
    ],
};

pub fn build_chord_root(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let tone = p.text("tone");
    let octave = p.int("octave");
    let gate_pct = p.number("gate");
    let vel = p.number("velocity");
    let pick = move |chord: &Chord| -> Option<Pitch> {
        let root = chord
            .root()
            .or_else(|| chord.bass().map(|b| b.pitch_class()))?;
        let pc = match tone.as_str() {
            "Lowest note" => chord.bass()?.pitch_class() as i32,
            "Fifth" => {
                let tuning = chord.tuning();
                // The chord tone closest to a perfect fifth above the root (works for dim/aug too).
                let fifth = (tuning.steps_per_octave() * 7 + 6) / 12;
                (0..tuning.steps_per_octave())
                    .map(|d| root as i32 + fifth + if d % 2 == 0 { d / 2 } else { -(d / 2 + 1) })
                    .find(|&pc| chord.contains_pc(pc))
                    .unwrap_or(root as i32)
            }
            _ => root as i32,
        };
        let pc = pc.rem_euclid(chord.tuning().steps_per_octave()) as u8;
        Some(Pitch::from_octave(chord.tuning(), octave, pc))
    };
    node(move |ins, ctx| {
        let Some(chords) = ins.chord(0) else {
            return Ok(vec![None]);
        };
        let mut out = Vec::new();
        if ins.rhythm(1).is_some() {
            each_onset(ins, 1, ctx, |t, onset, _| {
                if let Some(pitch) = pick(chords.at(t).value) {
                    out.push(Note {
                        start: t,
                        duration: gate(onset.duration, gate_pct),
                        pitch,
                        velocity: scale_velocity(onset.velocity, vel),
                        voice: 0,
                    });
                }
            });
        } else {
            for seg in chords.starts_in(ctx.start, ctx.end, ctx.meter.ticks_per_bar()) {
                if let Some(pitch) = pick(seg.value) {
                    out.push(Note {
                        start: seg.start,
                        duration: gate(seg.end - seg.start, gate_pct),
                        pitch,
                        velocity: scale_velocity(Onset::NORMAL, vel),
                        voice: 0,
                    });
                }
            }
        }
        notes(out)
    })
}

// ---- Drum hit --------------------------------------------------------------------

const DRUMS: &[(&str, i32)] = &[
    ("Kick", 36),
    ("Rim", 37),
    ("Snare", 38),
    ("Clap", 39),
    ("Closed hat", 42),
    ("Pedal hat", 44),
    ("Open hat", 46),
    ("Low tom", 45),
    ("Mid tom", 47),
    ("High tom", 50),
    ("Crash", 49),
    ("Ride", 51),
    ("Cowbell", 56),
    ("Shaker", 70),
];
const DRUM_NAMES: &[&str] = &[
    "Kick",
    "Rim",
    "Snare",
    "Clap",
    "Closed hat",
    "Pedal hat",
    "Open hat",
    "Low tom",
    "Mid tom",
    "High tom",
    "Crash",
    "Ride",
    "Cowbell",
    "Shaker",
];

pub static DRUM: NodeSpec = NodeSpec {
    kind: "drum",
    title: "Drum hit",
    category: "Parts",
    help: "Plays one General MIDI drum sound on every rhythm hit. Send it to MIDI channel 10 for GM drums.",
    inputs: &[port("rhythm", PortType::Rhythm)],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "sound", label: "Sound", kind: ParamKind::Choice { options: DRUM_NAMES, default: "Kick" } },
        ParamSpec { name: "note", label: "Note override", kind: ParamKind::Number { min: 0.0, max: 127.0, step: 1.0, default: 0.0 } },
        VELOCITY,
    ],
};

pub fn build_drum(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let sound = p.text("sound");
    let key = match p.int("note") {
        0 => DRUMS.iter().find(|d| d.0 == sound).map_or(36, |d| d.1),
        n => n,
    };
    let vel = p.number("velocity");
    node(move |ins, ctx| {
        if ins.rhythm(0).is_none() {
            return Ok(vec![None]);
        }
        let mut out = Vec::new();
        each_onset(ins, 0, ctx, |t, onset, _| {
            out.push(Note {
                start: t,
                duration: onset.duration.clamp(1, pm_theory::PPQ / 4),
                pitch: Pitch::midi(key),
                velocity: scale_velocity(onset.velocity, vel),
                voice: 0,
            });
        });
        notes(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::build;
    use crate::nodes::spec_for;
    use crate::spec::ParamValue;
    use crate::timeline::Timeline;
    use pm_theory::{Meter, PPQ};
    use std::collections::HashMap;

    fn ctx(start: Ticks, end: Ticks, salt: u64) -> EvalContext {
        EvalContext {
            start,
            end,
            meter: Meter::COMMON,
            salt,
        }
    }

    fn run(
        kind: &str,
        params: &[(&str, ParamValue)],
        inputs: Vec<Option<Value>>,
        c: &EvalContext,
    ) -> Vec<Note> {
        let values: HashMap<String, ParamValue> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let n = build(spec_for(kind).unwrap(), &values).unwrap();
        match n.eval(&Inputs { values: inputs }, c).unwrap().remove(0) {
            Some(Value::Notes(v)) => v,
            other => panic!("expected notes, got {other:?}"),
        }
    }

    fn names(notes: &[Note]) -> Vec<String> {
        notes.iter().map(|n| n.pitch.to_string()).collect()
    }

    fn c_major() -> Option<Value> {
        Some(Value::Scale(Timeline::constant(
            Scale::parse("C major").unwrap(),
        )))
    }

    #[test]
    fn arp_up_down_over_a_bar() {
        let v = run(
            "arp",
            &[("pattern", ParamValue::Text("Up-down".into()))],
            vec![
                Value::Chord(Timeline::constant(Chord::from_symbol("C", 4).unwrap()))
                    .convert(PortType::Scale),
                None,
            ],
            &ctx(0, PPQ * 2, 0),
        );
        assert_eq!(names(&v), ["C4", "E4", "G4", "C5", "G4", "E4", "C4", "E4"]);
        assert_eq!(v[1].start, PPQ / 4);
    }

    #[test]
    fn sequencer_degrees_and_rests() {
        let v = run(
            "sequencer",
            &[("degrees", ParamValue::Text("1 - 8 -1".into()))],
            vec![c_major(), None],
            &ctx(0, PPQ, 0),
        );
        assert_eq!(names(&v), ["C4", "C5", "B3"]);
        assert!(build(
            spec_for("sequencer").unwrap(),
            &HashMap::from([("degrees".into(), ParamValue::Text("1 0".into()))])
        )
        .is_err());
    }

    #[test]
    fn random_walk_lock_repeats_and_unlocked_varies() {
        let phrase = Meter::COMMON.ticks_per_bar() * 2;
        let locked = [("lock", ParamValue::Bool(true))];
        let a = run(
            "random_walk",
            &locked,
            vec![c_major(), None],
            &ctx(0, phrase, 1),
        );
        let b = run(
            "random_walk",
            &locked,
            vec![c_major(), None],
            &ctx(phrase, 2 * phrase, 2),
        );
        assert_eq!(names(&a), names(&b));
        let c = run(
            "random_walk",
            &[],
            vec![c_major(), None],
            &ctx(0, phrase, 1),
        );
        let d = run(
            "random_walk",
            &[],
            vec![c_major(), None],
            &ctx(0, phrase, 2),
        );
        assert_ne!(names(&c), names(&d));
        // Steps stay within the max step (2 degrees) and the first note is the root.
        assert_eq!(c[0].pitch.to_string(), "C4");
        let major = Scale::parse("C major").unwrap();
        for w in c.windows(2) {
            let (da, _) = major.degree_of(w[0].pitch).unwrap();
            let oa = major.degree_of(w[0].pitch).unwrap().1;
            let (db, ob) = major.degree_of(w[1].pitch).unwrap();
            let pos = |d: usize, o: i32| o * 7 + d as i32;
            assert!((pos(db, ob) - pos(da, oa)).abs() <= 2);
        }
    }

    #[test]
    fn held_chords_and_bass() {
        let prog = Timeline::from_steps(vec![
            (PPQ * 4, Chord::from_symbol("Am", 4).unwrap()),
            (PPQ * 4, Chord::from_symbol("F", 4).unwrap()),
        ])
        .unwrap();
        let c = ctx(0, PPQ * 8, 0);
        let pads = run(
            "play_chords",
            &[],
            vec![Some(Value::Chord(prog.clone())), None],
            &c,
        );
        assert_eq!(pads.len(), 6);
        assert_eq!(pads[3].start, PPQ * 4);
        let bass = run(
            "chord_root",
            &[],
            vec![Some(Value::Chord(prog.clone())), None],
            &c,
        );
        assert_eq!(names(&bass), ["A2", "F2"]);
        let fifths = run(
            "chord_root",
            &[("tone", ParamValue::Text("Fifth".into()))],
            vec![Some(Value::Chord(prog)), None],
            &c,
        );
        assert_eq!(names(&fifths), ["E2", "C2"]);
    }

    #[test]
    fn drums_use_gm_keys() {
        let r = Some(Value::Rhythm(Box::new(
            Rhythm::from_grid("x...x...", PPQ / 4).unwrap(),
        )));
        let v = run(
            "drum",
            &[("sound", ParamValue::Text("Snare".into()))],
            vec![r],
            &ctx(0, PPQ * 4, 0),
        );
        assert_eq!(v.len(), 4, "a half-bar pattern loops twice per bar");
        assert!(v.iter().all(|n| n.pitch.step() == 38));
    }
}
