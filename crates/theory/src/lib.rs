//! Music theory core for the procedural music node graph.
//!
//! Every value here is a small `Copy` type with a fixed maximum size, so the
//! crate builds without the standard library or a heap (`--no-default-features`).
//! That keeps the door open to running patches on a microcontroller later.
//!
//! Pitches are integer steps in an equal division of the octave ([`Tuning`]).
//! 12-EDO is the default, and in 12-EDO a pitch's step number equals its MIDI
//! note number (C4 = 60). Musical time is counted in integer [`Ticks`] at
//! [`PPQ`] ticks per quarter note.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

mod chord;
mod fixed;
pub mod harmony;
mod note;
mod pcset;
mod pitch;
mod progression;
mod rhythm;
mod roman;
mod scale;
mod spelling;
mod time;
mod tuning;
pub mod voicing;

pub use chord::{Chord, ChordName, ChordQuality, QUALITIES};
pub use fixed::{CapacityError, FixedVec};
pub use note::Note;
pub use pcset::PcSet;
pub use pitch::{Interval, Pitch};
pub use progression::{Progression, ProgressionStep};
pub use rhythm::{Onset, Rhythm};
pub use roman::parse_roman;
pub use scale::{Rounding, Scale, ScaleKind};
pub use spelling::{Letter, Spelling};
pub use time::{Meter, Ticks, PPQ};
pub use tuning::{Tuning, C4_HZ, MAX_EDO};

/// Maximum number of notes in one [`Chord`].
pub const MAX_CHORD_NOTES: usize = 16;
/// Maximum number of onsets in one [`Rhythm`].
pub const MAX_ONSETS: usize = 64;
/// Maximum number of steps in one [`Progression`].
pub const MAX_PROGRESSION_STEPS: usize = 64;
