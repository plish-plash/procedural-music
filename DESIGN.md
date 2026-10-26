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

**Musical time.** Time is counted in ticks at 960 per quarter note, shown to the user as bars.beats. Tempo can change by section; the meter is one for the whole song. Seconds only appear at the very end, in the scheduler.

**Static vs. timed values.** Every value can be a single value ("C Dorian") or a timeline of values ("Dm7 for 2 bars, G7 for 2 bars"). A node that accepts a static value also accepts a timeline and is applied per segment, so a Transpose node works on one chord or on a whole progression without a second version. A progression is simply a timeline of chords, and a key change a timeline of scales.

**Evaluation.** Each distinct set of section overrides compiles once into a flat graph (groups are flattened, nodes put in evaluation order, wires turned into slots). The scheduler asks the graph for a short window just ahead of the playhead, one sixteenth note at a time, pulls from the output nodes backward, and queues the resulting events for the audio engine and MIDI ports. Evaluation is cheap enough to run in full for every window; the one cache is the counterpoint solver's, which is keyed by problem, answers quickly at once and finishes a thorough search in the background. An edit recompiles the graph, which takes over from the next window.

**Previews follow playback.** Node previews, analysis nodes, the wire probe and auditions all show what is playing: the 4-bar block around the playhead, in the section and repeat playing, with its random choices and automation, evaluated slice by slice like playback. Stopped, they show where Play will start, with what was last heard.

**Randomness.** Generative nodes (Random walk, the Arpeggiator's Random pattern, Counterpoint's Variety, Probability, Humanize, Markov) draw from a stream derived from their seed and the phrase or tick. Unlocked, the stream also mixes in a salt that changes on every Play and differs per section and per repeat, so each pass is new. Locked, a node draws from its seed alone and plays the same every time; locking keeps exactly what was just heard, and "reroll" picks a new seed. A project-wide lock fixes the salt, making the whole piece reproducible, and the project reroll picks a new salt (or a new project seed when locked). Exports use the same salt as playback, so they match what was heard.

**Harmonic context.** A *Harmonic context* node carries the song's key and chord progression, usually fed by a Scale node (the key) and a Progression that reads its numerals in that key. Every Scale or Chord input left unwired anywhere in the project, inside groups too, reads it implicitly (the port shows "(context)"), so a bassline, a pad and a lead all follow the same progression without a wire from one shared source to every part. An explicit wire always overrides the context. Section switches and the nodes that feed the context itself don't read it. Section overrides on the key's Scale node or on the Progression change the context per section, for example a key change in B.

## Data types

Port types split into three layers: abstract theory (what notes are allowed), material (which notes, when) and signal (what you hear). Wires connect only compatible types; the one safe conversion happens automatically and shows as a small badge on the wire.

| Type | Holds | Example | Layer | Port |
| --- | --- | --- | --- | --- |
| Scale | Root plus a set of pitch classes that repeats every octave, with spelling; supports degree arithmetic | D Dorian, C major pentatonic | Theory | teal circle |
| Chord | A finite set of absolute pitches (a voicing), with optional root and symbol | Cmaj7 as C3 E3 G3 B3 | Theory | amber circle |
| Rhythm | A looping pattern of onsets, each with offset, duration, velocity (accent) and probability | Euclidean 5 of 8 over 1 bar | Material | pink square |
| Notes | Concrete events: start, duration, pitch, velocity, voice number | A pad part for 8 bars | Material | blue square |
| Control | A number over time: constant, LFO, ramp or steps | Filter cutoff ramp | Signal | violet triangle |
| Audio | A stereo sample stream inside the built-in sound engine | Synth output | Signal | lime triangle |

Scale and Chord values can be static or timelines (a progression); Rhythm loops; Notes are absolute events; Control is a function of time.

Type flow: theory → material → events → output

Every part follows this path: theory values constrain which pitches are allowed, material decides which and when, and Notes is the type every instrument and MIDI output accepts. Control drives settings: wiring a Control into a node's pinned setting automates it.

**Scale and chord are two views of one idea.** A chord's notes, repeated across octaves, form a scale (a "chord scale" or arpeggio field); choosing voices from a scale within a range gives a chord. A chord wired into a scale input becomes its chord scale automatically, and the *Voice* node goes the other way, which lets a melody be constrained to chord tones as easily as to a mode.

**Pitches and intervals** are types in the theory library (a step number in the tuning plus optional spelling, and a distance in steps), not port types: a single note is a one-note Chord, and an interval is a node setting such as Transpose's amount.

**Spelling.** Pitches carry an optional spelling (C# vs. Db) supplied by the scale or chord symbol they came from. Arithmetic ignores it; the staff view and theory displays use it.

**Tuning.** The theory library stores pitches as integer steps in an equal division of the octave (EDO), and its scales, chords and intervals work in any EDO. The app runs in 12-EDO; a project tuning setting (19, 22, 24, 31…) is after v1, since counterpoint, the synths and MIDI out (pitch bend) would all need to follow it.

**Implicit conversion** (shown with a badge): Chord → Scale (repeat across octaves). All other conversions are explicit nodes (Voice, Chord from degree, Arpeggiator, Play chords, Drum hit…).

## Nodes

Nodes are small, single-purpose and grouped into categories that follow the type flow; the exact set grows over time, and each node documents itself in the inspector. Nodes are compact on the canvas (title, ports and a red ! when something is wrong); selecting one shows its help, problems, a live preview of its output and all its settings in the inspector. The preview and any setting can be pinned to show on the node; previews are pinned by default for sources, progressions, Counterpoint and analysis nodes.

| Category | What its nodes do | Main types in → out |
| --- | --- | --- |
| Theory | Make keys, chords and progressions, and transform them: modes, transposition (chromatic or in the key), extensions, substitutions, modal borrowing, secondary dominants, scale set operations | → Scale, Chord |
| Voicing | Place chords' notes in octaves: pick voices in a range, lead voices smoothly from chord to chord, drop, invert, spread, open and close | Chord → Chord |
| Rhythm | Make and bend patterns: step grids, Euclidean and polyrhythms, combining, rotating, ratchets, swing, accents, probability | → Rhythm |
| Melody | Turn harmony and rhythm into lines (arpeggios, degree sequences, random walks, Markov chains, contours), or reshape a melody (retrograde, inversion, augmentation) | Chord, Scale, Rhythm, Notes → Notes |
| Parts | The usual jobs of a band's parts: block chords, basslines, drum hits | Chord, Rhythm → Notes |
| Notes | Operate on note streams: octave, quantize, velocity, humanize, legato, delay, filter, split, repeat, merge | Notes → Notes |
| Counterpoint | Write voices that obey the rules (see below), or harmonize a line in the key | Notes, Chord → Notes |
| Control | Numbers over time for automation: LFOs, ramps, steps | → Control |
| Structure | The Harmonic context and section switches; any selection can also become a group | any → any |
| Sound and effects | The built-in instruments, mixer and effects | Notes, Audio → Audio |
| Output | Speakers, and MIDI notes or controls to external gear | Notes, Audio, Control → |
| Analysis | Views that follow playback: piano roll and staff (with the rule overlay), circle of fifths, Tonnetz, interval matrix | any → (view only) |

Some ideas live outside the node list: the probe is hovering a wire; song form is the arrangement timeline; subgraphs are groups (Ctrl+G); a key is a Scale node; an interval is a node setting.

The analysis nodes are central to the theory goal: dropping a Tonnetz or circle-of-fifths node into a project shows the harmony moving while it plays. Unwired, they show the Harmonic context; wired, whatever chord or key they are given.

**Automation.** Pinning a number setting gives the node a Control input for it (seeds excepted). Control nodes produce values in the setting's own units: an LFO from 1 to 7 on Mode rotate's Degree walks the modes, and a Steps node on Chord from degree's Degree writes a progression. Settings are read every sixteenth while playing; synth, effect and mixer settings are automated in the sound engine.

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

**Solver.** A beam search across the harmonic slots of each phrase, keeping the best few partial solutions. It is deterministic for a given seed; a "variety" control adds seeded noise to costs so equally good answers can be explored (locked by default; unlocked, each pass explores a new answer). Species presets (first to fifth species) set the rhythm ratio and the dissonance rules.

**Explaining the result.** The rule-violation overlay marks every broken or costly rule on the piano roll and staff view with the rule's name, so the tool teaches as well as generates. Hovering a note shows why the solver chose it over the next-best option.

**Free voices.** Not every part must be solved. A lead from a random walk can join the group as a fixed line, and the other voices are then solved around it.

## Composition structure

A project is one graph organized into a harmonic context, a set of parts, and an arrangement of sections along the timeline.

**Parts.** A part is a group that ends in an instrument or MIDI output. New songs start from a template with four parts, each a group on the main canvas, fed through the Harmonic context by a key (a Scale node) and a Progression in that key:

| Part | Inputs | Chain inside the group | Output |
| --- | --- | --- | --- |
| Drums | none | Pattern grid per drum (kick, snare, hats) → Drum hit → Merge | Drum synth |
| Bass | Harmonic context | Euclidean 5 of 8 → Chord root (octave 2) | Mono synth |
| Pads | Harmonic context | Voice (4 notes, G3–G5) → Voice lead → Play chords | Poly synth → Chorus |
| Lead | Harmonic context | Euclidean → Arpeggiator → Section switch (plays in A and B) | Poly synth → Delay |

The parts meet in a Mixer → Reverb → Audio out. Each group exposes its key settings (◆) on its face, such as the drum patterns and the bass's notes per bar.

**Arrangement.** The timeline is a list of named sections (Intro 8 bars, A 16, B 16, A 16, Outro 8). Each section can set its own tempo and override any node parameter, which covers key and progression through the key's Scale node and the Progression; the meter is one for the whole song in v1. An "Edit section only" toggle sends parameter edits to the selected section, and overridden parameters are marked on the nodes. Section switch nodes (notes, rhythm, chords, scale) pick different inputs per section, for example a busier drum pattern in B. Each distinct set of overrides compiles to its own graph, and every section is evaluated in its own time from bar 1. Under the sections, each part has a lane with an overview of its notes, which expands into a piano roll or staff; the timeline zooms and scrolls.

**Variation over repeats.** Section switch lists can name a particular repeat (A#2 is the second A), unlocked random nodes play new material in every section and repeat, and a section can override a seed or a lock, so a second A section can differ (a new seed, an added voice) without copying the graph.

**Groups.** Any selection can be collapsed into a group (Ctrl+G); wires crossing its edge become its ports, and settings inside can be exposed (◆) to show on the group node. Groups nest, can be saved to a library shared between projects, and are flattened when the engine reads the project, so inner nodes are addressed by path (drums/kick). Nodes can be copied and pasted (with the wires between them) between graphs, groups and windows.

## Output: audio and MIDI

Every part ends in an output node, and each part chooses independently between the internal engine and an external destination, so a project can mix built-in drums with an external bass synth.

**Internal audio engine**

- Poly synth: 16 voices, two detuned oscillators (saw, square, triangle, sine) into a resonant low-pass filter with its own envelope, and an amp envelope.
- Mono synth with glide and legato, for bass and lead.
- Drum synth: synthesized kick, snare, clap, hats, toms, cymbals, rim, cowbell and shaker on General MIDI keys; no samples required.
- Effects as nodes: filter (low, high or band-pass), drive, chorus, tempo-synced delay (ping-pong) and reverb.
- Mixer: four inputs, each with level and pan. Several Audio out nodes are added together.
- A preview instrument for auditioning, always present.

**MIDI out**

- Per-part port, channel and program change; one Notes stream can split by voice to several channels (one per counterpoint voice).
- MIDI CC out turns a Control stream into control changes, pitch bend or channel pressure on a port and channel, read every sixteenth and sent only when the value changes.
- MIDI clock out (24 PPQN) with start and stop, to one port.
- Clock in: follow the tempo, start, continue and stop of incoming MIDI clock from one input. The tempo map is re-anchored on every beat, so playback stays in phase, not just in tempo; section tempos are ignored while following.
- Per-port latency offset in milliseconds, so external synths line up with the internal engine. It belongs to the computer's setup rather than the song, so it is kept with the app, not in project files.

**Export.** Standard MIDI file (one track per part, named after its group, with the tempo changes), and WAV render of the internal engine (offline, faster than real time). Exports use the current random salt, so they match what was heard. MIDI CC out streams are not written to the file. Notation export (MusicXML) is out of scope; MIDI files cover handoff to notation software.

**After v1**

- Sampler: WAV files and SoundFont (SF2) instruments for realistic timbres.
- MPE for per-note expression.
- MIDI in: play a chord to set a Chord node or the current harmony; record a melody into a fixed line, Markov training or a cantus firmus; map hardware knobs to node settings (MIDI learn).
- CV/Gate out: for modular rigs, pitch, gate and control streams sent as DC voltages through a DC-coupled audio interface, with per-output 1V/oct calibration.

## UI and interaction

The window has a top bar, the node canvas in the center, the inspector on the right, and the arrangement timeline along the bottom. The top bar holds the File menu (new, examples, open, save, export), undo and redo, the transport (Play, BPM, the project's randomness lock and reroll, the position and section), the Compose / Perform switch, and the MIDI menu (clock in and out, latency, refresh ports).

**Compose and Perform modes.** Composing a fixed piece and live performance are both core uses, so a mode switch changes what the UI emphasizes. Compose puts the arrangement timeline forward and plays the song. Perform hides the timeline for a row of section cues, loops the playing section, cues the next one at the bar line, enlarges exposed settings and seed locks as performance controls, and holds graph edits until the next bar line. Both modes edit the same graph.

- **Typed, colored ports.** Each type has a color and shape (theory types round, material square, signal triangular). Dragging a wire highlights only compatible ports; dropping it on empty canvas offers the nodes that accept it.
- **Inspector and previews.** A Scale node's preview is a keyboard (the tonic darker), a Progression's a keyboard with a strip of its chords, a rhythm's a step grid, notes a small piano roll or staff, a control a curve. Previews light up in time with playback and can be pinned to the node.
- **Audition anything.** Clicking a preview's ▶ or a theory output's name plays it once (a scale as a run, a chord as a block, a rhythm as clicks) through the preview instrument, without wiring it to an output.
- **Live editing.** Changes take effect at the next evaluation window, with no stop or recompile; in Perform mode they wait for the bar line.
- **Probe.** Hovering any wire shows its current value, drawn like a preview, with its text.
- **Theory views** are Analysis nodes (Circle of fifths, Tonnetz, Interval matrix) that follow the current harmony.
- **Add menu.** Double-click the canvas or press Tab to open a searchable list of nodes; it filters by type when opened from a dropped wire.
- **Undo history** across graph and parameter edits; copy, cut and paste; per-node seed with lock and reroll.

Not yet: per-node presets, opening the node search by typing on the canvas, and an option to quantize edits to the bar in Compose mode.

## Architecture and tech stack

Decision: a Tauri desktop app, with a Rust engine and a web-technology UI, because it gives low-latency audio, reliable MIDI on every OS and a fast way to build the node editor. A Rust engine also keeps a path open to microcontroller targets (see Future: hardware sequencer).

**Threads.** The UI (a web view) edits the graph model and sends changes to the engine as JSON commands. A scheduler thread polls the engine every millisecond: it compiles graphs when they change, fills the lookahead window, and sends timestamped MIDI. The audio thread renders the internal instruments from a message queue (rack changes, tempo, timed notes and setting changes) and never blocks on the engine. Long counterpoint searches run on background threads.

**Theory core as a library.** Pitch, Interval, Scale, Chord, Rhythm and the voice-leading solver live in pure libraries with no audio or UI dependencies (`pm-theory`, `pm-counterpoint`), so they can be unit-tested against known examples (e.g. textbook species exercises). The synth (`pm-synth`) and the graph engine (`pm-engine`) are separate crates.

**Project files.** JSON with a schema version: nodes (ID, type, parameters, position, groups with their inner graphs and exposed settings), wires, sections with their tempos and overrides, tempo, the project's randomness lock and seed, and the clock out and clock in ports. Saving a file stores the graph, not the rendered notes, so a project stays small and editable; the current project is also autosaved between sessions.

## Example patches

These patches double as acceptance tests: v1 is done when each can be built in under five minutes. All are built in under File › Examples.

1. **Modal explorer.** Scale (D Dorian) → Mode rotate (degree 1–7) → Degree sequencer (up and down two octaves) → Poly synth, with a Circle of fifths node. Turning the degree walks through all seven modes of the same pitch set.
2. **Progression to pads.** Scale (C major) → Progression (I–vi–IV–V) → Voice (4 notes, C3–C5) → Voice lead → Poly synth. Toggling Voice lead off shows the jumpier, unled voicings.
3. **Bass and lead from one harmony (Full band).** One progression feeds pads, a bassline from Chord root on a Euclidean rhythm, an arpeggiated lead and drums, all on the built-in synths. *Full band (MIDI)* sends the same parts to MIDI ports.
4. **First-species counterpoint.** A cantus firmus typed into a Degree sequencer + Counterpoint node (bass fixed, soprano free, 1st species) → Split voices → two synths, with the violation overlay in the Piano roll. *Chorale* writes four voices over a progression.
5. **Full song.** Template parts (drums, bass, pads, lead) with an arrangement of Intro 8 / A 16 / B 16 / A 16 / Outro 8; B transposes up a 4th and switches to a denser drum pattern. Export to MIDI file and WAV.

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

Build in six phases, each ending with something playable.

1. **Theory core.** Pitch, Interval, Scale, Chord, Rhythm and Progression types with unit tests; a command-line demo that prints and plays a progression over MIDI. Done.
2. **Graph and playback.** Node editor, typed wires, scheduler with lookahead, MIDI out and clock; patches 1–2 work against an external synth. Done.
3. **Internal sound.** Poly, mono and drum synths, mixer and effects; patch 3 works standalone. Done.
4. **Counterpoint.** Counterpoint node (all five species, rule groups, beam-search solver with local improvement), Split voices, Piano roll and the violation overlay; patch 4. Done.
5. **Composition.** Parts template, arrangement timeline, section overrides and tempos, Section switch nodes, groups with a library, Compose/Perform modes with section cueing, undo, staff view, MIDI/WAV export; patch 5. Done.
6. **Interaction and vocabulary.** Inspector with pinning, previews that follow playback, auditioning, the wire probe, Control automation, seed lock and reroll, the Harmonic context, analysis nodes, timeline lanes and zoom, copy and paste; the remaining theory, voicing, rhythm, note and melody nodes in the catalog; MIDI CC out, clock in and latency offsets. Done.

**After v1:** sampler, MIDI in, MPE, project tuning (EDO), per-node presets, CV/Gate output, then the hardware sequencer target (interpreter firmware first).
