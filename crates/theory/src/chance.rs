//! Whether a rhythm's hit plays on a given pass, decided by dice rolls.
//!
//! A roll passes some percent of the time, decided by its seed and the hit's absolute time,
//! so the same roll at the same time always agrees with itself. A hit's [`Chance`] depends
//! on up to [`MAX_ROLLS`] rolls through a truth table, so combining rhythms keeps exactly
//! which hits play: "A and B" plays where both A's and B's rolls pass, not on a new roll
//! with the same odds.

use crate::time::Ticks;

/// How many rolls one hit's chance can depend on. Combining more falls back to one roll
/// with the same odds (see [`Chance::combine`]).
pub const MAX_ROLLS: usize = 3;

/// One roll: passes `percent` of the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Roll {
    seed: u64,
    percent: u8,
}

impl Roll {
    const NONE: Roll = Roll { seed: 0, percent: 0 };

    fn passes(self, t: Ticks) -> bool {
        (chance_roll(self.seed, t) % 100) < self.percent as u64
    }
}

/// Whether a hit plays, as a function of up to [`MAX_ROLLS`] rolls: bit `i` of `table`
/// says whether it plays when the rolls that pass are the set bits of `i`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chance {
    rolls: [Roll; MAX_ROLLS],
    n: u8,
    table: u8,
}

impl Chance {
    /// Always plays.
    pub const ALWAYS: Chance = Chance { rolls: [Roll::NONE; MAX_ROLLS], n: 0, table: 1 };
    /// Never plays: a dropped hit.
    pub const NEVER: Chance = Chance { rolls: [Roll::NONE; MAX_ROLLS], n: 0, table: 0 };

    /// Plays `percent` of the time, rolled from `seed`.
    pub fn percent(seed: u64, percent: u8) -> Chance {
        match percent {
            0 => Chance::NEVER,
            100.. => Chance::ALWAYS,
            _ => {
                let mut rolls = [Roll::NONE; MAX_ROLLS];
                rolls[0] = Roll { seed, percent };
                Chance { rolls, n: 1, table: 0b10 }
            }
        }
    }

    /// Whether the hit plays at absolute time `t`.
    pub fn plays(&self, t: Ticks) -> bool {
        let row = (0..self.n as usize).fold(0, |row, i| row | (self.rolls[i].passes(t) as usize) << i);
        self.table >> row & 1 == 1
    }

    pub fn is_always(&self) -> bool {
        *self == Chance::ALWAYS
    }

    pub fn is_never(&self) -> bool {
        *self == Chance::NEVER
    }

    /// How often it plays, 0 to 100.
    pub fn percent_of_time(&self) -> u8 {
        odds(&self.rolls[..self.n as usize], |row| self.table >> row & 1 == 1)
    }

    pub fn and(self, other: Chance) -> Chance {
        self.combine(other, |a, b| a && b)
    }

    pub fn or(self, other: Chance) -> Chance {
        self.combine(other, |a, b| a || b)
    }

    /// Plays when this one does and `other` doesn't.
    pub fn and_not(self, other: Chance) -> Chance {
        self.combine(other, |a, b| a && !b)
    }

    /// Plays when `op` of whether each one plays is true. Rolls the two share count once.
    /// If together they depend on more than [`MAX_ROLLS`] rolls, the result is one roll
    /// with the same odds (no longer tied to the rolls it came from).
    pub fn combine(self, other: Chance, op: impl Fn(bool, bool) -> bool) -> Chance {
        let mut rolls = [Roll::NONE; 2 * MAX_ROLLS];
        let mut n = 0;
        let mut position = |r: Roll| match rolls[..n].iter().position(|x| *x == r) {
            Some(i) => i,
            None => {
                rolls[n] = r;
                n += 1;
                n - 1
            }
        };
        let pa: [usize; MAX_ROLLS] = core::array::from_fn(|i| if i < self.n as usize { position(self.rolls[i]) } else { 0 });
        let pb: [usize; MAX_ROLLS] = core::array::from_fn(|i| if i < other.n as usize { position(other.rolls[i]) } else { 0 });
        let pick = |c: &Chance, pos: &[usize; MAX_ROLLS], row: usize| {
            let own = (0..c.n as usize).fold(0, |r, i| r | (row >> pos[i] & 1) << i);
            c.table >> own & 1 == 1
        };
        let plays = |row: usize| op(pick(&self, &pa, row), pick(&other, &pb, row));
        if n > MAX_ROLLS {
            let percent = odds(&rolls[..n], &plays);
            let seed = rolls[..n].iter().fold(0x2545_f491_4f6c_dd1d, |h, r| chance_roll(h ^ r.seed, r.percent as Ticks));
            return Chance::percent(seed, percent);
        }
        let mut out = Chance { rolls: [Roll::NONE; MAX_ROLLS], n: n as u8, table: 0 };
        out.rolls[..n].copy_from_slice(&rolls[..n]);
        for row in 0..1usize << n {
            out.table |= (plays(row) as u8) << row;
        }
        out.simplify()
    }

    /// Without the rolls that make no difference.
    fn simplify(mut self) -> Chance {
        let mut i = 0;
        while i < self.n as usize {
            let rows = 1usize << self.n;
            let bit = |row: usize| self.table >> row & 1;
            let matters = (0..rows).any(|row| row >> i & 1 == 0 && bit(row) != bit(row | 1 << i));
            if matters {
                i += 1;
                continue;
            }
            // Keep the rows where roll i fails, renumbered without it.
            let mut table = 0;
            let mut k = 0;
            for row in (0..rows).filter(|row| row >> i & 1 == 0) {
                table |= bit(row) << k;
                k += 1;
            }
            self.table = table;
            self.rolls.copy_within(i + 1..self.n as usize, i);
            self.n -= 1;
            self.rolls[self.n as usize] = Roll::NONE;
        }
        if self.n == 0 {
            self.table &= 1;
        }
        self
    }
}

/// The percent of the time `plays(row)` is true when each roll passes independently (row bit
/// `i` set when roll `i` passes), rounded.
fn odds(rolls: &[Roll], plays: impl Fn(usize) -> bool) -> u8 {
    let scale = 100u64.pow(rolls.len() as u32);
    let total: u64 = (0..1usize << rolls.len())
        .filter(|&row| plays(row))
        .map(|row| {
            rolls.iter().enumerate().fold(1u64, |w, (i, r)| {
                w * if row >> i & 1 == 1 { r.percent as u64 } else { 100 - r.percent as u64 }
            })
        })
        .sum();
    ((total * 100 + scale / 2) / scale) as u8
}

/// A well-mixed number from a seed and a time (SplitMix64).
fn chance_roll(seed: u64, t: Ticks) -> u64 {
    let mut x = seed ^ (t as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many of `n` times (one per tick) it plays.
    fn count(c: Chance, n: Ticks) -> u32 {
        (0..n).filter(|&t| c.plays(t)).count() as u32
    }

    #[test]
    fn constants_and_single_rolls() {
        assert_eq!(count(Chance::ALWAYS, 100), 100);
        assert_eq!(count(Chance::NEVER, 100), 0);
        assert!(Chance::percent(5, 100).is_always());
        assert!(Chance::percent(5, 0).is_never());
        let half = count(Chance::percent(5, 50), 10_000);
        assert!((4_700..5_300).contains(&half), "{half}");
        assert_eq!(Chance::percent(5, 30).percent_of_time(), 30);
    }

    #[test]
    fn combinations_follow_the_same_rolls() {
        let (a, b) = (Chance::percent(1, 50), Chance::percent(2, 40));
        for t in 0..2_000 {
            let (pa, pb) = (a.plays(t), b.plays(t));
            assert_eq!(a.and(b).plays(t), pa && pb);
            assert_eq!(a.or(b).plays(t), pa || pb);
            assert_eq!(a.and_not(b).plays(t), pa && !pb);
            assert_eq!(a.combine(b, |x, y| x != y).plays(t), pa != pb);
        }
        assert_eq!(a.and(b).percent_of_time(), 20);
        assert_eq!(a.and_not(b).percent_of_time(), 30);
        // A roll combined with itself stays one roll.
        assert_eq!(a.and(a), a);
        assert!(a.and_not(a).is_never());
        assert!(a.or(Chance::ALWAYS).is_always());
        assert_eq!(a.and(Chance::ALWAYS), a);
        assert!(a.and(Chance::NEVER).is_never());
    }

    #[test]
    fn too_many_rolls_keep_their_odds() {
        let abc = Chance::percent(1, 50).and(Chance::percent(2, 50)).and(Chance::percent(3, 50));
        let d = Chance::percent(4, 50);
        let all = abc.and(d);
        assert_eq!(all.n, 1, "one roll with the same odds");
        assert_eq!(all.percent_of_time(), 6); // 1/16, rounded
    }
}
