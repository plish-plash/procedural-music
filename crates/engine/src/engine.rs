//! Transport, arrangement, lookahead scheduling and MIDI dispatch.
//!
//! The engine is driven by the caller's clock: every call takes `now` in seconds
//! (any monotonic origin). [`Engine::poll`] evaluates the graph a little ahead of
//! the playhead, queues timed MIDI messages, and returns the ones that are due.
//! Notes for built-in instruments are handed out early instead (see
//! [`Engine::take_audio`]), so the audio thread can place them sample-accurately.
//!
//! Time comes in two kinds. *Play ticks* count up from the moment Play is pressed and
//! never jump; *song ticks* are positions in the arrangement, which loop and jump when
//! the transport loops or a section is cued. Each section is evaluated in its own time
//! (starting at 0), with its own parameter overrides and tempo.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use pm_synth::{AudioMsg, ModuleKind, NoteEvent, RackDesc, TimedEvent, TimedSet};
use pm_theory::{Meter, Note, Ticks, PPQ};

use crate::audition;
use crate::graph::{
    describe, section_key, value_view, CompileReport, EvalContext, Graph, GraphDoc, SinkOutput,
};
use crate::json::Json;
use crate::nodes::{ControlMessage, Sink};
use crate::rng;
use crate::value::Value;

/// Port name meaning "the first MIDI port the app finds".
pub const DEFAULT_PORT: &str = "";

/// How many bars the editor's previews (and probes and auditions) evaluate.
const PREVIEW_BARS: u32 = 4;

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

/// The state of an incoming MIDI clock being followed.
#[derive(Default)]
struct ClockIn {
    /// When the last clock pulse arrived.
    last: Option<f64>,
    /// Recent gaps between pulses, in seconds (a beat's worth at most).
    gaps: VecDeque<f64>,
    /// Pulses since Start or Continue.
    count: u64,
    /// The tempo the pulses imply, once a few have arrived.
    bpm: Option<f64>,
}

/// Delay between a transport event and the first notes it schedules, so they are not
/// already late (Play and clock pulses use the same).
const START_DELAY: f64 = 0.05;

/// What the editor's previews evaluate (see [`Engine::preview_target`]).
struct PreviewTarget {
    /// Which of the song's graphs (its section's overrides).
    graph: usize,
    /// The preview window, section and salt.
    ctx: EvalContext,
    /// The section-local tick of interest (the playhead): automation is shown as it is here.
    focus: Ticks,
    bpm: f64,
}

/// One section of the compiled arrangement.
#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    /// [`section_key`] of the name.
    pub key: u64,
    /// Which time this name comes round (1 for the first).
    pub repeat: u32,
    /// Song tick where it starts.
    pub start: Ticks,
    pub length: Ticks,
    pub bpm: Option<f64>,
    /// Index into [`Song::graphs`].
    graph: usize,
}

impl Section {
    pub fn end(&self) -> Ticks {
        self.start + self.length
    }
}

/// The arrangement, compiled: one graph per distinct set of section overrides.
#[derive(Default)]
struct Song {
    /// `graphs[0]` is the graph without overrides.
    graphs: Vec<Graph>,
    racks: Vec<RackDesc>,
    sections: Vec<Section>,
    length: Ticks,
}

impl Song {
    fn compile(doc: &GraphDoc, meter: Meter) -> Song {
        let mut graphs = vec![Graph::compile(&doc.with_overrides(&[]))];
        let mut keys = vec![Vec::new()];
        let mut seen: HashMap<u64, u32> = HashMap::new();
        let mut sections = Vec::new();
        let mut start = 0;
        for s in &doc.sections {
            let graph = match keys.iter().position(|k| k == &s.overrides) {
                Some(i) => i,
                None => {
                    graphs.push(Graph::compile(&doc.with_overrides(&s.overrides)));
                    keys.push(s.overrides.clone());
                    graphs.len() - 1
                }
            };
            let key = section_key(&s.name);
            let repeat = seen.entry(key).or_insert(0);
            *repeat += 1;
            let length = s.bars.max(1) * meter.ticks_per_bar();
            sections.push(Section {
                name: s.name.clone(),
                key,
                repeat: *repeat,
                start,
                length,
                bpm: s.bpm,
                graph,
            });
            start += length;
        }
        let racks = graphs
            .iter()
            .map(|g| audition::with_preview(g.rack_desc()))
            .collect();
        Song {
            graphs,
            racks,
            sections,
            length: start,
        }
    }

    fn locate(&self, tick: Ticks) -> Option<usize> {
        self.sections
            .iter()
            .position(|s| tick >= s.start && tick < s.end())
    }

    fn base(&self) -> &Graph {
        &self.graphs[0]
    }
}

/// What the transport loops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once and stop at the end of the arrangement.
    Off,
    /// Loop the whole arrangement.
    Song,
    /// Loop one section.
    Section(usize),
}

/// Maps play ticks to song ticks.
#[derive(Clone, Copy, Debug, Default)]
struct Transport {
    /// Play tick where this mapping starts.
    p0: Ticks,
    /// Song tick at `p0`.
    from: Ticks,
    /// Loop range in song ticks.
    range: Option<(Ticks, Ticks)>,
    /// Without a loop: song tick where playback ends.
    end: Option<Ticks>,
}

impl Transport {
    /// (song tick, number of times the loop has wrapped), or `None` past the end.
    fn map(&self, p: Ticks) -> Option<(Ticks, u64)> {
        let d = p.saturating_sub(self.p0);
        match self.range {
            Some((a, b)) if b > a => {
                let off = (self.from - a) as u64 + d as u64;
                let len = (b - a) as u64;
                Some((a + (off % len) as Ticks, off / len))
            }
            _ => {
                let s = self.from + d;
                if self.end.is_some_and(|e| s >= e) {
                    None
                } else {
                    Some((s, 0))
                }
            }
        }
    }
}

/// A tempo change at a play tick.
#[derive(Clone, Copy, Debug)]
struct TempoPoint {
    tick: f64,
    time: f64,
    bpm: f64,
}

impl TempoPoint {
    fn ticks_per_second(&self) -> f64 {
        self.bpm / 60.0 * PPQ as f64
    }
}

/// Where playback is, for the UI.
#[derive(Clone, Debug, Default)]
pub struct Position {
    /// Song tick.
    pub song: Ticks,
    /// The section it is in, if there is an arrangement.
    pub section: Option<usize>,
    /// Tick within that section (the song tick without an arrangement).
    pub local: Ticks,
    /// Tempo in effect.
    pub bpm: f64,
}

pub struct Engine {
    song: Song,
    /// A new song waiting for the next bar line (when edits are quantized).
    pending_song: Option<Song>,
    meter: Meter,
    /// Song tempo; sections can set their own.
    bpm: f64,
    playing: bool,
    /// Tempo changes while playing, oldest first (never empty while playing).
    tempo: Vec<TempoPoint>,
    transport: Transport,
    loop_mode: LoopMode,
    /// A section to jump to at the next bar line.
    cue: Option<usize>,
    /// Song tick where Play starts.
    cursor: Ticks,
    /// Play tick where the arrangement ended (playback stops when the playhead gets there).
    end_at: Option<Ticks>,
    /// Which graph's rack the audio thread has.
    rack_now: usize,
    /// Everything before this play tick has been evaluated and queued.
    scheduled_until: Ticks,
    queue: BinaryHeap<Pending>,
    seq: u64,
    /// Sounding notes per (port, channel, key), counted so overlapping notes on one key
    /// don't cut each other off.
    active: HashMap<(String, u8, u8), u32>,
    clock_ports: Vec<String>,
    /// Seconds to shift messages for each port by (to line external synths up).
    latency: HashMap<String, f64>,
    /// The last control message sent for each (port, channel, kind), so unchanged values
    /// aren't sent again.
    controls_sent: HashMap<(String, u8, ControlMessage), Vec<u8>>,
    /// Following incoming MIDI clock: its tempo and transport drive playback.
    clock_in: Option<ClockIn>,
    /// What unlocked random nodes mix in: new for each Play (unless the project is
    /// locked), kept after Stop so previews still show what was heard.
    salt: u64,
    /// Whether `salt` has been played; the next Play then takes a fresh one.
    salt_played: bool,
    /// Project-wide lock: every playback uses the same salt (the project's seed).
    random_locked: bool,
    /// The caller's clock at the last poll or command (for previews of what's playing).
    now: f64,
    /// The document the current graph was built from (for offline rendering).
    doc: GraphDoc,
    /// The section whose overrides the editor is showing (previews use it).
    focus: Option<usize>,
    /// Messages for the audio thread, collected until [`Engine::take_audio`].
    audio: Vec<AudioMsg>,
    audio_events: Vec<TimedEvent>,
    /// Automated module settings for the audio thread, collected like `audio_events`.
    audio_sets: Vec<TimedSet>,
    /// The automated settings last sent for each rack module.
    module_state: HashMap<u64, ModuleKind>,
    next_note_id: u64,
    /// When true, slow work (counterpoint solving) finishes before evaluation returns,
    /// instead of improving in the background. For offline rendering and tests.
    pub synchronous: bool,
    /// When true, graph edits made while playing wait for the next bar line.
    pub quantize_edits: bool,
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
            song: Song::compile(&GraphDoc::default(), Meter::COMMON),
            pending_song: None,
            meter: Meter::COMMON,
            bpm: 110.0,
            playing: false,
            tempo: Vec::new(),
            transport: Transport::default(),
            loop_mode: LoopMode::Song,
            cue: None,
            cursor: 0,
            end_at: None,
            rack_now: 0,
            scheduled_until: 0,
            queue: BinaryHeap::new(),
            seq: 0,
            active: HashMap::new(),
            clock_ports: Vec::new(),
            latency: HashMap::new(),
            controls_sent: HashMap::new(),
            clock_in: None,
            salt: 0,
            salt_played: false,
            random_locked: false,
            now: 0.0,
            doc: GraphDoc::default(),
            focus: None,
            audio: vec![AudioMsg::Tempo(110.0)],
            audio_events: Vec::new(),
            audio_sets: Vec::new(),
            module_state: HashMap::new(),
            next_note_id: 1,
            synchronous: false,
            quantize_edits: false,
            lookahead: 0.12,
            slice: PPQ / 4,
        }
    }

    /// Replaces the project. While playing, already-queued notes still sound; the new
    /// graph takes over from the next evaluation window (or the next bar line, when
    /// edits are quantized).
    pub fn set_graph(&mut self, doc: &GraphDoc) -> CompileReport {
        let song = Song::compile(doc, self.meter);
        self.doc = doc.clone();
        if self.focus.is_some_and(|f| f >= song.sections.len()) {
            self.focus = None;
        }
        if self.cursor >= song.length.max(1) && !song.sections.is_empty() {
            self.cursor = 0;
        }
        let report = self.report_for(&song);
        if self.playing && self.quantize_edits {
            self.pending_song = Some(song);
        } else {
            self.install(song);
        }
        report
    }

    fn install(&mut self, song: Song) {
        let old_programs = self.song.base().programs();
        // Keep the rack of the section that is playing.
        let rack = self.rack_now.min(song.graphs.len() - 1);
        self.audio.push(AudioMsg::Rack(song.racks[rack].clone()));
        self.rack_now = rack;
        self.module_state.clear();
        if self.playing {
            // Apply instrument changes right away (due at the next poll).
            for (port, channel, program) in song.base().programs() {
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
        let range_changed = self.song.length != song.length
            || self
                .song
                .sections
                .iter()
                .map(|s| (s.start, s.length))
                .ne(song.sections.iter().map(|s| (s.start, s.length)));
        self.song = song;
        if self.playing && range_changed {
            // Sections moved: keep playing from the same song position under the new layout.
            let p = self.scheduled_until;
            let from = self.transport.map(p).map_or(0, |(s, _)| s);
            self.transport = self.make_transport(from.min(self.song.length.saturating_sub(1)), p);
            self.end_at = None;
        }
    }

    /// Sends the rack of one of the song's graphs. Its modules start from their own
    /// settings, so automated ones are sent again.
    fn push_rack(&mut self, graph: usize) {
        self.flush_events();
        self.audio
            .push(AudioMsg::Rack(self.song.racks[graph].clone()));
        self.rack_now = graph;
        self.module_state.clear();
    }

    /// Moves queued instrument notes and settings into the message list, so messages
    /// pushed after this keep their order relative to them.
    fn flush_events(&mut self) {
        if !self.audio_events.is_empty() {
            self.audio
                .push(AudioMsg::Events(std::mem::take(&mut self.audio_events)));
        }
        if !self.audio_sets.is_empty() {
            self.audio.push(AudioMsg::Set(std::mem::take(&mut self.audio_sets)));
        }
    }

    /// The salt unlocked random nodes use in a section (`None`: no arrangement) on a lap
    /// of the transport, so each section and each time round plays something new.
    fn section_salt(&self, section: Option<usize>, lap: u64) -> u64 {
        match section {
            Some(i) => rng::hash(&[self.salt, i as u64, lap]),
            None => self.salt,
        }
    }

    /// What the previews show: what's playing (the 4-bar block under the playhead, in
    /// its section and lap), or when stopped, where Play will start, with the salt last
    /// played (or rerolled since).
    fn preview_target(&self, song: &Song) -> PreviewTarget {
        let (tick, lap) = if self.playing {
            let p = self.playhead(self.now) as Ticks;
            self.transport.map(p).unwrap_or((self.song.length.saturating_sub(1), 0))
        } else {
            (self.cursor, 0)
        };
        let section = song.locate(tick);
        let (graph, local, key, repeat) = match section {
            Some(i) => {
                let s = &song.sections[i];
                (s.graph, tick - s.start, s.key, s.repeat)
            }
            None => (0, tick, 0, 0),
        };
        let window = self.meter.ticks_per_bar() * PREVIEW_BARS;
        let start = local - local % window;
        PreviewTarget {
            graph,
            focus: local,
            bpm: self.section_bpm(section),
            ctx: EvalContext {
                preview: true,
                sync: self.synchronous,
                section: key,
                repeat,
                ..EvalContext::new(start, start + window, self.meter, self.section_salt(section, lap))
            },
        }
    }

    /// Identifies what the previews show: it changes when the playhead enters another
    /// block, section or lap, the salt changes, or (with automation, which moves within a
    /// block) on every beat.
    pub fn preview_key(&self) -> u64 {
        let song = self.pending_song.as_ref().unwrap_or(&self.song);
        let t = self.preview_target(song);
        let beat = if song.graphs[t.graph].is_automated() && self.playing {
            t.focus / PPQ
        } else {
            0
        };
        rng::hash(&[t.graph as u64, t.ctx.section, t.ctx.repeat as u64, t.ctx.start as u64, t.ctx.salt, beat as u64])
            & 0xffff_ffff
    }

    fn report_for(&self, song: &Song) -> CompileReport {
        let t = self.preview_target(song);
        song.graphs[t.graph].report_window(&t.ctx, t.focus)
    }

    /// The value on one of a node's outputs as the previews show it, and the tempo there.
    fn focused_value(&self, node: &str, port: Option<&str>) -> Result<(Value, f64), String> {
        let song = self.pending_song.as_ref().unwrap_or(&self.song);
        let t = self.preview_target(song);
        Ok((song.graphs[t.graph].output_value(node, port, &t.ctx, t.focus)?, t.bpm))
    }

    /// What is on one of a node's outputs: a text description and, where one can be
    /// drawn, a view (as in the report). For hovering over a wire.
    pub fn probe(&self, node: &str, port: Option<&str>) -> Result<(String, Option<Json>), String> {
        let song = self.pending_song.as_ref().unwrap_or(&self.song);
        let t = self.preview_target(song);
        let value = song.graphs[t.graph].output_value(node, port, &t.ctx, t.focus)?;
        Ok((
            describe(&value, self.meter),
            value_view(&value, self.meter, t.ctx.start, t.ctx.end - t.ctx.start),
        ))
    }

    /// The seed that, with Lock on, makes a random node keep playing what the previews
    /// show (what's playing, or what just played).
    pub fn capture_seed(&self, node: &str) -> Result<u64, String> {
        let song = self.pending_song.as_ref().unwrap_or(&self.song);
        let t = self.preview_target(song);
        let seed = song.graphs[t.graph]
            .param_number(node, "seed")
            .ok_or_else(|| format!("'{node}' has no seed"))?;
        Ok(rng::fresh_seed(seed.max(0.0) as u64, t.ctx.salt))
    }

    /// Previews and views of the current graph.
    pub fn report(&self) -> CompileReport {
        self.report_for(self.pending_song.as_ref().unwrap_or(&self.song))
    }

    /// Notes the caller's clock, so previews know where the playhead is.
    pub fn set_now(&mut self, now: f64) {
        self.now = now;
    }

    // ---- Randomness ------------------------------------------------------------------

    /// The salt of the current (or last, or next) playback.
    pub fn salt(&self) -> u64 {
        self.salt
    }

    pub fn random_locked(&self) -> bool {
        self.random_locked
    }

    /// Sets the salt the next Play uses (and previews show).
    pub fn set_salt(&mut self, salt: u64) {
        self.salt = salt & 0xffff_ffff;
        self.salt_played = false;
    }

    /// Locks or unlocks randomness for the whole project. Locked, every playback uses the
    /// project's seed as its salt: `seed`, or without one, the current salt (keeping what
    /// was just heard). Returns the seed in use.
    pub fn set_random(&mut self, locked: bool, seed: Option<u64>) -> u64 {
        self.random_locked = locked;
        if let Some(seed) = seed {
            self.set_salt(seed);
        }
        self.salt
    }

    /// Something new: a new project seed when locked, a new salt (for the next Play and
    /// the previews) when not. Returns the salt now in use.
    pub fn reroll(&mut self, fresh: u64) -> u64 {
        self.set_salt(fresh);
        self.salt
    }

    /// Shows previews as heard in a section (`None`: without overrides).
    pub fn set_focus(&mut self, focus: Option<usize>) {
        self.focus = focus.filter(|&f| f < self.song.sections.len());
    }

    /// The document of the current graph.
    pub fn doc(&self) -> &GraphDoc {
        &self.doc
    }

    /// The compiled arrangement.
    pub fn sections(&self) -> &[Section] {
        &self.song.sections
    }

    /// Length of the arrangement in ticks (0 without one).
    pub fn song_length(&self) -> Ticks {
        self.song.length
    }

    /// Messages for the audio thread since the last call: rack changes, tempo, instrument
    /// notes (ahead of time) and stops, in order.
    pub fn take_audio(&mut self) -> Vec<AudioMsg> {
        self.flush_events();
        std::mem::take(&mut self.audio)
    }

    pub fn meter(&self) -> Meter {
        self.meter
    }

    /// The song tempo (sections may override it; see [`Engine::position`]).
    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// Ports that receive MIDI clock (24 per quarter note) and start/stop messages.
    pub fn set_clock_ports(&mut self, ports: Vec<String>) {
        self.clock_ports = ports;
    }

    pub fn clock_ports(&self) -> &[String] {
        &self.clock_ports
    }

    /// Latency offsets in milliseconds per port name: that port's messages go out this much
    /// later (or earlier, if negative, within the lookahead). The default port is "".
    pub fn set_midi_latency(&mut self, ms: Vec<(String, f64)>) {
        self.latency = ms
            .into_iter()
            .filter(|(_, ms)| ms.is_finite() && *ms != 0.0)
            .map(|(port, ms)| (port, ms.clamp(-500.0, 500.0) / 1000.0))
            .collect();
    }

    /// Starts or stops following incoming MIDI clock (the caller passes its messages to
    /// [`Engine::clock_message`]).
    pub fn set_clock_follow(&mut self, on: bool) {
        self.clock_in = on.then(ClockIn::default);
    }

    pub fn following_clock(&self) -> bool {
        self.clock_in.is_some()
    }

    /// The tempo of the clock being followed, once known.
    pub fn clock_bpm(&self) -> Option<f64> {
        self.clock_in.as_ref().and_then(|c| c.bpm)
    }

    /// One real-time message from the followed clock: Start (from the top), Continue
    /// (from the cursor), Stop, or a clock pulse (24 per quarter note). Returns messages
    /// to send now (note-offs when stopping). `fresh` is as for [`Engine::play`].
    pub fn clock_message(&mut self, now: f64, byte: u8, fresh: u64) -> Vec<MidiMessage> {
        if self.clock_in.is_none() {
            return Vec::new();
        }
        match byte {
            0xfa | 0xfb => {
                let out = self.stop();
                if byte == 0xfa {
                    self.cursor = 0;
                }
                if let Some(c) = self.clock_in.as_mut() {
                    c.count = 0;
                    c.last = None;
                }
                self.play(now, fresh);
                out
            }
            0xfc => self.stop(),
            0xf8 => {
                self.clock_pulse(now);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Measures the tempo, and on every beat re-anchors the tempo map so that beat lands
    /// with the incoming clock (keeping playback in phase, not just in tempo).
    fn clock_pulse(&mut self, now: f64) {
        let Some(c) = self.clock_in.as_mut() else {
            return;
        };
        if let Some(last) = c.last {
            let gap = now - last;
            // Ignore gaps from a paused or restarted clock.
            if gap > 0.0 && gap < 0.25 {
                c.gaps.push_back(gap);
                if c.gaps.len() > 24 {
                    c.gaps.pop_front();
                }
            }
        }
        c.last = Some(now);
        if c.gaps.len() >= 6 {
            let mean = c.gaps.iter().sum::<f64>() / c.gaps.len() as f64;
            c.bpm = Some((60.0 / (24.0 * mean)).clamp(20.0, 400.0));
        }
        let pulse = c.count;
        c.count += 1;
        let bpm = c.bpm;
        if !self.playing || pulse % 24 != 0 {
            return;
        }
        let current = self.tempo.last().map_or(self.bpm, |t| t.bpm);
        let bpm = bpm.unwrap_or(current);
        let tick = (pulse / 24) as f64 * PPQ as f64;
        self.tempo.retain(|t| t.tick < tick);
        self.tempo.push(TempoPoint { tick, time: now + START_DELAY, bpm });
        if (bpm - current).abs() > 0.5 {
            self.audio.push(AudioMsg::Tempo(bpm));
        }
    }

    fn tempo_at_time(&self, now: f64) -> Option<&TempoPoint> {
        let i = self.tempo.partition_point(|t| t.time <= now);
        self.tempo.get(i.saturating_sub(1))
    }

    fn tempo_at_tick(&self, tick: f64) -> Option<&TempoPoint> {
        let i = self.tempo.partition_point(|t| t.tick <= tick);
        self.tempo.get(i.saturating_sub(1))
    }

    /// The playhead in (fractional) play ticks.
    pub fn playhead(&self, now: f64) -> f64 {
        match self.tempo_at_time(now) {
            Some(t) if self.playing => (t.tick + (now - t.time) * t.ticks_per_second()).max(0.0),
            _ => 0.0,
        }
    }

    fn time_of(&self, tick: Ticks) -> f64 {
        let t = self
            .tempo_at_tick(tick as f64)
            .copied()
            .unwrap_or(TempoPoint {
                tick: 0.0,
                time: 0.0,
                bpm: self.bpm,
            });
        t.time + (tick as f64 - t.tick) / t.ticks_per_second()
    }

    /// Where playback is (or where it will start, when stopped).
    pub fn position(&self, now: f64) -> Position {
        let song = if self.playing {
            let p = self.playhead(now) as Ticks;
            match self.transport.map(p) {
                Some((s, _)) => s,
                None => self.song.length.saturating_sub(1),
            }
        } else {
            self.cursor
        };
        let section = self.song.locate(song);
        let local = section.map_or(song, |i| song - self.song.sections[i].start);
        let bpm = if self.playing {
            self.tempo_at_time(now).map_or(self.bpm, |t| t.bpm)
        } else {
            self.section_bpm(section)
        };
        Position {
            song,
            section,
            local,
            bpm,
        }
    }

    fn section_bpm(&self, section: Option<usize>) -> f64 {
        section
            .and_then(|i| self.song.sections.get(i)?.bpm)
            .unwrap_or(self.bpm)
    }

    /// Changes the song tempo without moving the playhead.
    pub fn set_bpm(&mut self, now: f64, bpm: f64) {
        let bpm = bpm.clamp(20.0, 400.0);
        self.bpm = bpm;
        if self.playing {
            let section = self.position(now).section;
            if section.and_then(|i| self.song.sections[i].bpm).is_none() {
                self.change_tempo_now(now, bpm);
            }
        } else {
            self.audio.push(AudioMsg::Tempo(bpm));
        }
    }

    fn change_tempo_now(&mut self, now: f64, bpm: f64) {
        let tick = self.playhead(now);
        self.tempo.retain(|t| t.tick <= tick);
        self.tempo.push(TempoPoint {
            tick,
            time: now,
            bpm,
        });
        self.audio.push(AudioMsg::Tempo(bpm));
    }

    /// Sets what the transport loops. Takes effect from the next evaluation window.
    pub fn set_loop(&mut self, mode: LoopMode) {
        let mode = match mode {
            LoopMode::Section(i) if i >= self.song.sections.len() => LoopMode::Song,
            m => m,
        };
        self.loop_mode = mode;
        if self.playing {
            let p = self.scheduled_until;
            let from = self.transport.map(p).map_or(self.cursor, |(s, _)| s);
            self.transport = self.make_transport(from, p);
            self.end_at = None;
        }
    }

    /// Where Play starts (a song tick; snapped to the bar).
    pub fn set_cursor(&mut self, tick: Ticks) {
        let bar = self.meter.ticks_per_bar();
        let max = self.song.length.saturating_sub(1);
        self.cursor = (tick.min(max) / bar) * bar;
    }

    /// Jumps to a section at the next bar line and loops it (performance cueing).
    /// When stopped, playback will start there.
    pub fn cue(&mut self, section: usize) {
        if section >= self.song.sections.len() {
            return;
        }
        if self.playing {
            self.cue = Some(section);
        } else {
            self.loop_mode = LoopMode::Section(section);
            self.cursor = self.song.sections[section].start;
        }
    }

    /// The section waiting to be cued, if any.
    pub fn cued(&self) -> Option<usize> {
        self.cue
    }

    fn make_transport(&self, from: Ticks, p0: Ticks) -> Transport {
        let song = &self.song;
        if song.sections.is_empty() {
            return Transport {
                p0,
                from,
                range: None,
                end: None,
            };
        }
        let range = match self.loop_mode {
            LoopMode::Off => None,
            LoopMode::Song => Some((0, song.length)),
            LoopMode::Section(i) => song.sections.get(i).map(|s| (s.start, s.end())),
        };
        let from = match range {
            Some((a, b)) if from < a || from >= b => a,
            _ => from.min(song.length.saturating_sub(1)),
        };
        Transport {
            p0,
            from,
            range,
            end: if range.is_none() {
                Some(song.length)
            } else {
                None
            },
        }
    }

    /// Starts from the cursor. `fresh` should differ every time: unlocked random nodes
    /// play something new on each Play, unless the project is locked, or the current salt
    /// hasn't been played yet (it was rerolled, and the previews already show it).
    pub fn play(&mut self, now: f64, fresh: u64) {
        if self.playing {
            return;
        }
        if let Some(song) = self.pending_song.take() {
            self.install(song);
        }
        self.playing = true;
        self.now = now;
        if self.salt_played && !self.random_locked {
            self.salt = fresh & 0xffff_ffff;
        }
        self.salt_played = true;
        self.scheduled_until = 0;
        self.end_at = None;
        self.cue = None;
        self.controls_sent.clear();
        self.transport = self.make_transport(self.cursor, 0);
        let first = self.song.locate(self.transport.from);
        let bpm = self.clock_bpm().unwrap_or_else(|| self.section_bpm(first));
        // A small delay so the first notes are not already late.
        self.tempo = vec![TempoPoint {
            tick: 0.0,
            time: now + START_DELAY,
            bpm,
        }];
        self.audio.push(AudioMsg::Tempo(bpm));
        let start_time = now + START_DELAY;
        for (port, channel, program) in self.song.base().programs() {
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
                start_time,
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
        self.cue = None;
        if let Some(song) = self.pending_song.take() {
            self.install(song);
        }
        if !self.playing && self.active.is_empty() {
            return out;
        }
        self.flush_events();
        self.audio.push(AudioMsg::Stop);
        // Automated modules keep their last settings; send them again next time.
        self.module_state.clear();
        self.audio.push(AudioMsg::Tempo(self.bpm));
        self.playing = false;
        self.tempo.clear();
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
        let port = match &event {
            Event::Raw(m) => &m.port,
            Event::Off { port, .. } | Event::On { port, .. } => port,
        };
        let at = at + self.latency.get(port).copied().unwrap_or(0.0);
        self.seq += 1;
        self.queue.push(Pending {
            at,
            seq: self.seq,
            event,
        });
    }

    /// Schedules ahead of the playhead and returns the messages due at `now`.
    pub fn poll(&mut self, now: f64) -> Vec<MidiMessage> {
        self.now = now;
        if self.playing {
            let playhead = self.playhead(now);
            if self.end_at.is_some_and(|e| playhead >= e as f64) {
                // The arrangement has finished.
                let mut out = Vec::new();
                while self.queue.peek().is_some_and(|p| p.at <= now) {
                    let p = self.queue.pop().expect("peeked");
                    self.dispatch(p.event, &mut out);
                }
                out.extend(self.stop());
                return out;
            }
            // If we fell far behind (the machine slept), skip ahead instead of
            // sending a burst of stale notes.
            let bar = self.meter.ticks_per_bar();
            if ((self.scheduled_until + 2 * bar) as f64) < playhead {
                self.scheduled_until = (playhead as Ticks / bar) * bar;
            }
            let horizon = self.playhead(now + self.lookahead) as Ticks;
            while self.scheduled_until < horizon && self.end_at.is_none() {
                let start = self.scheduled_until;
                self.schedule_window(start);
                self.scheduled_until = start + self.slice;
            }
            self.prune_tempo(now);
        }
        let mut out = Vec::new();
        while self.queue.peek().is_some_and(|p| p.at <= now) {
            let p = self.queue.pop().expect("peeked");
            self.dispatch(p.event, &mut out);
        }
        out
    }

    fn prune_tempo(&mut self, now: f64) {
        let keep_from = self.tempo.partition_point(|t| t.time <= now - 5.0);
        if keep_from > 1 {
            self.tempo.drain(..keep_from - 1);
        }
    }

    /// Seconds until the next queued message, if any (for the caller's sleep).
    pub fn next_due(&self, now: f64) -> Option<f64> {
        self.queue.peek().map(|p| (p.at - now).max(0.0))
    }

    /// Evaluates one slice starting at play tick `p` and queues what it produces.
    fn schedule_window(&mut self, p: Ticks) {
        let bar = self.meter.ticks_per_bar();
        if p.is_multiple_of(bar) {
            if let Some(song) = self.pending_song.take() {
                self.install(song);
            }
            if let Some(i) = self.cue.take() {
                self.loop_mode = LoopMode::Section(i);
                let start = self.song.sections[i].start;
                self.transport = self.make_transport(start, p);
            }
        }
        let Some((s, lap)) = self.transport.map(p) else {
            self.end_at = Some(p);
            return;
        };
        let section = self.song.locate(s);
        let salt = self.section_salt(section, lap);
        let (graph, local, key, repeat, bpm) = match section {
            Some(i) => {
                let sec = &self.song.sections[i];
                (sec.graph, s - sec.start, sec.key, sec.repeat, sec.bpm.unwrap_or(self.bpm))
            }
            None => (0, s, 0, 0, self.bpm),
        };
        // Tempo and sound changes at section boundaries (unless an incoming clock sets the tempo).
        let current = self.tempo_at_tick(p as f64).map_or(self.bpm, |t| t.bpm);
        if (current - bpm).abs() > 1e-9 && self.clock_in.is_none() {
            let time = self.time_of(p);
            self.tempo.retain(|t| t.tick < p as f64);
            self.tempo.push(TempoPoint {
                tick: p as f64,
                time,
                bpm,
            });
            self.audio.push(AudioMsg::Tempo(bpm));
        }
        if graph != self.rack_now {
            self.push_rack(graph);
        }
        let ctx = EvalContext {
            sync: self.synchronous,
            section: key,
            repeat,
            ..EvalContext::new(local, local + self.slice, self.meter, salt)
        };
        let (sinks, modules) = self.song.graphs[graph].evaluate_window(&ctx);
        // Automated sound settings go to the rack when they change.
        let at = self.time_of(p);
        for (module, kind) in modules {
            if self.module_state.get(&module) != Some(&kind) {
                self.module_state.insert(module, kind);
                self.audio_sets.push(TimedSet { at, module, kind });
            }
        }
        for mut sink in sinks {
            for n in &mut sink.notes {
                n.start = p + (n.start - local);
            }
            self.queue_sink(sink, at);
        }
        let pulse = PPQ / 24;
        let mut t = p.div_ceil(pulse) * pulse;
        while t < p + self.slice {
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

    /// A built-in instrument's note-on and note-off for `note`, at times in seconds.
    fn instrument_events(&mut self, module: u64, note: &Note, on: f64, off: f64) -> [TimedEvent; 2] {
        let id = self.next_note_id;
        self.next_note_id += 1;
        let (key, _) = note.pitch.tuning().to_midi(note.pitch.step());
        [
            TimedEvent {
                at: on,
                module,
                event: NoteEvent::On {
                    id,
                    freq: note.pitch.frequency() as f32,
                    key: key.clamp(0, 127) as u8,
                    velocity: note.velocity.clamp(1, 127),
                },
            },
            TimedEvent {
                at: off,
                module,
                event: NoteEvent::Off { id },
            },
        ]
    }

    /// Plays one of a node's outputs once through the preview instruments, as heard in
    /// the section in focus, cutting off any audition still sounding. Works whether or
    /// not the transport is playing. Returns the audition's length in seconds.
    pub fn audition(&mut self, now: f64, node: &str, port: Option<&str>) -> Result<f64, String> {
        let (value, bpm) = self.focused_value(node, port)?;
        let notes = audition::notes(&value, self.meter)?;
        let seconds = |t: Ticks| t as f64 * 60.0 / (bpm * PPQ as f64);
        // A little ahead, so the first note isn't already late when the audio thread sees it.
        let start = now + 0.03;
        let mut events = Vec::with_capacity(2 * notes.len());
        let mut end = start;
        for (module, note) in &notes {
            let off = start + seconds(note.end());
            end = end.max(off);
            events.extend(self.instrument_events(*module, note, start + seconds(note.start), off));
        }
        events.sort_by(|a, b| a.at.total_cmp(&b.at));
        self.audio
            .push(AudioMsg::Silence(vec![audition::SYNTH, audition::DRUMS]));
        self.audio.push(AudioMsg::Events(events));
        Ok(end - start)
    }

    /// Queues a sink's output from a window starting at time `at` (seconds).
    fn queue_sink(&mut self, sink: SinkOutput, at: f64) {
        let (port, channel) = match sink.sink {
            Sink::Midi { port, channel } => (port, channel & 0x0f),
            Sink::Control { port, channel, message } => {
                // Sent when the value changes.
                if let Some(value) = sink.control {
                    let bytes = message.bytes(channel, value);
                    let key = (port.clone(), channel, message);
                    if self.controls_sent.get(&key) != Some(&bytes) {
                        self.controls_sent.insert(key, bytes.clone());
                        self.push(at, Event::Raw(MidiMessage { port, bytes }));
                    }
                }
                return;
            }
            Sink::Instrument => {
                for note in sink.notes {
                    let (on, off) = (self.time_of(note.start), self.time_of(note.end()));
                    let events = self.instrument_events(sink.module, &note, on, off);
                    self.audio_events.extend(events);
                }
                return;
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

    /// Every note of the arrangement (or of `bars` bars without one), in song ticks,
    /// grouped by output node, as the current salt plays it (the first time through).
    /// With `sync` (for exports), slow nodes finish their best answer first and automation
    /// moves as finely as in playback; without it (the overview), once a beat.
    pub fn song_notes(&self, bars: u32, sync: bool) -> Vec<SinkOutput> {
        let song = self.pending_song.as_ref().unwrap_or(&self.song);
        let mut spans: Vec<(usize, Ticks, Ticks, u64, u32, u64)> = Vec::new();
        if song.sections.is_empty() {
            let length = bars.max(1) * self.meter.ticks_per_bar();
            spans.push((0, 0, length, 0, 0, self.section_salt(None, 0)));
        } else {
            for (i, s) in song.sections.iter().enumerate() {
                spans.push((s.graph, s.start, s.length, s.key, s.repeat, self.section_salt(Some(i), 0)));
            }
        }
        let slice = if sync { self.slice } else { PPQ };
        let mut out: Vec<SinkOutput> = Vec::new();
        for (graph, start, length, key, repeat, salt) in spans {
            let ctx = EvalContext {
                sync,
                section: key,
                repeat,
                ..EvalContext::new(0, length, self.meter, salt)
            };
            for mut sink in song.graphs[graph].evaluate_sliced(&ctx, slice) {
                for n in &mut sink.notes {
                    n.start += start;
                }
                match out.iter_mut().find(|o| o.node == sink.node) {
                    Some(o) => o.notes.extend(sink.notes),
                    None => out.push(sink),
                }
            }
        }
        out
    }

    /// The tempo map of the arrangement: (song tick, bpm) at every change.
    pub fn tempo_map(&self) -> Vec<(Ticks, f64)> {
        let mut map: Vec<(Ticks, f64)> = vec![(0, self.section_bpm(self.song.locate(0)))];
        for s in &self.song.sections {
            let bpm = s.bpm.unwrap_or(self.bpm);
            if map.last().is_some_and(|&(_, b)| (b - bpm).abs() > 1e-9) {
                map.push((s.start, bpm));
            }
        }
        map
    }

    /// Length in seconds of the arrangement (or of `bars` bars without one).
    pub fn duration(&self, bars: u32) -> f64 {
        let seconds = |ticks: Ticks, bpm: f64| ticks as f64 / PPQ as f64 * 60.0 / bpm;
        if self.song.sections.is_empty() {
            return seconds(bars * self.meter.ticks_per_bar(), self.bpm);
        }
        self.song
            .sections
            .iter()
            .map(|s| seconds(s.length, s.bpm.unwrap_or(self.bpm)))
            .sum()
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
            ..Default::default()
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
    fn audition_plays_through_the_preview_synth() {
        let mut e = Engine::new();
        e.set_graph(&doc("drum", &[]));
        e.take_audio();
        // A one-beat rhythm at 120 bpm loops to fill a bar: hits 0.5 s apart.
        e.set_bpm(0.0, 120.0);
        e.take_audio();
        let seconds = e.audition(10.0, "p", None).unwrap();
        assert!(seconds > 1.5 && seconds < 2.0, "{seconds}");
        let msgs = e.take_audio();
        assert_eq!(msgs[0], AudioMsg::Silence(vec![audition::SYNTH, audition::DRUMS]));
        let AudioMsg::Events(events) = &msgs[1] else {
            panic!("{msgs:?}")
        };
        let ons: Vec<f64> = events
            .iter()
            .filter(|ev| matches!(ev.event, NoteEvent::On { .. }))
            .map(|ev| ev.at)
            .collect();
        assert_eq!(ons.len(), 4);
        assert!(events.iter().all(|ev| ev.module == audition::DRUMS));
        assert!((ons[1] - ons[0] - 0.5).abs() < 1e-9);
        assert!(ons[0] > 10.0);
        // The output of a node that isn't there, or that makes notes, is refused.
        assert!(e.audition(10.0, "nope", None).is_err());
        assert!(e.audition(10.0, "d", None).is_err());
        // Every rack carries the preview instruments.
        e.set_graph(&doc("drum", &[]));
        let rack = e.take_audio().into_iter().find_map(|m| match m {
            AudioMsg::Rack(r) => Some(r),
            _ => None,
        });
        assert!(rack.unwrap().modules.iter().any(|m| m.id == audition::SYNTH));
    }

    #[test]
    fn automated_sound_settings_reach_the_rack() {
        let n = |id: &str, kind: &str, params: &[(&str, f64)]| NodeDoc {
            id: id.into(),
            kind: kind.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), ParamValue::Number(*v)))
                .collect(),
        };
        let doc = GraphDoc {
            nodes: vec![
                n("s", "poly_synth", &[]),
                n("l", "lfo", &[("min", 200.0), ("max", 4000.0), ("cycle", 1.0)]),
            ],
            wires: vec![WireDoc {
                from_node: "l".into(),
                from_port: "control".into(),
                to_node: "s".into(),
                to_port: "param.cutoff".into(),
            }],
            ..Default::default()
        };
        let mut e = Engine::new();
        e.set_graph(&doc);
        e.play(0.0, 1);
        run(&mut e, 2.0);
        let sets: Vec<(f64, f32)> = e
            .take_audio()
            .into_iter()
            .flat_map(|m| match m {
                AudioMsg::Set(s) => s,
                _ => Vec::new(),
            })
            .map(|s| match s.kind {
                ModuleKind::Poly(p) => (s.at, p.cutoff),
                k => panic!("{k:?}"),
            })
            .collect();
        assert!(sets.len() > 8, "{sets:?}");
        assert!(sets.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1));
        let (lo, hi) = sets.iter().fold((f32::MAX, 0f32), |(lo, hi), s| (lo.min(s.1), hi.max(s.1)));
        assert!(lo < 400.0 && hi > 3500.0, "{lo} {hi}");
        // After a stop, playing again sends the settings again.
        e.stop();
        e.take_audio();
        e.play(10.0, 2);
        run(&mut e, 10.2);
        assert!(e.take_audio().iter().any(|m| matches!(m, AudioMsg::Set(_))));
    }

    #[test]
    fn probe_shows_any_output() {
        let mut e = Engine::new();
        e.set_graph(&doc("drum", &[]));
        let (text, view) = e.probe("p", Some("rhythm")).unwrap();
        assert_eq!(text, "x...");
        assert_eq!(view.unwrap().get("kind").unwrap().as_str(), Some("steps"));
        let (text, view) = e.probe("d", None).unwrap();
        assert!(text.starts_with("16 notes in bars 1–4"), "{text}");
        assert_eq!(view.unwrap().get("kind").unwrap().as_str(), Some("roll"));
        assert_eq!(e.probe("p", Some("nope")).unwrap_err(), "no output 'nope'");
        assert_eq!(e.probe("o", None).unwrap_err(), "this node has no output");
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

    #[test]
    fn latency_shifts_a_port() {
        let first_note = |latency: f64| {
            let mut e = Engine::new();
            e.set_graph(&doc("drum", &[]));
            e.set_midi_latency(vec![(DEFAULT_PORT.into(), latency)]);
            e.set_bpm(0.0, 120.0);
            e.play(0.0, 1);
            run(&mut e, 0.3).into_iter().find(|(_, b)| b[0] & 0xf0 == 0x90).expect("a note").0
        };
        let plain = first_note(0.0);
        assert!((first_note(100.0) - plain - 0.1).abs() < 0.002);
        assert!((plain - first_note(-30.0) - 0.03).abs() < 0.002);
    }

    #[test]
    fn control_changes_are_sent_when_they_change() {
        let n = |id: &str, kind: &str, params: &[(&str, ParamValue)]| NodeDoc {
            id: id.into(),
            kind: kind.into(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        };
        let doc = GraphDoc {
            nodes: vec![
                // 0 to 127 and back over a bar, as steps of 0, 64 and 127 a beat each.
                n("s", "steps", &[("values", ParamValue::Text("0 64 127 127".into()))]),
                n("beats", "pattern", &[("pattern", ParamValue::Text("x".into())), ("step", ParamValue::Text("1/4".into()))]),
                n("cc", "midi_cc", &[("cc", ParamValue::Number(1.0)), ("channel", ParamValue::Number(2.0))]),
            ],
            wires: vec![
                WireDoc { from_node: "beats".into(), from_port: "rhythm".into(), to_node: "s".into(), to_port: "rhythm".into() },
                WireDoc { from_node: "s".into(), from_port: "control".into(), to_node: "cc".into(), to_port: "value".into() },
            ],
            ..Default::default()
        };
        let mut e = Engine::new();
        let report = e.set_graph(&doc);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        e.set_bpm(0.0, 120.0);
        e.play(0.0, 1);
        let sent: Vec<Vec<u8>> = run(&mut e, 2.2).into_iter().map(|(_, b)| b).filter(|b| b[0] == 0xb1).collect();
        assert_eq!(sent, [vec![0xb1, 1, 0], vec![0xb1, 1, 64], vec![0xb1, 1, 127], vec![0xb1, 1, 0]]);
    }

    #[test]
    fn follows_incoming_clock() {
        let mut e = Engine::new();
        e.set_bpm(0.0, 90.0);
        e.set_clock_follow(true);
        // Start, then pulses at 150 bpm (1/60 s apart).
        e.clock_message(1.0, 0xfa, 7);
        assert!(e.is_playing());
        let gap = 60.0 / (150.0 * 24.0);
        for i in 0..48 {
            e.clock_message(1.0 + i as f64 * gap, 0xf8, 0);
        }
        assert!((e.clock_bpm().unwrap() - 150.0).abs() < 0.01);
        // Beat 2 (pulse 24) was re-anchored: the playhead is a beat in a beat after it.
        let beat2 = 1.0 + 24.0 * gap + START_DELAY;
        assert!((e.playhead(beat2) - PPQ as f64).abs() < 1.0);
        assert!((e.playhead(beat2 + 0.4) - 2.0 * PPQ as f64).abs() < 1.0);
        e.clock_message(2.0, 0xfc, 0);
        assert!(!e.is_playing());
        // Not following: clock messages do nothing.
        e.set_clock_follow(false);
        e.clock_message(3.0, 0xfa, 0);
        assert!(!e.is_playing());
    }

    use crate::graph::SectionDoc;

    fn section(
        name: &str,
        bars: u32,
        bpm: Option<f64>,
        overrides: &[(&str, &str, ParamValue)],
    ) -> SectionDoc {
        SectionDoc {
            name: name.into(),
            bars,
            bpm,
            overrides: overrides
                .iter()
                .map(|(n, p, v)| (n.to_string(), p.to_string(), v.clone()))
                .collect(),
        }
    }

    /// Kicks on every beat; in section B the pattern is twice as busy and faster.
    fn arranged() -> GraphDoc {
        let mut d = doc("drum", &[]);
        d.sections = vec![
            section("A", 1, None, &[]),
            section(
                "B",
                1,
                Some(240.0),
                &[("p", "pattern", ParamValue::Text("x.".into()))],
            ),
        ];
        d
    }

    #[test]
    fn sections_play_in_order_with_their_overrides_and_tempo() {
        let mut e = Engine::new();
        assert!(e.set_graph(&arranged()).issues.is_empty());
        e.set_bpm(0.0, 120.0);
        e.set_loop(LoopMode::Off);
        e.play(0.0, 1);
        let sent = run(&mut e, 4.0);
        let ons: Vec<f64> = sent
            .iter()
            .filter(|(_, b)| b[0] == 0x99)
            .map(|(t, _)| *t)
            .collect();
        // A: 4 beats at 120 bpm (2 s). B: 8 eighths at 240 bpm (1 s), then the song ends.
        assert_eq!(ons.len(), 12, "{ons:?}");
        assert!((ons[4] - 2.05).abs() < 0.003, "B starts at 2 s: {}", ons[4]);
        assert!(
            (ons[5] - 2.175).abs() < 0.003,
            "eighths at 240 bpm: {}",
            ons[5]
        );
        assert!(!e.is_playing(), "stops at the end of the arrangement");
        assert_eq!(e.duration(0), 3.0);
        assert_eq!(e.tempo_map(), vec![(0, 120.0), (3840, 240.0)]);
    }

    #[test]
    fn loops_cues_and_cursor() {
        let mut e = Engine::new();
        e.set_graph(&arranged());
        e.set_bpm(0.0, 120.0);
        // Loop B only, starting from the cursor in A: playback jumps to B.
        e.set_loop(LoopMode::Section(1));
        e.play(0.0, 1);
        let sent = run(&mut e, 2.04);
        let ons = sent.iter().filter(|(_, b)| b[0] == 0x99).count();
        assert_eq!(ons, 16, "two passes of B");
        assert_eq!(e.position(2.04).section, Some(1));
        e.stop();
        // Cue A while B loops: it takes over at the next bar line.
        e.play(0.0, 1);
        let _ = run(&mut e, 0.3);
        e.cue(0);
        assert_eq!(e.cued(), Some(0));
        let mut now = 0.3;
        while now < 1.2 {
            e.poll(now);
            now += 0.001;
        }
        assert_eq!(e.position(1.2).section, Some(0));
        assert_eq!(e.loop_mode(), LoopMode::Section(0));
        e.stop();
        e.set_cursor(3840 + 100);
        assert_eq!(e.position(0.0).song, 3840);
    }

    #[test]
    fn quantized_edits_wait_for_the_bar_line() {
        let mut e = Engine::new();
        e.set_graph(&doc("drum", &[]));
        e.set_bpm(0.0, 120.0);
        e.quantize_edits = true;
        e.play(0.0, 1);
        let _ = run(&mut e, 0.6); // into beat 2
        let mut busier = doc("drum", &[]);
        busier.nodes[0]
            .params
            .insert("pattern".into(), ParamValue::Text("x.".into()));
        e.set_graph(&busier);
        let mut sent = Vec::new();
        let mut now = 0.6;
        while now < 2.32 {
            for m in e.poll(now) {
                sent.push((now, m.bytes));
            }
            now += 0.001;
        }
        let ons: Vec<f64> = sent
            .iter()
            .filter(|(_, b)| b[0] == 0x99)
            .map(|(t, _)| *t)
            .collect();
        // Beats 3 and 4 of bar 1 unchanged, then eighths from bar 2 (2.05 s).
        assert!(
            (ons[0] - 1.05).abs() < 0.003 && (ons[1] - 1.55).abs() < 0.003,
            "{ons:?}"
        );
        assert!(
            (ons[2] - 2.05).abs() < 0.003 && (ons[3] - 2.30).abs() < 0.003,
            "{ons:?}"
        );
    }

    #[test]
    fn song_notes_cover_the_arrangement() {
        let mut e = Engine::new();
        e.set_graph(&arranged());
        let notes = e.song_notes(4, true);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].notes.len(), 4 + 8);
        assert_eq!(notes[0].notes[4].start, 3840);
        assert_eq!(notes[0].notes[5].start, 3840 + 480);
    }

    #[test]
    fn salts_are_fresh_per_play_unless_locked_or_unplayed() {
        let mut e = Engine::new();
        e.set_graph(&doc("drum", &[]));
        e.set_salt(5);
        // A salt that hasn't been played yet (what the previews show) is what Play uses.
        e.play(0.0, 100);
        assert_eq!(e.salt(), 5);
        e.stop();
        assert_eq!(e.salt(), 5, "stopping keeps what was heard");
        e.play(1.0, 200);
        assert_eq!(e.salt(), 200, "then each Play is fresh");
        e.stop();
        // Locking the project keeps the current salt for every playback.
        assert_eq!(e.set_random(true, None), 200);
        e.play(2.0, 300);
        assert_eq!(e.salt(), 200);
        e.stop();
        // Rerolling picks a new one, locked or not.
        assert_eq!(e.reroll(400), 400);
        e.play(3.0, 500);
        assert_eq!(e.salt(), 400);
        e.stop();
        e.set_random(false, None);
        e.play(4.0, 600);
        assert_eq!(e.salt(), 600);
    }

    fn walk_doc(walk: &[(&str, ParamValue)]) -> GraphDoc {
        let n = |id: &str, kind: &str, params: &[(&str, ParamValue)]| NodeDoc {
            id: id.into(),
            kind: kind.into(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        };
        GraphDoc {
            nodes: vec![n("s", "scale", &[]), n("w", "random_walk", walk)],
            wires: vec![WireDoc {
                from_node: "s".into(),
                from_port: "scale".into(),
                to_node: "w".into(),
                to_port: "scale".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn previews_follow_the_salt_and_locking_keeps_them() {
        let preview = |e: &Engine| {
            let r = e.report();
            r.previews.iter().find(|p| p.0 == "w").unwrap().1.clone()
        };
        let mut e = Engine::new();
        e.set_graph(&walk_doc(&[]));
        e.set_salt(9);
        let heard = preview(&e);
        e.set_salt(10);
        assert_ne!(preview(&e), heard, "a new salt, new previews");
        e.set_salt(9);
        assert_eq!(preview(&e), heard);
        // Lock it with the captured seed: the same melody whatever the salt.
        let seed = e.capture_seed("w").unwrap();
        e.set_graph(&walk_doc(&[
            ("lock", ParamValue::Bool(true)),
            ("seed", ParamValue::Number(seed as f64)),
        ]));
        assert_eq!(preview(&e), heard);
        e.set_salt(11);
        assert_eq!(preview(&e), heard);
    }

    #[test]
    fn previews_follow_the_playhead() {
        let mut e = Engine::new();
        e.set_bpm(0.0, 120.0);
        e.set_graph(&walk_doc(&[]));
        let first = e.preview_key();
        e.play(0.0, 1);
        run(&mut e, 0.5);
        let playing = e.preview_key();
        assert_eq!(playing, first, "the previews already showed what Play plays");
        // 4 bars at 120 bpm is 8 s: the previews move on to the next block.
        run(&mut e, 8.2);
        assert_ne!(e.preview_key(), playing);
        let text = e.report().previews.into_iter().find(|p| p.0 == "w").unwrap().1;
        assert!(text.contains("bars 5–8"), "{text}");
    }
}
