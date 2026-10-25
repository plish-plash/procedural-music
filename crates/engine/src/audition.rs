//! Auditioning: playing a node's output once through a preview instrument, without
//! wiring it to an output. Every rack the engine sends carries the preview instruments,
//! so an audition sounds whatever the patch is.

use pm_synth::drums::DrumParams;
use pm_synth::dsp::Wave;
use pm_synth::synth::SynthParams;
use pm_synth::{ModuleDesc, ModuleKind, RackDesc};
use pm_theory::{Meter, Note, Pitch, Ticks, PPQ};

use crate::value::Value;

/// Rack id of the preview synth (fixed, so it keeps sounding across patch edits).
pub const SYNTH: u64 = 0x6175_6469_7469_6f01;
/// Rack id of the preview drum kit, which plays rhythms.
pub const DRUMS: u64 = 0x6175_6469_7469_6f02;
const OUT: u64 = 0x6175_6469_7469_6f03;

/// The longest audition, in bars.
const MAX_BARS: u32 = 8;
/// The drum kit's rim click, which plays rhythms.
const RIM: i32 = 37;

/// A rack with the preview instruments added.
pub fn with_preview(mut rack: RackDesc) -> RackDesc {
    let synth = SynthParams {
        wave: Wave::Triangle,
        detune: 0.0,
        cutoff: 4000.0,
        env_amount: 1.0,
        decay: 0.6,
        sustain: 0.5,
        release: 0.25,
        level: 0.35,
        ..SynthParams::default()
    };
    rack.modules.extend([
        ModuleDesc {
            id: SYNTH,
            kind: ModuleKind::Poly(synth),
            inputs: vec![],
        },
        ModuleDesc {
            id: DRUMS,
            kind: ModuleKind::Drums(DrumParams::default()),
            inputs: vec![],
        },
        ModuleDesc {
            id: OUT,
            kind: ModuleKind::Output { level: 0.8 },
            inputs: vec![Some(SYNTH), Some(DRUMS)],
        },
    ]);
    rack
}

/// What an audition of `value` plays, from tick 0: (preview module, note) pairs. A scale
/// is a run up from its root to the octave, a chord a block (a progression plays its
/// chords for their lengths), a rhythm one loop of rim clicks (at least a bar).
pub fn notes(value: &Value, meter: Meter) -> Result<Vec<(u64, Note)>, String> {
    let bar = meter.ticks_per_bar();
    let limit = MAX_BARS * bar;
    let note = |start: Ticks, duration: Ticks, pitch: Pitch, velocity: u8| Note {
        start,
        duration,
        pitch,
        velocity,
        voice: 0,
    };
    let mut out = Vec::new();
    match value {
        Value::Scale(tl) => {
            // A scale that changes over time plays as it is at the start.
            let scale = tl.values().next().ok_or("empty scale")?;
            let step = PPQ / 2;
            let top = scale.len() as i32;
            for d in 0..=top {
                let duration = if d == top { 2 * PPQ } else { step * 9 / 10 };
                out.push((SYNTH, note(d as Ticks * step, duration, scale.pitch(d, 4), 90)));
            }
        }
        Value::Chord(tl) => {
            let mut t = 0;
            for (d, chord) in tl.steps() {
                if t >= limit {
                    break;
                }
                let d = if tl.is_constant() { bar } else { *d };
                // A short gap so repeated chords strike again.
                let duration = d.saturating_sub(PPQ / 16).max(1);
                for &p in chord.notes() {
                    out.push((SYNTH, note(t, duration, p, 80)));
                }
                t += d;
            }
        }
        Value::Rhythm(r) => {
            let length = r.length().max(1);
            let loops = bar.div_ceil(length).clamp(1, (limit / length).max(1));
            for k in 0..loops {
                for o in r.onsets() {
                    let start = k * length + o.offset;
                    out.push((DRUMS, note(start, o.duration.max(PPQ / 8), Pitch::midi(RIM), o.velocity)));
                }
            }
        }
        Value::Notes(_) | Value::Control(_) => {
            return Err("only scales, chords and rhythms can be auditioned".into());
        }
    }
    if out.is_empty() {
        return Err("nothing to play".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Timeline;
    use pm_theory::{Chord, Rhythm, Scale};

    #[test]
    fn scale_runs_to_the_octave() {
        let s = Scale::parse("D Dorian").unwrap();
        let n = notes(&Value::Scale(Timeline::constant(s)), Meter::COMMON).unwrap();
        let keys: Vec<i32> = n.iter().map(|(_, n)| n.pitch.step()).collect();
        assert_eq!(keys, [62, 64, 65, 67, 69, 71, 72, 74]);
        assert!(n.iter().all(|(m, _)| *m == SYNTH));
    }

    #[test]
    fn progression_plays_its_chords_in_turn() {
        let bar = Meter::COMMON.ticks_per_bar();
        let c = Chord::from_symbol("C", 4).unwrap();
        let g = Chord::from_symbol("G", 3).unwrap();
        let tl = Timeline::from_steps(vec![(bar, c), (2 * bar, g)]).unwrap();
        let n = notes(&Value::Chord(tl), Meter::COMMON).unwrap();
        let starts: Vec<Ticks> = n.iter().map(|(_, n)| n.start).collect();
        assert_eq!(starts, [0, 0, 0, bar, bar, bar]);
        // A single chord sounds for a bar.
        let one = notes(&Value::Chord(Timeline::constant(c)), Meter::COMMON).unwrap();
        assert!(one.iter().all(|(_, n)| n.start == 0 && n.duration < bar && n.duration > bar / 2));
    }

    #[test]
    fn short_rhythm_fills_a_bar_on_the_drums() {
        let r = Rhythm::euclidean(3, 8, PPQ / 4).unwrap(); // half a bar
        let n = notes(&Value::Rhythm(Box::new(r)), Meter::COMMON).unwrap();
        assert_eq!(n.len(), 6);
        assert!(n.iter().all(|(m, n)| *m == DRUMS && n.pitch.step() == RIM));
    }

    #[test]
    fn notes_are_not_auditioned() {
        assert!(notes(&Value::Notes(vec![]), Meter::COMMON).is_err());
    }
}
