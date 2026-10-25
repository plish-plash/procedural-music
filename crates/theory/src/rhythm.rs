//! Rhythms: onsets over a fixed length that loops.

use core::fmt;

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
    /// Chance the onset plays, 0 to 100. Evaluated by the graph, not here.
    pub probability: u8,
}

impl Default for Onset {
    fn default() -> Self {
        Onset {
            offset: 0,
            duration: 0,
            velocity: Onset::NORMAL,
            probability: 100,
        }
    }
}

impl Onset {
    /// Velocity of an ordinary hit.
    pub const NORMAL: u8 = 96;
    /// Velocity of an accented hit.
    pub const ACCENT: u8 = 127;
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

    /// Parses a step grid, one character per step of `step` ticks:
    /// `x` hit, `X` accented hit, `.` or `-` rest, `_` tie (extends the previous hit).
    /// Spaces and `|` are ignored. Returns `None` on other characters or too many hits.
    pub fn from_grid(pattern: &str, step: Ticks) -> Option<Rhythm> {
        let mut rhythm = Rhythm::new(1);
        let mut pos: Ticks = 0;
        for c in pattern.chars() {
            match c {
                'x' | 'X' => {
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
                            probability: 100,
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

    /// Onsets of both rhythms; where both hit, the louder one wins. Lengths must match.
    pub fn union(&self, other: &Rhythm) -> Option<Rhythm> {
        if self.length != other.length {
            return None;
        }
        let mut out = *self;
        for o in other.onsets.iter() {
            match out.onsets.binary_search_by_key(&o.offset, |x| x.offset) {
                Ok(i) if out.onsets[i].velocity >= o.velocity => {}
                _ => out.add(*o).ok()?,
            }
        }
        Some(out)
    }

    /// Onsets of `self` where `other` also hits. Lengths must match.
    pub fn intersection(&self, other: &Rhythm) -> Option<Rhythm> {
        self.filter_by(other, true)
    }

    /// Onsets of `self` where `other` does not hit. Lengths must match.
    pub fn difference(&self, other: &Rhythm) -> Option<Rhythm> {
        self.filter_by(other, false)
    }

    /// Onsets where exactly one of the two rhythms hits. Lengths must match.
    pub fn xor(&self, other: &Rhythm) -> Option<Rhythm> {
        let a = self.difference(other)?;
        let b = other.difference(self)?;
        a.union(&b)
    }

    fn filter_by(&self, other: &Rhythm, keep_shared: bool) -> Option<Rhythm> {
        if self.length != other.length {
            return None;
        }
        let mut out = *self;
        out.onsets
            .retain(|o| other.hits_at(o.offset) == keep_shared);
        Some(out)
    }

    /// Whether an onset starts exactly at `offset`.
    pub fn hits_at(&self, offset: Ticks) -> bool {
        self.onsets
            .binary_search_by_key(&offset, |o| o.offset)
            .is_ok()
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

    /// Calls `f(absolute_time, onset)` for every onset starting in `[start, end)`,
    /// treating the rhythm as looping forever from time 0.
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
                if t >= start {
                    f(t, o);
                }
            }
            cycle += 1;
        }
    }
}

impl fmt::Display for Rhythm {
    /// A step grid like `x..x..x.`, on the finest step that fits the onsets and the grid it was built on.
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
                    f.write_str(if o.velocity >= Onset::ACCENT {
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
        let short = Rhythm::from_grid("x.", SIXTEENTH).unwrap();
        assert!(a.union(&short).is_none());
    }

    #[test]
    fn swing_moves_offbeats() {
        let eighths = Rhythm::from_grid("xxxx", PPQ / 2).unwrap();
        let swung = eighths.swing(PPQ / 2, 67);
        let offsets: std::vec::Vec<_> = swung.onsets().iter().map(|o| o.offset).collect();
        assert_eq!(offsets, [0, 643, 960, 1603]);
    }

    #[test]
    fn looping_window() {
        let r = Rhythm::from_grid("x..x", SIXTEENTH).unwrap(); // hits at 0 and 720, length 960
        let mut times = std::vec::Vec::new();
        r.for_each_in(500, 2500, |t, _| times.push(t));
        assert_eq!(times, [720, 960, 1680, 1920]);
    }
}
