//! Absolute pitches and intervals.

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::{Add, Neg, Sub};

use crate::spelling::Spelling;
use crate::tuning::Tuning;

/// A distance between pitches, in steps of a tuning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Interval {
    pub steps: i32,
    pub tuning: Tuning,
}

impl Interval {
    pub const fn new(tuning: Tuning, steps: i32) -> Interval {
        Interval { steps, tuning }
    }

    /// A 12-EDO interval of `n` semitones.
    pub const fn semitones(n: i32) -> Interval {
        Interval {
            steps: n,
            tuning: Tuning::TWELVE,
        }
    }

    /// One octave up in `tuning`.
    pub const fn octave(tuning: Tuning) -> Interval {
        Interval {
            steps: tuning.steps_per_octave(),
            tuning,
        }
    }

    /// Size in cents.
    pub fn cents(self) -> f64 {
        self.tuning.cents(self.steps)
    }

    /// The interval reduced to within one octave (0 ≤ steps < octave).
    pub fn simple(self) -> Interval {
        Interval {
            steps: self.steps.rem_euclid(self.tuning.steps_per_octave()),
            ..self
        }
    }

    /// The complement within an octave (m3 → M6).
    pub fn inverted(self) -> Interval {
        let n = self.tuning.steps_per_octave();
        Interval {
            steps: (n - self.simple().steps) % n,
            ..self
        }
    }

    /// Short name for 12-EDO intervals up to an octave (e.g. "m3", "P5"), by size only.
    pub fn short_name(self) -> Option<&'static str> {
        if !self.tuning.is_twelve() {
            return None;
        }
        const NAMES: [&str; 13] = [
            "P1", "m2", "M2", "m3", "M3", "P4", "TT", "P5", "m6", "M6", "m7", "M7", "P8",
        ];
        NAMES.get(self.steps.unsigned_abs() as usize).copied()
    }
}

impl Neg for Interval {
    type Output = Interval;
    fn neg(self) -> Interval {
        Interval {
            steps: -self.steps,
            ..self
        }
    }
}

impl Add for Interval {
    type Output = Interval;
    fn add(self, rhs: Interval) -> Interval {
        assert_eq!(self.tuning, rhs.tuning, "intervals in different tunings");
        Interval {
            steps: self.steps + rhs.steps,
            ..self
        }
    }
}

/// An absolute note: a step number in a tuning, with an optional letter spelling.
///
/// Equality, ordering and hashing use only the tuning and step, so C#4 == Db4.
/// The spelling is display information supplied by the scale or chord a pitch came from.
#[derive(Clone, Copy, Debug)]
pub struct Pitch {
    step: i32,
    tuning: Tuning,
    spelling: Option<Spelling>,
}

impl Pitch {
    /// A pitch by step number.
    pub const fn new(tuning: Tuning, step: i32) -> Pitch {
        Pitch {
            step,
            tuning,
            spelling: None,
        }
    }

    /// A 12-EDO pitch by MIDI note number.
    pub const fn midi(note: i32) -> Pitch {
        Pitch::new(Tuning::TWELVE, note)
    }

    /// A pitch from an octave number (4 = the octave starting at middle C) and pitch class.
    pub const fn from_octave(tuning: Tuning, octave: i32, pc: u8) -> Pitch {
        Pitch::new(tuning, (octave + 1) * tuning.steps_per_octave() + pc as i32)
    }

    /// A spelled 12-EDO pitch. The octave follows the letter, so B#3 sounds as C4.
    pub fn spelled(spelling: Spelling, octave: i32) -> Pitch {
        let natural = (octave + 1) * 12 + spelling.letter.natural_pc();
        Pitch {
            step: natural + spelling.accidental as i32,
            tuning: Tuning::TWELVE,
            spelling: Some(spelling),
        }
    }

    /// Parses a 12-EDO pitch such as "C4", "F#3", "Bb-1".
    pub fn parse(s: &str) -> Option<Pitch> {
        let (spelling, rest) = Spelling::parse_prefix(s.trim())?;
        let octave: i32 = rest.parse().ok()?;
        Some(Pitch::spelled(spelling, octave))
    }

    pub const fn step(self) -> i32 {
        self.step
    }

    pub const fn tuning(self) -> Tuning {
        self.tuning
    }

    pub const fn spelling(self) -> Option<Spelling> {
        self.spelling
    }

    /// The same pitch with a spelling attached (ignored unless it matches the pitch class).
    pub fn with_spelling(self, spelling: Option<Spelling>) -> Pitch {
        let spelling = match spelling {
            Some(sp) if self.tuning.is_twelve() && sp.pc() == self.pitch_class() => Some(sp),
            _ => None,
        };
        Pitch { spelling, ..self }
    }

    /// Pitch class: the step within the octave (0 = C).
    pub fn pitch_class(self) -> u8 {
        self.step.rem_euclid(self.tuning.steps_per_octave()) as u8
    }

    /// Octave number (C4 = middle C).
    pub fn octave(self) -> i32 {
        self.step.div_euclid(self.tuning.steps_per_octave()) - 1
    }

    /// Frequency in Hz.
    #[cfg(feature = "std")]
    pub fn frequency(self) -> f64 {
        self.tuning.frequency(self.step)
    }

    /// The interval from `self` up to `other` (negative if `other` is lower).
    pub fn interval_to(self, other: Pitch) -> Interval {
        assert_eq!(self.tuning, other.tuning, "pitches in different tunings");
        Interval::new(self.tuning, other.step - self.step)
    }

    /// The pitch moved by `steps` of its own tuning. The spelling is dropped.
    pub fn offset(self, steps: i32) -> Pitch {
        Pitch::new(self.tuning, self.step + steps)
    }
}

impl Default for Pitch {
    fn default() -> Self {
        Pitch::midi(60)
    }
}

impl PartialEq for Pitch {
    fn eq(&self, other: &Self) -> bool {
        self.step == other.step && self.tuning == other.tuning
    }
}

impl Eq for Pitch {}

impl Hash for Pitch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.step.hash(state);
        self.tuning.hash(state);
    }
}

impl PartialOrd for Pitch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pitch {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.tuning.divisions(), self.step).cmp(&(other.tuning.divisions(), other.step))
    }
}

impl Add<Interval> for Pitch {
    type Output = Pitch;
    fn add(self, rhs: Interval) -> Pitch {
        assert_eq!(
            self.tuning, rhs.tuning,
            "pitch and interval in different tunings"
        );
        self.offset(rhs.steps)
    }
}

impl Sub<Interval> for Pitch {
    type Output = Pitch;
    fn sub(self, rhs: Interval) -> Pitch {
        self + -rhs
    }
}

impl Sub for Pitch {
    type Output = Interval;
    fn sub(self, rhs: Pitch) -> Interval {
        rhs.interval_to(self)
    }
}

impl fmt::Display for Pitch {
    /// "C#4" in 12-EDO; "4+5\19" (octave 4, step 5 of 19) in other tunings.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.tuning.is_twelve() {
            let sp = self
                .spelling
                .unwrap_or_else(|| Spelling::sharp(self.pitch_class()));
            let natural = self.step - sp.accidental as i32;
            write!(f, "{}{}", sp, natural.div_euclid(12) - 1)
        } else {
            write!(
                f,
                "{}+{}\\{}",
                self.octave(),
                self.pitch_class(),
                self.tuning.divisions()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_and_octaves() {
        let c4 = Pitch::parse("C4").unwrap();
        assert_eq!(c4.step(), 60);
        assert_eq!(Pitch::parse("A4").unwrap().step(), 69);
        assert_eq!(Pitch::parse("C-1").unwrap().step(), 0);
        assert_eq!(Pitch::parse("B#3").unwrap(), c4);
        assert_eq!(Pitch::parse("Cb4").unwrap().step(), 59);
    }

    #[test]
    fn display_keeps_spelling_and_letter_octave() {
        assert_eq!(Pitch::parse("Db4").unwrap().to_string(), "Db4");
        assert_eq!(Pitch::parse("B#3").unwrap().to_string(), "B#3");
        assert_eq!(Pitch::parse("Cb4").unwrap().to_string(), "Cb4");
        assert_eq!(Pitch::midi(61).to_string(), "C#4");
        let t19 = Tuning::edo(19).unwrap();
        assert_eq!(Pitch::from_octave(t19, 4, 5).to_string(), "4+5\\19");
    }

    #[test]
    fn interval_arithmetic() {
        let c4 = Pitch::midi(60);
        let g4 = c4 + Interval::semitones(7);
        assert_eq!(g4.step(), 67);
        assert_eq!((g4 - c4).short_name(), Some("P5"));
        assert_eq!(Interval::semitones(3).inverted().short_name(), Some("M6"));
        assert_eq!(Interval::semitones(-14).simple().steps, 10);
    }

    #[test]
    fn spelled_equals_unspelled() {
        assert_eq!(Pitch::parse("C#4").unwrap(), Pitch::parse("Db4").unwrap());
        // A spelling for the wrong pitch class is ignored.
        let p = Pitch::midi(60).with_spelling(Spelling::parse("D"));
        assert!(p.spelling().is_none());
    }
}
