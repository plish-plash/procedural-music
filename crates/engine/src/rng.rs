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
