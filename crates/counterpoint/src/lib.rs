//! A rule-based counterpoint solver.
//!
//! A [`Problem`] lists voices from top to bottom. Each is either fixed (a given line, such
//! as a cantus firmus) or free. Free voices get a rhythm from the chosen [`Species`],
//! measured against a time base of "measures" (the fixed line's notes, or the chords of a
//! progression). Every candidate note is scored by [`rules`](crate::rules) that add a cost
//! for each thing a teacher would mark; a beam search keeps the cheapest partial
//! solutions as it moves through time.
//!
//! The [`Solution`] keeps every rule that was broken, and for each free note the
//! next-best pitch and how much worse it would have been, so the UI can explain choices.
//!
//! Pitches are 12-EDO MIDI note numbers.

pub mod rules;

use pm_theory::{Chord, Pitch, Scale, Ticks};

pub use rules::{Group, Rule, RuleSet, HARD};

/// Rhythm of the free voices against each measure of the time base.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Species {
    /// Note against note.
    First,
    /// Two notes per measure; the second may be a passing tone.
    Second,
    /// Four notes per measure; passing and neighbor tones.
    Third,
    /// Syncopation: notes tied over the bar line, with suspensions.
    Fourth,
    /// Florid: a mix of the others.
    Fifth,
}

/// A note of a fixed line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FixedNote {
    pub start: Ticks,
    pub duration: Ticks,
    /// MIDI note number.
    pub pitch: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VoiceDef {
    pub name: String,
    /// Lowest and highest MIDI note.
    pub low: i32,
    pub high: i32,
    /// A given line; `None` means the solver writes this voice.
    pub fixed: Option<Vec<FixedNote>>,
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Problem {
    /// Voices from top to bottom.
    pub voices: Vec<VoiceDef>,
    /// The time base: (start, duration) of each measure, in order.
    pub measures: Vec<(Ticks, Ticks)>,
    /// The key over time: (start, end, scale). Empty means C major.
    pub keys: Vec<(Ticks, Ticks, Scale)>,
    /// An optional progression: (start, end, chord).
    pub chords: Vec<(Ticks, Ticks, Chord)>,
    pub species: Species,
    pub rules: RuleSet,
    /// How many partial solutions the search keeps.
    pub beam: usize,
    /// 0 for the best-scoring answer; higher values add seeded noise to costs, giving
    /// different but still rule-abiding answers.
    pub variety: u32,
    pub seed: u64,
}

/// One note position of one voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub voice: usize,
    pub start: Ticks,
    pub duration: Ticks,
    /// 2 on the downbeat of a measure, 1 halfway through, 0 elsewhere.
    pub strength: u8,
    pub measure: usize,
    pub fixed: Option<i32>,
}

impl Slot {
    pub fn end(&self) -> Ticks {
        self.start + self.duration
    }
}

/// A broken rule, attached to the slot it is blamed on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Violation {
    pub rule: Rule,
    pub slot: usize,
    /// The other note involved, for rules between two voices.
    pub other: Option<usize>,
    pub cost: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub slots: Vec<Slot>,
    /// MIDI note per slot.
    pub pitches: Vec<i32>,
    pub cost: f32,
    pub violations: Vec<Violation>,
    /// For each free slot: the best other pitch and how much it would add to the total
    /// cost (negative if the search missed a better local choice).
    pub alternatives: Vec<Option<(i32, f32)>>,
}

impl Solution {
    /// Violations worth showing (cost of at least 1).
    pub fn issues(&self) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(|v| v.rule.is_shown() && v.cost >= 1.0)
    }
}

const UNSET: i32 = i32::MIN;

fn ic(semitones: i32) -> i32 {
    semitones.rem_euclid(12)
}

fn is_perfect(ic: i32) -> bool {
    ic == 0 || ic == 7
}

fn consonant(lo: i32, hi: i32, lo_is_bass: bool) -> bool {
    match ic(hi - lo) {
        0 | 3 | 4 | 7 | 8 | 9 => true,
        5 => !lo_is_bass,
        _ => false,
    }
}

fn hash(parts: &[u64]) -> u64 {
    parts.iter().fold(0x9e37_79b9_7f4a_7c15u64, |h, &p| {
        let mut x = h ^ p.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 31)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^ (x >> 29)
    })
}

/// Scoring context: the slots and everything precomputed about them.
struct Ctx<'a> {
    p: &'a Problem,
    slots: Vec<Slot>,
    voice_slots: Vec<Vec<usize>>,
    /// Position of each slot within its voice.
    pos: Vec<usize>,
    steps: Vec<Ticks>,
    onsets: Vec<Vec<usize>>,
    candidates: Vec<Vec<i32>>,
    default_key: Scale,
    bottom: usize,
}

struct Acc<'v> {
    total: f32,
    rules: RuleSet,
    rec: Option<&'v mut Vec<Violation>>,
}

impl Acc<'_> {
    fn add(&mut self, rule: Rule, slot: usize, other: Option<usize>, cost: f32) {
        if cost == 0.0 || !self.rules.allows(rule.group()) {
            return;
        }
        self.total += cost;
        if let Some(rec) = self.rec.as_deref_mut() {
            rec.push(Violation {
                rule,
                slot,
                other,
                cost,
            });
        }
    }
}

impl<'a> Ctx<'a> {
    fn new(p: &'a Problem) -> Ctx<'a> {
        let mut slots = Vec::new();
        let n_measures = p.measures.len();
        for (v, voice) in p.voices.iter().enumerate() {
            match &voice.fixed {
                Some(notes) => {
                    for note in notes {
                        let (measure, strength) = Self::locate(p, note.start);
                        slots.push(Slot {
                            voice: v,
                            start: note.start,
                            duration: note.duration.max(1),
                            strength,
                            measure,
                            fixed: Some(note.pitch),
                        });
                    }
                }
                None => {
                    for (i, &(start, dur)) in p.measures.iter().enumerate() {
                        let last = i + 1 == n_measures;
                        for (offset, length) in Self::rhythm(p, i, dur, last) {
                            let t = start + offset;
                            let strength = if offset == 0 {
                                2
                            } else if offset * 2 == dur {
                                1
                            } else {
                                0
                            };
                            slots.push(Slot {
                                voice: v,
                                start: t,
                                duration: length,
                                strength,
                                measure: i,
                                fixed: None,
                            });
                        }
                    }
                }
            }
        }
        let mut voice_slots = vec![Vec::new(); p.voices.len()];
        let mut order: Vec<usize> = (0..slots.len()).collect();
        order.sort_by_key(|&i| (slots[i].start, slots[i].voice));
        let mut pos = vec![0; slots.len()];
        for &i in &order {
            let v = slots[i].voice;
            pos[i] = voice_slots[v].len();
            voice_slots[v].push(i);
        }
        let mut steps: Vec<Ticks> = slots.iter().map(|s| s.start).collect();
        steps.sort_unstable();
        steps.dedup();
        let mut onsets = vec![Vec::new(); steps.len()];
        for &i in &order {
            let k = steps.binary_search(&slots[i].start).expect("step exists");
            onsets[k].push(i);
        }
        let default_key = Scale::parse("C major").expect("valid");
        let mut ctx = Ctx {
            p,
            slots,
            voice_slots,
            pos,
            steps,
            onsets,
            candidates: Vec::new(),
            default_key,
            bottom: p.voices.len().saturating_sub(1),
        };
        ctx.candidates = (0..ctx.slots.len())
            .map(|i| ctx.make_candidates(i))
            .collect();
        ctx
    }

    /// The measure containing `t` and the metric strength of `t` within it.
    fn locate(p: &Problem, t: Ticks) -> (usize, u8) {
        let i = p.measures.iter().rposition(|m| m.0 <= t).unwrap_or(0);
        let Some(&(start, dur)) = p.measures.get(i) else {
            return (0, 2);
        };
        let offset = t - start;
        let strength = if offset == 0 {
            2
        } else if offset * 2 == dur {
            1
        } else {
            0
        };
        (i, strength)
    }

    /// (offset, length) of each free note in measure `i`.
    fn rhythm(p: &Problem, i: usize, dur: Ticks, last: bool) -> Vec<(Ticks, Ticks)> {
        let whole = vec![(0, dur)];
        if last || dur < 4 {
            return whole;
        }
        let (h, q) = (dur / 2, dur / 4);
        let n = p.measures.len();
        match p.species {
            Species::First => whole,
            Species::Second => vec![(0, h), (h, dur - h)],
            Species::Third => vec![(0, q), (q, q), (2 * q, q), (3 * q, dur - 3 * q)],
            Species::Fourth => {
                // Tied over the bar line: start halfway, end halfway through the next
                // measure. The penultimate measure resolves on its second half.
                if i + 2 < n {
                    let next = p.measures[i + 1];
                    let tie = next.0 + next.1 / 2 - p.measures[i].0;
                    vec![(h, tie - h)]
                } else {
                    // Penultimate: the suspension (tied in from before) resolves halfway.
                    vec![(h, dur - h)]
                }
            }
            Species::Fifth => {
                if i + 2 == n {
                    return vec![(0, h), (h, dur - h)];
                }
                let patterns: [&[Ticks]; 5] =
                    [&[2, 2], &[1, 1, 1, 1], &[2, 1, 1], &[1, 1, 2], &[3, 1]];
                let pick =
                    patterns[(hash(&[p.seed, i as u64, 5]) % patterns.len() as u64) as usize];
                let mut out = Vec::new();
                let mut at = 0;
                for (k, &units) in pick.iter().enumerate() {
                    let len = if k + 1 == pick.len() {
                        dur - at
                    } else {
                        units * q
                    };
                    out.push((at, len));
                    at += len;
                }
                out
            }
        }
    }

    fn key_at(&self, t: Ticks) -> &Scale {
        self.p
            .keys
            .iter()
            .find(|(s, e, _)| *s <= t && t < *e)
            .or_else(|| self.p.keys.last())
            .map_or(&self.default_key, |k| &k.2)
    }

    fn chord_at(&self, t: Ticks) -> Option<(usize, &Chord)> {
        self.p
            .chords
            .iter()
            .enumerate()
            .find(|(_, (s, e, _))| *s <= t && t < *e)
            .map(|(i, c)| (i, &c.2))
    }

    fn make_candidates(&self, i: usize) -> Vec<i32> {
        let s = self.slots[i];
        if let Some(f) = s.fixed {
            return vec![f];
        }
        let voice = &self.p.voices[s.voice];
        let key = self.key_at(s.start);
        let tonic = key.root() as i32;
        let penultimate = s.measure + 2 == self.p.measures.len();
        (voice.low..=voice.high)
            .filter(|&n| {
                let p = Pitch::midi(n);
                key.contains(p) || (penultimate && ic(n - tonic) == 11)
            })
            .collect()
    }

    fn prev(&self, i: usize) -> Option<usize> {
        let v = self.slots[i].voice;
        let k = self.pos[i];
        (k > 0).then(|| self.voice_slots[v][k - 1])
    }

    fn sounding(&self, voice: usize, t: Ticks) -> Option<usize> {
        let list = &self.voice_slots[voice];
        let k = list.partition_point(|&i| self.slots[i].start <= t);
        let i = *list.get(k.checked_sub(1)?)?;
        (t < self.slots[i].end()).then_some(i)
    }

    /// The lowest assigned pitch sounding at `t`.
    fn lowest(&self, a: &[i32], t: Ticks) -> Option<i32> {
        (0..self.p.voices.len())
            .filter_map(|v| self.sounding(v, t))
            .map(|i| a[i])
            .filter(|&x| x != UNSET)
            .min()
    }

    fn is_free(&self, i: usize) -> bool {
        self.slots[i].fixed.is_none()
    }

    /// Whether slot `i` clashes with any other assigned voice at time `t`.
    fn dissonant_at(&self, a: &[i32], i: usize, t: Ticks) -> bool {
        let v = self.slots[i].voice;
        let low = self.lowest(a, t);
        (0..self.p.voices.len())
            .filter(|&w| w != v)
            .filter_map(|w| self.sounding(w, t))
            .any(|m| {
                if a[m] == UNSET {
                    return false;
                }
                let (lo, hi) = (a[i].min(a[m]), a[i].max(a[m]));
                !consonant(lo, hi, Some(lo) == low)
            })
    }

    /// Cost of everything decided at step `k`, given the (partial) assignment `a`.
    fn step_terms(&self, k: usize, a: &[i32], acc: &mut Acc) {
        let t = self.steps[k];
        let last_step = k + 1 == self.steps.len();
        if last_step {
            self.ending(a, acc);
        }
        for &n in &self.onsets[k] {
            if a[n] == UNSET {
                continue;
            }
            let v = self.slots[n].voice;
            if self.is_free(n) {
                self.melodic(n, a, acc);
                self.harmony(n, a, acc);
                let voice = &self.p.voices[v];
                let centre = (voice.low + voice.high) as f32 / 2.0;
                let off = ((a[n] as f32 - centre).abs() - 5.0).max(0.0);
                acc.add(Rule::Tessitura, n, None, off * 0.1);
                if self.p.variety > 0 {
                    let noise =
                        (hash(&[self.p.seed, n as u64, a[n] as u64]) % 1000) as f32 / 1000.0;
                    acc.add(Rule::Variety, n, None, noise * self.p.variety as f32 * 0.04);
                }
                if self.prev(n).is_none() {
                    self.beginning(n, a, acc);
                }
            }
            for w in 0..self.p.voices.len() {
                if w == v {
                    continue;
                }
                let Some(m) = self.sounding(w, t) else {
                    continue;
                };
                if a[m] == UNSET || (!self.is_free(n) && !self.is_free(m)) {
                    continue;
                }
                let m_onset = self.slots[m].start == t;
                if m_onset && w < v {
                    continue; // counted when visiting m
                }
                self.vertical(k, n, m, a, last_step, acc);
            }
        }
        self.chord_complete(k, a, acc);
    }

    /// With a progression and three or more voices: on strong beats the chord should
    /// have its third, and the leading tone shouldn't be doubled.
    fn chord_complete(&self, k: usize, a: &[i32], acc: &mut Acc) {
        let t = self.steps[k];
        if self.p.voices.len() < 3 {
            return;
        }
        let Some(blame) = self.onsets[k]
            .iter()
            .copied()
            .find(|&n| self.is_free(n) && self.slots[n].strength == 2)
        else {
            return;
        };
        let Some((_, chord)) = self.chord_at(t) else {
            return;
        };
        let Some(root) = chord.root() else { return };
        let mut pcs = [0u8; 12];
        for w in 0..self.p.voices.len() {
            match self.sounding(w, t) {
                Some(m) if a[m] != UNSET => pcs[a[m].rem_euclid(12) as usize] += 1,
                _ => return, // not every voice is known yet
            }
        }
        let third = [3, 4]
            .iter()
            .map(|i| (root as i32 + i) % 12)
            .find(|&pc| chord.contains_pc(pc));
        if let Some(third) = third {
            if pcs[third as usize] == 0 {
                acc.add(Rule::MissingThird, blame, None, 6.0);
            }
        }
        let lt = (self.key_at(t).root() as i32 + 11) % 12;
        if chord.contains_pc(lt) && pcs[lt as usize] > 1 {
            acc.add(Rule::DoubledLeadingTone, blame, None, 4.0);
        }
    }

    fn melodic(&self, n: usize, a: &[i32], acc: &mut Acc) {
        let Some(q) = self.prev(n) else { return };
        let p = a[n];
        let m = p - a[q];
        let am = m.abs();
        let s = self.slots[n];
        let repeat_cost = if self.p.chords.is_empty() {
            if self.p.species == Species::First {
                4.0
            } else {
                3.0
            }
        } else if s.voice == 0 {
            // Over a progression, holding common tones is good voice leading,
            // except in the melody.
            1.0
        } else if s.voice == self.bottom {
            1.5
        } else {
            0.0
        };
        match am {
            0 => acc.add(Rule::RepeatedNote, n, None, repeat_cost),
            1 | 2 => {}
            3 | 4 => acc.add(Rule::LargeLeap, n, None, 0.3),
            5 => acc.add(Rule::LargeLeap, n, None, 1.0),
            6 => acc.add(Rule::MelodicTritone, n, None, HARD),
            7 => acc.add(Rule::LargeLeap, n, None, 1.5),
            8 => acc.add(Rule::LargeLeap, n, None, if m > 0 { 3.0 } else { 6.0 }),
            9 => acc.add(Rule::LargeLeap, n, None, 6.0),
            12 => acc.add(Rule::LargeLeap, n, None, 4.0),
            _ => acc.add(Rule::LeapTooLarge, n, None, HARD),
        }
        if am == 1 {
            // C to C#: the same letter, raised. Awkward in a vocal line.
            let key = self.key_at(s.start);
            let (in_p, in_q) = (
                key.contains(Pitch::midi(p)),
                key.contains(Pitch::midi(a[q])),
            );
            if in_p != in_q {
                let (diatonic, chromatic) = if in_p { (p, a[q]) } else { (a[q], p) };
                if chromatic - diatonic == 1 {
                    acc.add(Rule::ChromaticStep, n, None, 4.0);
                }
            }
        }
        // The same figure as the previous measure (sequences in 3rd and 5th species).
        self.repeated_figure(n, a, acc);
        if am == 3 {
            let key = self.key_at(s.start);
            if let (Some((dp, op)), Some((dq, oq))) = (
                key.degree_of(Pitch::midi(p)),
                key.degree_of(Pitch::midi(a[q])),
            ) {
                let steps =
                    (op * key.len() as i32 + dp as i32) - (oq * key.len() as i32 + dq as i32);
                if steps.abs() == 1 {
                    acc.add(Rule::AugmentedSecond, n, None, HARD);
                }
            }
        }
        if let Some(r) = self.prev(q) {
            let prev = a[q] - a[r];
            if prev.abs() >= 5 {
                if m == 0 || m.signum() == prev.signum() {
                    acc.add(Rule::UnrecoveredLeap, n, None, 4.0);
                } else if am > 2 {
                    acc.add(Rule::UnrecoveredLeap, n, None, 1.5);
                }
            }
            if let Some(r2) = self.prev(r) {
                if am > 0 && p == a[r] && a[q] == a[r2] {
                    acc.add(Rule::Oscillation, n, None, 2.0);
                }
            }
            if prev.abs() >= 3 && am >= 3 && prev.signum() == m.signum() {
                acc.add(Rule::LeapsSameDirection, n, None, 2.0);
                if (p - a[r]).abs() > 12 {
                    acc.add(Rule::LeapsSameDirection, n, None, 6.0);
                }
            }
            self.resolution(q, r, n, a, acc);
        } else {
            self.resolution(q, usize::MAX, n, a, acc);
        }
        self.tendency(q, n, a, acc);
    }

    /// When `n` completes a measure of two or more notes, compares its intervals with the
    /// previous measure's.
    fn repeated_figure(&self, n: usize, a: &[i32], acc: &mut Acc) {
        let s = self.slots[n];
        let list = &self.voice_slots[s.voice];
        let k = self.pos[n];
        let is_last_in_measure = list
            .get(k + 1)
            .is_none_or(|&x| self.slots[x].measure != s.measure);
        if !is_last_in_measure || s.measure == 0 {
            return;
        }
        let here: Vec<i32> = list
            .iter()
            .filter(|&&i| self.slots[i].measure == s.measure)
            .map(|&i| a[i])
            .collect();
        let before: Vec<i32> = list
            .iter()
            .filter(|&&i| self.slots[i].measure + 1 == s.measure)
            .map(|&i| a[i])
            .collect();
        if here.len() >= 3 && here.len() == before.len() {
            let shape = |v: &[i32]| v.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>();
            if shape(&here) == shape(&before) {
                acc.add(Rule::RepeatedFigure, n, None, 3.0);
            }
        }
    }

    /// Checks how the previous note `q` (approached from `r`) was left for `n`.
    fn resolution(&self, q: usize, r: usize, n: usize, a: &[i32], acc: &mut Acc) {
        let sq = self.slots[q];
        let p = a[n];
        let leave = p - a[q];
        // A weak-beat dissonance must be left by step: passing (same direction) or
        // neighbor (back again).
        if sq.strength < 2 && self.p.species != Species::First && self.dissonant_at(a, q, sq.start)
        {
            if leave.abs() > 2 || leave == 0 {
                acc.add(Rule::UnresolvedDissonance, q, Some(n), HARD);
            } else if r != usize::MAX {
                let approach = a[q] - a[r];
                if approach.signum() != leave.signum() && self.p.species == Species::Second {
                    acc.add(Rule::NeighborInSecondSpecies, q, Some(n), 4.0);
                }
            }
        }
        // A note held into a dissonance on a strong beat (a suspension) must fall by step.
        let first = self.steps.partition_point(|&t| t <= sq.start);
        let suspended = self.steps[first..]
            .iter()
            .take_while(|&&t| t < sq.end())
            .any(|&t| {
                let strong = self.onsets[self.steps.binary_search(&t).expect("step")]
                    .iter()
                    .any(|&o| self.slots[o].strength == 2);
                strong && self.dissonant_at(a, q, t)
            });
        if suspended && !(leave == -1 || leave == -2) {
            acc.add(Rule::UnresolvedSuspension, q, Some(n), HARD);
        }
    }

    fn tendency(&self, q: usize, n: usize, a: &[i32], acc: &mut Acc) {
        let sq = self.slots[q];
        let key = self.key_at(sq.start);
        let tonic = key.root() as i32;
        let p = a[n];
        if ic(a[q] - tonic) == 11 && sq.voice != self.bottom && p != a[q] + 1 {
            acc.add(
                Rule::LeadingTone,
                q,
                Some(n),
                if sq.voice == 0 { 4.0 } else { 2.0 },
            );
        }
        if let (Some((ci, chord)), Some((cn, _))) =
            (self.chord_at(sq.start), self.chord_at(self.slots[n].start))
        {
            if ci != cn && chord.len() >= 4 {
                if let Some(root) = chord.root() {
                    let seventh =
                        matches!(ic(a[q] - root as i32), 10 | 11) && chord.contains_pc(a[q]);
                    if seventh && !(p - a[q] == -1 || p - a[q] == -2) {
                        acc.add(Rule::SeventhResolution, q, Some(n), 4.0);
                    }
                }
            }
        }
    }

    fn harmony(&self, n: usize, a: &[i32], acc: &mut Acc) {
        let s = self.slots[n];
        let Some((_, chord)) = self.chord_at(s.start) else {
            return;
        };
        let p = a[n];
        let tone = chord.contains_pc(p);
        if s.strength == 2 && !tone {
            acc.add(Rule::NonChordTone, n, None, 5.0);
        } else if !tone {
            let by_step = self.prev(n).is_some_and(|q| (p - a[q]).abs() <= 2);
            if !by_step {
                acc.add(Rule::NonChordTone, n, None, 2.0);
            }
        }
        if s.voice == self.bottom && s.strength == 2 {
            if let Some(root) = chord.root() {
                if ic(p - root as i32) != 0 {
                    acc.add(Rule::BassNotRoot, n, None, 2.5);
                }
            }
        }
    }

    fn beginning(&self, n: usize, a: &[i32], acc: &mut Acc) {
        let s = self.slots[n];
        let p = a[n];
        if s.voice == self.bottom {
            let root = match self.chord_at(s.start) {
                Some((_, c)) => c
                    .root()
                    .map_or(self.key_at(s.start).root() as i32, |r| r as i32),
                None => self.key_at(s.start).root() as i32,
            };
            if ic(p - root) != 0 {
                acc.add(Rule::ImperfectStart, n, None, 4.0);
            }
        } else if let Some(b) = self.sounding(self.bottom, s.start) {
            if a[b] != UNSET && !is_perfect(ic(p - a[b])) && self.p.voices.len() == 2 {
                acc.add(Rule::ImperfectStart, n, Some(b), 6.0);
            }
        }
    }

    fn vertical(&self, k: usize, n: usize, m: usize, a: &[i32], last_step: bool, acc: &mut Acc) {
        let t = self.steps[k];
        let (sn, sm) = (self.slots[n], self.slots[m]);
        let (pn, pm) = (a[n], a[m]);
        let (lo, hi) = (pn.min(pm), pn.max(pm));
        let low = self.lowest(a, t);
        let cons = consonant(lo, hi, Some(lo) == low);
        let m_onset = sm.start == t;
        let blame = if self.is_free(n) { n } else { m };
        let other = if blame == n { m } else { n };
        let (upper, lower) = if sn.voice < sm.voice { (n, m) } else { (m, n) };

        // Spacing between voices.
        if a[upper] < a[lower] {
            acc.add(Rule::VoiceCrossing, blame, Some(other), 25.0);
        }
        let before = |i: usize| {
            if self.slots[i].start == t {
                self.prev(i).map(|q| a[q])
            } else {
                Some(a[i])
            }
        };
        if self.is_free(n) {
            if let Some(prev_m) = before(m) {
                if (sn.voice < sm.voice && pn < prev_m) || (sn.voice > sm.voice && pn > prev_m) {
                    acc.add(Rule::VoiceOverlap, n, Some(m), 6.0);
                }
            }
        }
        if self.slots[upper].voice + 1 == self.slots[lower].voice {
            let limit = if self.slots[lower].voice == self.bottom {
                24
            } else {
                12
            };
            if a[upper] - a[lower] > limit {
                acc.add(Rule::WideSpacing, blame, Some(other), 3.0);
            }
        }
        let edge = (k == 0 || last_step) && self.p.voices.len() == 2;
        if hi == lo && !edge {
            acc.add(Rule::Unison, blame, Some(other), 4.0);
        }

        // Dissonance.
        if !cons {
            let tied_onset = self.p.species == Species::Fourth && self.is_free(n) && sn.start == t;
            if self.p.species == Species::First || tied_onset {
                // Note against note has no dissonance; in 4th species the tied note must
                // be consonant where it starts (the preparation).
                acc.add(Rule::StrongDissonance, blame, Some(other), HARD);
            } else if sn.strength == 2 {
                let suspension = !m_onset
                    && self.is_free(m)
                    && matches!(self.p.species, Species::Fourth | Species::Fifth)
                    && !self.dissonant_at(a, m, sm.start);
                if !suspension {
                    acc.add(Rule::StrongDissonance, blame, Some(other), HARD);
                }
            } else if self.is_free(n) {
                let by_step = self
                    .prev(n)
                    .is_some_and(|q| (pn - a[q]).abs() <= 2 && pn != a[q]);
                if !by_step {
                    acc.add(Rule::DissonanceByLeap, n, Some(m), HARD);
                }
            } else {
                acc.add(Rule::DissonanceByLeap, m, Some(n), 4.0);
            }
        }

        // Motion between the two voices (only when both move now).
        if m_onset {
            if let (Some(qn), Some(qm)) = (self.prev(n), self.prev(m)) {
                let (a0, b0) = (a[qn], a[qm]);
                let (dn, dm) = (pn - a0, pm - b0);
                if dn != 0 && dm != 0 && dn.signum() == dm.signum() {
                    let ic0 = ic((a0 - b0).abs());
                    let ic1 = ic(hi - lo);
                    if is_perfect(ic1) && ic0 == ic1 {
                        let rule = if ic1 == 7 {
                            Rule::ParallelFifths
                        } else {
                            Rule::ParallelOctaves
                        };
                        acc.add(rule, blame, Some(other), HARD);
                    } else if is_perfect(ic1) {
                        let outer =
                            self.slots[upper].voice == 0 && self.slots[lower].voice == self.bottom;
                        let upper_leap = if upper == n { dn.abs() } else { dm.abs() };
                        if outer && upper_leap > 2 {
                            acc.add(Rule::HiddenPerfect, blame, Some(other), 6.0);
                        }
                    } else {
                        acc.add(Rule::SimilarMotion, blame, Some(other), 0.5);
                        let class = |i: i32| {
                            if i == 3 || i == 4 {
                                3
                            } else if i == 8 || i == 9 {
                                8
                            } else {
                                -1
                            }
                        };
                        if class(ic0) != -1 && class(ic0) == class(ic1) {
                            if let (Some(rn), Some(rm)) = (self.prev(qn), self.prev(qm)) {
                                let icr = ic((a[rn] - a[rm]).abs());
                                let moved =
                                    (a0 - a[rn]).signum() == (b0 - a[rm]).signum() && a0 != a[rn];
                                if moved && class(icr) == class(ic0) {
                                    acc.add(Rule::ParallelImperfectRun, blame, Some(other), 3.0);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Perfect intervals on successive downbeats (a weak beat between doesn't hide them).
        if sn.strength == 2
            && matches!(
                self.p.species,
                Species::Second | Species::Third | Species::Fifth
            )
            && sn.measure > 0
        {
            let t0 = self.p.measures[sn.measure - 1].0;
            if let (Some(x), Some(y)) = (self.sounding(sn.voice, t0), self.sounding(sm.voice, t0)) {
                if a[x] != UNSET && a[y] != UNSET {
                    let ic0 = ic((a[x] - a[y]).abs());
                    let ic1 = ic(hi - lo);
                    if is_perfect(ic1) && ic0 == ic1 && a[x] != pn && a[y] != pm {
                        acc.add(Rule::AccentedParallels, blame, Some(other), 10.0);
                    }
                }
            }
        }
    }

    /// Terms that need the whole line: its high point and range.
    /// The last step: the final sonority, and how the top voice arrives at it.
    fn ending(&self, a: &[i32], acc: &mut Acc) {
        let Some(&t) = self.steps.last() else { return };
        let bottom = self.sounding(self.bottom, t);
        if let Some(b) = bottom.filter(|&b| a[b] != UNSET) {
            let key = self.key_at(t);
            if ic(a[b] - key.root() as i32) != 0 {
                let blame = if self.is_free(b) {
                    Some(b)
                } else {
                    (0..self.p.voices.len())
                        .filter_map(|v| self.sounding(v, t))
                        .find(|&i| self.is_free(i))
                };
                if let Some(blame) = blame {
                    acc.add(Rule::FinalNotTonic, blame, None, 8.0);
                }
            }
            if self.p.voices.len() == 2 {
                if let Some(top) = self.sounding(0, t) {
                    if top != b && a[top] != UNSET && ic(a[top] - a[b]) != 0 {
                        let blame = if self.is_free(top) { top } else { b };
                        acc.add(Rule::ImperfectEnd, blame, None, 8.0);
                    }
                }
            }
        }
        if let Some(list) = self.voice_slots.first() {
            let v = 0;
            if self.p.voices[v].fixed.is_none()
                && list.len() >= 2
                && a[list[list.len() - 1]] != UNSET
            {
                let (last, prev) = (list[list.len() - 1], list[list.len() - 2]);
                let step = (a[last] - a[prev]).abs();
                if step > 2 || step == 0 {
                    acc.add(Rule::FinalByLeap, last, None, 5.0);
                } else if a[last] - a[prev] == 2 {
                    // Rising a whole step to the final: a raised leading tone is stronger.
                    acc.add(Rule::NoLeadingTone, prev, Some(last), 2.0);
                }
            }
        }
    }

    fn final_terms(&self, a: &[i32], acc: &mut Acc) {
        for (v, list) in self.voice_slots.iter().enumerate() {
            if self.p.voices[v].fixed.is_some() || list.is_empty() {
                continue;
            }
            let max = list.iter().map(|&i| a[i]).max().unwrap_or(0);
            let min = list.iter().map(|&i| a[i]).min().unwrap_or(0);
            let tops: Vec<usize> = list.iter().copied().filter(|&i| a[i] == max).collect();
            // Notes repeated in a row (a held note restruck) don't count twice.
            let distinct = tops
                .windows(2)
                .filter(|w| self.pos[w[1]] != self.pos[w[0]] + 1)
                .count()
                + 1;
            if distinct > 1 && v == 0 {
                acc.add(
                    Rule::RepeatedHighPoint,
                    *tops.last().expect("non-empty"),
                    None,
                    2.0 * (distinct - 1) as f32,
                );
            }
            if max - min > 16 {
                acc.add(Rule::WideRange, list[list.len() - 1], None, 2.0);
            }
        }
    }

    /// A hash of the last three notes of every voice that have started by step `k`.
    fn signature(&self, k: usize, a: &[i32]) -> u64 {
        let t = self.steps[k];
        let mut parts = Vec::with_capacity(2 * self.voice_slots.len());
        for list in &self.voice_slots {
            let started = list.partition_point(|&i| self.slots[i].start <= t);
            for &i in &list[started.saturating_sub(3)..started] {
                parts.push(a[i] as u64);
            }
            parts.push(u64::MAX);
        }
        hash(&parts)
    }

    /// The assignment with fixed notes filled in and free notes unset.
    fn initial(&self) -> Vec<i32> {
        self.slots
            .iter()
            .map(|s| s.fixed.unwrap_or(UNSET))
            .collect()
    }

    /// Scores a finished assignment and works out the alternatives.
    fn finish(&self, a: Vec<i32>) -> Solution {
        let n = self.slots.len();
        let mut violations = Vec::new();
        let cost = self.score(&a, Some(&mut violations));

        // For each free note, the best alternative pitch with everything else unchanged.
        let alternatives = (0..n)
            .map(|i| {
                if !self.is_free(i) {
                    return None;
                }
                let w = self.window(i);
                let here = self.partial_score(&a, w);
                let mut trial = a.clone();
                self.candidates[i]
                    .iter()
                    .filter(|&&c| c != a[i])
                    .map(|&c| {
                        trial[i] = c;
                        (c, self.partial_score(&trial, w) - here)
                    })
                    .min_by(|x, y| x.1.total_cmp(&y.1))
            })
            .collect();

        Solution {
            slots: self.slots.clone(),
            pitches: a,
            cost,
            violations,
            alternatives,
        }
    }

    /// The steps whose costs can depend on slot `i`: from its onset to the end of the
    /// measure after next (rules look back at most about a measure).
    fn window(&self, i: usize) -> (usize, usize) {
        let s = self.slots[i];
        let k0 = self.steps.partition_point(|&t| t < s.start);
        let until = self
            .p
            .measures
            .get(s.measure + 3)
            .map_or(Ticks::MAX, |m| m.0)
            .max(s.end());
        let k1 = self.steps.partition_point(|&t| t < until);
        (k0, k1)
    }

    /// Cost of steps `k0..k1` plus the whole-line terms.
    fn partial_score(&self, a: &[i32], (k0, k1): (usize, usize)) -> f32 {
        let mut acc = Acc {
            total: 0.0,
            rules: self.p.rules,
            rec: None,
        };
        for k in k0..k1 {
            self.step_terms(k, a, &mut acc);
        }
        self.final_terms(a, &mut acc);
        acc.total
    }

    /// Local search after the beam: change one note, or two neighbouring notes of a voice,
    /// whenever that lowers the total cost. Fixes choices the beam made too early.
    fn improve(&self, a: &mut [i32]) {
        self.improve_passes(a, 6);
    }

    fn improve_passes(&self, a: &mut [i32], passes: usize) {
        let free: Vec<usize> = (0..self.slots.len()).filter(|&i| self.is_free(i)).collect();
        let pairs: Vec<(usize, usize)> = self
            .voice_slots
            .iter()
            .flat_map(|list| list.windows(2).map(|w| (w[0], w[1])))
            .filter(|&(x, y)| self.is_free(x) && self.is_free(y))
            .collect();
        for _pass in 0..passes {
            let mut improved = false;
            for &i in &free {
                let w = self.window(i);
                let mut best = self.partial_score(a, w);
                let mut best_c = a[i];
                for &c in &self.candidates[i] {
                    if c == best_c {
                        continue;
                    }
                    a[i] = c;
                    let trial = self.partial_score(a, w);
                    if trial < best - 1e-4 {
                        best = trial;
                        best_c = c;
                        improved = true;
                    }
                }
                a[i] = best_c;
            }
            for &(i, j) in &pairs {
                let w = (self.window(i).0, self.window(j).1);
                let (wi, wj) = (a[i], a[j]);
                let mut best = self.partial_score(a, w);
                let (mut bi, mut bj) = (wi, wj);
                for &ci in &self.candidates[i] {
                    // Only nearby notes: pairs nudge a phrase, they don't rewrite it.
                    if (ci - wi).abs() > 7 {
                        continue;
                    }
                    for &cj in &self.candidates[j] {
                        if (cj - wj).abs() > 7 || (ci == wi && cj == wj) {
                            continue;
                        }
                        a[i] = ci;
                        a[j] = cj;
                        let trial = self.partial_score(a, w);
                        if trial < best - 1e-4 {
                            best = trial;
                            bi = ci;
                            bj = cj;
                            improved = true;
                        }
                    }
                }
                a[i] = bi;
                a[j] = bj;
            }
            if !improved {
                break;
            }
        }
    }

    fn score(&self, a: &[i32], rec: Option<&mut Vec<Violation>>) -> f32 {
        let mut acc = Acc {
            total: 0.0,
            rules: self.p.rules,
            rec,
        };
        for k in 0..self.steps.len() {
            self.step_terms(k, a, &mut acc);
        }
        self.final_terms(a, &mut acc);
        acc.total
    }
}

#[derive(Clone)]
struct State {
    a: Vec<i32>,
    /// Cost of all steps before the current one.
    base: f32,
    cost: f32,
}

fn beam_search(ctx: &Ctx, start: &[i32], width: usize, per: usize) -> Vec<i32> {
    let mut beam = vec![State {
        a: start.to_vec(),
        base: 0.0,
        cost: 0.0,
    }];
    for k in 0..ctx.steps.len() {
        let free: Vec<usize> = ctx.onsets[k]
            .iter()
            .copied()
            .filter(|&i| ctx.is_free(i))
            .collect();
        if free.is_empty() {
            for st in beam.iter_mut() {
                let mut acc = Acc {
                    total: 0.0,
                    rules: ctx.p.rules,
                    rec: None,
                };
                ctx.step_terms(k, &st.a, &mut acc);
                st.cost = st.base + acc.total;
            }
        }
        for &slot in &free {
            let mut next = Vec::with_capacity(beam.len() * ctx.candidates[slot].len());
            for st in &beam {
                for &c in &ctx.candidates[slot] {
                    let mut a = st.a.clone();
                    a[slot] = c;
                    let mut acc = Acc {
                        total: 0.0,
                        rules: ctx.p.rules,
                        rec: None,
                    };
                    ctx.step_terms(k, &a, &mut acc);
                    next.push(State {
                        a,
                        base: st.base,
                        cost: st.base + acc.total,
                    });
                }
            }
            next.sort_by(|x, y| x.cost.total_cmp(&y.cost));
            // Keep at most `per` paths for each recent melodic context, so the beam holds
            // genuinely different continuations rather than near-copies.
            let mut counts: std::collections::HashMap<u64, usize> =
                std::collections::HashMap::with_capacity(width * 2);
            let mut kept = 0;
            next.retain(|st| {
                if kept >= width {
                    return false;
                }
                let c = counts.entry(ctx.signature(k, &st.a)).or_insert(0);
                *c += 1;
                let keep = *c <= per;
                kept += keep as usize;
                keep
            });
            if next.is_empty() {
                break; // a voice with an empty range: leave it unset
            }
            beam = next;
        }
        for st in beam.iter_mut() {
            st.base = st.cost;
        }
    }
    // Add the whole-line terms and pick the best.
    let best = beam
        .into_iter()
        .map(|st| {
            let mut acc = Acc {
                total: 0.0,
                rules: ctx.p.rules,
                rec: None,
            };
            ctx.final_terms(&st.a, &mut acc);
            (st.cost + acc.total, st.a)
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, a)| a)
        .unwrap_or_default();
    best.iter()
        .map(|&x| if x == UNSET { 60 } else { x })
        .collect()
}

/// A fast, rougher answer: a narrow beam and one polishing pass. Use it while [`solve`]
/// runs in the background.
pub fn solve_quick(problem: &Problem) -> Solution {
    let ctx = Ctx::new(problem);
    let start = ctx.initial();
    let mut a = beam_search(&ctx, &start, 16, 3);
    ctx.improve_passes(&mut a, 1);
    ctx.finish(a)
}

/// Solves a counterpoint problem.
pub fn solve(problem: &Problem) -> Solution {
    let ctx = Ctx::new(problem);
    let start = ctx.initial();
    // Beam search from a few angles (wide and narrow, more or less merging of similar
    // paths), each polished by local search, in parallel; keep the cheapest. More free
    // voices mean more branching per step, so the beam narrows to keep time in check.
    let free_voices = problem
        .voices
        .iter()
        .filter(|v| v.fixed.is_none())
        .count()
        .max(1);
    let width = (problem.beam / free_voices.div_ceil(2)).clamp(4, 1024);
    let configs = [(width, 3), (width / 2, 8), (width * 2, 1)];
    let a = std::thread::scope(|scope| {
        let ctx = &ctx;
        let start = &start;
        let handles: Vec<_> = configs
            .iter()
            .map(|&(w, per)| {
                scope.spawn(move || {
                    let mut a = beam_search(ctx, start, w, per);
                    ctx.improve(&mut a);
                    (ctx.score(&a, None), a)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("solver thread"))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, a)| a)
            .expect("at least one config")
    });

    ctx.finish(a)
}

#[cfg(test)]
mod tests;
