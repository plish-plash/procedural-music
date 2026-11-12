//! MIDI input: notes recorded into the project, and live notes and controllers (in
//! Perform mode).

use pm_theory::{Chord, Note, Pitch, Ticks, PPQ};

use super::{step_ticks, Inputs, NodeImpl};
use crate::control::Control;
use crate::graph::EvalContext;
use crate::spec::{port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::timeline::Timeline;
use crate::value::Value;

const CHANNELS: &[&str] = &[
    "Omni", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];

const QUANTIZE: &[&str] = &["Off", "1/4", "1/8", "1/16", "1/32", "1/8T", "1/16T"];

const CHANNEL: ParamSpec = ParamSpec {
    name: "channel",
    label: "Channel",
    kind: ParamKind::Choice { options: CHANNELS, default: "Omni" },
};

const QUANTIZE_PARAM: ParamSpec = ParamSpec {
    name: "quantize",
    label: "Quantize",
    kind: ParamKind::Choice { options: QUANTIZE, default: "Off" },
};

/// Zero-based channel, or `None` for any.
fn channel(p: &Params) -> Option<u8> {
    p.text("channel").parse::<u8>().ok().map(|c| c.clamp(1, 16) - 1)
}

fn grid(p: &Params) -> Option<Ticks> {
    let q = p.text("quantize");
    (q != "Off").then(|| step_ticks(&q))
}

/// The shortest note a release can make.
const MIN_LENGTH: Ticks = PPQ / 16;

/// What live nodes say outside Perform mode.
const PERFORM_ONLY: &str = "plays the MIDI input in Perform mode";

// ---- Recorded notes ----------------------------------------------------------

pub static RECORDED: NodeSpec = NodeSpec {
    kind: "recorded",
    title: "Recorded notes",
    category: "Input",
    help: "Notes played on the MIDI input, looping every Bars. Press record and play: recording \
           starts where the loop does and lasts one pass. Quantize snaps them to a grid as they play.",
    inputs: &[],
    outputs: &[port("notes", PortType::Notes)],
    params: &[
        ParamSpec { name: "notes", label: "Notes", kind: ParamKind::Data },
        ParamSpec { name: "bars", label: "Bars", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 2.0 } },
        QUANTIZE_PARAM,
        ParamSpec { name: "transpose", label: "Transpose", kind: ParamKind::Number { min: -24.0, max: 24.0, step: 1.0, default: 0.0 } },
    ],
};

/// A recorded note: (start in the loop, duration, MIDI key, velocity).
pub type Recorded = (Ticks, Ticks, u8, u8);

/// Recorded notes as saved: "start:duration:key:velocity" for each, separated by spaces.
pub fn encode_recorded(notes: &[Recorded]) -> String {
    let parts: Vec<String> = notes.iter().map(|(s, d, k, v)| format!("{s}:{d}:{k}:{v}")).collect();
    parts.join(" ")
}

pub fn decode_recorded(text: &str) -> Result<Vec<Recorded>, String> {
    text.split_whitespace()
        .map(|item| {
            let f: Vec<&str> = item.split(':').collect();
            let num = |i: usize| f.get(i).and_then(|s| s.parse::<u32>().ok());
            match (num(0), num(1), num(2), num(3)) {
                (Some(s), Some(d), Some(k), Some(v)) if f.len() == 4 => {
                    Ok((s, d.max(1), k.min(127) as u8, v.clamp(1, 127) as u8))
                }
                _ => Err(format!("'{item}' is not a recorded note")),
            }
        })
        .collect()
}

pub fn build_recorded(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let notes = decode_recorded(&p.text("notes"))?;
    let bars = p.int("bars").max(1) as u32;
    let grid = grid(p);
    let transpose = p.int("transpose");
    super::theory::node(move |_, ctx| {
        let length = ctx.meter.ticks_per_bar() * bars;
        let mut out = Vec::new();
        for &(start, duration, key, velocity) in &notes {
            if start >= length {
                continue;
            }
            let at = match grid {
                Some(g) => ((start + g / 2) / g * g) % length,
                None => start,
            };
            let key = key as i32 + transpose;
            if !(0..=127).contains(&key) {
                continue;
            }
            let first = ctx.start.saturating_sub(at).div_ceil(length);
            let mut t = first * length + at;
            while t < ctx.end {
                if t >= ctx.start {
                    out.push(Note { start: t, duration, pitch: Pitch::midi(key), velocity, voice: 0, held: 0 });
                }
                t += length;
            }
        }
        out.sort_by_key(|n| (n.start, n.pitch));
        Ok(vec![Some(Value::Notes(out))])
    })
}

// ---- MIDI in -----------------------------------------------------------------

pub static MIDI_IN: NodeSpec = NodeSpec {
    kind: "midi_in",
    title: "MIDI in",
    category: "Input",
    help: "Notes played on the MIDI input, in Perform mode. They sound a little late (Live lookahead \
           in the MIDI menu sets how much), or on the next Quantize step. Chord is the keys held \
           now; with Latch, the last chord played. Nothing exports: record a part to keep it.",
    inputs: &[],
    outputs: &[port("notes", PortType::Notes), port("chord", PortType::Chord)],
    params: &[
        CHANNEL,
        QUANTIZE_PARAM,
        ParamSpec { name: "transpose", label: "Transpose", kind: ParamKind::Number { min: -24.0, max: 24.0, step: 1.0, default: 0.0 } },
        ParamSpec { name: "latch", label: "Latch", kind: ParamKind::Bool { default: false } },
    ],
};

struct MidiIn {
    channel: Option<u8>,
    grid: Option<Ticks>,
    transpose: i32,
    latch: bool,
}

impl NodeImpl for MidiIn {
    fn eval(&self, _: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        let live = ctx.live.ok_or(PERFORM_ONLY)?;
        let key = |k: u8| {
            let k = k as i32 + self.transpose;
            (0..=127).contains(&k).then(|| Pitch::midi(k))
        };
        let mut out = Vec::new();
        for n in live.input.notes() {
            if self.channel.is_some_and(|c| c != n.channel) {
                continue;
            }
            let Some(pitch) = key(n.key) else { continue };
            let Ok(on) = Ticks::try_from(n.on as i64 - live.offset) else { continue };
            let start = self.grid.map_or(on, |g| on.div_ceil(g) * g);
            if start < ctx.start || start >= ctx.end {
                continue;
            }
            let (duration, held) = match n.off {
                Some(off) => {
                    let off = (off as i64 - live.offset).max(0) as Ticks;
                    (off.saturating_sub(start).max(MIN_LENGTH), 0)
                }
                // Still held: it ends on release.
                None => (ctx.meter.ticks_per_bar(), n.tag),
            };
            out.push(Note { start, duration, pitch, velocity: n.velocity, voice: 0, held });
        }
        out.sort_by_key(|n| (n.start, n.pitch));
        let pitches: Vec<Pitch> = live
            .input
            .held_keys(self.channel, self.latch)
            .into_iter()
            .filter_map(key)
            .collect();
        let chord = (!pitches.is_empty())
            .then(|| Chord::from_pitches(&pitches).ok())
            .flatten()
            .map(|c| Value::Chord(Timeline::constant(c)));
        Ok(vec![Some(Value::Notes(out)), chord])
    }
}

pub fn build_midi_in(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    Ok(Box::new(MidiIn {
        channel: channel(p),
        grid: grid(p),
        transpose: p.int("transpose"),
        latch: p.flag("latch"),
    }))
}

// ---- MIDI CC in --------------------------------------------------------------

pub static MIDI_IN_CC: NodeSpec = NodeSpec {
    kind: "midi_in_cc",
    title: "MIDI CC in",
    category: "Input",
    help: "A knob, fader or wheel on the MIDI input (a control change number), in Perform mode, \
           as a value between Min and Max.",
    inputs: &[],
    outputs: &[port("control", PortType::Control)],
    params: &[
        CHANNEL,
        ParamSpec { name: "cc", label: "CC number", kind: ParamKind::Number { min: 0.0, max: 127.0, step: 1.0, default: 1.0 } },
        ParamSpec { name: "min", label: "Min", kind: ParamKind::Number { min: -20_000.0, max: 20_000.0, step: 0.01, default: 0.0 } },
        ParamSpec { name: "max", label: "Max", kind: ParamKind::Number { min: -20_000.0, max: 20_000.0, step: 0.01, default: 127.0 } },
    ],
};

pub fn build_midi_in_cc(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let channel = channel(p);
    let cc = p.int("cc").clamp(0, 127) as u8;
    let (min, max) = (p.number("min"), p.number("max"));
    super::theory::node(move |_, ctx| {
        let live = ctx.live.ok_or(PERFORM_ONLY)?;
        let v = live.input.cc(channel, cc).unwrap_or(0) as f64 / 127.0;
        Ok(vec![Some(Value::Control(Control::Constant(min + (max - min) * v)))])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{LiveCtx, LiveInput};
    use crate::spec::ParamValue;
    use pm_theory::Meter;
    use std::collections::HashMap;

    fn build(spec: &'static NodeSpec, params: &[(&str, ParamValue)]) -> Box<dyn NodeImpl> {
        let values: HashMap<String, ParamValue> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        crate::nodes::build(spec, &values).unwrap()
    }

    fn text(s: &str) -> ParamValue {
        ParamValue::Text(s.into())
    }

    fn eval(n: &dyn NodeImpl, ctx: &EvalContext) -> Vec<Option<Value>> {
        n.eval(&Inputs { values: vec![] }, ctx).unwrap()
    }

    #[test]
    fn recorded_notes_loop_and_quantize() {
        let notes = encode_recorded(&[(10, 200, 60, 100), (1000, 100, 64, 80)]);
        assert_eq!(decode_recorded(&notes).unwrap()[1], (1000, 100, 64, 80));
        assert!(decode_recorded("1:2:3").is_err());
        let n = build(&RECORDED, &[("notes", text(&notes)), ("bars", ParamValue::Number(1.0))]);
        let bar = Meter::COMMON.ticks_per_bar();
        let Some(Value::Notes(out)) = eval(n.as_ref(), &EvalContext::new(0, 2 * bar, Meter::COMMON, 0)).remove(0) else {
            panic!()
        };
        let starts: Vec<Ticks> = out.iter().map(|n| n.start).collect();
        assert_eq!(starts, [10, 1000, bar + 10, bar + 1000]);
        let q = build(&RECORDED, &[("notes", text(&notes)), ("quantize", text("1/4"))]);
        let Some(Value::Notes(out)) = eval(q.as_ref(), &EvalContext::new(0, bar, Meter::COMMON, 0)).remove(0) else {
            panic!()
        };
        let starts: Vec<Ticks> = out.iter().map(|n| n.start).collect();
        assert_eq!(starts, [0, 960]);
    }

    #[test]
    fn live_nodes_need_perform_mode() {
        let n = build(&MIDI_IN, &[]);
        assert!(n.eval(&Inputs { values: vec![] }, &EvalContext::new(0, 240, Meter::COMMON, 0)).is_err());
    }

    #[test]
    fn midi_in_places_notes_and_chords() {
        let mut live = LiveInput::default();
        let tag = live.place(0, 60, 100, 1000);
        live.place(0, 64, 90, 1010);
        live.place_off(0, 64, 1500);
        live.press(0, 60, 100);
        live.press(0, 67, 100);
        let ctx = |start, end| EvalContext {
            live: Some(LiveCtx { input: &live, offset: 500 }),
            ..EvalContext::new(start, end, Meter::COMMON, 0)
        };
        let n = build(&MIDI_IN, &[("transpose", ParamValue::Number(12.0))]);
        let out = eval(n.as_ref(), &ctx(480, 720));
        let Some(Value::Notes(notes)) = &out[0] else { panic!() };
        // Play ticks 1000 and 1010 are local 500 and 510.
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].start, notes[0].held, notes[0].pitch), (500, tag, Pitch::midi(72)));
        assert_eq!((notes[1].start, notes[1].duration, notes[1].held), (510, 490, 0));
        let Some(Value::Chord(chord)) = &out[1] else { panic!() };
        assert_eq!(chord.at(0).value.notes(), [Pitch::midi(72), Pitch::midi(79)]);
        // Quantized to the next eighth: local 960, in a later window.
        let q = build(&MIDI_IN, &[("quantize", text("1/8"))]);
        let Some(Value::Notes(notes)) = &eval(q.as_ref(), &ctx(960, 1200))[0] else { panic!() };
        assert_eq!(notes.iter().map(|n| n.start).collect::<Vec<_>>(), [960, 960]);
    }

    #[test]
    fn cc_in_scales_the_value() {
        let mut live = LiveInput::default();
        live.set_cc(3, 74, 127);
        let n = build(&MIDI_IN_CC, &[("cc", ParamValue::Number(74.0)), ("min", ParamValue::Number(200.0)), ("max", ParamValue::Number(800.0))]);
        let ctx = EvalContext { live: Some(LiveCtx { input: &live, offset: 0 }), ..EvalContext::new(0, 240, Meter::COMMON, 0) };
        let Some(Value::Control(c)) = &eval(n.as_ref(), &ctx)[0] else { panic!() };
        assert_eq!(c.at(0), 800.0);
    }
}
