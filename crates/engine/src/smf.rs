//! Standard MIDI file export: one track per part, plus a tempo track.

use pm_theory::{Ticks, PPQ};

use crate::engine::Engine;
use crate::graph::SinkOutput;
use crate::nodes::Sink;
use crate::spec::ParamValue;

/// One track's events as (tick, bytes), before encoding.
struct Track {
    events: Vec<(Ticks, u8, Vec<u8>)>,
}

impl Track {
    fn new(name: &str) -> Track {
        let mut t = Track { events: Vec::new() };
        t.meta(0, 0x03, name.as_bytes());
        t
    }

    /// `order` breaks ties at one tick: meta first, then note-offs, then note-ons.
    fn push(&mut self, tick: Ticks, order: u8, bytes: Vec<u8>) {
        self.events.push((tick, order, bytes));
    }

    fn meta(&mut self, tick: Ticks, kind: u8, data: &[u8]) {
        let mut bytes = vec![0xff, kind];
        write_vlq(&mut bytes, data.len() as u32);
        bytes.extend(data);
        self.push(tick, 0, bytes);
    }

    fn encode(mut self) -> Vec<u8> {
        self.events.sort_by_key(|e| (e.0, e.1));
        let mut data = Vec::new();
        let mut last = 0;
        for (tick, _, bytes) in &self.events {
            write_vlq(&mut data, tick - last);
            data.extend(bytes);
            last = *tick;
        }
        data.extend([0x00, 0xff, 0x2f, 0x00]);
        let mut out = b"MTrk".to_vec();
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        out
    }
}

fn write_vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut buf = [0u8; 5];
    let mut i = buf.len() - 1;
    buf[i] = (value & 0x7f) as u8;
    value >>= 7;
    while value > 0 {
        i -= 1;
        buf[i] = (value & 0x7f) as u8 | 0x80;
        value >>= 7;
    }
    out.extend(&buf[i..]);
}

/// A part's display name: its group's title, else the node id.
pub fn part_name(engine: &Engine, node: &str) -> String {
    engine.doc().parts.get(node).cloned().unwrap_or_else(|| {
        let kind = engine
            .doc()
            .nodes
            .iter()
            .find(|n| n.id == node)
            .map(|n| n.kind.as_str())
            .unwrap_or("");
        match kind {
            "poly_synth" => format!("Poly synth {node}"),
            "mono_synth" => format!("Mono synth {node}"),
            "drum_synth" => format!("Drums {node}"),
            _ => format!("MIDI out {node}"),
        }
    })
}

/// Encodes the whole arrangement (or `bars` bars without one) as a format-1 MIDI file.
/// Built-in instruments get channels of their own (drums on channel 10); MIDI outputs
/// keep their channel and program.
pub fn export(engine: &Engine, bars: u32, salt: u64) -> Vec<u8> {
    let parts: Vec<SinkOutput> = engine.song_notes(bars, true, salt);
    let meter = engine.meter();
    let mut tracks = Vec::new();

    let mut conductor = Track::new("Tempo");
    let denominator = (meter.unit as f64).log2() as u8;
    conductor.meta(0, 0x58, &[meter.beats, denominator, 24, 8]);
    for (tick, bpm) in engine.tempo_map() {
        let us = (60_000_000.0 / bpm).round() as u32;
        conductor.meta(tick, 0x51, &us.to_be_bytes()[1..]);
    }
    let end = engine.song_length().max(bars * meter.ticks_per_bar());
    tracks.push(conductor);

    let mut next_channel = 0u8;
    let mut take_channel = || {
        let c = next_channel;
        next_channel = (next_channel + 1) % 16;
        if next_channel == 9 {
            next_channel = 10;
        }
        c
    };
    for part in parts {
        let node = engine.doc().nodes.iter().find(|n| n.id == part.node);
        let kind = node.map(|n| n.kind.as_str()).unwrap_or("");
        let (channel, program) = match &part.sink {
            Sink::Midi { channel, .. } => {
                let program = match node.and_then(|n| n.params.get("program")) {
                    Some(ParamValue::Number(p)) if *p >= 1.0 => Some((*p as u8 - 1).min(127)),
                    _ => None,
                };
                (*channel & 0x0f, program)
            }
            Sink::Instrument if kind == "drum_synth" => (9, None),
            Sink::Instrument => (take_channel(), None),
        };
        let mut track = Track::new(&part_name(engine, &part.node));
        if let Some(p) = program {
            track.push(0, 1, vec![0xc0 | channel, p]);
        }
        for n in &part.notes {
            let (key, _) = n.pitch.tuning().to_midi(n.pitch.step());
            if !(0..=127).contains(&key) {
                continue;
            }
            let key = key as u8;
            let off = n.end().min(end.max(n.start + 1));
            track.push(
                n.start,
                3,
                vec![0x90 | channel, key, n.velocity.clamp(1, 127)],
            );
            track.push(off, 2, vec![0x80 | channel, key, 0]);
        }
        tracks.push(track);
    }

    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend((PPQ as u16).to_be_bytes());
    for t in tracks {
        out.extend(t.encode());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::parse_graph;
    use crate::json::Json;

    #[test]
    fn exports_one_track_per_part_with_tempo_changes() {
        let doc = parse_graph(
            &Json::parse(
                r#"{"nodes":[
                  {"id":"g","type":"group","title":"Drums","graph":{"nodes":[
                     {"id":"p","type":"pattern","params":{"pattern":"x","step":"1/4"}},
                     {"id":"d","type":"drum","params":{}},
                     {"id":"k","type":"drum_synth","params":{}}],
                   "wires":[{"from":{"node":"p","port":"rhythm"},"to":{"node":"d","port":"rhythm"}},
                            {"from":{"node":"d","port":"notes"},"to":{"node":"k","port":"notes"}}]}},
                  {"id":"o","type":"midi_out","params":{"channel":3,"program":5}},
                  {"id":"s","type":"scale","params":{}},
                  {"id":"q","type":"sequencer","params":{"degrees":"1 2"}}],
                 "wires":[{"from":{"node":"s","port":"scale"},"to":{"node":"q","port":"scale"}},
                          {"from":{"node":"q","port":"notes"},"to":{"node":"o","port":"notes"}}],
                 "arrangement":{"sections":[{"name":"A","bars":1},{"name":"B","bars":1,"bpm":150}]}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let mut e = Engine::new();
        e.set_graph(&doc);
        e.set_bpm(0.0, 100.0);
        let bytes = export(&e, 8, 0);
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(
            u16::from_be_bytes([bytes[10], bytes[11]]),
            3,
            "tempo + two parts"
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Drums"));
        // Tempo 100 bpm = 600000 us (0x0927C0); 150 bpm = 400000 us (0x061A80).
        let has = |pat: &[u8]| bytes.windows(pat.len()).any(|w| w == pat);
        assert!(has(&[0xff, 0x51, 0x03, 0x09, 0x27, 0xc0]));
        assert!(has(&[0xff, 0x51, 0x03, 0x06, 0x1a, 0x80]));
        assert!(has(&[0xc2, 4]), "program 5 on channel 3");
        assert!(has(&[0x99, 36]), "kick on channel 10");
    }
}
