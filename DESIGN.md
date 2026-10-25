# Procedural Music Node Graph — Design Document

Oct 24, 2026 · @Elliot

## Overview

The app is a node graph where music-theory objects (scales, chords, rhythms, progressions) are typed values that flow between nodes and end as notes played by a built-in synth or sent out as MIDI. Its purpose is to make theory visible and playable, especially how several voices fit together over a shared harmony.

**Goals**

- Explore theory interactively: every scale, chord and progression can be seen, heard and transformed.
- Write multi-voice harmony and counterpoint, with voice-leading rules the user can switch on and inspect.
- Build a full piece: lead, bass, pads and drums, arranged into sections.
- Run standalone with its own audio engine, or drive external synths over MIDI (per track).
- Stay live: edit the graph while it plays, with no stop/recompile cycle.
- Stay fresh by default: each pass is newly random, and locking a node's seed (or the project's) makes it repeat exactly.

**Non-goals (v1)**

- Audio recording, sample editing or a full mixing console.
- Engraving-quality notation (a simple staff view is in scope; publishing scores is not).
- Hosting third-party VST/AU plugins; third-party sounds come through MIDI out.

## Core concepts

The graph is a pure function of musical time: given a time window, a seed and the current parameters, it returns the note and control events in that window. Nothing in the graph holds hidden playback state, which is what makes seeking, looping, live editing and reproducible output possible.

**Musical time.** Time is counted in ticks at 960 per quarter note, shown to the user as bars:beats:ticks. Tempo and meter live on a global transport and can change by section. Seconds only appear at the very end, in the scheduler.

**Static vs. timed values.** Every data type exists in two forms: a single value ("C Dorian") and a timeline of values ("Dm7 for 2 bars, G7 for 2 bars"). A node that accepts a static value also accepts a timeline and is evaluated per segment, so a Transpose node works on one chord or on a whole progression without a second version.

**Evaluation.** The scheduler asks the graph for a window just ahead of the playhead (default one bar, at least 100 ms), pulls from the output nodes backward, and queues the resulting events for the audio engine and MIDI ports. Results are cached per node and window; editing a node invalidates only what lies downstream of it.

**Randomness.** Generative nodes are freshly random on every pass by default. Each node has a lock: when locked, it draws from a stream derived from its seed, the node ID and the bar number, so bar 17 repeats exactly. A project-wide lock makes the whole piece reproducible; "reroll" picks a new seed for a locked node.

**Harmonic context.** A global context carries the key, scale timeline and chord progression. Any node can read it implicitly, so a bassline, a pad and a lead all follow the same progression without wires from one shared source to every part. An explicit input always overrides the context.

## Data types

Port types split into three layers: abstract theory (what notes are allowed), material (which notes, when) and signal (what you hear). Wires connect only compatible types; the few safe conversions happen automatically and show as a small badge on the wire.

| Type | Holds | Example | Layer |
| --- | --- | --- | --- |
| Pitch | One absolute note: a step number in the current tuning (12-EDO by default) plus optional spelling | F#4, 66 | Theory |
| Interval | Distance in semitones, plus a diatonic size when spelled | minor 3rd, +3 | Theory |
| Scale | Root plus a set of pitch classes that repeats every period (octave by default); supports degree arithmetic | D Dorian, C major pentatonic | Theory |
| Chord | A finite set of absolute pitches (a voicing), with optional root and symbol | Cmaj7 as C3 E3 G3 B3 | Theory |
| Progression | Timeline of chords (and optionally scales), with durations | ii–V–I, 2 bars each | Theory |
| Rhythm | Onsets over a length, each with duration, accent and probability | Euclidean 5 of 8 over 1 bar | Material |
| Degree sequence | Melodic material as scale degrees or relative steps, not yet pitched | 1 3 2 5, or +2 −1 +3 | Material |
| Notes | Concrete events: time, duration, pitch, velocity, voice ID, articulation | A pad part for 8 bars | Material |
| Control | A number over time: constant, curve, LFO or automation | Filter cutoff ramp | Signal |
| Trigger | Time points with no pitch | Section start, every bar | Signal |
| Audio | A sample stream inside the audio engine | Synth output | Signal |

Type flow: theory → material → events → output

Every part follows this path: theory values constrain which pitches are allowed, material decides which and when, and Notes is the single type every output accepts.

**Scale and chord are two views of one idea.** A chord's notes, repeated across octaves, form a scale (a "chord scale" or arpeggio field); choosing voices from a scale within a range gives a chord. The *Repeat* and *Voice* nodes convert between them, which lets a melody be constrained to chord tones as easily as to a mode.

**Spelling.** Pitches carry an optional spelling (C# vs. Db) supplied by the scale they came from. Arithmetic ignores it; the staff view and theory displays use it.

**Tuning.** Pitches are integer steps in an equal division of the octave (EDO). 12-EDO is the default and the main use case; other EDOs (19, 22, 24, 31…) are a project or section setting. Intervals, scales and chords are stored in steps of the active tuning, and conversion to Hz, MIDI note plus pitch bend, or MPE happens at the output.

**Implicit conversions** (shown with a badge): Chord → Scale (repeat across octaves), Pitch → Chord (one-note chord), static value → timeline (one segment for the whole piece), Rhythm + Pitch → Notes (every onset plays that pitch). All other conversions are explicit nodes.

## Node catalog

About 60 nodes in ten categories cover v1; each node shows a live preview of its output on its face (a mini keyboard, chord diagram or rhythm grid).

| Category | Nodes | Main types in → out |
| --- | --- | --- |
| Theory sources | Key, Scale (presets + custom pitch-class picker), Chord (symbol or played), Progression (Roman numerals or chord symbols), Interval | → Scale, Chord, Progression |
| Theory transforms | Transpose (chromatic / diatonic), Mode rotate, Invert, Chord from degree, Extend (7ths, 9ths), Substitute (tritone, relative), Borrow (modal mixture), Secondary dominant, Scale ∩ / ∪ / − | Scale, Chord → Scale, Chord |
| Voicing | Voice (pick N notes in a range), Drop 2 / Drop 3, Spread, Close / Open, Voice lead (minimal motion between chords) | Chord, Progression → Chord, Progression |
| Rhythm | Pattern grid, Euclidean, Polyrhythm, Subdivide, Swing, Probability, Rotate, Combine (and / or / xor), Accent | → Rhythm |
| Melody | Sequencer (degrees), Random walk, Markov (trainable on a melody), Contour, Arpeggiator, Motif transform (retrograde, inversion, augmentation) | Scale, Rhythm, Degree seq → Notes |
| Counterpoint | Counterpoint (voices, species, rule groups and solver in one node), Split voices, Harmonizer (add parallel thirds/sixths in key) | Notes, Progression → Notes |
| Note operations | Merge, Split by voice / range, Filter, Quantize to scale, Humanize, Velocity curve, Delay, Legato, Octave | Notes → Notes |
| Structure | Section switch, Repeat N bars, Song form, Subgraph (macro), Context override | any → any |
| Instruments and output | Poly synth, Drum synth, Sampler, Mixer, Effects, Audio out, MIDI out, CV/Gate out | Notes, Control → Audio, MIDI |
| Analysis and display | Piano roll, Staff view, Circle of fifths, Tonnetz, Interval matrix, Rule-violation overlay, Probe | any → (view only) |

The analysis nodes are central to the theory goal: dropping a Tonnetz or circle-of-fifths view on any chord or progression wire shows its movement while it plays.

## Voice leading and counterpoint

Counterpoint is handled by one Counterpoint node: up to four named voices (soprano, alto, tenor, bass) sharing one harmonic context, with rules applied as weighted costs and a solver that picks the lowest-cost notes. Rules come in groups (parallels, dissonance, melody, spacing, tendency tones, harmony, cadence) that the user can switch off on the node to hear what changes. (An earlier plan split this into Voice group, Species solver and Rule set nodes; in practice the three were always used together, so they were merged.)

**Voices.** Each voice is Off, Free or Fixed, with a range (e.g. soprano C4–G5); one voice can be fixed to the line wired into the node (a cantus firmus or a user-written melody). With no fixed voice the node writes over a chord progression in chorale style. It outputs one Notes stream tagged by voice, which Split voices routes to separate instruments or MIDI channels. The node plans a whole phrase at a time, caches solutions by problem, and during live playback uses a quick answer at once while a thorough search finishes in the background.

**Rules as costs.** A rule returns a cost for a candidate move, and hard rules return infinity. Starting rule set:

- Parallel and hidden 5ths and octaves
- Voice crossing and overlap
- Spacing (no more than an octave between upper adjacent voices)
- Range limits and leap size (leaps over a 6th are costly; leaps should recover by step)
- Motion balance (prefer contrary and oblique motion)
- Dissonance treatment: passing tones, neighbor tones, suspensions with preparation and resolution
- Tendency tones (leading tone resolves up, chordal 7th resolves down)

**Solver.** A beam search across the harmonic slots of each section, keeping the best few partial solutions. It is deterministic for a given seed; a "variety" control adds seeded noise to costs so equally good answers can be explored. Species presets (first to fifth species) set the rhythm ratio and the dissonance rules.

**Explaining the result.** The rule-violation overlay marks every broken or costly rule on the piano roll and staff view with the rule's name, so the tool teaches as well as generates. Hovering a note shows why the solver chose it over the next-best option.

**Free voices.** Not every part must be solved. A lead from a random walk can join the group as a fixed line, and the other voices are then solved around it.

## Composition structure

A project is one graph organized into a global context, a set of parts, and an arrangement of sections along the timeline.

**Parts.** A part is a subgraph that ends in an instrument or MIDI output. New projects start from a template with four parts, each collapsible to a single node on the main canvas:

| Part | Typical inputs | Typical chain | Output |
| --- | --- | --- | --- |
| Drums | Global clock | Euclidean / pattern grid per drum → Probability → Humanize | Drum synth or MIDI ch. 10 |
| Bass | Progression | Chord root → Rhythm → Octave → Legato | Mono synth or MIDI |
| Pads | Progression | Voice (4 notes, C3–C5) → Voice lead → long gates | Poly synth or MIDI |
| Lead | Progression, Scale | Chord scale → Random walk / Markov → Rhythm → Quantize | Mono synth or MIDI |

**Arrangement.** The timeline is a list of named sections (Intro 8 bars, A 16, B 16, A 16, Outro 8). Each section can set its own tempo and override any node parameter, which covers key and progression through the Progression node; the meter is one for the whole song in v1. An "Edit this section only" toggle sends parameter edits to the selected section, and overridden parameters are marked on the nodes. Section switch nodes (notes, rhythm, chords, scale) pick different inputs per section, for example a busier drum pattern in B. Each distinct set of overrides compiles to its own graph, and every section is evaluated in its own time from bar 1.

**Variation over repeats.** Section switch lists can name a particular repeat (A#2 is the second A), unlocked random nodes play new material in every section, and a section can override a seed, so a second A section can differ (a new seed, an added voice) without copying the graph.

**Subgraphs.** Any selection can be collapsed into a group (Ctrl+G); wires crossing its edge become its ports, and parameters inside can be pinned to show on the group node. Groups nest, can be saved to a library shared between projects, and are flattened when the engine reads the project, so inner nodes are addressed by path (drums/kick).

## Output: audio and MIDI

Every part ends in an output node, and each part chooses independently between the internal engine and an external destination, so a project can mix built-in drums with an external bass synth.

**Internal audio engine**

- Poly synth: 2 oscillators (saw, square, sine, wavetable), filter with envelope, amp envelope, up to 16 voices.
- Mono synth with glide, for bass and lead.
- Drum synth: synthesized kick, snare, hats, clap and toms; no samples required.
- Sampler: loads WAV files and SoundFont (SF2) instruments for realistic timbres.
- Effects and mixer: filter, delay, reverb, chorus, per-part volume, pan and sends.

**MIDI out**

- Per-part port and channel; one Notes stream can split by voice to several channels (one per counterpoint voice).
- Control streams map to CC, pitch bend or aftertouch; optional MPE for per-note expression.
- MIDI clock out (24 PPQN) with start/stop/continue, or sync to incoming clock.
- Per-port latency offset in milliseconds, so external synths line up with the internal engine.

**MIDI in**

- Play a chord to set a Chord node or the current harmony.
- Record a melody into a fixed line, Markov training or a cantus firmus.
- Map hardware knobs to node parameters (MIDI learn).

**CV/Gate out (after v1).** For modular rigs, pitch, gate and control streams can be sent as DC voltages through a DC-coupled audio interface, with per-output 1V/oct calibration.

**Export.** Standard MIDI file (one track per part), and WAV render of the internal engine (offline, faster than real time). Notation export (MusicXML) is out of scope; MIDI files cover handoff to notation software.

## UI and interaction

The window has three areas: the node canvas in the center, an inspector on the right, and the arrangement timeline with a piano roll / staff view along the bottom.

**Compose and Perform modes.** Composing a fixed piece and live performance are both core uses, so a mode switch changes what the UI emphasizes. Compose puts the arrangement timeline, piano roll and staff view forward. Perform hides the timeline, enlarges exposed parameters and seed locks as performance controls, and quantizes edits to the bar by default. Both modes edit the same graph.

- **Typed, colored ports.** Each type has a color and shape (theory types round, material square, signal triangular). Dragging a wire highlights only compatible ports.
- **Live previews on nodes.** A Scale node shows a one-octave keyboard, a Chord node its notes, a Rhythm node a step grid with a moving playhead. Previews light up in time with playback.
- **Audition anything.** Clicking a theory output plays it once (a scale as a run, a chord as a block) through a preview instrument, without wiring it to an output.
- **Live editing.** Changes take effect at the next evaluation window, with no stop or recompile; an optional "quantize edits to bar" setting avoids mid-bar jumps.
- **Probe.** Hovering any wire shows its current value.
- **Theory views.** Circle of fifths, Tonnetz and interval matrix can be docked as panels and follow the current harmony.
- **Search-to-add.** Typing on the canvas opens a node search that filters by the type of the wire being dragged.
- **Undo history** across graph and parameter edits; presets per node; per-node seed with lock and reroll.

## Architecture and tech stack

Decision: a Tauri desktop app, with a Rust engine and a web-technology UI, because it gives low-latency audio, reliable MIDI on every OS and a fast way to build the node editor. A Rust engine also keeps a path open to microcontroller targets (see Future: hardware sequencer).

**Threads.** The UI thread edits the graph model and sends changes to the engine. An evaluation thread compiles the graph and fills the lookahead window. The audio thread renders the internal instruments from a lock-free event queue and never allocates. A MIDI thread sends timestamped messages.

**Theory core as a library.** Pitch, Scale, Chord, Rhythm and the voice-leading solver live in a pure library with no audio or UI dependencies, so they can be unit-tested against known examples (e.g. textbook species exercises).

**Project files.** JSON with a schema version: nodes (ID, type, parameters, position), wires, arrangement, seed and output routing. Saving a file stores the graph, not the rendered notes, so a project stays small and editable.

## Example patches

These five patches double as acceptance tests: v1 is done when each can be built in under five minutes.

1. **Modal explorer.** Scale (D Dorian) → Mode rotate (knob 1–7) → Arpeggiator → Poly synth, with a circle-of-fifths panel. Turning the knob walks through all seven modes of the same pitch set.
2. **Progression to pads.** Progression (I–vi–IV–V in C) → Voice (4 notes, C3–C5) → Voice lead → Poly synth. Toggling Voice lead off shows the jumpier, unled voicings.
3. **Bass and lead from one harmony.** The same progression feeds Chord root → Euclidean 5 of 8 → Octave −2 → Mono synth for bass, and Chord → Repeat (chord scale) → Random walk → Quantize → lead synth over MIDI ch. 2.
4. **First-species counterpoint.** Fixed line (a cantus firmus played in over MIDI) + Counterpoint node (bass fixed, soprano free, 1st species) → Split by voice → two synths, with the violation overlay on.
5. **Full song.** Template parts (drums, bass, pads, lead) with an arrangement of Intro 8 / A 16 / B 16 / A 16 / Outro 8; B transposes up a 4th and switches to a denser drum pattern. Export to MIDI file and WAV. (Built in as the Full song example.)

## Future: hardware sequencer

After v1, a patch should be able to "compile" to a microcontroller, turning the app into a programming tool for a hardware MIDI sequencer. Nothing for this ships in v1, but four v1 engine choices keep the path open at little cost.

- **Portable core.** The theory core and event evaluation are written as no\_std-compatible Rust: no threads, files or OS calls inside them. Audio, MIDI drivers and UI stay outside.
- **Graph IR.** The editor's graph is lowered to a flat, typed intermediate representation (nodes in evaluation order, wires as fixed slots). The desktop engine runs this IR, so the hardware path reuses it instead of re-implementing the graph.
- **Bounded memory.** Each evaluation node declares its maximum sizes (voices, notes per window, pattern steps), so it can run without heap allocation.
- **Integer time and pitch.** Ticks and tuning steps are integers; floating point appears only in audio rendering and display.

**What compiles.** Theory, rhythm, melody, note-operation and structure nodes, MIDI out and (later) CV/Gate out. Internal instruments, effects and analysis views do not. The counterpoint solver is likely too heavy for small chips; its output can be frozen into a fixed sequence first.

| Approach | How it works | Trade-off |
| --- | --- | --- |
| Interpreter firmware | One generic firmware runs the IR; patches upload as data over USB or MIDI SysEx | No toolchain for the user and no reflashing per patch; slower, and limited to nodes the firmware includes |
| Code generation | The IR becomes Rust source, compiled into a dedicated binary | Fastest and smallest; needs a compiler toolchain and a flash per patch |

The interpreter is the likely first step. Candidate chips include the RP2040/RP2350 and STM32 families, with USB MIDI and DIN MIDI out.

- [ ] Should the app also lay out hardware controls, mapping exposed patch parameters to the device's knobs, buttons and display?

## Roadmap

Build in five phases, each ending with something playable.

1. **Theory core.** Pitch, Interval, Scale, Chord, Rhythm and Progression types with unit tests; a command-line demo that prints and plays a progression over MIDI.
2. **Graph and playback.** Node editor, typed wires, scheduler with lookahead, MIDI out and clock; patches 1–2 work against an external synth.
3. **Internal sound.** Poly, mono and drum synths, mixer and effects; patch 3 works standalone.
4. **Counterpoint.** Counterpoint node (all five species, rule groups, beam-search solver with local improvement), Split voices, Piano roll and the violation overlay; patch 4. Done.
5. **Composition.** Parts template, arrangement timeline, section overrides and tempos, Section switch nodes, groups with a library, Compose/Perform modes with section cueing, undo, staff view, MIDI/WAV export; patch 5. Done.

**After v1:** CV/Gate output, then the hardware sequencer target (interpreter firmware first).
