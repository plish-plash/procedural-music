//! Musical time in integer ticks.

use core::fmt;

/// Musical time in ticks. At [`PPQ`] ticks per quarter note a `u32` covers
/// over 100,000 bars of 4/4.
pub type Ticks = u32;

/// Ticks per quarter note.
pub const PPQ: Ticks = 960;

/// A time signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Meter {
    /// Beats per bar (the top number).
    pub beats: u8,
    /// Note value of one beat (the bottom number): 4 = quarter, 8 = eighth.
    pub unit: u8,
}

impl Meter {
    pub const COMMON: Meter = Meter { beats: 4, unit: 4 };

    /// A meter, or `None` if `unit` is not a power of two up to 64 or `beats` is 0.
    pub const fn new(beats: u8, unit: u8) -> Option<Meter> {
        if beats == 0 || unit == 0 || unit > 64 || !unit.is_power_of_two() {
            None
        } else {
            Some(Meter { beats, unit })
        }
    }

    pub const fn ticks_per_beat(self) -> Ticks {
        PPQ * 4 / self.unit as Ticks
    }

    pub const fn ticks_per_bar(self) -> Ticks {
        self.ticks_per_beat() * self.beats as Ticks
    }

    /// Ticks for `bars` bars.
    pub const fn bars(self, bars: u32) -> Ticks {
        self.ticks_per_bar() * bars
    }

    /// Splits a time into zero-based (bar, beat, tick within beat).
    pub fn position(self, ticks: Ticks) -> (u32, u32, u32) {
        let bar = ticks / self.ticks_per_bar();
        let in_bar = ticks % self.ticks_per_bar();
        (
            bar,
            in_bar / self.ticks_per_beat(),
            in_bar % self.ticks_per_beat(),
        )
    }
}

impl Default for Meter {
    fn default() -> Self {
        Meter::COMMON
    }
}

impl fmt::Display for Meter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.beats, self.unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters() {
        assert_eq!(Meter::COMMON.ticks_per_bar(), 3840);
        let six_eight = Meter::new(6, 8).unwrap();
        assert_eq!(six_eight.ticks_per_beat(), 480);
        assert_eq!(six_eight.ticks_per_bar(), 2880);
        assert_eq!(Meter::COMMON.position(3840 + 960 * 2 + 10), (1, 2, 10));
        assert!(Meter::new(4, 3).is_none());
    }
}
