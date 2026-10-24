# Procedural Music (working title)

A node-graph app for procedural music, built around music theory and multi-voice
harmony. Roadmap phases 1 (theory core), 2 (graph, scheduler, MIDI out, node editor)
and 3 (built-in instruments, effects and mixer) are in place; counterpoint (phase 4)
comes next.

## Layout

| Path | What it is |
| --- | --- |
| `crates/theory` (`pm-theory`) | Theory core: tunings, pitches, scales, chords, Roman numerals, rhythms, progressions, voicing and voice leading. No dependencies; builds without `std`. |
| `crates/synth` (`pm-synth`) | Built-in sound: poly and mono subtractive synths, a synthesized GM drum kit, filter, drive, chorus, delay, reverb, mixer, and the rack that wires them together. No dependencies. |
| `crates/engine` (`pm-engine`) | Node catalog, graph evaluation, lookahead scheduler, MIDI dispatch and offline rendering, plus the JSON command API the UI talks to. Depends only on `pm-theory` and `pm-synth`. |
| `crates/cli` (`pm`) | Command-line theory demo that writes MIDI files. |
| `app` (`pm-app`) | Tauri 2 desktop app: hosts the editor UI (`app/ui`, plain HTML/JS, no build step), runs the engine on a scheduler thread, plays audio with `cpal` and sends MIDI with `midir`. Its own Cargo workspace. |
| `scripts` | `build.bat` (tests + app build, output to `build-log.txt`) and `run.bat` (start the app). |

## Running the app (Windows)

Double-click `scripts\run.bat`, or from this folder:

```
cargo run --manifest-path app\Cargo.toml
```

The first build downloads and compiles Tauri, which takes a few minutes.

The examples play through the built-in instruments on your default audio output. To
drive external synths instead, end a chain in **MIDI out** and pick the port (with no
hardware, the default port is Windows' **Microsoft GS Wavetable Synth**); set **Clock**
in the toolbar to send MIDI clock. **Export WAV** renders the built-in instruments to a
file faster than real time.

### Using the editor

- **Examples ▾** loads the demo patches: *Modal explorer*, *Progression to pads*, *Full band*
  (all on the built-in synths) and *Full band (MIDI)*.
- Double-click the canvas (or press Tab) to add a node; type to search.
- Drag from an output dot to an input dot to connect. Drop a wire on empty canvas to
  get a menu of nodes that accept it. Drag a connected input's dot to move or remove its wire.
- Port colors: scale (teal), chord (amber), rhythm (pink), notes (blue), audio (lime
  triangles). A chord wired into a scale input is converted automatically (shown with a
  badge on the wire). Sound chains run instrument → effects → Mixer → Audio out.
- Every node shows a live preview of its output, and problems in red.
- Space plays and stops; edits apply while playing. Del removes, Ctrl+D duplicates,
  F frames everything, the mouse wheel zooms and dragging the background pans.
- Ctrl+S saves a `.pmpatch` file; the current patch is also autosaved between sessions.

## Developing

```
cargo test --workspace                           # theory, synth and engine tests
cargo run -p pm-engine --example dev_server      # the editor in a browser at http://127.0.0.1:8765
cargo run --release -p pm-engine --example render -- patch.pmpatch out.wav 8
cargo run -p pm-cli -- prog --key "C major" --out demo.mid "ii7 V7 Imaj7 -"
```

The dev server runs the real engine behind the UI but sends no MIDI, which makes it
handy for working on the editor.

## Design decisions in the code

- **Pitches are integer steps of an EDO.** 12-EDO is the default, and its step numbers
  equal MIDI note numbers (C4 = 60). Other EDOs up to 64 work in the theory core;
  MIDI output currently sends the nearest 12-EDO note (pitch bend comes later).
- **Spelling is display data.** C#4 equals Db4; scales supply spellings, so F major
  spells Bb.
- **The graph is a pure function of time.** Theory values travel as whole looping
  timelines; note generators return only the notes that start in the requested window.
  The scheduler evaluates a sixteenth at a time about 120 ms ahead of the playhead, so
  edits are heard within a beat.
- **Audio is sample-accurate.** Instrument notes are sent to the audio thread ahead of
  time with timestamps, and the rack applies them within 64 samples (1.3 ms). Sound
  state (held notes, delay and reverb tails) survives edits to a node's settings.
- **Built-in synths play any tuning exactly**, because they receive frequencies rather
  than MIDI note numbers.
- **Randomness is fresh unless locked.** Random nodes mix in a value that changes every
  time Play is pressed; turning on Lock makes their phrases repeat exactly.
- **Fixed-size theory values and integer time** (960 ticks per quarter note) keep the
  door open to running patches on a microcontroller later.
