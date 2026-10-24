//! The node catalog: specs (shared with the UI) and implementations.

mod melody;
mod note_ops;
mod output;
mod rhythm;
mod sound;
mod theory;

use std::collections::HashMap;

use pm_theory::{Chord, Note, Rhythm, Scale, Ticks, PPQ};

use pm_synth::ModuleKind;

use crate::graph::EvalContext;
use crate::spec::{NodeSpec, ParamValue, Params};
use crate::timeline::Timeline;
use crate::value::Value;

/// A compiled node: parameters are parsed once when the graph is built.
pub trait NodeImpl: Send + Sync {
    /// One entry per output port. `None` means "nothing to send".
    fn eval(&self, inputs: &Inputs, ctx: &EvalContext) -> Result<Vec<Option<Value>>, String>;

    /// For nodes that consume notes: where they go, or `None` if muted.
    fn sink(&self) -> Option<Sink> {
        None
    }

    fn is_sink(&self) -> bool {
        false
    }

    /// For output nodes: a program change to send when playback starts (port, channel, program 0-127).
    fn program(&self) -> Option<(String, u8, u8)> {
        None
    }

    /// For sound nodes: the module this node becomes in the audio rack.
    fn module(&self) -> Option<ModuleKind> {
        None
    }
}

/// Where a sink node sends its notes.
#[derive(Clone, Debug, PartialEq)]
pub enum Sink {
    /// A MIDI port (empty = default) and zero-based channel.
    Midi { port: String, channel: u8 },
    /// The node's own instrument in the audio rack.
    Instrument,
}

/// Input values for one evaluation, already converted to each port's type.
pub struct Inputs {
    pub(crate) values: Vec<Option<Value>>,
}

impl Inputs {
    pub fn scale(&self, i: usize) -> Option<&Timeline<Scale>> {
        match self.values.get(i)? {
            Some(Value::Scale(s)) => Some(s),
            _ => None,
        }
    }

    pub fn chord(&self, i: usize) -> Option<&Timeline<Chord>> {
        match self.values.get(i)? {
            Some(Value::Chord(c)) => Some(c),
            _ => None,
        }
    }

    pub fn rhythm(&self, i: usize) -> Option<&Rhythm> {
        match self.values.get(i)? {
            Some(Value::Rhythm(r)) => Some(r),
            _ => None,
        }
    }

    pub fn notes(&self, i: usize) -> &[Note] {
        match self.values.get(i) {
            Some(Some(Value::Notes(n))) => n,
            _ => &[],
        }
    }
}

static SPECS: &[&NodeSpec] = &[
    // Theory sources
    &theory::SCALE,
    &theory::CHORD,
    &theory::PROGRESSION,
    // Theory transforms
    &theory::MODE,
    &theory::TRANSPOSE,
    &theory::VOICE,
    &theory::VOICE_LEAD,
    &theory::DROP,
    &theory::INVERT,
    // Rhythm
    &rhythm::PATTERN,
    &rhythm::EUCLID,
    &rhythm::COMBINE,
    &rhythm::SWING,
    // Melody and parts
    &melody::ARP,
    &melody::SEQUENCER,
    &melody::RANDOM_WALK,
    &melody::PLAY_CHORDS,
    &melody::CHORD_ROOT,
    &melody::DRUM,
    // Note operations
    &note_ops::OCTAVE,
    &note_ops::QUANTIZE,
    &note_ops::VELOCITY,
    &note_ops::MERGE,
    // Built-in sound
    &sound::POLY_SYNTH,
    &sound::MONO_SYNTH,
    &sound::DRUM_SYNTH,
    &sound::MIXER,
    &sound::FX_FILTER,
    &sound::FX_DRIVE,
    &sound::FX_CHORUS,
    &sound::FX_DELAY,
    &sound::FX_REVERB,
    // Output
    &sound::AUDIO_OUT,
    &output::MIDI_OUT,
];

/// Every node spec, in menu order.
pub fn specs() -> &'static [&'static NodeSpec] {
    SPECS
}

pub fn spec_for(kind: &str) -> Option<&'static NodeSpec> {
    SPECS.iter().copied().find(|s| s.kind == kind)
}

/// Builds a node from its kind and parameter values.
pub fn build(
    spec: &'static NodeSpec,
    values: &HashMap<String, ParamValue>,
) -> Result<Box<dyn NodeImpl>, String> {
    let p = Params::new(spec, values);
    match spec.kind {
        "scale" => theory::build_scale(&p),
        "chord" => theory::build_chord(&p),
        "progression" => theory::build_progression(&p),
        "mode" => theory::build_mode(&p),
        "transpose" => theory::build_transpose(&p),
        "voice" => theory::build_voice(&p),
        "voice_lead" => theory::build_voice_lead(&p),
        "drop" => theory::build_drop(&p),
        "invert" => theory::build_invert(&p),
        "pattern" => rhythm::build_pattern(&p),
        "euclid" => rhythm::build_euclid(&p),
        "combine" => rhythm::build_combine(&p),
        "swing" => rhythm::build_swing(&p),
        "arp" => melody::build_arp(&p),
        "sequencer" => melody::build_sequencer(&p),
        "random_walk" => melody::build_random_walk(&p),
        "play_chords" => melody::build_play_chords(&p),
        "chord_root" => melody::build_chord_root(&p),
        "drum" => melody::build_drum(&p),
        "octave" => note_ops::build_octave(&p),
        "quantize" => note_ops::build_quantize(&p),
        "velocity" => note_ops::build_velocity(&p),
        "merge" => note_ops::build_merge(&p),
        "midi_out" => output::build_midi_out(&p),
        "poly_synth" | "mono_synth" | "drum_synth" | "mixer" | "fx_filter" | "fx_drive"
        | "fx_chorus" | "fx_delay" | "fx_reverb" | "audio_out" => sound::build(spec.kind, &p),
        other => Err(format!("unknown node kind '{other}'")),
    }
}

// ---- Shared helpers -------------------------------------------------------

pub(crate) const STEP_OPTIONS: &[&str] = &["1/4", "1/8", "1/16", "1/32", "1/8T", "1/16T"];

pub(crate) fn step_ticks(name: &str) -> Ticks {
    match name {
        "1/4" => PPQ,
        "1/8" => PPQ / 2,
        "1/32" => PPQ / 8,
        "1/8T" => PPQ / 3,
        "1/16T" => PPQ / 6,
        _ => PPQ / 4,
    }
}

/// A straight sixteenth-note pulse, used when a rhythm input is left empty.
pub(crate) fn default_pulse() -> Rhythm {
    Rhythm::from_grid("x", PPQ / 4).expect("valid grid")
}

/// Number of onsets of a looping rhythm in `[from, to)`, without walking every loop.
pub(crate) fn onsets_between(rhythm: &Rhythm, from: Ticks, to: Ticks) -> u64 {
    if to <= from || rhythm.is_empty() {
        return 0;
    }
    let len = rhythm.length();
    let count_before = |t: Ticks| -> u64 {
        let full = (t / len) as u64 * rhythm.len() as u64;
        let partial = rhythm
            .onsets()
            .iter()
            .filter(|o| o.offset < t % len)
            .count() as u64;
        full + partial
    };
    count_before(to) - count_before(from)
}

/// Scales an onset velocity by a percentage, keeping it in 1..=127.
pub(crate) fn scale_velocity(velocity: u8, percent: f64) -> u8 {
    (velocity as f64 * percent / 100.0)
        .round()
        .clamp(1.0, 127.0) as u8
}

pub(crate) fn gate(duration: Ticks, percent: f64) -> Ticks {
    ((duration as f64 * percent / 100.0).round() as Ticks).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_builds_with_defaults() {
        let empty = HashMap::new();
        for spec in specs() {
            if let Err(e) = build(spec, &empty) {
                panic!("{} failed to build with defaults: {e}", spec.kind);
            }
        }
    }

    #[test]
    fn kinds_are_unique() {
        let mut kinds: Vec<_> = specs().iter().map(|s| s.kind).collect();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds.len(), specs().len());
    }

    #[test]
    fn counts_onsets_across_loops() {
        let r = Rhythm::from_grid("x.x.", PPQ / 4).unwrap(); // onsets at 0 and 480, length 960
        assert_eq!(onsets_between(&r, 0, 960), 2);
        assert_eq!(onsets_between(&r, 0, 961), 3);
        assert_eq!(onsets_between(&r, 100, 2000), 4); // 480, 960, 1440, 1920
        assert_eq!(onsets_between(&r, 960 * 1000, 960 * 1000 + 1), 1);
    }
}
