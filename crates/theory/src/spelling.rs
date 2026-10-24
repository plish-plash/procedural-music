//! Letter-name spelling for 12-EDO (C# vs. Db).

use core::fmt;

/// A note letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum Letter {
    #[default]
    C,
    D,
    E,
    F,
    G,
    A,
    B,
}

const LETTERS: [Letter; 7] = [
    Letter::C,
    Letter::D,
    Letter::E,
    Letter::F,
    Letter::G,
    Letter::A,
    Letter::B,
];
const NATURAL_PC: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

impl Letter {
    /// Position in C D E F G A B (0 to 6).
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The letter at a position, wrapping in both directions.
    pub fn from_index(index: i32) -> Letter {
        LETTERS[index.rem_euclid(7) as usize]
    }

    /// The pitch class of the natural note (C = 0 … B = 11).
    pub const fn natural_pc(self) -> i32 {
        NATURAL_PC[self as usize]
    }

    /// The letter `n` positions up (negative = down).
    pub fn up(self, n: i32) -> Letter {
        Letter::from_index(self.index() as i32 + n)
    }

    /// Parses an upper- or lower-case letter.
    pub fn from_char(c: char) -> Option<Letter> {
        match c.to_ascii_uppercase() {
            'C' => Some(Letter::C),
            'D' => Some(Letter::D),
            'E' => Some(Letter::E),
            'F' => Some(Letter::F),
            'G' => Some(Letter::G),
            'A' => Some(Letter::A),
            'B' => Some(Letter::B),
            _ => None,
        }
    }

    /// The upper-case character.
    pub const fn as_char(self) -> char {
        match self {
            Letter::C => 'C',
            Letter::D => 'D',
            Letter::E => 'E',
            Letter::F => 'F',
            Letter::G => 'G',
            Letter::A => 'A',
            Letter::B => 'B',
        }
    }
}

/// A spelled 12-EDO pitch class: a letter plus sharps (positive) or flats (negative).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Spelling {
    pub letter: Letter,
    pub accidental: i8,
}

const SHARP_NAMES: [(Letter, i8); 12] = [
    (Letter::C, 0),
    (Letter::C, 1),
    (Letter::D, 0),
    (Letter::D, 1),
    (Letter::E, 0),
    (Letter::F, 0),
    (Letter::F, 1),
    (Letter::G, 0),
    (Letter::G, 1),
    (Letter::A, 0),
    (Letter::A, 1),
    (Letter::B, 0),
];

const FLAT_NAMES: [(Letter, i8); 12] = [
    (Letter::C, 0),
    (Letter::D, -1),
    (Letter::D, 0),
    (Letter::E, -1),
    (Letter::E, 0),
    (Letter::F, 0),
    (Letter::G, -1),
    (Letter::G, 0),
    (Letter::A, -1),
    (Letter::A, 0),
    (Letter::B, -1),
    (Letter::B, 0),
];

impl Spelling {
    pub const fn new(letter: Letter, accidental: i8) -> Spelling {
        Spelling { letter, accidental }
    }

    /// The pitch class (0 to 11).
    pub fn pc(self) -> u8 {
        (self.letter.natural_pc() + self.accidental as i32).rem_euclid(12) as u8
    }

    /// Spelling of a pitch class using sharps (C, C#, D, …).
    pub fn sharp(pc: u8) -> Spelling {
        let (letter, accidental) = SHARP_NAMES[(pc % 12) as usize];
        Spelling { letter, accidental }
    }

    /// Spelling of a pitch class using flats (C, Db, D, …).
    pub fn flat(pc: u8) -> Spelling {
        let (letter, accidental) = FLAT_NAMES[(pc % 12) as usize];
        Spelling { letter, accidental }
    }

    /// The usual spelling of a key root: Db, Eb, F#, Ab, Bb for the black keys.
    pub fn key_default(pc: u8) -> Spelling {
        match pc % 12 {
            1 | 3 | 8 | 10 => Spelling::flat(pc),
            _ => Spelling::sharp(pc),
        }
    }

    /// Spelling of `pc` using the given letter, or `None` if that needs more
    /// than two sharps or flats.
    pub fn with_letter(letter: Letter, pc: u8) -> Option<Spelling> {
        let diff = (pc as i32 - letter.natural_pc()).rem_euclid(12);
        let accidental = if diff > 6 { diff - 12 } else { diff };
        if accidental.abs() > 2 {
            None
        } else {
            Some(Spelling {
                letter,
                accidental: accidental as i8,
            })
        }
    }

    /// The other common spelling (C# ↔ Db), if there is one with at most one accidental.
    pub fn enharmonic(self) -> Option<Spelling> {
        let pc = self.pc();
        let other = if self.accidental > 0 {
            Spelling::flat(pc)
        } else if self.accidental < 0 {
            Spelling::sharp(pc)
        } else {
            return None;
        };
        if other == self || other.accidental == 0 {
            None
        } else {
            Some(other)
        }
    }

    /// Parses a spelling at the start of `s` (e.g. "F#", "Bb", "Ebb") and returns
    /// it with the rest of the string. Accepts `#`, `b`, `♯`, `♭` and `x` (double sharp).
    pub fn parse_prefix(s: &str) -> Option<(Spelling, &str)> {
        let mut chars = s.char_indices();
        let (_, first) = chars.next()?;
        let letter = Letter::from_char(first)?;
        let mut accidental: i8 = 0;
        let mut end = first.len_utf8();
        for (i, c) in chars {
            let delta = match c {
                '#' | '♯' => 1,
                'b' | '♭' => -1,
                'x' => 2,
                _ => break,
            };
            accidental = accidental.checked_add(delta)?;
            end = i + c.len_utf8();
        }
        Some((Spelling { letter, accidental }, &s[end..]))
    }

    /// Parses a whole string as a spelling.
    pub fn parse(s: &str) -> Option<Spelling> {
        match Spelling::parse_prefix(s.trim())? {
            (sp, "") => Some(sp),
            _ => None,
        }
    }
}

impl fmt::Display for Spelling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.letter.as_char())?;
        let symbol = if self.accidental > 0 { '#' } else { 'b' };
        for _ in 0..self.accidental.unsigned_abs() {
            write!(f, "{symbol}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_pc() {
        assert_eq!(Spelling::parse("F#").unwrap().pc(), 6);
        assert_eq!(Spelling::parse("Gb").unwrap().pc(), 6);
        assert_eq!(Spelling::parse("Cb").unwrap().pc(), 11);
        assert_eq!(Spelling::parse("B#").unwrap().pc(), 0);
        assert_eq!(Spelling::parse("Ebb").unwrap().pc(), 2);
        assert_eq!(Spelling::parse("Fx").unwrap().pc(), 7);
        assert!(Spelling::parse("H").is_none());
        assert!(Spelling::parse("C#m").is_none());
        let (sp, rest) = Spelling::parse_prefix("Bbm7").unwrap();
        assert_eq!((sp.to_string().as_str(), rest), ("Bb", "m7"));
    }

    #[test]
    fn with_letter() {
        assert_eq!(
            Spelling::with_letter(Letter::E, 3).unwrap().to_string(),
            "Eb"
        );
        assert_eq!(
            Spelling::with_letter(Letter::B, 0).unwrap().to_string(),
            "B#"
        );
        assert!(Spelling::with_letter(Letter::C, 6).is_none());
    }

    #[test]
    fn enharmonics() {
        assert_eq!(
            Spelling::parse("C#").unwrap().enharmonic(),
            Spelling::parse("Db")
        );
        assert_eq!(Spelling::parse("C").unwrap().enharmonic(), None);
    }
}
