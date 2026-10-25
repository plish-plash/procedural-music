//! Control signals: a number over musical time, which drives node parameters.
//!
//! Like everything else in the graph, a control is a pure function of time, so seeking,
//! looping and rendering see the same value at the same tick.

use std::f64::consts::TAU;

use pm_theory::Ticks;

use crate::timeline::Timeline;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Sine,
    Triangle,
    /// Rises from min to max, then drops back.
    Saw,
    Square,
}

impl Shape {
    pub fn from_name(name: &str) -> Shape {
        match name {
            "Triangle" => Shape::Triangle,
            "Saw" => Shape::Saw,
            "Square" => Shape::Square,
            _ => Shape::Sine,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Shape::Sine => "sine",
            Shape::Triangle => "triangle",
            Shape::Saw => "saw",
            Shape::Square => "square",
        }
    }

    /// The shape at `phase` (0..1 over one cycle), from 0 to 1.
    fn at(self, phase: f64) -> f64 {
        match self {
            // Starts at the bottom, like the others.
            Shape::Sine => 0.5 - 0.5 * (TAU * phase).cos(),
            Shape::Triangle => 1.0 - (2.0 * phase - 1.0).abs(),
            Shape::Saw => phase,
            Shape::Square => {
                if phase < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// A repeating wave between `min` and `max`, one cycle every `cycle` ticks.
    Lfo {
        shape: Shape,
        cycle: Ticks,
        min: f64,
        max: f64,
        /// Offset into the cycle, 0..1.
        phase: f64,
    },
    /// A straight line from `from` to `to` over `length` ticks, then holds (or repeats).
    Ramp {
        from: f64,
        to: f64,
        length: Ticks,
        looped: bool,
    },
    /// Values held for their durations, looping.
    Steps(Timeline<f64>),
}

impl Control {
    /// The value at tick `t`.
    pub fn at(&self, t: Ticks) -> f64 {
        match self {
            Control::Lfo {
                shape,
                cycle,
                min,
                max,
                phase,
            } => {
                let cycle = (*cycle).max(1);
                let p = ((t % cycle) as f64 / cycle as f64 + phase).fract();
                min + (max - min) * shape.at(p)
            }
            Control::Ramp {
                from,
                to,
                length,
                looped,
            } => {
                let length = (*length).max(1);
                let t = if *looped { t % length } else { t.min(length) };
                from + (to - from) * t as f64 / length as f64
            }
            Control::Steps(tl) => *tl.at(t).value,
        }
    }
}

fn num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v}")
    } else {
        format!("{}", (v * 100.0).round() / 100.0)
    }
}

impl Control {
    /// A short description, with lengths in bars of `bar` ticks: "sine 0–7 every 2 bars".
    pub fn describe(&self, bar: Ticks) -> String {
        let bars = |t: Ticks| {
            let n = t as f64 / bar.max(1) as f64;
            format!("{} bar{}", num(n), if n == 1.0 { "" } else { "s" })
        };
        match self {
            Control::Lfo {
                shape,
                cycle,
                min,
                max,
                ..
            } => format!(
                "{} {}–{} every {}",
                shape.name(),
                num(*min),
                num(*max),
                bars(*cycle)
            ),
            Control::Ramp {
                from,
                to,
                length,
                looped,
            } => format!(
                "ramp {}→{} over {}{}",
                num(*from),
                num(*to),
                bars(*length),
                if *looped { ", repeating" } else { "" }
            ),
            Control::Steps(tl) => {
                let values: Vec<String> = tl.values().map(|v| num(*v)).collect();
                format!("steps {}", values.join(" "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_shapes() {
        let lfo = |shape| Control::Lfo {
            shape,
            cycle: 400,
            min: 2.0,
            max: 6.0,
            phase: 0.0,
        };
        let sine = lfo(Shape::Sine);
        assert!((sine.at(0) - 2.0).abs() < 1e-9);
        assert!((sine.at(200) - 6.0).abs() < 1e-9);
        assert!((sine.at(400) - 2.0).abs() < 1e-9, "loops");
        assert!((lfo(Shape::Triangle).at(100) - 4.0).abs() < 1e-9);
        assert!((lfo(Shape::Saw).at(300) - 5.0).abs() < 1e-9);
        assert_eq!(lfo(Shape::Square).at(199), 2.0);
        assert_eq!(lfo(Shape::Square).at(200), 6.0);
        let shifted = Control::Lfo {
            shape: Shape::Saw,
            cycle: 400,
            min: 0.0,
            max: 1.0,
            phase: 0.5,
        };
        assert!((shifted.at(0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn ramps_hold_or_repeat() {
        let ramp = |looped| Control::Ramp {
            from: 10.0,
            to: 20.0,
            length: 100,
            looped,
        };
        assert_eq!(ramp(false).at(50), 15.0);
        assert_eq!(ramp(false).at(500), 20.0);
        assert_eq!(ramp(true).at(150), 15.0);
    }

    #[test]
    fn steps_hold_their_values() {
        let s = Control::Steps(Timeline::from_steps(vec![(100, 0.0), (100, 5.0)]).unwrap());
        assert_eq!(s.at(99), 0.0);
        assert_eq!(s.at(100), 5.0);
        assert_eq!(s.at(250), 0.0);
        assert_eq!(s.describe(400), "steps 0 5");
        let lfo = Control::Lfo {
            shape: Shape::Sine,
            cycle: 800,
            min: 0.0,
            max: 7.0,
            phase: 0.0,
        };
        assert_eq!(lfo.describe(400), "sine 0–7 every 2 bars");
    }
}
