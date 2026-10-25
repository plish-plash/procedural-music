//! Renders a patch file through the built-in instruments to a WAV file, or exports it as
//! a standard MIDI file when the output ends in `.mid`. With an arrangement, the whole
//! song is rendered and `bars` is ignored.
//!
//! `cargo run --release -p pm-engine --example render -- patch.pmpatch out.wav [bars]`

use pm_engine::api::parse_graph;
use pm_engine::json::Json;
use pm_engine::{offline, smf, Engine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [patch, out, rest @ ..] = args.as_slice() else {
        eprintln!("usage: render <patch> <out.wav|out.mid> [bars]");
        std::process::exit(2);
    };
    let bars: u32 = rest.first().and_then(|b| b.parse().ok()).unwrap_or(8);
    let text = std::fs::read_to_string(patch).expect("read patch");
    let json = Json::parse(&text).expect("parse patch");
    let doc = parse_graph(&json).expect("read graph");
    let bpm = json.get("bpm").and_then(Json::as_f64).unwrap_or(110.0);

    let mut engine = Engine::new();
    engine.synchronous = true;
    let report = engine.set_graph(&doc);
    for issue in &report.issues {
        eprintln!("issue: {}: {}", issue.node, issue.message);
    }
    engine.set_bpm(0.0, bpm);

    let t = std::time::Instant::now();
    if out.ends_with(".mid") {
        let bytes = smf::export(&engine, bars, 7);
        std::fs::write(out, &bytes).expect("write midi");
        println!(
            "{} bytes of MIDI in {:.2} s",
            bytes.len(),
            t.elapsed().as_secs_f64()
        );
        return;
    }
    let audio = offline::render(&doc, bpm, bars, 2.0, 48_000, 7);
    std::fs::write(out, pm_synth::wav::encode_stereo(&audio, 48_000)).expect("write wav");
    println!(
        "{} s of audio in {:.2} s",
        audio.len() / 2 / 48_000,
        t.elapsed().as_secs_f64()
    );
}
