//! Voicings: placing a chord's pitch classes into actual octaves.

use crate::chord::Chord;
use crate::fixed::FixedVec;
use crate::pitch::Pitch;

/// The chord's pitch classes stacked upward from its root (or lowest note),
/// starting at the first root at or above `floor`.
pub fn close(chord: &Chord, floor: Pitch) -> Chord {
    let pcs = chord.pitch_classes();
    if pcs.is_empty() {
        return Chord::empty(chord.tuning());
    }
    let root = chord
        .root()
        .or_else(|| chord.bass().map(|p| p.pitch_class()))
        .expect("non-empty chord");
    let mut out = Chord::empty(chord.tuning());
    let mut prev: Option<Pitch> = None;
    for pc in pcs.iter_from(root) {
        let lower_bound = match prev {
            Some(p) => p.offset(1),
            None => floor,
        };
        let p = first_at_or_above(lower_bound, pc);
        let _ = out.add(p.with_spelling(spelling_for(chord, pc)));
        prev = Some(p);
    }
    out.with_root(root)
}

/// Drop 2: the second-highest note moves down an octave.
pub fn drop2(chord: &Chord) -> Chord {
    drop_nth(chord, 2)
}

/// Drop 3: the third-highest note moves down an octave.
pub fn drop3(chord: &Chord) -> Chord {
    drop_nth(chord, 3)
}

fn drop_nth(chord: &Chord, nth: usize) -> Chord {
    let notes = chord.notes();
    if notes.len() < nth {
        return *chord;
    }
    let idx = notes.len() - nth;
    let octave = chord.tuning().steps_per_octave();
    let mut out = Chord::empty(chord.tuning());
    for (i, &p) in notes.iter().enumerate() {
        let moved = if i == idx {
            p.offset(-octave).with_spelling(p.spelling())
        } else {
            p
        };
        let _ = out.add(moved);
    }
    match chord.root() {
        Some(r) => out.with_root(r),
        None => out,
    }
}

/// Voices `target` with the same number of notes as `prev`, moving each voice as little as
/// possible, within `[low, high]`.
///
/// The search prefers, in order of cost: small total motion, covering as many of the
/// target's pitch classes as there are voices, no crossed or doubled voices, and the
/// root in the bass. With no previous chord it returns a close voicing above `low`.
/// Up to 6 voices are searched exhaustively; larger chords move each voice greedily.
pub fn lead(prev: &Chord, target: &Chord, low: Pitch, high: Pitch) -> Chord {
    let tuning = target.tuning();
    let octave = tuning.steps_per_octave();
    let target_pcs: FixedVec<u8, 64> = collect(target.pitch_classes().iter());
    if prev.is_empty() || target_pcs.is_empty() {
        return close(target, low);
    }
    let voices = prev.notes();
    let n = voices.len();
    let k = target_pcs.len();

    // Best pitch for each (voice, target pc), or None if no such pitch is in range.
    let mut options = [[None::<Pitch>; 64]; crate::MAX_CHORD_NOTES];
    for (v, &from) in voices.iter().enumerate() {
        for (j, &pc) in target_pcs.iter().enumerate() {
            options[v][j] = nearest_in_range(from, pc, low, high);
        }
    }

    let cost_of = |choice: &[usize]| -> Option<i32> {
        let mut pitches: FixedVec<Pitch, { crate::MAX_CHORD_NOTES }> = FixedVec::new();
        let mut motion = 0;
        for (v, &j) in choice.iter().enumerate() {
            let p = options[v][j]?;
            motion += (p.step() - voices[v].step()).abs();
            let _ = pitches.push(p);
        }
        let mut cost = motion;
        // Crossed or doubled voices (previous voices are in ascending order).
        for pair in pitches.windows(2) {
            if pair[0] >= pair[1] {
                cost += octave;
            }
        }
        // Coverage of the target's pitch classes.
        let mut used = 0u64;
        for &j in choice {
            used |= 1 << j;
        }
        let missing = n.min(k) as i32 - used.count_ones() as i32;
        cost += missing.max(0) * octave;
        // Root in the bass.
        if let (Some(root), Some(bass)) = (target.root(), pitches.iter().min()) {
            if bass.pitch_class() != root {
                cost += octave / 4;
            }
        }
        Some(cost)
    };

    let mut best: Option<(i32, FixedVec<usize, { crate::MAX_CHORD_NOTES }>)> = None;
    if n <= 6 && k <= 8 {
        let mut choice: FixedVec<usize, { crate::MAX_CHORD_NOTES }> = FixedVec::new();
        for _ in 0..n {
            let _ = choice.push(0);
        }
        loop {
            if let Some(cost) = cost_of(&choice) {
                if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                    best = Some((cost, choice));
                }
            }
            // Next combination (mixed-radix counter).
            let mut i = 0;
            loop {
                if i == n {
                    break;
                }
                choice[i] += 1;
                if choice[i] < k {
                    break;
                }
                choice[i] = 0;
                i += 1;
            }
            if i == n {
                break;
            }
        }
    } else {
        let mut choice: FixedVec<usize, { crate::MAX_CHORD_NOTES }> = FixedVec::new();
        for v in 0..n {
            let j = (0..k)
                .filter_map(|j| options[v][j].map(|p| ((p.step() - voices[v].step()).abs(), j)))
                .min()
                .map_or(0, |(_, j)| j);
            let _ = choice.push(j);
        }
        best = cost_of(&choice).map(|c| (c, choice));
    }

    let Some((_, choice)) = best else {
        return close(target, low);
    };
    let mut out = Chord::empty(tuning);
    for (v, &j) in choice.iter().enumerate() {
        if let Some(p) = options[v][j] {
            let _ = out.add(p.with_spelling(spelling_for(target, p.pitch_class())));
        }
    }
    match target.root() {
        Some(r) => out.with_root(r),
        None => out,
    }
}

/// Close position over the bass: the bass stays, and the chord's other pitch classes are
/// packed into the octave above it.
pub fn close_over_bass(chord: &Chord) -> Chord {
    let Some(bass) = chord.bass() else {
        return *chord;
    };
    let mut out = Chord::empty(chord.tuning());
    let _ = out.add(bass);
    for pc in chord.pitch_classes().iter() {
        if pc != bass.pitch_class() {
            let _ = out.add(first_at_or_above(bass.offset(1), pc).with_spelling(spelling_for(chord, pc)));
        }
    }
    with_root_of(out, chord)
}

/// Open position: close position over the bass, then every other note above the bass (the
/// first, third…) up an octave, so C E G becomes C G E.
pub fn open(chord: &Chord) -> Chord {
    let closed = close_over_bass(chord);
    let octave = chord.tuning().steps_per_octave();
    let mut out = Chord::empty(chord.tuning());
    for (i, &p) in closed.notes().iter().enumerate() {
        let _ = out.add(if i % 2 == 1 { p.offset(octave).with_spelling(p.spelling()) } else { p });
    }
    with_root_of(out, chord)
}

/// Keeps the bass and spreads the other notes evenly over `span` steps above it, each placed
/// in the octave nearest its share of the span and above the note before.
pub fn spread(chord: &Chord, span: i32) -> Chord {
    let closed = close_over_bass(chord);
    let notes = closed.notes();
    if notes.len() < 3 {
        return closed;
    }
    let octave = chord.tuning().steps_per_octave();
    let bass = notes[0];
    let mut out = Chord::empty(chord.tuning());
    let _ = out.add(bass);
    let mut prev = bass;
    let last = (notes.len() - 1) as i32;
    for (i, &p) in notes.iter().enumerate().skip(1) {
        let target = bass.step() + span * i as i32 / last;
        // The pitch of p's class nearest the target, then above the previous note.
        let mut step = target - (target - p.step()).rem_euclid(octave);
        if target - step > octave / 2 {
            step += octave;
        }
        while step <= prev.step() {
            step += octave;
        }
        let placed = Pitch::new(chord.tuning(), step).with_spelling(p.spelling());
        let _ = out.add(placed);
        prev = placed;
    }
    with_root_of(out, chord)
}

fn with_root_of(out: Chord, chord: &Chord) -> Chord {
    match chord.root() {
        Some(r) => out.with_root(r),
        None => out,
    }
}

fn collect(iter: impl Iterator<Item = u8>) -> FixedVec<u8, 64> {
    let mut v = FixedVec::new();
    for x in iter {
        let _ = v.push(x);
    }
    v
}

fn spelling_for(chord: &Chord, pc: u8) -> Option<crate::Spelling> {
    chord
        .notes()
        .iter()
        .filter_map(|p| p.spelling())
        .find(|sp| sp.pc() == pc)
}

/// The lowest pitch with class `pc` at or above `floor`.
fn first_at_or_above(floor: Pitch, pc: u8) -> Pitch {
    let n = floor.tuning().steps_per_octave();
    let up = (pc as i32 - floor.pitch_class() as i32).rem_euclid(n);
    floor.offset(up)
}

/// The pitch with class `pc` in `[low, high]` nearest to `from` (the lower one on a tie).
fn nearest_in_range(from: Pitch, pc: u8, low: Pitch, high: Pitch) -> Option<Pitch> {
    let n = from.tuning().steps_per_octave();
    let mut p = first_at_or_above(low, pc);
    let mut best: Option<Pitch> = None;
    while p <= high {
        let better = match best {
            None => true,
            Some(b) => (p.step() - from.step()).abs() < (b.step() - from.step()).abs(),
        };
        if better {
            best = Some(p);
        }
        p = p.offset(n);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Progression, Scale};
    use std::format;

    fn sym(s: &str) -> Chord {
        Chord::from_symbol(s, 4).unwrap()
    }

    #[test]
    fn close_and_drop_voicings() {
        let c = close(&sym("Cmaj7"), Pitch::parse("E3").unwrap());
        assert_eq!(format!("{c:#}"), "[C4 E4 G4 B4]");
        assert_eq!(format!("{:#}", drop2(&c)), "[G3 C4 E4 B4]");
        assert_eq!(format!("{:#}", drop3(&c)), "[E3 C4 G4 B4]");
        assert_eq!(drop2(&c).to_string(), "Cmaj7/G");
    }

    #[test]
    fn close_open_and_spread() {
        let wide = Chord::from_pitches(&[
            Pitch::parse("C3").unwrap(),
            Pitch::parse("G4").unwrap(),
            Pitch::parse("E5").unwrap(),
        ])
        .unwrap();
        assert_eq!(format!("{:#}", close_over_bass(&wide)), "[C3 E3 G3]");
        assert_eq!(format!("{:#}", open(&wide)), "[C3 G3 E4]");
        let s = spread(&sym("Cmaj7"), 24);
        assert_eq!(format!("{s:#}"), "[C4 E4 G5 B5]");
        assert_eq!(s.pitch_classes(), sym("Cmaj7").pitch_classes());
    }

    #[test]
    fn leads_ii_v_i_smoothly() {
        let low = Pitch::parse("C3").unwrap();
        let high = Pitch::parse("C6").unwrap();
        let key = Scale::parse("C major").unwrap();
        let prog = Progression::from_roman("ii7 V7 Imaj7", &key, 960, 4).unwrap();
        let led = prog.map_with_previous(|prev, chord| match prev {
            Some(p) => lead(p, chord, low, high),
            None => *chord,
        });
        let voicings: std::vec::Vec<_> = led
            .steps()
            .iter()
            .map(|s| format!("{:#}", s.chord))
            .collect();
        // Common tones hold and the other voices move by step, so total motion stays small.
        let total: i32 = led
            .steps()
            .windows(2)
            .map(|w| {
                w[0].chord
                    .notes()
                    .iter()
                    .zip(w[1].chord.notes())
                    .map(|(a, b)| (a.step() - b.step()).abs())
                    .sum::<i32>()
            })
            .sum();
        assert!(total <= 8, "too much motion: {voicings:?}");
        // Every chord keeps four voices and its quality.
        for (step, name) in led.steps().iter().zip(["Dm7", "G7", "Cmaj7"]) {
            assert_eq!(step.chord.len(), 4);
            assert_eq!(step.chord.pitch_classes(), sym(name).pitch_classes());
        }
    }

    #[test]
    fn respects_range() {
        let prev = sym("C"); // C4 E4 G4
        let low = Pitch::parse("C4").unwrap();
        let high = Pitch::parse("G4").unwrap();
        let out = lead(&prev, &sym("G"), low, high);
        assert!(
            out.notes().iter().all(|p| *p >= low && *p <= high),
            "{out:#}"
        );
    }
}
