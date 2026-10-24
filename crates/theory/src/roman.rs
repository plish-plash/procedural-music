//! Roman-numeral chord symbols relative to a key.

use crate::chord::{find_quality, Chord};
use crate::scale::{Scale, ScaleKind};
use crate::spelling::Spelling;

const NUMERALS: [(&str, i32); 7] = [
    ("vii", 6),
    ("vi", 5),
    ("iv", 3),
    ("v", 4),
    ("iii", 2),
    ("ii", 1),
    ("i", 0),
];

/// Parses a Roman numeral in a seven-note 12-EDO key, with the root in `octave`.
///
/// - Case sets the triad: `IV` major, `ii` minor.
/// - A leading `b` or `#` alters the root: `bVII`, `#iv`.
/// - Suffixes: `o` diminished, `+` augmented, `ø7` half-diminished, `7`, `maj7`, `9`.
///   Upper-case `7` is a dominant seventh; lower-case `7` a minor seventh.
/// - Secondary chords: `V/V`, `viio7/ii` (the target is treated as a major key).
///
/// Qualities come from the numeral as written, not from the key, so `ii` in a minor key
/// is minor unless written `iiø7` or `iio`.
pub fn parse_roman(numeral: &str, key: &Scale, octave: i32) -> Option<Chord> {
    if !key.tuning().is_twelve() || key.len() != 7 {
        return None;
    }
    let numeral = numeral.trim();
    if let Some((chord, target)) = numeral.split_once('/') {
        let target_chord = parse_roman(target, key, octave)?;
        let target_root = target_chord.root()?;
        let spelling = target_chord
            .notes()
            .iter()
            .filter_map(|p| p.spelling())
            .find(|sp| sp.pc() == target_root)
            .unwrap_or_else(|| Spelling::key_default(target_root));
        let local_key = Scale::preset(ScaleKind::Major, spelling);
        return parse_roman(chord, &local_key, octave);
    }

    // Accidentals.
    let mut rest = numeral;
    let mut accidental = 0;
    loop {
        if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
            accidental -= 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
            accidental += 1;
            rest = r;
        } else {
            break;
        }
    }

    // Numeral (longest match first; case decides major/minor).
    let (len, degree) = NUMERALS.iter().find_map(|&(text, degree)| {
        let head = rest.get(..text.len())?;
        head.eq_ignore_ascii_case(text)
            .then_some((text.len(), degree))
    })?;
    let head = &rest[..len];
    let upper = head.chars().all(|c| c.is_ascii_uppercase());
    let lower = head.chars().all(|c| c.is_ascii_lowercase());
    if !upper && !lower {
        return None;
    }
    let suffix = &rest[len..];

    let quality = match (upper, suffix) {
        (true, "") => "",
        (true, "+") => "aug",
        (true, "7") => "7",
        (true, "+7") => "aug7",
        (true, "maj7" | "M7") => "maj7",
        (true, "9") => "9",
        (true, "maj9" | "M9") => "maj9",
        (true, "sus4" | "sus") => "sus4",
        (true, "7sus4") => "7sus4",
        (true, "6") => "6",
        (false, "") => "m",
        (false, "o") => "dim",
        (false, "o7") => "dim7",
        (false, "ø7" | "ø" | "%7") => "m7b5",
        (false, "7") => "m7",
        (false, "maj7" | "M7") => "mMaj7",
        (false, "9") => "m9",
        (false, "6") => "m6",
        _ => return None,
    };
    let quality = find_quality(quality)?;

    let natural = key.pitch(degree, octave);
    let root = natural.offset(accidental);
    let spelling = natural
        .spelling()
        .and_then(|sp| Spelling::with_letter(sp.letter, root.pitch_class()));
    Some(Chord::from_quality(root.with_spelling(spelling), quality))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    fn name(numeral: &str, key: &str) -> std::string::String {
        let key = Scale::parse(key).unwrap();
        match parse_roman(numeral, &key, 4) {
            Some(chord) => format!("{chord}"),
            None => "none".into(),
        }
    }

    #[test]
    fn diatonic_in_major() {
        let got: std::vec::Vec<_> = ["I", "ii", "iii", "IV", "V7", "vi", "viio"]
            .iter()
            .map(|n| name(n, "C major"))
            .collect();
        assert_eq!(got, ["C", "Dm", "Em", "F", "G7", "Am", "Bdim"]);
        assert_eq!(name("ii7", "Bb major"), "Cm7");
        assert_eq!(name("IVmaj7", "Eb major"), "Abmaj7");
    }

    #[test]
    fn minor_keys_and_borrowed_chords() {
        assert_eq!(name("i", "A minor"), "Am");
        assert_eq!(name("iiø7", "A minor"), "Bm7b5");
        assert_eq!(name("III", "A minor"), "C");
        assert_eq!(name("V7", "A minor"), "E7");
        assert_eq!(name("bVII", "C major"), "Bb");
        assert_eq!(name("bVI", "C major"), "Ab");
        assert_eq!(name("#ivø7", "C major"), "F#m7b5");
    }

    #[test]
    fn secondary_dominants() {
        assert_eq!(name("V/V", "C major"), "D");
        assert_eq!(name("V7/ii", "C major"), "A7");
        assert_eq!(name("viio7/V", "C major"), "F#dim7");
        assert_eq!(name("V7/IV", "F major"), "F7");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(name("Vx", "C major"), "none");
        assert_eq!(name("Iv", "C major"), "none");
        assert_eq!(name("", "C major"), "none");
    }
}
