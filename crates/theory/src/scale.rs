//! Scales: a root plus a pitch-class set that repeats every octave.

use core::fmt;

use crate::chord::Chord;
use crate::fixed::FixedVec;
use crate::pcset::PcSet;
use crate::pitch::{Interval, Pitch};
use crate::spelling::{Letter, Spelling};
use crate::tuning::Tuning;

/// Named 12-EDO scales.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScaleKind {
    Major,
    NaturalMinor,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    WholeTone,
    OctatonicHalfWhole,
    OctatonicWholeHalf,
    Chromatic,
}

struct KindInfo {
    kind: ScaleKind,
    name: &'static str,
    steps: &'static [u8],
    aliases: &'static [&'static str],
}

const KINDS: [KindInfo; 16] = [
    KindInfo {
        kind: ScaleKind::Major,
        name: "Major",
        steps: &[2, 2, 1, 2, 2, 2, 1],
        aliases: &["major", "maj", "ionian"],
    },
    KindInfo {
        kind: ScaleKind::NaturalMinor,
        name: "Minor",
        steps: &[2, 1, 2, 2, 1, 2, 2],
        aliases: &["minor", "min", "natural minor", "aeolian"],
    },
    KindInfo {
        kind: ScaleKind::HarmonicMinor,
        name: "Harmonic minor",
        steps: &[2, 1, 2, 2, 1, 3, 1],
        aliases: &["harmonic minor"],
    },
    KindInfo {
        kind: ScaleKind::MelodicMinor,
        name: "Melodic minor",
        steps: &[2, 1, 2, 2, 2, 2, 1],
        aliases: &["melodic minor", "jazz minor"],
    },
    KindInfo {
        kind: ScaleKind::Dorian,
        name: "Dorian",
        steps: &[2, 1, 2, 2, 2, 1, 2],
        aliases: &["dorian"],
    },
    KindInfo {
        kind: ScaleKind::Phrygian,
        name: "Phrygian",
        steps: &[1, 2, 2, 2, 1, 2, 2],
        aliases: &["phrygian"],
    },
    KindInfo {
        kind: ScaleKind::Lydian,
        name: "Lydian",
        steps: &[2, 2, 2, 1, 2, 2, 1],
        aliases: &["lydian"],
    },
    KindInfo {
        kind: ScaleKind::Mixolydian,
        name: "Mixolydian",
        steps: &[2, 2, 1, 2, 2, 1, 2],
        aliases: &["mixolydian"],
    },
    KindInfo {
        kind: ScaleKind::Locrian,
        name: "Locrian",
        steps: &[1, 2, 2, 1, 2, 2, 2],
        aliases: &["locrian"],
    },
    KindInfo {
        kind: ScaleKind::MajorPentatonic,
        name: "Major pentatonic",
        steps: &[2, 2, 3, 2, 3],
        aliases: &["major pentatonic", "pentatonic"],
    },
    KindInfo {
        kind: ScaleKind::MinorPentatonic,
        name: "Minor pentatonic",
        steps: &[3, 2, 2, 3, 2],
        aliases: &["minor pentatonic"],
    },
    KindInfo {
        kind: ScaleKind::Blues,
        name: "Blues",
        steps: &[3, 2, 1, 1, 3, 2],
        aliases: &["blues"],
    },
    KindInfo {
        kind: ScaleKind::WholeTone,
        name: "Whole tone",
        steps: &[2, 2, 2, 2, 2, 2],
        aliases: &["whole tone", "wholetone"],
    },
    KindInfo {
        kind: ScaleKind::OctatonicHalfWhole,
        name: "Octatonic (half-whole)",
        steps: &[1, 2, 1, 2, 1, 2, 1, 2],
        aliases: &["octatonic half-whole", "half-whole", "dominant diminished"],
    },
    KindInfo {
        kind: ScaleKind::OctatonicWholeHalf,
        name: "Octatonic (whole-half)",
        steps: &[2, 1, 2, 1, 2, 1, 2, 1],
        aliases: &[
            "octatonic",
            "octatonic whole-half",
            "whole-half",
            "diminished",
        ],
    },
    KindInfo {
        kind: ScaleKind::Chromatic,
        name: "Chromatic",
        steps: &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        aliases: &["chromatic"],
    },
];

impl ScaleKind {
    fn info(self) -> &'static KindInfo {
        KINDS
            .iter()
            .find(|k| k.kind == self)
            .expect("every kind has info")
    }

    /// Display name ("Harmonic minor").
    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// Step sizes in semitones from the root, summing to 12.
    pub fn steps(self) -> &'static [u8] {
        self.info().steps
    }

    /// Looks up a scale name, ignoring case ("dorian", "Natural Minor", "ionian").
    pub fn from_name(name: &str) -> Option<ScaleKind> {
        let name = name.trim();
        KINDS
            .iter()
            .find(|k| k.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
            .map(|k| k.kind)
    }

    /// All kinds, in display order.
    pub fn all() -> impl Iterator<Item = ScaleKind> {
        KINDS.iter().map(|k| k.kind)
    }

    fn pc_set(self) -> PcSet {
        set_from_steps(Tuning::TWELVE, 0, self.steps()).expect("presets sum to 12")
    }
}

/// How [`Scale::quantize`] treats a pitch that is not in the scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rounding {
    /// The closest scale pitch; the lower one on a tie.
    #[default]
    Nearest,
    /// The closest scale pitch at or below.
    Down,
    /// The closest scale pitch at or above.
    Up,
}

/// A root plus a set of pitch classes that repeats every octave.
///
/// Degrees count from 0 (the root). Degree 7 of a seven-note scale is the root
/// an octave up; negative degrees go below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scale {
    root: u8,
    set: PcSet,
    root_spelling: Option<Spelling>,
}

fn set_from_steps(tuning: Tuning, root: u8, steps: &[u8]) -> Option<PcSet> {
    let total: i32 = steps.iter().map(|&s| s as i32).sum();
    if total != tuning.steps_per_octave() || steps.contains(&0) {
        return None;
    }
    let mut set = PcSet::empty(tuning);
    let mut pc = root as i32;
    for &s in steps {
        set.insert(pc);
        pc += s as i32;
    }
    Some(set)
}

impl Scale {
    /// A scale from a root pitch class and a set that contains it.
    pub fn new(root: u8, set: PcSet) -> Option<Scale> {
        if !set.contains(root as i32) {
            return None;
        }
        let root = root % set.tuning().divisions();
        let mut scale = Scale {
            root,
            set,
            root_spelling: None,
        };
        scale.root_spelling = scale.best_root_spelling();
        Some(scale)
    }

    /// A scale from step sizes (e.g. `[2, 2, 1, 2, 2, 2, 1]`), which must sum to one octave.
    pub fn from_steps(tuning: Tuning, root: u8, steps: &[u8]) -> Option<Scale> {
        Scale::new(
            root,
            set_from_steps(tuning, root % tuning.divisions(), steps)?,
        )
    }

    /// A named 12-EDO scale on a spelled root.
    pub fn preset(kind: ScaleKind, root: Spelling) -> Scale {
        let set = kind.pc_set().transpose(root.pc() as i32);
        Scale {
            root: root.pc(),
            set,
            root_spelling: Some(root),
        }
    }

    /// Parses "D dorian", "Bb major", "F# harmonic minor" (12-EDO).
    pub fn parse(s: &str) -> Option<Scale> {
        let (root, rest) = Spelling::parse_prefix(s.trim())?;
        let rest = rest.trim();
        let kind = if rest.is_empty() {
            ScaleKind::Major
        } else {
            ScaleKind::from_name(rest)?
        };
        Some(Scale::preset(kind, root))
    }

    pub fn tuning(&self) -> Tuning {
        self.set.tuning()
    }

    /// Root pitch class.
    pub fn root(&self) -> u8 {
        self.root
    }

    pub fn root_spelling(&self) -> Option<Spelling> {
        self.root_spelling
    }

    /// Replaces the root's spelling (12-EDO only; must match the root).
    pub fn with_root_spelling(mut self, spelling: Spelling) -> Scale {
        if self.tuning().is_twelve() && spelling.pc() == self.root {
            self.root_spelling = Some(spelling);
        }
        self
    }

    pub fn pc_set(&self) -> PcSet {
        self.set
    }

    /// Number of notes per octave.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Distances of each degree above the root, ascending (first is 0).
    pub fn degree_offsets(&self) -> FixedVec<u8, 64> {
        let n = self.tuning().divisions();
        let mut out = FixedVec::new();
        for pc in self.set.iter_from(self.root) {
            let _ = out.push((pc + n - self.root) % n);
        }
        out
    }

    /// The pitch of a degree, counted from the root in `octave` (4 = middle-C octave).
    pub fn pitch(&self, degree: i32, octave: i32) -> Pitch {
        let offsets = self.degree_offsets();
        let n = offsets.len() as i32;
        let wraps = degree.div_euclid(n);
        let idx = degree.rem_euclid(n) as usize;
        let root = Pitch::from_octave(self.tuning(), octave + wraps, self.root);
        let p = root.offset(offsets[idx] as i32);
        p.with_spelling(self.spell_pc(p.pitch_class()))
    }

    /// For a pitch in the scale, its degree (0-based) and the octave of the root below it.
    pub fn degree_of(&self, pitch: Pitch) -> Option<(usize, i32)> {
        if pitch.tuning() != self.tuning() || !self.set.contains(pitch.step()) {
            return None;
        }
        let n = self.tuning().steps_per_octave();
        let from_root = pitch.step() - self.root as i32;
        let octave = from_root.div_euclid(n) - 1;
        let offset = from_root.rem_euclid(n) as u8;
        let idx = self.degree_offsets().iter().position(|&o| o == offset)?;
        Some((idx, octave))
    }

    pub fn contains(&self, pitch: Pitch) -> bool {
        pitch.tuning() == self.tuning() && self.set.contains(pitch.step())
    }

    /// Moves a pitch by `steps` scale degrees (a "diatonic transpose").
    /// Returns `None` if the pitch is not in the scale.
    pub fn transpose_diatonic(&self, pitch: Pitch, steps: i32) -> Option<Pitch> {
        let (idx, octave) = self.degree_of(pitch)?;
        Some(self.pitch(idx as i32 + steps, octave))
    }

    /// Snaps a pitch to the scale.
    pub fn quantize(&self, pitch: Pitch, rounding: Rounding) -> Pitch {
        assert_eq!(
            pitch.tuning(),
            self.tuning(),
            "pitch and scale in different tunings"
        );
        let spelled = |p: Pitch| p.with_spelling(self.spell_pc(p.pitch_class()));
        if self.set.contains(pitch.step()) {
            return spelled(pitch);
        }
        for d in 1..=self.tuning().steps_per_octave() {
            let down = pitch.offset(-d);
            let up = pitch.offset(d);
            match rounding {
                Rounding::Down if self.set.contains(down.step()) => return spelled(down),
                Rounding::Up if self.set.contains(up.step()) => return spelled(up),
                Rounding::Nearest if self.set.contains(down.step()) => return spelled(down),
                Rounding::Nearest if self.set.contains(up.step()) => return spelled(up),
                _ => {}
            }
        }
        pitch // empty scale
    }

    /// The scale starting on another of its degrees (degree 1 of C major gives D Dorian).
    pub fn mode(&self, degree: i32) -> Scale {
        let offsets = self.degree_offsets();
        let idx = degree.rem_euclid(offsets.len() as i32) as usize;
        let n = self.tuning().divisions();
        let root = (self.root + offsets[idx]) % n;
        Scale {
            root,
            set: self.set,
            root_spelling: self.spell_pc(root),
        }
    }

    /// The scale moved by an interval.
    pub fn transpose(&self, interval: Interval) -> Scale {
        assert_eq!(
            interval.tuning,
            self.tuning(),
            "interval and scale in different tunings"
        );
        let root =
            (self.root as i32 + interval.steps).rem_euclid(self.tuning().steps_per_octave()) as u8;
        let mut scale = Scale {
            root,
            set: self.set.transpose(interval.steps),
            root_spelling: None,
        };
        scale.root_spelling = scale.best_root_spelling();
        scale
    }

    /// A chord stacked in scale thirds on a degree: `size` 3 gives triads, 4 sevenths.
    pub fn chord_on_degree(&self, degree: i32, size: usize, octave: i32) -> Chord {
        let mut chord = Chord::empty(self.tuning());
        for k in 0..size {
            let _ = chord.add(self.pitch(degree + 2 * k as i32, octave));
        }
        chord.with_root(self.pitch(degree, octave).pitch_class())
    }

    /// The named kind, if this 12-EDO scale matches one.
    pub fn kind(&self) -> Option<ScaleKind> {
        if !self.tuning().is_twelve() {
            return None;
        }
        let normalized = self.set.transpose(-(self.root as i32));
        ScaleKind::all().find(|k| k.pc_set() == normalized)
    }

    /// Letter spelling for a pitch class in this scale's context (12-EDO only).
    /// Seven-note scales get one letter per degree, so F major spells Bb, not A#.
    pub fn spell_pc(&self, pc: u8) -> Option<Spelling> {
        if !self.tuning().is_twelve() {
            return None;
        }
        let root = self.root_spelling?;
        if self.len() == 7 {
            let n = self.tuning().divisions();
            let offset = (pc + n - self.root) % n;
            if let Some(idx) = self.degree_offsets().iter().position(|&o| o == offset) {
                return Spelling::with_letter(root.letter.up(idx as i32), pc);
            }
        }
        let flat_side = root.accidental < 0 || (root.letter == Letter::F && root.accidental == 0);
        Some(if flat_side {
            Spelling::flat(pc)
        } else {
            Spelling::sharp(pc)
        })
    }

    /// Picks the root spelling that gives the fewest accidentals (Db major, not C# major).
    fn best_root_spelling(&self) -> Option<Spelling> {
        if !self.tuning().is_twelve() {
            return None;
        }
        let default = Spelling::key_default(self.root);
        if self.len() != 7 {
            return Some(default);
        }
        let mut best = default;
        let mut best_cost = self.accidental_cost(default);
        for candidate in [Spelling::sharp(self.root), Spelling::flat(self.root)] {
            let cost = self.accidental_cost(candidate);
            if cost < best_cost {
                best = candidate;
                best_cost = cost;
            }
        }
        Some(best)
    }

    fn accidental_cost(&self, root: Spelling) -> i32 {
        let trial = Scale {
            root_spelling: Some(root),
            ..*self
        };
        self.degree_offsets()
            .iter()
            .map(|&o| match trial.spell_pc((self.root + o) % 12) {
                Some(sp) => sp.accidental.unsigned_abs() as i32,
                None => 100,
            })
            .sum()
    }
}

impl fmt::Display for Scale {
    /// "D Dorian", or the root and the step pattern for unnamed scales.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.root_spelling {
            Some(sp) => write!(f, "{sp}")?,
            None => write!(f, "{}\\{}", self.root, self.tuning().divisions())?,
        }
        if let Some(kind) = self.kind() {
            return write!(f, " {}", kind.name());
        }
        f.write_str(" [")?;
        let offsets = self.degree_offsets();
        let n = self.tuning().divisions();
        for (i, &o) in offsets.iter().enumerate() {
            let next = offsets.get(i + 1).copied().unwrap_or(n);
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{}", next - o)?;
        }
        f.write_str("]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(scale: &Scale, octave: i32) -> std::string::String {
        (0..scale.len() as i32)
            .map(|d| scale.pitch(d, octave).to_string())
            .collect::<std::vec::Vec<_>>()
            .join(" ")
    }

    #[test]
    fn spelled_major_scales() {
        assert_eq!(
            names(&Scale::parse("C major").unwrap(), 4),
            "C4 D4 E4 F4 G4 A4 B4"
        );
        assert_eq!(
            names(&Scale::parse("F major").unwrap(), 4),
            "F4 G4 A4 Bb4 C5 D5 E5"
        );
        assert_eq!(
            names(&Scale::parse("F# major").unwrap(), 4),
            "F#4 G#4 A#4 B4 C#5 D#5 E#5"
        );
        assert_eq!(
            names(&Scale::parse("Gb major").unwrap(), 4),
            "Gb4 Ab4 Bb4 Cb5 Db5 Eb5 F5"
        );
        assert_eq!(
            names(&Scale::parse("D harmonic minor").unwrap(), 4),
            "D4 E4 F4 G4 A4 Bb4 C#5"
        );
    }

    #[test]
    fn unspelled_root_picks_fewest_accidentals() {
        let major = Scale::from_steps(Tuning::TWELVE, 1, &[2, 2, 1, 2, 2, 2, 1]).unwrap();
        assert_eq!(major.to_string(), "Db Major"); // not C# (7 sharps)
        let minor = Scale::from_steps(Tuning::TWELVE, 1, &[2, 1, 2, 2, 1, 2, 2]).unwrap();
        assert_eq!(minor.to_string(), "C# Minor"); // not Db minor (8 flats)
    }

    #[test]
    fn modes_and_kinds() {
        let c = Scale::parse("C major").unwrap();
        assert_eq!(c.mode(1).to_string(), "D Dorian");
        assert_eq!(c.mode(4).to_string(), "G Mixolydian");
        assert_eq!(c.mode(6).to_string(), "B Locrian");
        assert_eq!(
            Scale::parse("A aeolian").unwrap().kind(),
            Some(ScaleKind::NaturalMinor)
        );
    }

    #[test]
    fn degrees_wrap_across_octaves() {
        let c = Scale::parse("C major").unwrap();
        assert_eq!(c.pitch(7, 4).to_string(), "C5");
        assert_eq!(c.pitch(-1, 4).to_string(), "B3");
        assert_eq!(c.degree_of(Pitch::parse("E5").unwrap()), Some((2, 5)));
        assert_eq!(c.degree_of(Pitch::parse("Eb5").unwrap()), None);
        let up_a_sixth = c
            .transpose_diatonic(Pitch::parse("G4").unwrap(), 5)
            .unwrap();
        assert_eq!(up_a_sixth.to_string(), "E5");
    }

    #[test]
    fn quantize() {
        let c = Scale::parse("C major pentatonic").unwrap();
        let f4 = Pitch::parse("F4").unwrap();
        assert_eq!(c.quantize(f4, Rounding::Nearest).to_string(), "E4");
        assert_eq!(c.quantize(f4, Rounding::Up).to_string(), "G4");
        assert_eq!(c.quantize(f4, Rounding::Down).to_string(), "E4");
        let bb = Scale::parse("Bb major").unwrap();
        assert_eq!(
            bb.quantize(Pitch::midi(63), Rounding::Nearest).to_string(),
            "Eb4"
        );
    }

    #[test]
    fn diatonic_chords() {
        let c = Scale::parse("C major").unwrap();
        assert_eq!(c.chord_on_degree(1, 4, 4).to_string(), "Dm7");
        assert_eq!(c.chord_on_degree(4, 4, 3).to_string(), "G7");
        assert_eq!(c.chord_on_degree(6, 3, 3).to_string(), "Bdim");
        let a = Scale::parse("A harmonic minor").unwrap();
        assert_eq!(a.chord_on_degree(4, 4, 3).to_string(), "E7");
    }

    #[test]
    fn other_edos() {
        let t19 = Tuning::edo(19).unwrap();
        // 19-EDO diatonic major: whole tone 3, semitone 2.
        let major = Scale::from_steps(t19, 0, &[3, 3, 2, 3, 3, 3, 2]).unwrap();
        assert_eq!(major.len(), 7);
        assert_eq!(major.pitch(7, 4).step(), major.pitch(0, 4).step() + 19);
        assert!(major.root_spelling().is_none());
        assert_eq!(major.to_string(), "0\\19 [3 3 2 3 3 3 2]");
        assert!(Scale::from_steps(t19, 0, &[2, 2, 1, 2, 2, 2, 1]).is_none());
    }

    #[test]
    fn interval_transpose_respells() {
        let c = Scale::parse("C major").unwrap();
        assert_eq!(c.transpose(Interval::semitones(3)).to_string(), "Eb Major");
        assert_eq!(c.transpose(Interval::semitones(6)).to_string(), "F# Major");
    }
}
