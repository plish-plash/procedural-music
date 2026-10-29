//! Rhythms: onsets over a fixed length that loops.

use core::fmt;

use crate::chance::Chance;
use crate::fixed::FixedVec;
use crate::time::Ticks;
use crate::MAX_ONSETS;

/// One hit in a rhythm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Onset {
    /// Start, in ticks from the beginning of the rhythm.
    pub offset: Ticks,
    /// Gate length in ticks.
    pub duration: Ticks,
    /// 1 to 127.
    pub velocity: u8,
    /// Whether it plays on a given pass of the loop (rolled at its absolute time; see
    /// [`Rhythm::for_each_in`]). A hit that never plays is dropped: it still counts as a
    /// hit, but is silent.
    pub chance: Chance,
}

impl Default for Onset {
    fn default() -> Self {
        Onset {
            offset: 0,
            duration: 0,
            velocity: Onset::NORMAL,
            chance: Chance::ALWAYS,
        }
    }
}

impl Onset {
    /// Velocity of an ordinary hit.
    pub const NORMAL: u8 = 96;
    /// Velocity of an accented hit.
    pub const ACCENT: u8 = 127;

    /// How often it plays, 0 to 100.
    pub fn probability(&self) -> u8 {
        self.chance.percent_of_time()
    }
}

/// A looping pattern of onsets, sorted by offset, with at most one onset per offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rhythm {
    length: Ticks,
    /// Display grid hint: the step size the rhythm was built on.
    grid: Ticks,
    onsets: FixedVec<Onset, MAX_ONSETS>,
}

impl Rhythm {
    /// An empty rhythm of the given length (at least 1 tick).
    pub fn new(length: Ticks) -> Rhythm {
        let length = length.max(1);
        Rhythm {
            length,
            grid: length,
            onsets: FixedVec::new(),
        }
    }

    /// The same rhythm with another display grid.
    pub fn with_grid(mut self, grid: Ticks) -> Rhythm {
        self.grid = grid.max(1);
        self
    }

    /// Every onset given a further `percent` chance of playing (accents unchanged when
    /// `keep_accents`), rolled from `seed`.
    pub fn with_chance(&self, percent: u8, keep_accents: bool, seed: u64) -> Rhythm {
        let roll = Chance::percent(seed, percent);
        let mut out = *self;
        for o in out.onsets.iter_mut() {
            if !(keep_accents && o.velocity >= Onset::ACCENT) {
                o.chance = o.chance.and(roll);
            }
        }
        out
    }

    /// Every onset's velocity replaced by `f(onset)`.
    pub fn with_velocities(&self, mut f: impl FnMut(&Onset) -> u8) -> Rhythm {
        let mut out = *self;
        for o in out.onsets.iter_mut() {
            o.velocity = f(o).clamp(1, 127);
        }
        out
    }

    /// Each onset split into `n` equal hits (ratchets). `None` if that is more than
    /// [`MAX_ONSETS`] hits.
    pub fn subdivide(&self, n: u32) -> Option<Rhythm> {
        let n = n.max(1);
        let mut out = Rhythm {
            onsets: FixedVec::new(),
            ..*self
        };
        for o in self.onsets.iter() {
            let each = (o.duration / n).max(1);
            for k in 0..n {
                out.add(Onset {
                    offset: o.offset + k * each,
                    duration: each,
                    ..*o
                })
                .ok()?;
            }
        }
        out.grid = (self.grid / n).max(1);
        Some(out)
    }

    /// Parses a step grid, one character per step of `step` ticks:
    /// `x` hit, `X` accented hit, `o` dropped hit (counts as a hit but doesn't play),
    /// `.` or `-` rest, `_` tie (extends the previous hit). Spaces and `|` are ignored.
    /// Returns `None` on other characters or too many hits.
    pub fn from_grid(pattern: &str, step: Ticks) -> Option<Rhythm> {
        let mut rhythm = Rhythm::new(1);
        let mut pos: Ticks = 0;
        for c in pattern.chars() {
            match c {
                'x' | 'X' | 'o' => {
                    let velocity = if c == 'X' {
                        Onset::ACCENT
                    } else {
                        Onset::NORMAL
                    };
                    rhythm
                        .onsets
                        .push(Onset {
                            offset: pos,
                            duration: step,
                            velocity,
                            chance: if c == 'o' { Chance::NEVER } else { Chance::ALWAYS },
                        })
                        .ok()?;
                }
                '_' => {
                    if let Some(last) = rhythm.onsets.last_mut() {
                        last.duration += step;
                    }
                }
                '.' | '-' => {}
                ' ' | '|' => continue,
                _ => return None,
            }
            pos += step;
        }
        rhythm.length = pos.max(1);
        rhythm.grid = step.max(1);
        Some(rhythm)
    }

    /// A Euclidean rhythm: `hits` onsets spread as evenly as possible over `steps` steps.
    /// E(3, 8) is the tresillo `x..x..x.`. Returns `None` if `hits > steps` or `steps == 0`.
    pub fn euclidean(hits: u32, steps: u32, step: Ticks) -> Option<Rhythm> {
        if steps == 0 || hits > steps || hits as usize > MAX_ONSETS {
            return None;
        }
        let mut rhythm = Rhythm::new(steps * step);
        rhythm.grid = step.max(1);
        for i in 0..steps {
            if (i * hits) % steps < hits {
                rhythm
                    .onsets
                    .push(Onset {
                        offset: i * step,
                        duration: step,
                        ..Onset::default()
                    })
                    .ok()?;
            }
        }
        Some(rhythm)
    }

    pub fn length(&self) -> Ticks {
        self.length
    }

    /// The step size the rhythm was built on (a display hint).
    pub fn grid(&self) -> Ticks {
        self.grid
    }

    pub fn onsets(&self) -> &[Onset] {
        &self.onsets
    }

    pub fn len(&self) -> usize {
        self.onsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.onsets.is_empty()
    }

    /// Adds an onset (replacing one at the same offset). Offsets wrap into the length.
    pub fn add(&mut self, mut onset: Onset) -> Result<(), crate::CapacityError> {
        onset.offset %= self.length;
        match self
            .onsets
            .binary_search_by_key(&onset.offset, |o| o.offset)
        {
            Ok(i) => {
                self.onsets[i] = onset;
                Ok(())
            }
            Err(i) => self.onsets.insert(i, onset),
        }
    }

    /// The rhythm shifted later by `ticks` (wrapping). Negative values shift earlier.
    pub fn rotate(&self, ticks: i64) -> Rhythm {
        let len = self.length as i64;
        let mut out = Rhythm {
            onsets: FixedVec::new(),
            ..*self
        };
        for o in self.onsets.iter() {
            let offset = (o.offset as i64 + ticks).rem_euclid(len) as Ticks;
            let _ = out.add(Onset { offset, ..*o });
        }
        out
    }

    /// The length two rhythms loop over together (the least common multiple of their
    /// lengths), or `None` if that doesn't fit in [`Ticks`].
    pub fn common_length(&self, other: &Rhythm) -> Option<Ticks> {
        (self.length / gcd(self.length, other.length)).checked_mul(other.length)
    }

    /// Calls `f(offset, onset)` for this rhythm's onsets, looped to fill `length` (a
    /// multiple of its own); `f` returns `false` to stop.
    fn each_looped(&self, length: Ticks, mut f: impl FnMut(Ticks, &Onset) -> bool) {
        if self.onsets.is_empty() {
            return;
        }
        let mut base = 0;
        while base < length {
            for o in self.onsets.iter() {
                if !f(base + o.offset, o) {
                    return;
                }
            }
            base += self.length;
        }
    }

    /// An empty rhythm for the result of combining `self` with `other` over `length`.
    fn combined(&self, other: &Rhythm, length: Ticks) -> Rhythm {
        Rhythm {
            length,
            grid: gcd(self.grid, other.grid).max(1),
            onsets: FixedVec::new(),
            ..*self
        }
    }

    /// Onsets of both rhythms, playing when either does; where both hit, the louder one's
    /// velocity and length win. Rhythms of different lengths are looped until they line up
    /// (see [`Rhythm::common_length`]). `None` if that is more than [`MAX_ONSETS`] hits.
    pub fn union(&self, other: &Rhythm) -> Option<Rhythm> {
        let length = self.common_length(other)?;
        let mut out = self.combined(other, length);
        let mut fits = true;
        for r in [self, other] {
            r.each_looped(length, |offset, o| {
                let new = Onset { offset, ..*o };
                match out.onsets.binary_search_by_key(&offset, |x| x.offset) {
                    Ok(i) => {
                        let have = out.onsets[i];
                        let chance = have.chance.or(o.chance);
                        out.onsets[i] = Onset { chance, ..if o.velocity > have.velocity { new } else { have } };
                    }
                    Err(i) => fits = out.onsets.insert(i, new).is_ok(),
                }
                fits
            });
        }
        fits.then_some(out)
    }

    /// Onsets of `self` where `other` also hits, playing when both do; looped as in
    /// [`Rhythm::union`].
    pub fn intersection(&self, other: &Rhythm) -> Option<Rhythm> {
        self.filter_by(other, |a, b| b.map(|b| a.and(b)))
    }

    /// Onsets of `self` where `other` doesn't hit, looped as in [`Rhythm::union`]. Where
    /// `other`'s hit is left to chance, the onset stays, and drops when that hit plays.
    pub fn difference(&self, other: &Rhythm) -> Option<Rhythm> {
        self.filter_by(other, |a, b| match b {
            Some(b) if b.is_always() => None,
            Some(b) => Some(a.and_not(b)),
            None => Some(a),
        })
    }

    /// Onsets of `self`, dropped where `other` hits: they stay hits (counted by what
    /// steps along a rhythm) but don't play. Looped as in [`Rhythm::union`].
    pub fn drop_hits(&self, other: &Rhythm) -> Option<Rhythm> {
        self.filter_by(other, |a, b| Some(b.map_or(a, |b| a.and_not(b))))
    }

    /// Onsets where exactly one of the two rhythms hits (or plays), looped as in
    /// [`Rhythm::union`].
    pub fn xor(&self, other: &Rhythm) -> Option<Rhythm> {
        let a = self.difference(other)?;
        let b = other.difference(self)?;
        a.union(&b)
    }

    /// `self`'s onsets, looped to the common length, with chances from `f(own chance, the
    /// chance of other's onset there)`; `None` from `f` leaves the onset out.
    fn filter_by(&self, other: &Rhythm, f: impl Fn(Chance, Option<Chance>) -> Option<Chance>) -> Option<Rhythm> {
        let length = self.common_length(other)?;
        let mut out = self.combined(other, length);
        let mut fits = true;
        self.each_looped(length, |offset, o| {
            if let Some(chance) = f(o.chance, other.onset_at(offset % other.length).map(|b| b.chance)) {
                fits = out.onsets.push(Onset { offset, chance, ..*o }).is_ok();
            }
            fits
        });
        fits.then_some(out)
    }

    /// The onset starting exactly at `offset`, if any.
    pub fn onset_at(&self, offset: Ticks) -> Option<&Onset> {
        let i = self.onsets.binary_search_by_key(&offset, |o| o.offset).ok()?;
        Some(&self.onsets[i])
    }

    /// Whether an onset starts exactly at `offset`.
    pub fn hits_at(&self, offset: Ticks) -> bool {
        self.onset_at(offset).is_some()
    }

    /// Swing: onsets on the off-beat of each pair of `grid` steps move later.
    /// `percent` is where the off-beat lands within the pair: 50 is straight, 67 is a
    /// triplet feel, 75 is a dotted feel.
    pub fn swing(&self, grid: Ticks, percent: u8) -> Rhythm {
        let pair = grid * 2;
        let swung = pair * percent.clamp(1, 99) as Ticks / 100;
        let mut out = Rhythm {
            onsets: FixedVec::new(),
            ..*self
        };
        for o in self.onsets.iter() {
            let within = o.offset % pair;
            let offset = if within == grid {
                o.offset - grid + swung
            } else {
                o.offset
            };
            let _ = out.add(Onset { offset, ..*o });
        }
        out
    }

    /// Every gate length scaled to `percent` of its current value (at least 1 tick).
    pub fn with_gate(&self, percent: u32) -> Rhythm {
        let mut out = *self;
        for o in out.onsets.iter_mut() {
            o.duration = (o.duration * percent / 100).max(1);
        }
        out
    }

    /// Calls `f(absolute_time, onset)` for every onset starting in `[start, end)` that
    /// plays (see [`Onset::chance`]), treating the rhythm as looping forever from time 0.
    pub fn for_each_in(&self, start: Ticks, end: Ticks, mut f: impl FnMut(Ticks, &Onset)) {
        if start >= end || self.onsets.is_empty() {
            return;
        }
        let mut cycle = start / self.length;
        loop {
            let base = cycle * self.length;
            if base >= end {
                break;
            }
            for o in self.onsets.iter() {
                let t = base + o.offset;
                if t >= end {
                    break;
                }
                if t >= start && o.chance.plays(t) {
                    f(t, o);
                }
            }
            cycle += 1;
        }
    }
}

impl fmt::Display for Rhythm {
    /// A step grid like `x..x..o.` (as [`Rhythm::from_grid`] reads it), on the finest step
    /// that fits the onsets and the grid it was built on.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let step = self
            .onsets
            .iter()
            .fold(gcd(self.length, self.grid), |g, o| {
                gcd(gcd(g, o.offset), o.duration)
            });
        let step = step.max(1);
        let steps = self.length / step;
        if steps > 256 {
            return write!(
                f,
                "<{} onsets over {} ticks>",
                self.onsets.len(),
                self.length
            );
        }
        let mut i = 0;
        while i < steps {
            let t = i * step;
            match self.onsets.iter().find(|o| o.offset == t) {
                Some(o) => {
                    f.write_str(if o.chance.is_never() {
                        "o"
                    } else if o.velocity >= Onset::ACCENT {
                        "X"
                    } else {
                        "x"
                    })?;
                    let ties = (o.duration / step).saturating_sub(1).min(steps - i - 1);
                    for _ in 0..ties {
                        f.write_str("_")?;
                    }
                    i += 1 + ties;
                }
                None => {
                    f.write_str(".")?;
                    i += 1;
                }
            }
        }
        Ok(())
    }
}

fn gcd(a: Ticks, b: Ticks) -> Ticks {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::PPQ;
    use std::format;

    const SIXTEENTH: Ticks = PPQ / 4;

    #[test]
    fn grid_round_trip() {
        let r = Rhythm::from_grid("X..x|x_.x", SIXTEENTH).unwrap();
        assert_eq!(r.length(), 8 * SIXTEENTH);
        assert_eq!(r.len(), 4);
        assert_eq!(r.onsets()[2].duration, 2 * SIXTEENTH);
        assert_eq!(format!("{r}"), "X..xx_.x");
        assert!(Rhythm::from_grid("x?x", SIXTEENTH).is_none());
    }

    #[test]
    fn euclidean_patterns() {
        let show = |k, n| format!("{}", Rhythm::euclidean(k, n, SIXTEENTH).unwrap());
        assert_eq!(show(3, 8), "x..x..x.");
        assert_eq!(show(4, 16), "x...x...x...x...");
        assert_eq!(show(0, 4), "....");
        assert_eq!(Rhythm::euclidean(5, 8, SIXTEENTH).unwrap().len(), 5);
        assert!(Rhythm::euclidean(9, 8, SIXTEENTH).is_none());
    }

    #[test]
    fn combine_and_rotate() {
        let a = Rhythm::from_grid("x.x.", SIXTEENTH).unwrap();
        let b = Rhythm::from_grid("xx..", SIXTEENTH).unwrap();
        assert_eq!(format!("{}", a.union(&b).unwrap()), "xxx.");
        assert_eq!(format!("{}", a.intersection(&b).unwrap()), "x...");
        assert_eq!(format!("{}", a.xor(&b).unwrap()), ".xx.");
        assert_eq!(format!("{}", a.rotate(SIXTEENTH as i64)), ".x.x");
        assert_eq!(format!("{}", a.rotate(-(SIXTEENTH as i64))), ".x.x");
    }

    #[test]
    fn combining_follows_each_hits_chance() {
        let span = 64 * 16 * SIXTEENTH;
        let played = |r: &Rhythm| {
            let mut v = std::vec::Vec::new();
            r.for_each_in(0, span, |t, _| v.push(t));
            v
        };
        let all = Rhythm::from_grid("xxxx", SIXTEENTH).unwrap();
        let a = Rhythm::from_grid("xx.x", SIXTEENTH).unwrap().with_chance(50, false, 3);
        let b = Rhythm::from_grid("x.x.", SIXTEENTH).unwrap().with_chance(50, false, 9);
        let (pa, pb) = (played(&a), played(&b));
        for t in (0..span).step_by(SIXTEENTH as usize) {
            let (ia, ib) = (pa.contains(&t), pb.contains(&t));
            let check = |r: Rhythm, expect: bool, what: &str| assert_eq!(played(&r).contains(&t), expect, "{what} at {t}");
            check(a.union(&b).unwrap(), ia || ib, "or");
            check(a.intersection(&b).unwrap(), ia && ib, "and");
            check(a.difference(&b).unwrap(), ia && !ib, "a not b");
            check(a.xor(&b).unwrap(), ia != ib, "xor");
            check(all.drop_hits(&b).unwrap(), !ib, "drop");
        }
        // B's chancy hits leave A's as hits that sometimes drop; certain ones remove or drop them.
        assert_eq!(all.difference(&b).unwrap().len(), 4);
        let certain = Rhythm::from_grid("x.x.", SIXTEENTH).unwrap();
        assert_eq!(format!("{}", all.difference(&certain).unwrap()), ".x.x");
        assert_eq!(format!("{}", all.drop_hits(&certain).unwrap()), "oxox");
        // Dropped hits read and print as 'o', and And keeps them.
        let dropped = Rhythm::from_grid("xo..", SIXTEENTH).unwrap();
        assert_eq!(format!("{dropped}"), "xo..");
        assert_eq!(dropped.onsets()[1].probability(), 0);
        assert_eq!(format!("{}", dropped.intersection(&all).unwrap()), "xo..");
        assert_eq!(format!("{}", dropped.xor(&all).unwrap()), ".xxx", "a dropped hit doesn't cancel a played one");
    }

    #[test]
    fn combines_rhythms_of_different_lengths() {
        // 3 steps against 4: they line up every 12.
        let three = Rhythm::from_grid("x..", SIXTEENTH).unwrap();
        let four = Rhythm::from_grid("X...", SIXTEENTH).unwrap();
        let or = three.union(&four).unwrap();
        assert_eq!(or.length(), 12 * SIXTEENTH);
        assert_eq!(format!("{or}"), "X..xX.x.Xx..");
        assert_eq!(format!("{}", three.intersection(&four).unwrap()), "x...........");
        assert_eq!(format!("{}", three.xor(&four).unwrap()), "...xX.x.Xx..");
        assert_eq!(format!("{}", three.difference(&four).unwrap()), "...x..x..x..");
        // A shorter rhythm that divides the longer one keeps the longer length.
        let eighths = Rhythm::from_grid("x.", SIXTEENTH).unwrap();
        let a = Rhythm::from_grid("xx..xx..", SIXTEENTH).unwrap();
        assert_eq!(format!("{}", a.intersection(&eighths).unwrap()), "x...x...");
        // Each keeps its own hits' timing when the result is played.
        let mut times = std::vec::Vec::new();
        or.for_each_in(0, 24 * SIXTEENTH, |t, _| times.push(t / SIXTEENTH));
        assert_eq!(times, [0, 3, 4, 6, 8, 9, 12, 15, 16, 18, 20, 21]);
        // More hits than a rhythm holds.
        let busy = Rhythm::from_grid("xxxxxxxxxxxxxxxxx", SIXTEENTH).unwrap(); // 17 steps
        let sixteen = Rhythm::from_grid("xxxxxxxxxxxxxxxx", SIXTEENTH).unwrap();
        assert!(busy.union(&sixteen).is_none());
        assert!(busy.intersection(&Rhythm::new(SIXTEENTH * 16)).is_some(), "an empty result fits");
    }

    #[test]
    fn swing_moves_offbeats() {
        let eighths = Rhythm::from_grid("xxxx", PPQ / 2).unwrap();
        let swung = eighths.swing(PPQ / 2, 67);
        let offsets: std::vec::Vec<_> = swung.onsets().iter().map(|o| o.offset).collect();
        assert_eq!(offsets, [0, 643, 960, 1603]);
    }

    #[test]
    fn chance_and_ratchets() {
        let r = Rhythm::from_grid("xxxxxxxxxxxxxxxX", SIXTEENTH).unwrap();
        let count = |r: &Rhythm| {
            let mut n = 0;
            r.for_each_in(0, 100 * r.length(), |_, _| n += 1);
            n
        };
        assert_eq!(count(&r.with_chance(100, false, 7)), 1600);
        // Only the accent survives 0%.
        assert_eq!(count(&r.with_chance(0, true, 7)), 100);
        let half = count(&r.with_chance(50, false, 7));
        assert!((700..900).contains(&half), "{half}");
        // The same seed picks the same hits; another seed others.
        assert_eq!(half, count(&r.with_chance(50, false, 7)));
        let mut a = std::vec::Vec::new();
        let mut b = std::vec::Vec::new();
        r.with_chance(50, false, 7).for_each_in(0, 3840, |t, _| a.push(t));
        r.with_chance(50, false, 8).for_each_in(0, 3840, |t, _| b.push(t));
        assert_ne!(a, b);

        let s = Rhythm::from_grid("x_..x...", SIXTEENTH).unwrap().subdivide(2).unwrap();
        assert_eq!(format!("{s}"), "x_x_....xx......");
    }

    #[test]
    fn looping_window() {
        let r = Rhythm::from_grid("x..x", SIXTEENTH).unwrap(); // hits at 0 and 720, length 960
        let mut times = std::vec::Vec::new();
        r.for_each_in(500, 2500, |t, _| times.push(t));
        assert_eq!(times, [720, 960, 1680, 1920]);
    }
}
