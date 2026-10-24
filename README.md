# Procedural Music (working title)

A node-graph app for procedural music, built around music theory and multi-voice
harmony. This repository currently holds **roadmap phase 1: the theory core** and a
command-line demo. The Tauri app (node editor, scheduler, audio engine) comes next.

## Layout

| Path | What it is |
| --- | --- |
| `crates/theory` (`pm-theory`) | Theory core library: tunings, pitches, scales, chords, Roman numerals, rhythms, progressions, voicing and voice leading. No dependencies; builds without `std` for future microcontroller targets. |
| `crates/cli` (`pm`) | Command-line demo that prints theory objects and writes MIDI files. |

## Getting started (Windows)

1. Install Rust from <https://rustup.rs> (accept the defaults; it installs the Visual
   Studio C++ build tools if they are missing).
2. In this folder, run the tests:

   ```
   cargo test
   ```

3. Try the demo:

   ```
   cargo run -p pm-cli -- scale "Bb dorian"
   cargo run -p pm-cli -- chord F#m7b5
   cargo run -p pm-cli -- edo 19 3,3,2,3,3,3,2
   cargo run -p pm-cli -- euclid 5 8
   cargo run -p pm-cli -- prog --key "C major" --out demo.mid "ii7 V7 Imaj7 - vi7 ii7 V7/V V7"
   ```

   `demo.mid` has pads (channel 1, voice-led), a tresillo bass on the chord roots
   (channel 2) and a simple beat (channel 10, General MIDI drums). Open it in any DAW
   or MIDI player, or route it to your synths.

Run `cargo run -p pm-cli -- help` for every option.

## Design decisions in the code

- **Pitches are integer steps of an EDO.** 12-EDO is the default, and its step numbers
  equal MIDI note numbers (C4 = 60). Other EDOs up to 64 work everywhere except chord
  symbols and Roman numerals, which are 12-EDO by nature. `Tuning::to_midi` gives the
  nearest MIDI note plus a cents offset for pitch bend or MPE.
- **Spelling is display data.** C#4 equals Db4; scales supply spellings, so F major
  spells Bb and a scale built from an unspelled root picks the spelling with the fewest
  accidentals (Db major, C# minor).
- **Scales and chords convert both ways.** `Chord::to_scale` repeats a chord across
  octaves; `Scale::chord_on_degree` stacks scale thirds.
- **Integer time.** 960 ticks per quarter note (`PPQ`).
- **Fixed-size, `Copy` values.** Chords hold up to 16 notes, rhythms 64 onsets,
  progressions 64 chords, all without heap allocation, so the same code can later run
  on a hardware sequencer. `cargo build -p pm-theory --no-default-features` builds the
  `no_std` version (frequency conversion is the only `std`-only feature).

## Status

Phase 1 of the roadmap in the design document: done, apart from live MIDI output, which
arrives with the engine in phase 2 (the demo writes MIDI files instead).
