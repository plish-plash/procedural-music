//! Chords: finite sets of absolute pitches (voicings), with symbols for 12-EDO.

use core::fmt;

use crate::fixed::{CapacityError, FixedVec};
use crate::pcset::PcSet;
use crate::pitch::{Interval, Pitch};
use crate::scale::Scale;
use crate::spelling::Spelling;
use crate::tuning::Tuning;
use crate::MAX_CHORD_NOTES;

/// A 12-EDO chord quality: its symbol suffix and its tones as
/// (semitones above the root, letter steps above the root).
#[derive(Debug)]
pub struct ChordQuality {
    /// Canonical suffix ("m7b5").
    pub suffix: &'static str,
    /// Accepted spellings of the suffix when parsing.
    pub aliases: &'static [&'static str],
    pub tones: &'static [(u8, u8)],
}

impl ChordQuality {
    fn pc_set(&self) -> PcSet {
        let mut set = PcSet::empty(Tuning::TWELVE);
        for &(semis, _) in self.tones {
            set.insert(semis as i32);
        }
        set
    }
}

macro_rules! q {
    ($suffix:expr, [$($alias:expr),*], [$(($s:expr, $l:expr)),*]) => {
        ChordQuality { suffix: $suffix, aliases: &[$suffix, $($alias),*], tones: &[$(($s, $l)),*] }
    };
}

/// Known chord qualities, in the order they are preferred when naming a chord
/// (so C E G A is named C6 before Am7/C).
pub static QUALITIES: [ChordQuality; 25] = [
    q!("", ["maj", "M"], [(0, 0), (4, 2), (7, 4)]),
    q!("m", ["min", "-"], [(0, 0), (3, 2), (7, 4)]),
    q!("dim", ["o"], [(0, 0), (3, 2), (6, 4)]),
    q!("aug", ["+"], [(0, 0), (4, 2), (8, 4)]),
    q!("sus4", ["sus"], [(0, 0), (5, 3), (7, 4)]),
    q!("sus2", [], [(0, 0), (2, 1), (7, 4)]),
    q!("5", [], [(0, 0), (7, 4)]),
    q!("7", ["dom7"], [(0, 0), (4, 2), (7, 4), (10, 6)]),
    q!(
        "maj7",
        ["M7", "^7", "Δ7", "Δ"],
        [(0, 0), (4, 2), (7, 4), (11, 6)]
    ),
    q!("m7", ["min7", "-7"], [(0, 0), (3, 2), (7, 4), (10, 6)]),
    q!(
        "m7b5",
        ["ø7", "ø", "-7b5", "min7b5"],
        [(0, 0), (3, 2), (6, 4), (10, 6)]
    ),
    q!("dim7", ["o7"], [(0, 0), (3, 2), (6, 4), (9, 6)]),
    q!("6", [], [(0, 0), (4, 2), (7, 4), (9, 5)]),
    q!("m6", ["min6", "-6"], [(0, 0), (3, 2), (7, 4), (9, 5)]),
    q!(
        "mMaj7",
        ["mM7", "m(maj7)", "minMaj7"],
        [(0, 0), (3, 2), (7, 4), (11, 6)]
    ),
    q!("aug7", ["+7", "7#5"], [(0, 0), (4, 2), (8, 4), (10, 6)]),
    q!("7sus4", ["7sus"], [(0, 0), (5, 3), (7, 4), (10, 6)]),
    q!("add9", [], [(0, 0), (4, 2), (7, 4), (14, 8)]),
    q!("9", [], [(0, 0), (4, 2), (7, 4), (10, 6), (14, 8)]),
    q!("maj9", ["M9"], [(0, 0), (4, 2), (7, 4), (11, 6), (14, 8)]),
    q!(
        "m9",
        ["min9", "-9"],
        [(0, 0), (3, 2), (7, 4), (10, 6), (14, 8)]
    ),
    q!("7b9", [], [(0, 0), (4, 2), (7, 4), (10, 6), (13, 8)]),
    q!("7#9", [], [(0, 0), (4, 2), (7, 4), (10, 6), (15, 8)]),
    q!(
        "11",
        [],
        [(0, 0), (4, 2), (7, 4), (10, 6), (14, 8), (17, 10)]
    ),
    q!(
        "13",
        [],
        [(0, 0), (4, 2), (7, 4), (10, 6), (14, 8), (21, 12)]
    ),
];

/// A chord symbol: root, quality suffix and optional slash bass ("Am7/C").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordName {
    pub root: Spelling,
    pub quality: &'static str,
    pub bass: Option<Spelling>,
}

impl fmt::Display for ChordName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.root, self.quality)?;
        if let Some(bass) = self.bass {
            write!(f, "/{bass}")?;
        }
        Ok(())
    }
}

/// A finite set of absolute pitches (a voicing), sorted low to high, with an
/// optional root pitch class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    tuning: Tuning,
    notes: FixedVec<Pitch, MAX_CHORD_NOTES>,
    root: Option<u8>,
}

impl Default for Chord {
    fn default() -> Self {
        Chord::empty(Tuning::TWELVE)
    }
}

impl Chord {
    /// A chord with no notes.
    pub fn empty(tuning: Tuning) -> Chord {
        Chord {
            tuning,
            notes: FixedVec::new(),
            root: None,
        }
    }

    /// A chord from pitches (duplicates are merged). An empty slice gives an empty 12-EDO chord.
    pub fn from_pitches(pitches: &[Pitch]) -> Result<Chord, CapacityError> {
        let tuning = pitches.first().map(|p| p.tuning()).unwrap_or_default();
        let mut chord = Chord::empty(tuning);
        for &p in pitches {
            chord.add(p)?;
        }
        Ok(chord)
    }

    /// A root-position 12-EDO chord from a quality, spelled from the root.
    pub fn from_quality(root: Pitch, quality: &ChordQuality) -> Chord {
        assert!(root.tuning().is_twelve(), "chord qualities are 12-EDO");
        let mut chord = Chord::empty(Tuning::TWELVE);
        for &(semis, letters) in quality.tones {
            let p = root.offset(semis as i32);
            let spelling = root.spelling().and_then(|sp| {
                Spelling::with_letter(sp.letter.up(letters as i32), p.pitch_class())
            });
            let _ = chord.add(p.with_spelling(spelling));
        }
        chord.with_root(root.pitch_class())
    }

    /// Parses a chord symbol ("Cmaj7", "F#m7b5", "Bb7/D", "Ebsus4") with the root in `octave`.
    /// A slash bass is placed below the rest of the chord.
    pub fn from_symbol(symbol: &str, octave: i32) -> Option<Chord> {
        let symbol = symbol.trim();
        let (body, bass) = match symbol.split_once('/') {
            Some((body, bass)) => (body, Some(Spelling::parse(bass)?)),
            None => (symbol, None),
        };
        let (root, suffix) = Spelling::parse_prefix(body)?;
        let quality = find_quality(suffix)?;
        let mut chord = Chord::from_quality(Pitch::spelled(root, octave), quality);
        if let Some(bass) = bass {
            let lowest = chord.bass()?;
            let mut gap = (lowest.pitch_class() as i32 - bass.pc() as i32).rem_euclid(12);
            if gap == 0 {
                gap = 12;
            }
            let bass_pitch = lowest.offset(-gap).with_spelling(Some(bass));
            chord.add(bass_pitch).ok()?;
        }
        Some(chord)
    }

    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    /// Adds a pitch, keeping notes sorted. A pitch already present is not duplicated
    /// (but gains a spelling if it had none).
    pub fn add(&mut self, pitch: Pitch) -> Result<(), CapacityError> {
        if self.notes.is_empty() {
            self.tuning = pitch.tuning();
        }
        assert_eq!(
            pitch.tuning(),
            self.tuning,
            "pitch and chord in different tunings"
        );
        match self.notes.binary_search(&pitch) {
            Ok(i) => {
                if self.notes[i].spelling().is_none() {
                    self.notes[i] = pitch;
                }
                Ok(())
            }
            Err(i) => self.notes.insert(i, pitch),
        }
    }

    /// The notes, low to high.
    pub fn notes(&self) -> &[Pitch] {
        &self.notes
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// The root pitch class, if known.
    pub fn root(&self) -> Option<u8> {
        self.root
    }

    /// The same notes with a root pitch class set.
    pub fn with_root(mut self, pc: u8) -> Chord {
        self.root = Some(pc % self.tuning.divisions());
        self
    }

    /// The lowest note.
    pub fn bass(&self) -> Option<Pitch> {
        self.notes.first().copied()
    }

    /// The highest note.
    pub fn top(&self) -> Option<Pitch> {
        self.notes.last().copied()
    }

    /// The chord's pitch classes.
    pub fn pitch_classes(&self) -> PcSet {
        let mut set = PcSet::empty(self.tuning);
        for p in self.notes.iter() {
            set.insert(p.step());
        }
        set
    }

    pub fn contains_pc(&self, pc: i32) -> bool {
        self.pitch_classes().contains(pc)
    }

    /// The chord's pitch classes repeated across octaves, as a scale (a "chord scale").
    /// The root is the chord root, or else the bass.
    pub fn to_scale(&self) -> Option<Scale> {
        let root = self.root.or_else(|| self.bass().map(|p| p.pitch_class()))?;
        let mut scale = Scale::new(root, self.pitch_classes())?;
        if let Some(sp) = self
            .notes
            .iter()
            .filter_map(|p| p.spelling())
            .find(|sp| sp.pc() == root)
        {
            scale = scale.with_root_spelling(sp);
        }
        Some(scale)
    }

    /// Every note moved by an interval. Spellings are dropped.
    pub fn transpose(&self, interval: Interval) -> Chord {
        assert_eq!(
            interval.tuning, self.tuning,
            "interval and chord in different tunings"
        );
        let mut out = Chord::empty(self.tuning);
        for p in self.notes.iter() {
            let _ = out.add(p.offset(interval.steps));
        }
        out.root = self
            .root
            .map(|r| (r as i32 + interval.steps).rem_euclid(self.tuning.steps_per_octave()) as u8);
        out
    }

    /// Inverts the chord: each positive step moves the lowest note up by octaves until it
    /// is the highest; each negative step moves the highest note below the lowest.
    pub fn invert(&self, steps: i32) -> Chord {
        let mut chord = *self;
        let octave = self.tuning.steps_per_octave();
        if chord.notes.len() < 2 {
            return chord;
        }
        for _ in 0..steps.unsigned_abs() {
            if steps > 0 {
                let low = chord.notes.remove(0);
                let top = *chord.notes.last().expect("at least one note left");
                let mut moved = low;
                while moved <= top {
                    moved = moved.offset(octave).with_spelling(low.spelling());
                }
                let _ = chord.add(moved);
            } else {
                let high = chord.notes.pop().expect("at least two notes");
                let bottom = chord.notes[0];
                let mut moved = high;
                while moved >= bottom {
                    moved = moved.offset(-octave).with_spelling(high.spelling());
                }
                let _ = chord.add(moved);
            }
        }
        chord
    }

    /// The chord symbol, if this is a 12-EDO chord with a known quality.
    /// Tries the chord's root (or bass) first, then the other notes as root (giving a slash chord).
    pub fn name(&self) -> Option<ChordName> {
        if !self.tuning.is_twelve() || self.notes.is_empty() {
            return None;
        }
        let pcs = self.pitch_classes();
        let bass_pc = self.notes[0].pitch_class();
        let preferred = self.root.unwrap_or(bass_pc);
        let candidates =
            core::iter::once(preferred).chain(pcs.iter().filter(move |&pc| pc != preferred));
        for root in candidates {
            let relative = pcs.transpose(-(root as i32));
            if let Some(quality) = QUALITIES.iter().find(|q| q.pc_set() == relative) {
                let spell = |pc: u8| {
                    self.notes
                        .iter()
                        .filter_map(|p| p.spelling())
                        .find(|sp| sp.pc() == pc)
                        .unwrap_or_else(|| Spelling::key_default(pc))
                };
                let bass = (bass_pc != root).then(|| spell(bass_pc));
                return Some(ChordName {
                    root: spell(root),
                    quality: quality.suffix,
                    bass,
                });
            }
        }
        None
    }
}

/// Finds a quality by suffix or alias (exact match).
pub(crate) fn find_quality(suffix: &str) -> Option<&'static ChordQuality> {
    QUALITIES.iter().find(|q| q.aliases.contains(&suffix))
}

impl fmt::Display for Chord {
    /// The chord symbol when known ("Dm7"), otherwise the notes ("[C4 D4 F#4]").
    /// `{:#}` always prints the notes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !f.alternate() {
            if let Some(name) = self.name() {
                return write!(f, "{name}");
            }
        }
        f.write_str("[")?;
        for (i, p) in self.notes.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{p}")?;
        }
        f.write_str("]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    fn notes(symbol: &str) -> std::string::String {
        format!("{:#}", Chord::from_symbol(symbol, 4).unwrap())
    }

    #[test]
    fn parse_symbols_with_spelling() {
        assert_eq!(notes("C"), "[C4 E4 G4]");
        assert_eq!(notes("Bbm7"), "[Bb4 Db5 F5 Ab5]");
        assert_eq!(notes("F#m7b5"), "[F#4 A4 C5 E5]");
        assert_eq!(notes("Ebmaj7"), "[Eb4 G4 Bb4 D5]");
        assert_eq!(notes("Bdim7"), "[B4 D5 F5 Ab5]");
        assert_eq!(notes("G7/B"), "[B3 G4 B4 D5 F5]");
        assert!(Chord::from_symbol("Cfoo", 4).is_none());
    }

    #[test]
    fn names_round_trip() {
        for symbol in [
            "C", "Am", "F#dim", "Ebaug", "Dsus4", "G7", "Cmaj7", "Bm7b5", "Ab9", "Em7",
        ] {
            let chord = Chord::from_symbol(symbol, 3).unwrap();
            assert_eq!(chord.to_string(), symbol);
        }
    }

    #[test]
    fn inversions_name_as_slash_chords() {
        let c = Chord::from_symbol("C", 4).unwrap();
        let first = c.invert(1);
        assert_eq!(format!("{first:#}"), "[E4 G4 C5]");
        assert_eq!(first.to_string(), "C/E");
        assert_eq!(c.invert(2).to_string(), "C/G");
        assert_eq!(format!("{:#}", c.invert(-1)), "[G3 C4 E4]");
        // Without a stored root, the bass is tried first: C E G A is C6.
        let c6 = Chord::from_pitches(&[60, 64, 67, 69].map(Pitch::midi)).unwrap();
        assert_eq!(c6.to_string(), "C6");
        let am7 = c6.with_root(9);
        assert_eq!(am7.to_string(), "Am7/C");
    }

    #[test]
    fn transpose_and_chord_scale() {
        let d = Chord::from_symbol("C", 4)
            .unwrap()
            .transpose(Interval::semitones(2));
        assert_eq!(d.to_string(), "D");
        let scale = Chord::from_symbol("Cmaj7", 4).unwrap().to_scale().unwrap();
        assert_eq!(scale.len(), 4);
        assert_eq!(scale.pitch(4, 4).to_string(), "C5");
    }

    #[test]
    fn duplicates_merge() {
        let mut c = Chord::from_pitches(&[Pitch::midi(60), Pitch::midi(60)]).unwrap();
        assert_eq!(c.len(), 1);
        c.add(Pitch::midi(48)).unwrap();
        assert_eq!(c.bass(), Some(Pitch::midi(48)));
    }

    #[test]
    fn unnamed_chords_print_notes() {
        let cluster = Chord::from_pitches(&[60, 61, 62].map(Pitch::midi)).unwrap();
        assert_eq!(cluster.to_string(), "[C4 C#4 D4]");
    }
}
