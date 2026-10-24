//! Transport, lookahead scheduling and MIDI dispatch.
//!
//! The engine is driven by the caller's clock: every call takes `now` in seconds
//! (any monotonic origin). [`Engine::poll`] evaluates the graph a little ahead of
//! the playhead, queues timed MIDI messages, and returns the ones that are due.
//! Notes for built-in instruments are handed out early instead (see
//! [`Engine::take_audio`]), so the audio thread can place them sample-accurately.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use pm_synth::{AudioMsg, NoteEvent, TimedEvent};
use pm_theory::{Meter, Ticks, PPQ};

use crate::graph::{CompileReport, EvalContext, Graph, GraphDoc};
use crate::nodes::Sink;

/// Port name meaning "the first MIDI port the app finds".
pub const DEFAULT_PORT: &str = "";

/// Bytes to send to a named MIDI port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiMessage {
    pub port: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
enum Event {
    Raw(MidiMessage),
    Off {
        port: String,
        channel: u8,
        key: u8,
    },
    On {
        port: String,
        channel: u8,
        key: u8,
        velocity: u8,
    },
}

impl Event {
    /// Same-time ordering: clock and transport first, then note-offs, then note-ons.
    fn rank(&self) -> u8 {
        match self {
            Event::Raw(_) => 0,
            Event::Off { .. } => 1,
            Event::On { .. } => 2,
        }
    }
}

struct Pending {
    at: f64,
    seq: u64,
    event: Event,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Pending {}
impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pending {
    /// Reversed so the `BinaryHeap` pops the earliest event first.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .at
            .total_cmp(&self.at)
            .then_with(|| other.event.rank().cmp(&self.event.rank()))
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

pub struct Engine {
    graph: Graph,
    meter: Meter,
    bpm: f64,
    playing: bool,
    /// Playhead anchor: at `anchor_time` seconds the playhead was at `anchor_tick`.
    anchor_time: f64,
    anchor_tick: f64,
    /// Everything before this tick has been evaluated and queued.
    scheduled_until: Ticks,
    queue: BinaryHeap<Pending>,
    seq: u64,
    /// Sounding notes per (port, channel, key), counted so overlapping notes on one key
    /// don't cut each other off.
    active: HashMap<(String, u8, u8), u32>,
    clock_ports: Vec<String>,
    salt: u64,
    /// The document the current graph was built from (for offline rendering).
    doc: GraphDoc,
    /// Messages for the audio thread, collected until [`Engine::take_audio`].
    audio: Vec<AudioMsg>,
    audio_events: Vec<TimedEvent>,
    next_note_id: u64,
    /// How far ahead of the playhead to schedule, in seconds.
    pub lookahead: f64,
    /// Size of each evaluation window in ticks. Graph edits take effect at the next window.
    pub slice: Ticks,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            graph: Graph::default(),
            meter: Meter::COMMON,
            bpm: 110.0,
            playing: false,
            anchor_time: 0.0,
            anchor_tick: 0.0,
            scheduled_until: 0,
            queue: BinaryHeap::new(),
            seq: 0,
            active: HashMap::new(),
            clock_ports: Vec::new(),
            salt: 0,
            doc: GraphDoc::default(),
            audio: vec![AudioMsg::Tempo(110.0)],
            audio_events: Vec::new(),
            next_note_id: 1,
            lookahead: 0.12,
            slice: PPQ / 4,
        }
    }

    /// Replaces the graph. While playing, already-queued notes still sound; the new
    /// graph takes over from the next evaluation window.
    pub fn set_graph(&mut self, doc: &GraphDoc) -> CompileReport {
        let old_programs = self.graph.programs();
        self.graph = Graph::compile(doc);
        self.doc = doc.clone();
        self.audio.push(AudioMsg::Rack(self.graph.rack_desc()));
        if self.playing {
            // Apply instrument changes right away (due at the next poll).
            for (port, channel, program) in self.graph.programs() {
                if !old_programs.contains(&(port.clone(), channel, program)) {
                    self.push(
                        0.0,
                        Event::Raw(MidiMessage {
                            port,
                            bytes: vec![0xc0 | channel, program],
                        }),
                    );
                }
            }
        }
        self.graph.report(self.meter, 4)
    }

    /// The document of the current graph.
    pub fn doc(&self) -> &GraphDoc {
        &self.doc
    }

    /// Messages for the audio thread since the last call: rack changes, tempo, instrument
    /// notes (ahead of time) and stops, in order.
    pub fn take_audio(&mut self) -> Vec<AudioMsg> {
        if !self.audio_events.is_empty() {
            self.audio
                .push(AudioMsg::Events(std::mem::take(&mut self.audio_events)));
        }
        std::mem::take(&mut self.audio)
    }

    pub fn meter(&self) -> Meter {
        self.meter
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Ports that receive MIDI clock (24 per quarter note) and start/stop messages.
    pub fn set_clock_ports(&mut self, ports: Vec<String>) {
        self.clock_ports = ports;
    }

    pub fn clock_ports(&self) -> &[String] {
        &self.clock_ports
    }

    fn ticks_per_second(&self) -> f64 {
        self.bpm / 60.0 * PPQ as f64
    }

    /// The playhead in (fractional) ticks.
    pub fn playhead(&self, now: f64) -> f64 {
        if self.playing {
            (self.anchor_tick + (now - self.anchor_time) * self.ticks_per_second()).max(0.0)
        } else {
            self.anchor_tick
        }
    }

    fn time_of(&self, tick: Ticks) -> f64 {
        self.anchor_time + (tick as f64 - self.anchor_tick) / self.ticks_per_second()
    }

    /// Changes the tempo without moving the playhead.
    pub fn set_bpm(&mut self, now: f64, bpm: f64) {
        let bpm = bpm.clamp(20.0, 400.0);
        if self.playing {
            self.anchor_tick = self.playhead(now);
            self.anchor_time = now;
        }
        self.bpm = bpm;
        self.audio.push(AudioMsg::Tempo(bpm));
    }

    /// Starts from the top. `salt` should differ every time so unlocked random nodes
    /// play something new.
    pub fn play(&mut self, now: f64, salt: u64) {
        if self.playing {
            return;
        }
        self.playing = true;
        // A small delay so the first notes are not already late.
        self.anchor_time = now + 0.05;
        self.anchor_tick = 0.0;
        self.scheduled_until = 0;
        self.salt = salt;
        for (port, channel, program) in self.graph.programs() {
            self.push(
                now,
                Event::Raw(MidiMessage {
                    port,
                    bytes: vec![0xc0 | channel, program],
                }),
            );
        }
        for port in self.clock_ports.clone() {
            self.push(
                now,
                Event::Raw(MidiMessage {
                    port: port.clone(),
                    bytes: vec![0xf2, 0, 0],
                }),
            );
            self.push(
                self.anchor_time,
                Event::Raw(MidiMessage {
                    port,
                    bytes: vec![0xfa],
                }),
            );
        }
    }

    /// Stops, drops everything queued, and returns note-offs for every sounding note
    /// (plus clock stop), to be sent immediately.
    pub fn stop(&mut self) -> Vec<MidiMessage> {
        let mut out = Vec::new();
        if !self.playing && self.active.is_empty() {
            return out;
        }
        if !self.audio_events.is_empty() {
            self.audio
                .push(AudioMsg::Events(std::mem::take(&mut self.audio_events)));
        }
        self.audio.push(AudioMsg::Stop);
        self.playing = false;
        self.anchor_tick = 0.0;
        self.queue.clear();
        let mut active: Vec<_> = self.active.drain().map(|(k, _)| k).collect();
        active.sort();
        for (port, channel, key) in active {
            out.push(MidiMessage {
                port,
                bytes: vec![0x80 | channel, key, 0],
            });
        }
        for port in &self.clock_ports {
            out.push(MidiMessage {
                port: port.clone(),
                bytes: vec![0xfc],
            });
        }
        out
    }

    fn push(&mut self, at: f64, event: Event) {
        self.seq += 1;
        self.queue.push(Pending {
            at,
            seq: self.seq,
            event,
        });
    }

    /// Schedules ahead of the playhead and returns the messages due at `now`.
    pub fn poll(&mut self, now: f64) -> Vec<MidiMessage> {
        if self.playing {
            let playhead = self.playhead(now) as Ticks;
            // If we fell far behind (the machine slept), skip ahead instead of
            // sending a burst of stale notes.
            if self.scheduled_until + 2 * self.meter.ticks_per_bar() < playhead {
                self.scheduled_until = playhead;
            }
            let horizon = self.playhead(now + self.lookahead) as Ticks;
            while self.scheduled_until < horizon {
                let start = self.scheduled_until;
                let end = start + self.slice;
                self.schedule_window(start, end);
                self.scheduled_until = end;
            }
        }
        let mut out = Vec::new();
        while self.queue.peek().is_some_and(|p| p.at <= now) {
            let p = self.queue.pop().expect("peeked");
            self.dispatch(p.event, &mut out);
        }
        out
    }

    /// Seconds until the next queued message, if any (for the caller's sleep).
    pub fn next_due(&self, now: f64) -> Option<f64> {
        self.queue.peek().map(|p| (p.at - now).max(0.0))
    }

    fn schedule_window(&mut self, start: Ticks, end: Ticks) {
        let ctx = EvalContext {
            start,
            end,
            meter: self.meter,
            salt: self.salt,
        };
        for sink in self.graph.evaluate(&ctx) {
            let (port, channel) = match sink.sink {
                Sink::Midi { port, channel } => (port, channel & 0x0f),
                Sink::Instrument => {
                    for note in sink.notes {
                        let id = self.next_note_id;
                        self.next_note_id += 1;
                        let (key, _) = note.pitch.tuning().to_midi(note.pitch.step());
                        let on = NoteEvent::On {
                            id,
                            freq: note.pitch.frequency() as f32,
                            key: key.clamp(0, 127) as u8,
                            velocity: note.velocity.clamp(1, 127),
                        };
                        let at = self.time_of(note.start);
                        self.audio_events.push(TimedEvent {
                            at,
                            module: sink.module,
                            event: on,
                        });
                        let at = self.time_of(note.end());
                        self.audio_events.push(TimedEvent {
                            at,
                            module: sink.module,
                            event: NoteEvent::Off { id },
                        });
                    }
                    continue;
                }
            };
            for note in sink.notes {
                let (key, _cents) = note.pitch.tuning().to_midi(note.pitch.step());
                if !(0..=127).contains(&key) {
                    continue;
                }
                let key = key as u8;
                let velocity = note.velocity.clamp(1, 127);
                let port = port.clone();
                self.push(
                    self.time_of(note.start),
                    Event::On {
                        port: port.clone(),
                        channel,
                        key,
                        velocity,
                    },
                );
                self.push(self.time_of(note.end()), Event::Off { port, channel, key });
            }
        }
        let pulse = PPQ / 24;
        let mut t = start.div_ceil(pulse) * pulse;
        while t < end {
            for port in self.clock_ports.clone() {
                self.push(
                    self.time_of(t),
                    Event::Raw(MidiMessage {
                        port,
                        bytes: vec![0xf8],
                    }),
                );
            }
            t += pulse;
        }
    }

    fn dispatch(&mut self, event: Event, out: &mut Vec<MidiMessage>) {
        match event {
            Event::Raw(m) => out.push(m),
            Event::On {
                port,
                channel,
                key,
                velocity,
            } => {
                let count = self.active.entry((port.clone(), channel, key)).or_insert(0);
                if *count > 0 {
                    // Retrigger: end the sounding note so the new one is heard.
                    out.push(MidiMessage {
                        port: port.clone(),
                        bytes: vec![0x80 | channel, key, 0],
                    });
                }
                *count += 1;
                out.push(MidiMessage {
                    port,
                    bytes: vec![0x90 | channel, key, velocity],
                });
            }
            Event::Off { port, channel, key } => {
                let k = (port, channel, key);
                if let Some(count) = self.active.get_mut(&k) {
                    *count -= 1;
                    if *count == 0 {
                        self.active.remove(&k);
                        out.push(MidiMessage {
                            port: k.0,
                            bytes: vec![0x80 | channel, key, 0],
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphDoc, NodeDoc, WireDoc};
    use crate::spec::ParamValue;

    fn doc(kind: &str, params: &[(&str, ParamValue)]) -> GraphDoc {
        let n = |id: &str, kind: &str, params: &[(&str, ParamValue)]| NodeDoc {
            id: id.into(),
            kind: kind.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        };
        GraphDoc {
            nodes: vec![
                n(
                    "p",
                    "pattern",
                    &[("pattern", ParamValue::Text("x...".into()))],
                ),
                n("d", kind, params),
                n("o", "midi_out", &[("channel", ParamValue::Number(10.0))]),
            ],
            wires: vec![
                WireDoc {
                    from_node: "p".into(),
                    from_port: "rhythm".into(),
                    to_node: "d".into(),
                    to_port: "rhythm".into(),
                },
                WireDoc {
                    from_node: "d".into(),
                    from_port: "notes".into(),
                    to_node: "o".into(),
                    to_port: "notes".into(),
                },
            ],
        }
    }

    /// Runs the engine with a fake clock and returns (time, bytes) of everything sent.
    fn run(engine: &mut Engine, until: f64) -> Vec<(f64, Vec<u8>)> {
        let mut sent = Vec::new();
        let mut now = 0.0;
        while now < until {
            for m in engine.poll(now) {
                sent.push((now, m.bytes));
            }
            now += 0.001;
        }
        sent
    }

    #[test]
    fn plays_quarter_notes_on_time() {
        let mut e = Engine::new();
        let report = e.set_graph(&doc("drum", &[]));
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        e.set_bpm(0.0, 120.0);
        e.play(0.0, 1);
        let sent = run(&mut e, 2.06);
        let ons: Vec<f64> = sent
            .iter()
            .filter(|(_, b)| b[0] == 0x99)
            .map(|(t, _)| *t)
            .collect();
        // 120 bpm: a beat every 0.5 s, starting after the 50 ms start delay.
        assert_eq!(ons.len(), 5, "{ons:?}");
        for (i, t) in ons.iter().enumerate() {
            let expected = 0.05 + 0.5 * i as f64;
            assert!(
                (t - expected).abs() < 0.0025,
                "note {i} at {t}, expected {expected}"
            );
        }
        assert!(sent.iter().any(|(_, b)| b == &vec![0x99, 36, 96]));
    }

    #[test]
    fn stop_releases_sounding_notes() {
        let mut e = Engine::new();
        e.set_graph(&doc("drum", &[]));
        e.play(0.0, 1);
        let _ = run(&mut e, 0.06); // first note-on sent, its note-off still queued
        let offs = e.stop();
        assert_eq!(
            offs,
            vec![MidiMessage {
                port: String::new(),
                bytes: vec![0x89, 36, 0]
            }]
        );
        assert!(e.poll(10.0).is_empty());
        assert!(e.stop().is_empty());
    }

    #[test]
    fn overlapping_notes_retrigger_without_cutting() {
        let mut e = Engine::new();
        // A long gate makes each hit overlap the next.
        let mut d = doc(
            "sequencer",
            &[
                ("degrees", ParamValue::Text("1".into())),
                ("gate", ParamValue::Number(200.0)),
            ],
        );
        d.nodes.push(NodeDoc {
            id: "s".into(),
            kind: "scale".into(),
            params: Default::default(),
        });
        d.wires.push(WireDoc {
            from_node: "s".into(),
            from_port: "scale".into(),
            to_node: "d".into(),
            to_port: "scale".into(),
        });
        assert!(e.set_graph(&d).issues.is_empty());
        e.set_bpm(0.0, 120.0);
        e.play(0.0, 1);
        let sent = run(&mut e, 1.3);
        let kinds: Vec<u8> = sent.iter().map(|(_, b)| b[0] & 0xf0).collect();
        // on, (off+on at the next hit), (off+on), ... never two ons without an off between.
        assert_eq!(&kinds[..5], &[0x90, 0x80, 0x90, 0x80, 0x90]);
        let releases = e.stop();
        assert_eq!(releases.len(), 1);
    }

    #[test]
    fn program_changes_at_start_and_on_edit() {
        let mut e = Engine::new();
        let mut d = doc("drum", &[]);
        d.nodes[2]
            .params
            .insert("program".into(), ParamValue::Number(49.0));
        e.set_graph(&d);
        e.play(0.0, 1);
        assert_eq!(e.poll(0.0)[0].bytes, vec![0xc9, 48]);
        d.nodes[2]
            .params
            .insert("program".into(), ParamValue::Number(1.0));
        e.set_graph(&d);
        assert!(e.poll(0.001).iter().any(|m| m.bytes == vec![0xc9, 0]));
    }

    #[test]
    fn tempo_change_keeps_playhead() {
        let mut e = Engine::new();
        e.set_bpm(0.0, 120.0);
        e.play(0.0, 1);
        let before = e.playhead(1.05);
        e.set_bpm(1.05, 60.0);
        assert!((e.playhead(1.05) - before).abs() < 1e-6);
        assert!((e.playhead(2.05) - before - PPQ as f64).abs() < 1e-6);
    }

    #[test]
    fn clock_sends_24_pulses_per_beat() {
        let mut e = Engine::new();
        e.set_clock_ports(vec!["synth".into()]);
        e.set_bpm(0.0, 120.0);
        e.play(0.0, 1);
        let sent = run(&mut e, 0.549);
        let start = sent
            .iter()
            .position(|(_, b)| b == &vec![0xfa])
            .expect("start sent");
        let pulses = sent.iter().filter(|(_, b)| b == &vec![0xf8]).count();
        assert_eq!(pulses, 24, "one beat at 120 bpm lasts 0.5 s");
        assert!(sent[..start].iter().any(|(_, b)| b == &vec![0xf2, 0, 0]));
        assert!(e.stop().iter().any(|m| m.bytes == vec![0xfc]));
    }
}
