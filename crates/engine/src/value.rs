//! Values carried on wires.

use pm_theory::{Chord, Note, Rhythm, Scale};

use crate::control::Control;
use crate::spec::PortType;
use crate::timeline::Timeline;

/// A value on a wire. Theory values are whole timelines (they are small);
/// `Notes` holds only the events that start in the current evaluation window.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Scale(Timeline<Scale>),
    Chord(Timeline<Chord>),
    Rhythm(Box<Rhythm>),
    Notes(Vec<Note>),
    Control(Control),
}

impl Value {
    pub fn port_type(&self) -> PortType {
        match self {
            Value::Scale(_) => PortType::Scale,
            Value::Chord(_) => PortType::Chord,
            Value::Rhythm(_) => PortType::Rhythm,
            Value::Notes(_) => PortType::Notes,
            Value::Control(_) => PortType::Control,
        }
    }

    /// The value converted for an input of type `to`, applying implicit conversions
    /// (Chord → Scale repeats the chord across octaves).
    pub fn convert(&self, to: PortType) -> Option<Value> {
        match (self, to) {
            (v, t) if v.port_type() == t => Some(v.clone()),
            (Value::Chord(chords), PortType::Scale) => {
                let scales = chords.map(|c| c.to_scale());
                // Steps whose chord was empty have no scale; keep the timeline shape
                // by borrowing the nearest valid scale.
                let fallback = scales.values().flatten().next().copied()?;
                Some(Value::Scale(scales.map(|s| s.unwrap_or(fallback))))
            }
            _ => None,
        }
    }
}
