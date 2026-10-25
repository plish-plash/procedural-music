//! Development server: serves the UI from `app/ui` and runs UI commands against the
//! real engine, so the editor can be tried in a browser without the desktop app.
//! MIDI is not sent anywhere; note-ons are counted instead.
//!
//! Run from the workspace root: `cargo run -p pm-engine --example dev_server -- 8765`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pm_engine::api;
use pm_engine::json::Json;
use pm_engine::Engine;

struct State {
    engine: Mutex<Engine>,
    start: Instant,
    notes_sent: AtomicU64,
    root: PathBuf,
}

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(8765);
    let root = PathBuf::from("app/ui");
    assert!(
        root.join("index.html").exists(),
        "run from the workspace root (app/ui/index.html not found)"
    );
    let state = Arc::new(State {
        engine: Mutex::new(Engine::new()),
        start: Instant::now(),
        notes_sent: AtomicU64::new(0),
        root,
    });

    let s = state.clone();
    std::thread::spawn(move || loop {
        let now = s.start.elapsed().as_secs_f64();
        let sent = {
            let mut engine = s.engine.lock().unwrap();
            let sent = engine.poll(now);
            // Instrument notes would go to the audio thread; count them as sent.
            let instrument_ons: usize = engine
                .take_audio()
                .iter()
                .map(|m| match m {
                    pm_synth::AudioMsg::Events(e) => e
                        .iter()
                        .filter(|e| matches!(e.event, pm_synth::NoteEvent::On { .. }))
                        .count(),
                    _ => 0,
                })
                .sum();
            s.notes_sent
                .fetch_add(instrument_ons as u64, Ordering::Relaxed);
            sent
        };
        let ons = sent.iter().filter(|m| m.bytes[0] & 0xf0 == 0x90).count() as u64;
        s.notes_sent.fetch_add(ons, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(2));
    });

    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("dev server on http://127.0.0.1:{port}");
    for stream in listener.incoming().flatten() {
        let s = state.clone();
        std::thread::spawn(move || {
            let _ = serve(stream, &s);
        });
    }
}

fn serve(mut stream: TcpStream, state: &State) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    if method == "POST" && path.starts_with("/api/") {
        let command = &path[5..];
        let args = Json::parse(std::str::from_utf8(&body).unwrap_or("null")).unwrap_or(Json::Null);
        let reply = match command {
            "midi_ports" => Ok(Json::Arr(vec![Json::str("Dev server (no MIDI)")])),
            "notes_sent" => Ok(Json::Num(state.notes_sent.load(Ordering::Relaxed) as f64)),
            "export_midi" => {
                let bars = args.get("bars").and_then(Json::as_f64).unwrap_or(8.0) as u32;
                let bytes = {
                    let engine = state.engine.lock().unwrap();
                    pm_engine::smf::export(&engine, bars, 1)
                };
                std::fs::write("export.mid", bytes)
                    .map(|_| Json::str("export.mid"))
                    .map_err(|e| e.to_string())
            }
            "export_wav" => {
                let bars = args.get("bars").and_then(Json::as_f64).unwrap_or(8.0) as u32;
                let (doc, bpm) = {
                    let engine = state.engine.lock().unwrap();
                    (engine.doc().clone(), engine.bpm())
                };
                let audio = pm_engine::offline::render(&doc, bpm, bars, 2.0, 48_000, 1);
                std::fs::write("export.wav", pm_synth::wav::encode_stereo(&audio, 48_000))
                    .map(|_| Json::str("export.wav"))
                    .map_err(|e| e.to_string())
            }
            _ => {
                let now = state.start.elapsed().as_secs_f64();
                let salt = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1);
                let mut engine = state.engine.lock().unwrap();
                api::handle(&mut engine, now, salt, command, &args).map(|r| r.json)
            }
        };
        let (status, text) = match reply {
            Ok(j) => ("200 OK", j.to_string()),
            Err(e) => ("400 Bad Request", Json::str(e).to_string()),
        };
        return respond(&mut stream, status, "application/json", text.as_bytes());
    }

    let rel = path
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.contains("..") {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"no");
    }
    let file = state.root.join(rel);
    match std::fs::read(&file) {
        Ok(bytes) => respond(&mut stream, "200 OK", mime(&file), &bytes),
        Err(_) => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, status: &str, ty: &str, body: &[u8]) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)
}
