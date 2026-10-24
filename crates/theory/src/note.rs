//! Concrete note events.

use crate::pitch::Pitch;
use crate::time::Ticks;

/// A note event: the material every output (instrument, MIDI, CV) accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Note {
    /// Absolute start time.
    pub start: Ticks,
    pub duration: Ticks,
    pub pitch: Pitch,
    /// 1 to 127.
    pub velocity: u8,
    /// Which voice of a part produced it (used to split counterpoint voices to channels).
    pub voice: u8,
}

impl Note {
    /// The tick at which the note ends.
    pub fn end(&self) -> Ticks {
        self.start + self.duration
    }
}
