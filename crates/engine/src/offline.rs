//! Offline rendering: plays a patch through the built-in instruments faster than real
//! time and returns the audio. MIDI outputs are ignored.

use pm_synth::Rack;

use crate::engine::Engine;
use crate::graph::GraphDoc;

/// Renders `bars` bars of a patch, plus `tail` seconds for releases and echoes, as
/// interleaved stereo samples.
pub fn render(
    doc: &GraphDoc,
    bpm: f64,
    bars: u32,
    tail: f64,
    sample_rate: u32,
    salt: u64,
) -> Vec<f32> {
    const BLOCK: usize = 256;
    let sr = sample_rate as f64;
    let mut engine = Engine::new();
    engine.set_graph(doc);
    engine.set_bpm(0.0, bpm);
    let beats = (bars * engine.meter().beats as u32) as f64 * 4.0 / engine.meter().unit as f64;
    let length = beats * 60.0 / bpm;
    engine.play(0.0, salt);
    // `play` starts the music 50 ms in; drop that lead-in from the output.
    let lead_in = 0.05;
    let total_frames = ((length + tail) * sr).ceil() as usize;
    let skip = (lead_in * sr).round() as usize;

    let mut rack = Rack::new(sample_rate as f32);
    let mut out = vec![0.0f32; 2 * (total_frames + skip)];
    let mut frame = 0;
    let mut stopped = false;
    while frame < total_frames + skip {
        let t = frame as f64 / sr;
        if !stopped && t >= lead_in + length {
            engine.stop();
            stopped = true;
        }
        engine.poll(t);
        for msg in engine.take_audio() {
            rack.handle(msg);
        }
        let n = BLOCK.min(total_frames + skip - frame);
        rack.render(t, &mut out[2 * frame..2 * (frame + n)]);
        frame += n;
    }
    out.drain(..2 * skip);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{NodeDoc, WireDoc};
    use crate::spec::ParamValue;

    fn node(id: &str, kind: &str, params: &[(&str, ParamValue)]) -> NodeDoc {
        NodeDoc {
            id: id.into(),
            kind: kind.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    fn wire(a: &str, ap: &str, b: &str, bp: &str) -> WireDoc {
        WireDoc {
            from_node: a.into(),
            from_port: ap.into(),
            to_node: b.into(),
            to_port: bp.into(),
        }
    }

    #[test]
    fn kick_on_every_beat() {
        let doc = GraphDoc {
            nodes: vec![
                node(
                    "p",
                    "pattern",
                    &[("pattern", ParamValue::Text("x...".into()))],
                ),
                node("d", "drum", &[]),
                node("k", "drum_synth", &[]),
                node("o", "audio_out", &[]),
            ],
            wires: vec![
                wire("p", "rhythm", "d", "rhythm"),
                wire("d", "notes", "k", "notes"),
                wire("k", "audio", "o", "audio"),
            ],
        };
        let sr = 48_000;
        let audio = render(&doc, 120.0, 1, 0.5, sr, 1);
        assert_eq!(audio.len(), 2 * (2.5 * sr as f64) as usize);
        // Onsets: the first sample of each beat whose level jumps.
        let left: Vec<f32> = audio.chunks(2).map(|f| f[0]).collect();
        let beat = sr as usize / 2;
        for b in 0..4 {
            let window = &left[b * beat..b * beat + 2000];
            let peak = window.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.1, "beat {b} peak {peak}");
            let first = window.iter().position(|x| x.abs() > 0.01).unwrap();
            assert!(first < 200, "beat {b} starts {first} samples late");
            // Silence just before each beat (the previous kick has decayed).
            if b > 0 {
                let before = &left[b * beat - 300..b * beat - 50];
                assert!(before.iter().all(|x| x.abs() < 0.05));
            }
        }
        assert!(audio.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }
}
