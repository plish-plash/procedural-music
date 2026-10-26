//! Harmony in a key: extending chords, substitutions, modal borrowing and moving chords
//! by scale steps. Chord qualities are 12-EDO; chords in other tunings come back unchanged
//! where a quality is needed.

use crate::chord::{Chord, ChordQuality, QUALITIES};
use crate::pitch::{Interval, Pitch};
use crate::scale::{Rounding, Scale};
use crate::spelling::Spelling;

/// The chord's root pitch class, or else its bass.
fn root_pc(chord: &Chord) -> Option<u8> {
    chord.root().or_else(|| chord.bass().map(|p| p.pitch_class()))
}

/// The lowest note of the chord's root pitch class, or else the bass.
fn root_note(chord: &Chord) -> Option<Pitch> {
    let pc = root_pc(chord)?;
    chord
        .notes()
        .iter()
        .find(|p| p.pitch_class() == pc)
        .copied()
        .or_else(|| chord.bass())
}

/// A chord quality by its canonical suffix ("m7").
pub fn quality(suffix: &str) -> &'static ChordQuality {
    QUALITIES
        .iter()
        .find(|q| q.suffix == suffix)
        .expect("known chord quality")
}

/// Whether the chord (relative to its root) has `semitones` among its pitch classes.
fn has(chord: &Chord, root: u8, semitones: i32) -> bool {
    chord.contains_pc(root as i32 + semitones)
}

/// Adds the key's thirds above the chord's root until it has `size` tones: 4 gives a 7th
/// chord, 5 a 9th, 6 an 11th, 7 a 13th. Existing notes are kept and new ones are placed
/// above the root. Chords whose root isn't in the key come back unchanged.
pub fn extend(chord: &Chord, key: &Scale, size: usize) -> Chord {
    let Some(root) = root_note(chord) else {
        return *chord;
    };
    let Some((degree, octave)) = key.degree_of(root) else {
        return *chord;
    };
    let mut out = *chord;
    for k in 3..size {
        let p = key.pitch(degree as i32 + 2 * k as i32, octave);
        if !out.contains_pc(p.step()) {
            let _ = out.add(p);
        }
    }
    out
}

/// The tritone substitute of a dominant chord (one with a major third and a minor
/// seventh): the same chord a tritone lower. Other chords come back unchanged.
pub fn tritone_sub(chord: &Chord) -> Chord {
    match root_pc(chord) {
        Some(r) if chord.tuning().is_twelve() && has(chord, r, 4) && has(chord, r, 10) => {
            chord.transpose(Interval::semitones(-6))
        }
        _ => *chord,
    }
}

/// A root-position chord of `suffix` quality on pitch class `pc`, its root as close as
/// possible to `near`.
fn chord_on(pc: u8, suffix: &str, near: Pitch) -> Chord {
    let spelling = Spelling::key_default(pc);
    let mut root = Pitch::spelled(spelling, near.octave());
    if root.step() - near.step() > 6 {
        root = Pitch::spelled(spelling, near.octave() - 1);
    } else if near.step() - root.step() > 6 {
        root = Pitch::spelled(spelling, near.octave() + 1);
    }
    Chord::from_quality(root, quality(suffix))
}

/// The relative chord: a major chord becomes its relative minor (a minor third down) and a
/// minor chord its relative major (a minor third up). Sevenths become the matching seventh
/// (maj7 ↔ m7). Chords that are neither come back unchanged.
pub fn relative(chord: &Chord) -> Chord {
    let (Some(r), Some(at)) = (root_pc(chord), root_note(chord)) else {
        return *chord;
    };
    if !chord.tuning().is_twelve() {
        return *chord;
    }
    let seventh = has(chord, r, 10) || has(chord, r, 11);
    if has(chord, r, 4) {
        chord_on((r + 9) % 12, if seventh { "m7" } else { "m" }, at.offset(-3))
    } else if has(chord, r, 3) {
        chord_on((r + 3) % 12, if seventh { "maj7" } else { "" }, at.offset(3))
    } else {
        *chord
    }
}

/// The parallel chord: the third flips between major and minor, every other note stays.
pub fn parallel(chord: &Chord) -> Chord {
    let Some(r) = root_pc(chord) else {
        return *chord;
    };
    if !chord.tuning().is_twelve() {
        return *chord;
    }
    let (major, minor) = (has(chord, r, 4), has(chord, r, 3));
    if major == minor {
        return *chord;
    }
    let from = if major { 4 } else { 3 };
    let shift = if major { -1 } else { 1 };
    let mut out = Chord::empty(chord.tuning());
    for &p in chord.notes() {
        let interval = (p.pitch_class() as i32 - r as i32).rem_euclid(12);
        let _ = out.add(if interval == from { p.offset(shift) } else { p });
    }
    out.with_root(r)
}

/// The chord rebuilt in another scale on the same root (modal mixture): every note that is a
/// degree of `key` becomes the same degree of `from`, so IV in C major borrowed from C minor
/// is iv (F Ab C). Notes outside the key stay. The two scales must have the same size.
pub fn borrow(chord: &Chord, key: &Scale, from: &Scale) -> Chord {
    if key.len() != from.len() || key.tuning() != from.tuning() {
        return *chord;
    }
    let map = |p: Pitch| match key.degree_of(p) {
        Some((degree, octave)) => from.pitch(degree as i32, octave),
        None => p,
    };
    let mut out = Chord::empty(chord.tuning());
    for &p in chord.notes() {
        let _ = out.add(map(p));
    }
    match root_note(chord) {
        Some(root) => out.with_root(map(root).pitch_class()),
        None => out,
    }
}

/// Every note moved by `steps` degrees of the key. A note outside the key moves as far as
/// the key note just below it does, so chromatic tones keep their place.
pub fn transpose_in_key(chord: &Chord, key: &Scale, steps: i32) -> Chord {
    if key.tuning() != chord.tuning() || key.is_empty() {
        return *chord;
    }
    let shift = |p: Pitch| match key.transpose_diatonic(p, steps) {
        Some(q) => q,
        None => {
            let below = key.quantize(p, Rounding::Down);
            let moved = key.transpose_diatonic(below, steps).unwrap_or(below);
            p.offset(moved.step() - below.step())
        }
    };
    let mut out = Chord::empty(chord.tuning());
    for &p in chord.notes() {
        let _ = out.add(shift(p));
    }
    match root_note(chord) {
        Some(root) => out.with_root(shift(root).pitch_class()),
        None => out,
    }
}

/// The dominant seventh that resolves to a chord rooted on `target` (V7 of it), in the
/// octave of `near`.
pub fn dominant_of(target: u8, near: Pitch) -> Chord {
    chord_on((target + 7) % 12, "7", near)
}

/// The degree (0-based) of the chord's root in the key, if the root is in it.
pub fn root_degree(chord: &Chord, key: &Scale) -> Option<usize> {
    let root = root_note(chord)?;
    key.degree_of(root).map(|(d, _)| d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(c: &Chord) -> String {
        c.name().map(|n| n.to_string()).unwrap_or_default()
    }

    #[test]
    fn extends_in_the_key() {
        let key = Scale::parse("C major").unwrap();
        let c = Chord::from_symbol("C", 3).unwrap();
        assert_eq!(name(&extend(&c, &key, 4)), "Cmaj7");
        assert_eq!(name(&extend(&c, &key, 5)), "Cmaj9");
        let g = Chord::from_symbol("G", 3).unwrap();
        assert_eq!(name(&extend(&g, &key, 4)), "G7");
        let d = Chord::from_symbol("Dm", 3).unwrap();
        assert_eq!(name(&extend(&d, &key, 4)), "Dm7");
        // Root outside the key: unchanged.
        let eb = Chord::from_symbol("Eb", 3).unwrap();
        assert_eq!(extend(&eb, &key, 4), eb);
    }

    #[test]
    fn substitutions() {
        let g7 = Chord::from_symbol("G7", 3).unwrap();
        assert_eq!(name(&tritone_sub(&g7)), "Db7");
        let c = Chord::from_symbol("C", 4).unwrap();
        assert_eq!(tritone_sub(&c), c);
        assert_eq!(name(&relative(&c)), "Am");
        assert_eq!(name(&relative(&Chord::from_symbol("Am7", 4).unwrap())), "Cmaj7");
        assert_eq!(name(&parallel(&c)), "Cm");
        assert_eq!(name(&parallel(&Chord::from_symbol("Dm7", 4).unwrap())), "D7");
    }

    #[test]
    fn borrows_from_the_parallel_minor() {
        let key = Scale::parse("C major").unwrap();
        let minor = Scale::parse("C minor").unwrap();
        let f = Chord::from_symbol("F", 3).unwrap();
        assert_eq!(name(&borrow(&f, &key, &minor)), "Fm");
        let a = Chord::from_symbol("Am", 3).unwrap();
        assert_eq!(name(&borrow(&a, &key, &minor)), "Ab");
    }

    #[test]
    fn moves_by_scale_steps() {
        let key = Scale::parse("C major").unwrap();
        let c = Chord::from_symbol("C", 4).unwrap();
        assert_eq!(name(&transpose_in_key(&c, &key, 1)), "Dm");
        assert_eq!(name(&transpose_in_key(&c, &key, 4)), "G");
        // A chromatic note follows the key note below it.
        let d7 = Chord::from_symbol("D7", 4).unwrap();
        assert_eq!(name(&transpose_in_key(&d7, &key, 1)), "E7");
    }

    #[test]
    fn secondary_dominants() {
        let near = Pitch::parse("G3").unwrap();
        assert_eq!(name(&dominant_of(7, near)), "D7");
        assert_eq!(name(&dominant_of(9, near)), "E7");
        let key = Scale::parse("C major").unwrap();
        assert_eq!(root_degree(&Chord::from_symbol("G", 3).unwrap(), &key), Some(4));
    }
}
