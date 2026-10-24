//! Values that change over musical time and loop.

use pm_theory::Ticks;

/// One stretch of a timeline in absolute ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment<'a, T> {
    pub start: Ticks,
    /// Exclusive. `Ticks::MAX` for a constant timeline.
    pub end: Ticks,
    /// Position of this step within the timeline (0 for constants).
    pub index: usize,
    pub value: &'a T,
}

/// A value over time: a list of (duration, value) steps that loops from tick 0.
/// A timeline with a single step of duration 0 is a constant.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline<T> {
    steps: Vec<(Ticks, T)>,
}

impl<T> Timeline<T> {
    /// The same value for all time.
    pub fn constant(value: T) -> Self {
        Timeline {
            steps: vec![(0, value)],
        }
    }

    /// Steps of (duration, value). Zero durations are raised to 1; an empty list
    /// gives `None`.
    pub fn from_steps(steps: Vec<(Ticks, T)>) -> Option<Self> {
        if steps.is_empty() {
            return None;
        }
        let steps = steps.into_iter().map(|(d, v)| (d.max(1), v)).collect();
        Some(Timeline { steps })
    }

    pub fn is_constant(&self) -> bool {
        self.steps.len() == 1 && self.steps[0].0 == 0
    }

    /// Length of one loop (0 for a constant).
    pub fn length(&self) -> Ticks {
        if self.is_constant() {
            0
        } else {
            self.steps.iter().map(|s| s.0).sum()
        }
    }

    pub fn steps(&self) -> &[(Ticks, T)] {
        &self.steps
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.steps.iter().map(|s| &s.1)
    }

    /// The segment sounding at `time`.
    pub fn at(&self, time: Ticks) -> Segment<'_, T> {
        let length = self.length();
        if length == 0 {
            return Segment {
                start: 0,
                end: Ticks::MAX,
                index: 0,
                value: &self.steps[0].1,
            };
        }
        let loop_start = time - time % length;
        let mut start = loop_start;
        for (index, (duration, value)) in self.steps.iter().enumerate() {
            if time < start + duration {
                return Segment {
                    start,
                    end: start + duration,
                    index,
                    value,
                };
            }
            start += duration;
        }
        unreachable!("time is inside the loop")
    }

    /// Segments whose start lies in `[start, end)`. A constant has one segment
    /// starting at every multiple of `period` (used to re-strike held chords).
    pub fn starts_in(&self, start: Ticks, end: Ticks, period: Ticks) -> Vec<Segment<'_, T>> {
        let mut out = Vec::new();
        if start >= end {
            return out;
        }
        let length = self.length();
        if length == 0 {
            let period = period.max(1);
            let mut t = start.div_ceil(period) * period;
            while t < end {
                out.push(Segment {
                    start: t,
                    end: t.saturating_add(period),
                    index: 0,
                    value: &self.steps[0].1,
                });
                t += period;
            }
            return out;
        }
        let mut loop_start = start - start % length;
        while loop_start < end {
            let mut s = loop_start;
            for (index, (duration, value)) in self.steps.iter().enumerate() {
                if s >= end {
                    break;
                }
                if s >= start {
                    out.push(Segment {
                        start: s,
                        end: s + duration,
                        index,
                        value,
                    });
                }
                s += duration;
            }
            loop_start += length;
        }
        out
    }

    /// Applies `f` to every value, keeping durations.
    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> Timeline<U> {
        Timeline {
            steps: self.steps.iter().map(|(d, v)| (*d, f(v))).collect(),
        }
    }

    /// Applies `f(previous_output, value)` in order; used for voice leading.
    pub fn map_with_previous<U: Clone>(
        &self,
        mut f: impl FnMut(Option<&U>, &T) -> U,
    ) -> Timeline<U> {
        let mut steps: Vec<(Ticks, U)> = Vec::with_capacity(self.steps.len());
        for (d, v) in &self.steps {
            let out = f(steps.last().map(|s| &s.1), v);
            steps.push((*d, out));
        }
        Timeline { steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_loop() {
        let t = Timeline::from_steps(vec![(100, 'a'), (50, 'b')]).unwrap();
        assert_eq!(t.length(), 150);
        assert_eq!(*t.at(120).value, 'b');
        let seg = t.at(160);
        assert_eq!((seg.start, seg.end, *seg.value), (150, 250, 'a'));
    }

    #[test]
    fn segment_starts_in_window() {
        let t = Timeline::from_steps(vec![(100, 'a'), (50, 'b')]).unwrap();
        let s: Vec<_> = t
            .starts_in(90, 260, 0)
            .iter()
            .map(|s| (s.start, *s.value))
            .collect();
        assert_eq!(s, [(100, 'b'), (150, 'a'), (250, 'b')]);
        let c = Timeline::constant('c');
        let s: Vec<_> = c.starts_in(10, 300, 100).iter().map(|s| s.start).collect();
        assert_eq!(s, [100, 200]);
        assert_eq!(c.at(5_000_000).end, Ticks::MAX);
    }
}
