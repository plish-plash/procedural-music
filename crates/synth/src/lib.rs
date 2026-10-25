//! Built-in instruments and effects: subtractive poly and mono synths, a synthesized
//! drum kit, delay, reverb, chorus, filter, drive and a mixer, wired together by a
//! [`Rack`] that renders stereo audio from timed note events.
//!
//! No dependencies; the desktop app feeds the rack from its audio callback.

// DSP objects use `next()` for "the next sample"; they are not iterators.
#![allow(clippy::should_implement_trait)]

pub mod drums;
pub mod dsp;
pub mod fx;
pub mod rack;
pub mod synth;
pub mod wav;

pub use rack::{AudioMsg, ModuleDesc, ModuleKind, Rack, RackDesc, TimedEvent, TimedSet};

/// A note for an instrument. `id` pairs each note-off with its note-on; `freq` is exact
/// (any tuning); `key` is the nearest MIDI key, used by the drum kit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteEvent {
    On {
        id: u64,
        freq: f32,
        key: u8,
        velocity: u8,
    },
    Off {
        id: u64,
    },
}
