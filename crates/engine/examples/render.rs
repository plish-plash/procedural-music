//! Renders a patch file through the built-in instruments to a WAV file.
//!
//! `cargo run --release -p pm-engine --example render -- patch.pmpatch out.wav [bars]`

use pm_engine::api::parse_graph;
use pm_engine::json::Json;
use pm_engine::offline;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [patch, out, rest @ ..] = args.as_slice() else {
        eprintln!("usage: render <patch> <out.wav> [bars]");
        std::process::exit(2);
    };
    let bars: u32 = rest.first().and_then(|b| b.parse().ok()).unwrap_or(8);
    let text = std::fs::read_to_string(patch).expect("read patch");
    let json = Json::parse(&text).expect("parse patch");
    let doc = parse_graph(&json).expect("read graph");
    let bpm = json.get("bpm").and_then(Json::as_f64).unwrap_or(110.0);
    let t = std::time::Instant::now();
    let audio = offline::render(&doc, bpm, bars, 2.0, 48_000, 7);
    std::fs::write(out, pm_synth::wav::encode_stereo(&audio, 48_000)).expect("write wav");
    println!(
        "{} s of audio in {:.2} s",
        audio.len() / 2 / 48_000,
        t.elapsed().as_secs_f64()
    );
}
