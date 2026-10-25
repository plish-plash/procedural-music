//! Stateless hashing randomness: the same inputs always give the same number.

/// SplitMix64 finalizer.
pub fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Hashes several values into one.
pub fn hash(parts: &[u64]) -> u64 {
    parts
        .iter()
        .fold(0x51_7cc1_b727_220a_u64, |acc, &p| mix(acc ^ mix(p)))
}

/// Seeds run from 0 to 9999 (the range of a node's Seed parameter).
pub const SEEDS: u64 = 10_000;

/// The seed an unlocked node plays from in a pass with this salt. Locking a node with
/// this as its seed keeps exactly what it was playing.
pub fn fresh_seed(seed: u64, salt: u64) -> u64 {
    hash(&[seed, salt]) % SEEDS
}

/// The seed a random node draws from: its own when locked, a fresh one each pass when not.
pub fn stream(lock: bool, seed: u64, salt: u64) -> u64 {
    if lock {
        seed
    } else {
        fresh_seed(seed, salt)
    }
}

/// A number in `0..n` (n > 0) from a hash.
pub fn below(h: u64, n: u64) -> u64 {
    if n == 0 {
        0
    } else {
        h % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_spread() {
        assert_eq!(hash(&[1, 2, 3]), hash(&[1, 2, 3]));
        assert_ne!(hash(&[1, 2, 3]), hash(&[1, 2, 4]));
        let mut counts = [0u32; 4];
        for i in 0..4000 {
            counts[below(hash(&[i]), 4) as usize] += 1;
        }
        assert!(
            counts.iter().all(|&c| (800..1200).contains(&c)),
            "{counts:?}"
        );
    }
}
