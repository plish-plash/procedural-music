//! Progressions: chords (and optionally scales) over time.

use core::fmt;

use crate::chord::Chord;
use crate::fixed::{CapacityError, FixedVec};
use crate::pitch::Interval;
use crate::roman::parse_roman;
use crate::scale::Scale;
use crate::time::Ticks;
use crate::MAX_PROGRESSION_STEPS;

/// One chord of a progression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProgressionStep {
    pub chord: Chord,
    pub duration: Ticks,
    /// The scale in force during this chord, when known (e.g. the key of a Roman numeral).
    pub scale: Option<Scale>,
}

/// A timeline of chords that loops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Progression {
    steps: FixedVec<ProgressionStep, MAX_PROGRESSION_STEPS>,
}

impl Progression {
    pub fn new() -> Progression {
        Progression::default()
    }

    pub fn push(
        &mut self,
        chord: Chord,
        duration: Ticks,
        scale: Option<Scale>,
    ) -> Result<(), CapacityError> {
        self.steps.push(ProgressionStep {
            chord,
            duration: duration.max(1),
            scale,
        })
    }

    /// Parses space-separated chord symbols, each lasting `duration`.
    /// `-` extends the previous chord by another `duration`; `|` is ignored.
    pub fn from_symbols(text: &str, duration: Ticks, octave: i32) -> Option<Progression> {
        Progression::parse_tokens(text, duration, |token| {
            Some((Chord::from_symbol(token, octave)?, None))
        })
    }

    /// Parses space-separated Roman numerals in `key` ("ii V7 I"), each lasting `duration`.
    /// `-` extends the previous chord; `|` is ignored. Every step records the key as its scale.
    pub fn from_roman(
        text: &str,
        key: &Scale,
        duration: Ticks,
        octave: i32,
    ) -> Option<Progression> {
        Progression::parse_tokens(text, duration, |token| {
            Some((parse_roman(token, key, octave)?, Some(*key)))
        })
    }

    fn parse_tokens(
        text: &str,
        duration: Ticks,
        mut parse: impl FnMut(&str) -> Option<(Chord, Option<Scale>)>,
    ) -> Option<Progression> {
        let mut prog = Progression::new();
        for token in text.split_whitespace() {
            match token {
                "|" => {}
                "-" => prog.steps.last_mut()?.duration += duration,
                _ => {
                    let (chord, scale) = parse(token)?;
                    prog.push(chord, duration, scale).ok()?;
                }
            }
        }
        Some(prog)
    }

    pub fn steps(&self) -> &[ProgressionStep] {
        &self.steps
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Total length of one pass.
    pub fn length(&self) -> Ticks {
        self.steps.iter().map(|s| s.duration).sum()
    }

    /// The step sounding at `time` (looping), its index and its absolute start time.
    pub fn at(&self, time: Ticks) -> Option<(usize, Ticks, &ProgressionStep)> {
        let length = self.length();
        if length == 0 {
            return None;
        }
        let loop_start = time - time % length;
        let mut start = loop_start;
        for (i, step) in self.steps.iter().enumerate() {
            if time < start + step.duration {
                return Some((i, start, step));
            }
            start += step.duration;
        }
        None
    }

    /// Every chord and scale moved by an interval.
    pub fn transpose(&self, interval: Interval) -> Progression {
        let mut out = *self;
        for step in out.steps.iter_mut() {
            step.chord = step.chord.transpose(interval);
            step.scale = step.scale.map(|s| s.transpose(interval));
        }
        out
    }

    /// Replaces each chord with `f(previous_output, chord)`; used for voice leading.
    pub fn map_with_previous(
        &self,
        mut f: impl FnMut(Option<&Chord>, &Chord) -> Chord,
    ) -> Progression {
        let mut out = *self;
        let mut prev: Option<Chord> = None;
        for step in out.steps.iter_mut() {
            step.chord = f(prev.as_ref(), &step.chord);
            prev = Some(step.chord);
        }
        out
    }
}

impl fmt::Display for Progression {
    /// Chord symbols separated by spaces.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{}", step.chord)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Meter;
    use std::format;

    #[test]
    fn roman_progression_in_key() {
        let key = Scale::parse("Bb major").unwrap();
        let bar = Meter::COMMON.ticks_per_bar();
        let prog = Progression::from_roman("ii7 V7 | Imaj7 -", &key, bar, 3).unwrap();
        assert_eq!(format!("{prog}"), "Cm7 F7 Bbmaj7");
        assert_eq!(prog.length(), 4 * bar);
        assert_eq!(prog.steps()[2].duration, 2 * bar);
        assert_eq!(prog.steps()[0].scale, Some(key));
    }

    #[test]
    fn lookup_loops() {
        let bar = Meter::COMMON.ticks_per_bar();
        let prog = Progression::from_symbols("C - F G", bar, 4).unwrap();
        let (i, start, step) = prog.at(bar + 5).unwrap();
        assert_eq!((i, start, format!("{}", step.chord)), (0, 0, "C".into()));
        let (i, start, _) = prog.at(4 * bar + 3 * bar).unwrap();
        assert_eq!((i, start), (2, 7 * bar));
    }

    #[test]
    fn transpose_moves_scales_too() {
        let key = Scale::parse("C major").unwrap();
        let prog = Progression::from_roman("I IV V", &key, 960, 4)
            .unwrap()
            .transpose(Interval::semitones(2));
        assert_eq!(format!("{prog}"), "D G A");
        assert_eq!(format!("{}", prog.steps()[0].scale.unwrap()), "D Major");
    }

    #[test]
    fn bad_tokens_fail() {
        assert!(Progression::from_symbols("C Qm7", 960, 4).is_none());
        assert!(Progression::from_symbols("- C", 960, 4).is_none());
    }
}
