# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Procedural Music is a node-graph music app: typed theory values (scales, chords, rhythms) flow between nodes and become notes for built-in synths or MIDI. Rust engine, Tauri 2 desktop app, plain-JS editor UI. `README.md` is the user guide; `DESIGN.md` is the design doc and should stay accurate to how the app actually works (update it when behaviour it describes changes).

## Commands

```
cargo test --workspace                          # all crate tests (theory, synth, counterpoint, engine, cli)
cargo test -p pm-engine <name filter>           # one test, or a module (e.g. `nodes::melody`)
cargo build -p pm-theory --no-default-features  # the theory core must stay no_std
cargo check --manifest-path app/Cargo.toml      # the desktop app is a separate Cargo workspace
cargo run --manifest-path app/Cargo.toml        # run the desktop app
cargo run -p pm-engine --example dev_server     # editor in a browser at http://127.0.0.1:8765 (real engine, no MIDI/audio)
cargo run --release -p pm-engine --example render -- patch.pmpatch out.wav   # or out.mid
```

Stop a running `dev_server` before `cargo test`/`build`, or linking fails on Windows because the exe is locked.

The UI has no build step and no test runner (Node isn't installed). To check UI changes, run the dev server and drive `app/ui` in headless Edge (`msedge --headless=new --dump-dom` or `--screenshot`, with a fresh `--user-data-dir`) using a temporary page in `app/ui` that loads `index.html` in an iframe; delete the page afterwards. `requestAnimationFrame` doesn't fire headless.

Shell is Windows PowerShell 5.1: edit files with the editor tools, not `Get-Content`/`Set-Content` (encoding problems).

## Architecture

Crates (dependency order): `pm-theory` (pitches, scales, chords, rhythms, voicing, harmony; no deps, `no_std`, fixed-size `Copy` values) → `pm-synth` (instruments, effects, the audio rack) and `pm-counterpoint` (beam-search solver with rule costs and explanations) → `pm-engine` (nodes, graph, scheduler, JSON API) → `app` (Tauri host: scheduler thread, `cpal` audio thread, `midir` MIDI).

**The graph is a pure function of time.** `Graph::compile` (crates/engine/src/graph.rs) flattens groups into path ids (`drums/k`), orders nodes and resolves wires into slots. Each window is evaluated with an `EvalContext` (start/end ticks, meter, salt, section, repeat). Theory values are whole looping `Timeline`s; note generators return only notes starting in the window, and sinks keep only those. Nodes needing a wider span of input (counterpoint phrases, note delays, Markov training) override `NodeImpl::input_window` and trim their own output.

**Engine and transport** (engine.rs): the scheduler evaluates one sixteenth slice at a time (~120 ms ahead), queues timed MIDI and instrument events, and sends rack/tempo/automation messages to the audio thread. Each distinct set of section overrides compiles to its own graph (`Song`), evaluated in section-local time. Previews (`report_for`/`preview_target`) evaluate the 4-bar block under the playhead with the same salt and automation as playback; `previewKey` in status tells the UI when to refetch.

**Implicit harmonic context:** at compile time, unwired Scale/Chord inputs of nodes that don't feed the `context` node are wired to it (`select_*` nodes excepted). Recorded in `report.context`; the UI marks those ports "(context)".

**Randomness:** random nodes use `rng::stream(lock, seed, ctx.salt)` hashed with the phrase or tick. Unlocked streams change with the per-play, per-section salt; locked ones use the seed. Any node with `lock` + `seed` params automatically gets the Randomness UI (inspector row, 🔒/🎲 on the node).

**Automation:** pinning a number param creates an input port `param.<name>` of type Control; `ParamSpec::automatable` decides which can (not seeds). Automated nodes are rebuilt per value (cached variants).

**UI** (`app/ui`, ES modules): `api.js` calls the engine through Tauri `invoke`, or HTTP `POST /api/<command>` on the dev server; commands are handled in `crates/engine/src/api.rs`. `editor.js` is the canvas and document model, `inspector.js` the side panel, `views.js` the preview renderers, `timeline.js` the arrangement, `main.js` the wiring and toolbar, `templates.js`/`examples.js` the built-in patches. The UI is spec-driven: `specs_json()` sends every node's ports and params (with kinds such as `pitchset`, and `when` show-if conditions from the `CONDITIONS` table in spec.rs), so most node work needs no UI changes.

## Adding or changing a node

1. Define a `NodeSpec` and a `build_*` fn in the matching `crates/engine/src/nodes/<category>.rs` (closure nodes via `theory::node`, or a struct implementing `NodeImpl` for sinks, views or wider input windows). Put pure music logic in `pm-theory` with unit tests.
2. Register it in `SPECS` (menu order) and the `build` match in `crates/engine/src/nodes/mod.rs`. `every_spec_builds_with_defaults` checks defaults.
3. Keep `help` concise and about that node only: don't explain pinning/automation, Lock/Seed, the Harmonic context, previews, or sections restarting time (documented once, globally).
4. If ports or params change, update the bundled examples (`app/ui/examples.js`) and template (`app/ui/templates.js`) so they still load without issues.

Saved projects don't need backward compatibility yet: rename or remove params and kinds freely, without migration code. Keep the version-keyed framework in `app/ui/editor.js` (`DOC_VERSION`, `MIGRATIONS`, `migrateDoc`) for later.

Example layout convention (comment in examples.js): left-to-right flow, 50px between nodes. Most nodes' first port sits 46px below their top and the second 68px, so nodes joined by first ports share a top edge, and a rhythm feeding a second input sits 22px lower.
