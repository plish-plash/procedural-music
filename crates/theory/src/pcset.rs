//! Pitch-class sets as bit masks.

use core::fmt;

use crate::fixed::FixedVec;
use crate::tuning::Tuning;

/// A set of pitch classes in a tuning, stored as a 64-bit mask (bit `n` = class `n`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PcSet {
    bits: u64,
    tuning: Tuning,
}

impl PcSet {
    /// The empty set.
    pub const fn empty(tuning: Tuning) -> PcSet {
        PcSet { bits: 0, tuning }
    }

    /// Every pitch class of the tuning.
    pub fn chromatic(tuning: Tuning) -> PcSet {
        PcSet {
            bits: full_mask(tuning),
            tuning,
        }
    }

    /// A set from pitch classes (values are reduced modulo the octave).
    pub fn from_pcs(tuning: Tuning, pcs: &[i32]) -> PcSet {
        let mut set = PcSet::empty(tuning);
        for &pc in pcs {
            set.insert(pc);
        }
        set
    }

    /// A set from a raw mask (bits above the octave are dropped).
    pub fn from_bits(tuning: Tuning, bits: u64) -> PcSet {
        PcSet {
            bits: bits & full_mask(tuning),
            tuning,
        }
    }

    pub const fn bits(self) -> u64 {
        self.bits
    }

    pub const fn tuning(self) -> Tuning {
        self.tuning
    }

    fn reduce(self, pc: i32) -> u32 {
        pc.rem_euclid(self.tuning.steps_per_octave()) as u32
    }

    pub fn insert(&mut self, pc: i32) {
        self.bits |= 1 << self.reduce(pc);
    }

    pub fn remove(&mut self, pc: i32) {
        self.bits &= !(1 << self.reduce(pc));
    }

    pub fn contains(self, pc: i32) -> bool {
        self.bits & (1 << self.reduce(pc)) != 0
    }

    pub const fn len(self) -> usize {
        self.bits.count_ones() as usize
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Pitch classes in ascending order from 0.
    pub fn iter(self) -> impl Iterator<Item = u8> {
        let bits = self.bits;
        (0..64u8).filter(move |&i| bits & (1 << i) != 0)
    }

    /// Pitch classes in ascending order starting from `start` and wrapping.
    pub fn iter_from(self, start: u8) -> impl Iterator<Item = u8> {
        let n = self.tuning.divisions();
        let bits = self.bits;
        (0..n)
            .map(move |i| (start + i) % n)
            .filter(move |&pc| bits & (1 << pc) != 0)
    }

    /// The set transposed by `steps` (rotation within the octave).
    pub fn transpose(self, steps: i32) -> PcSet {
        let n = self.tuning.divisions() as u32;
        let s = self.reduce(steps);
        if s == 0 {
            return self;
        }
        let rotated = (self.bits << s) | (self.bits >> (n - s));
        PcSet::from_bits(self.tuning, rotated)
    }

    /// The set inverted around pitch class 0 (pc → -pc).
    pub fn invert(self) -> PcSet {
        let mut out = PcSet::empty(self.tuning);
        for pc in self.iter() {
            out.insert(-(pc as i32));
        }
        out
    }

    pub fn union(self, other: PcSet) -> PcSet {
        self.check(other);
        PcSet {
            bits: self.bits | other.bits,
            ..self
        }
    }

    pub fn intersection(self, other: PcSet) -> PcSet {
        self.check(other);
        PcSet {
            bits: self.bits & other.bits,
            ..self
        }
    }

    pub fn difference(self, other: PcSet) -> PcSet {
        self.check(other);
        PcSet {
            bits: self.bits & !other.bits,
            ..self
        }
    }

    pub fn complement(self) -> PcSet {
        PcSet {
            bits: !self.bits & full_mask(self.tuning),
            ..self
        }
    }

    pub fn is_subset(self, other: PcSet) -> bool {
        self.check(other);
        self.bits & !other.bits == 0
    }

    /// Interval-class vector: entry `i` counts pairs of pitch classes `i + 1`
    /// steps apart (the shorter way round), for `i` up to half an octave.
    /// For 12-EDO this is the familiar 6-entry vector (major scale = 254361).
    pub fn interval_vector(self) -> FixedVec<u8, 32> {
        let n = self.tuning.steps_per_octave();
        let classes = (n / 2) as usize;
        let mut counts = [0u8; 32];
        let pcs: FixedVec<u8, 64> = self.iter().fold(FixedVec::new(), |mut v, pc| {
            let _ = v.push(pc);
            v
        });
        for (i, &a) in pcs.iter().enumerate() {
            for &b in &pcs[i + 1..] {
                let d = (b as i32 - a as i32).rem_euclid(n);
                let ic = d.min(n - d);
                if ic > 0 {
                    counts[ic as usize - 1] = counts[ic as usize - 1].saturating_add(1);
                }
            }
        }
        FixedVec::from_slice(&counts[..classes]).unwrap_or_default()
    }

    fn check(self, other: PcSet) {
        assert_eq!(
            self.tuning, other.tuning,
            "pitch-class sets in different tunings"
        );
    }
}

fn full_mask(tuning: Tuning) -> u64 {
    let n = tuning.divisions() as u32;
    if n >= 64 {
        u64::MAX
    } else {
        (1u64 << n) - 1
    }
}

impl fmt::Display for PcSet {
    /// "{0, 4, 7}"
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (i, pc) in self.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{pc}")?;
        }
        f.write_str("}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn twelve(pcs: &[i32]) -> PcSet {
        PcSet::from_pcs(Tuning::TWELVE, pcs)
    }

    #[test]
    fn transpose_wraps() {
        let c_major = twelve(&[0, 4, 7]);
        assert_eq!(c_major.transpose(5), twelve(&[5, 9, 0]));
        assert_eq!(c_major.transpose(-12), c_major);
        let t64 = Tuning::edo(64).unwrap();
        let s = PcSet::from_pcs(t64, &[0, 63]);
        assert_eq!(s.transpose(1), PcSet::from_pcs(t64, &[1, 0]));
    }

    #[test]
    fn set_operations() {
        let major = twelve(&[0, 2, 4, 5, 7, 9, 11]);
        let triad = twelve(&[0, 4, 7]);
        assert!(triad.is_subset(major));
        assert_eq!(major.complement().len(), 5);
        assert_eq!(major.difference(triad).len(), 4);
        assert_eq!(triad.invert(), twelve(&[0, 8, 5]));
    }

    #[test]
    fn interval_vectors() {
        let major = twelve(&[0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(major.interval_vector().as_slice(), &[2, 5, 4, 3, 6, 1]);
        let triad = twelve(&[0, 4, 7]);
        assert_eq!(triad.interval_vector().as_slice(), &[0, 0, 1, 1, 1, 0]);
    }

    #[test]
    fn iter_from_root() {
        let d_dorian = twelve(&[2, 4, 5, 7, 9, 11, 0]);
        let v: FixedVec<u8, 12> = FixedVec::from_slice(&[2, 4, 5, 7, 9, 11, 0]).unwrap();
        let mut got: FixedVec<u8, 12> = FixedVec::new();
        for pc in d_dorian.iter_from(2) {
            got.push(pc).unwrap();
        }
        assert_eq!(got, v);
    }
}
