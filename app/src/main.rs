//! Procedural Music desktop app: hosts the node editor UI, runs the engine on a
//! scheduler thread, plays the built-in instruments through the sound card and sends
//! MIDI out.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod midi;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pm_engine::api;
use pm_engine::json::Json;
use pm_engine::{Engine, MidiMessage};
use tauri::Manager;

struct Shared {
    engine: Mutex<Engine>,
    midi: Mutex<midi::Midi>,
    audio: audio::Audio,
    start: Instant,
}

impl Shared {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn send(&self, messages: &[MidiMessage]) {
        if messages.is_empty() {
            return;
        }
        let mut midi = self.midi.lock().unwrap();
        for m in messages {
            midi.send(&m.port, &m.bytes);
        }
    }

    /// Forwards rack changes, tempo, notes and stops to the audio thread.
    fn flush_audio(&self, engine: &mut Engine) {
        for msg in engine.take_audio() {
            let _ = self.audio.tx.send(msg);
        }
    }
}

fn salt() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

/// Runs a UI command (see `pm_engine::api`). Arguments and reply are JSON text.
#[tauri::command]
fn engine(state: tauri::State<'_, Arc<Shared>>, command: String, args: String) -> Result<String, String> {
    let args = if args.trim().is_empty() { Json::Null } else { Json::parse(&args)? };
    let now = state.now();
    let reply = {
        let mut engine = state.engine.lock().unwrap();
        let reply = api::handle(&mut engine, now, salt(), &command, &args)?;
        state.flush_audio(&mut engine);
        reply
    };
    state.send(&reply.midi);
    let mut json = reply.json;
    if command == "status" {
        if let Json::Obj(fields) = &mut json {
            if let Some(err) = state.midi.lock().unwrap().last_error.clone() {
                fields.push(("midiError".into(), Json::Str(err)));
            }
            let audio = state.audio.status.lock().unwrap().clone();
            if let Some(device) = audio.device {
                fields.push(("audio".into(), Json::Str(device)));
            }
            if let Some(err) = audio.error {
                fields.push(("audioError".into(), Json::Str(err)));
            }
        }
    }
    Ok(json.to_string())
}

/// Lists MIDI output ports and closes open connections so they are reopened fresh.
#[tauri::command]
fn midi_ports(state: tauri::State<'_, Arc<Shared>>) -> Vec<String> {
    state.midi.lock().unwrap().reset();
    midi::port_names()
}

/// Saves a project. Without a path, asks where. Returns the path written, or None if cancelled.
#[tauri::command]
async fn save_project(contents: String, path: Option<String>) -> Result<Option<String>, String> {
    let path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => match rfd::FileDialog::new()
            .set_title("Save patch")
            .add_filter("Procedural Music patch", &["pmpatch"])
            .set_file_name("untitled.pmpatch")
            .save_file()
        {
            Some(p) => p,
            None => return Ok(None),
        },
    };
    std::fs::write(&path, contents).map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// Asks for a project file and returns (path, contents), or None if cancelled.
#[tauri::command]
async fn open_project() -> Result<Option<(String, String)>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open patch")
        .add_filter("Procedural Music patch", &["pmpatch", "json"])
        .pick_file()
    else {
        return Ok(None);
    };
    let contents = std::fs::read_to_string(&path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    Ok(Some((path.display().to_string(), contents)))
}

/// Renders the arrangement (or `bars` bars without one) through the built-in instruments (faster
/// than real time) and saves a WAV file. Returns the path, or None if cancelled.
#[tauri::command]
async fn export_wav(state: tauri::State<'_, Arc<Shared>>, bars: u32) -> Result<Option<String>, String> {
    let (doc, bpm) = {
        let engine = state.engine.lock().unwrap();
        (engine.doc().clone(), engine.bpm())
    };
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export WAV")
        .add_filter("WAV audio", &["wav"])
        .set_file_name("patch.wav")
        .save_file()
    else {
        return Ok(None);
    };
    const RATE: u32 = 48_000;
    let audio = pm_engine::offline::render(&doc, bpm, bars.clamp(1, 512), 3.0, RATE, salt());
    std::fs::write(&path, pm_synth::wav::encode_stereo(&audio, RATE))
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// Exports the arrangement (or `bars` bars without one) as a standard MIDI file, one
/// track per part. Returns the path, or None if cancelled.
#[tauri::command]
async fn export_midi(state: tauri::State<'_, Arc<Shared>>, bars: u32) -> Result<Option<String>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export MIDI file")
        .add_filter("Standard MIDI file", &["mid"])
        .set_file_name("song.mid")
        .save_file()
    else {
        return Ok(None);
    };
    let bytes = {
        let engine = state.engine.lock().unwrap();
        pm_engine::smf::export(&engine, bars.clamp(1, 999), salt())
    };
    std::fs::write(&path, bytes).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// The folder holding saved groups (the subgraph library).
fn library_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("library");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// A file name for a library entry: letters, digits, spaces, dashes and underscores.
fn library_file(name: &str) -> Result<String, String> {
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() {
        return Err("give the group a name first".into());
    }
    Ok(format!("{clean}.pmgroup"))
}

/// Saved groups as (name, contents).
#[tauri::command]
fn library_list(app: tauri::AppHandle) -> Result<Vec<(String, String)>, String> {
    let dir = library_dir(&app)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("pmgroup") {
            continue;
        }
        if let (Some(stem), Ok(text)) = (path.file_stem().and_then(|s| s.to_str()), std::fs::read_to_string(&path)) {
            out.push((stem.to_string(), text));
        }
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    Ok(out)
}

#[tauri::command]
fn library_save(app: tauri::AppHandle, name: String, contents: String) -> Result<(), String> {
    let path = library_dir(&app)?.join(library_file(&name)?);
    std::fs::write(&path, contents).map_err(|e| format!("couldn't save {}: {e}", path.display()))
}

#[tauri::command]
fn library_delete(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let path = library_dir(&app)?.join(library_file(&name)?);
    std::fs::remove_file(&path).map_err(|e| format!("couldn't remove {}: {e}", path.display()))
}

fn main() {
    let start = Instant::now();
    let shared = Arc::new(Shared {
        engine: Mutex::new(Engine::new()),
        midi: Mutex::new(midi::Midi::default()),
        audio: audio::start(start),
        start,
    });

    // Scheduler: evaluates ahead of the playhead, sends MIDI as it comes due and hands
    // instrument notes to the audio thread early.
    let s = shared.clone();
    std::thread::Builder::new()
        .name("scheduler".into())
        .spawn(move || loop {
            let now = s.now();
            let due = {
                let mut engine = s.engine.lock().unwrap();
                let due = engine.poll(now);
                s.flush_audio(&mut engine);
                due
            };
            s.send(&due);
            std::thread::sleep(Duration::from_millis(1));
        })
        .expect("start scheduler thread");

    tauri::Builder::default()
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            engine,
            midi_ports,
            save_project,
            open_project,
            export_wav,
            export_midi,
            library_list,
            library_save,
            library_delete
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Release any sounding notes before the app goes away.
                let shared = window.state::<Arc<Shared>>();
                let offs = {
                    let mut engine = shared.engine.lock().unwrap();
                    let offs = engine.stop();
                    shared.flush_audio(&mut engine);
                    offs
                };
                shared.send(&offs);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Procedural Music");
}
