//! Equal divisions of the octave.

/// The largest supported number of divisions (pitch-class sets are 64-bit masks).
pub const MAX_EDO: u8 = 64;

/// An equal division of the octave (EDO). 12-EDO is ordinary equal temperament.
///
/// Pitch steps are numbered so that C4 is step `5 * divisions`; in 12-EDO this
/// makes step numbers identical to MIDI note numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tuning {
    divisions: u8,
}

/// Frequency of C4 in Hz (12-EDO, A4 = 440 Hz). Shared by all tunings.
pub const C4_HZ: f64 = 261.625_565_300_598_6;

impl Tuning {
    /// 12-tone equal temperament.
    pub const TWELVE: Tuning = Tuning { divisions: 12 };

    /// An equal division of the octave into `divisions` steps (1 to 64).
    pub const fn edo(divisions: u8) -> Option<Tuning> {
        if divisions == 0 || divisions > MAX_EDO {
            None
        } else {
            Some(Tuning { divisions })
        }
    }

    /// Number of steps per octave.
    pub const fn divisions(self) -> u8 {
        self.divisions
    }

    /// Steps per octave as an `i32`, for arithmetic.
    pub const fn steps_per_octave(self) -> i32 {
        self.divisions as i32
    }

    /// Whether this is 12-EDO, where letter-name spelling applies.
    pub const fn is_twelve(self) -> bool {
        self.divisions == 12
    }

    /// The step number of C4.
    pub const fn c4_step(self) -> i32 {
        5 * self.divisions as i32
    }

    /// Size of `steps` in cents (1200 cents per octave).
    pub fn cents(self, steps: i32) -> f64 {
        steps as f64 * 1200.0 / self.divisions as f64
    }

    /// Frequency in Hz of a pitch step.
    #[cfg(feature = "std")]
    pub fn frequency(self, step: i32) -> f64 {
        let octaves = (step - self.c4_step()) as f64 / self.divisions as f64;
        C4_HZ * 2f64.powf(octaves)
    }

    /// The nearest 12-EDO MIDI note to a step, and the offset from it in cents
    /// (between -50 and +50). Use the offset for pitch bend or MPE.
    pub fn to_midi(self, step: i32) -> (i32, f64) {
        let cents_from_c4 = self.cents(step - self.c4_step());
        let semis = cents_from_c4 / 100.0;
        let nearest = round_to_i32(semis);
        (60 + nearest, (semis - nearest as f64) * 100.0)
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning::TWELVE
    }
}

/// Round half away from zero without `std`.
pub(crate) fn round_to_i32(x: f64) -> i32 {
    if x >= 0.0 {
        (x + 0.5) as i32
    } else {
        (x - 0.5) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_matches_midi() {
        assert_eq!(Tuning::TWELVE.c4_step(), 60);
        assert_eq!(Tuning::TWELVE.to_midi(69), (69, 0.0));
        assert!((Tuning::TWELVE.frequency(69) - 440.0).abs() < 1e-9);
    }

    #[test]
    fn nineteen_edo() {
        let t = Tuning::edo(19).unwrap();
        assert_eq!(t.c4_step(), 95);
        // One octave up is exactly double.
        assert!((t.frequency(95 + 19) / t.frequency(95) - 2.0).abs() < 1e-12);
        // 19-EDO step 1 = 63.16 cents: nearest MIDI note is C#4, about -36.8 cents off.
        let (note, cents) = t.to_midi(96);
        assert_eq!(note, 61);
        assert!((cents + 36.842).abs() < 0.01);
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(Tuning::edo(0).is_none());
        assert!(Tuning::edo(65).is_none());
        assert!(Tuning::edo(64).is_some());
    }
}
