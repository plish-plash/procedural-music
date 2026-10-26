//! MIDI ports (WinMM on Windows, CoreMIDI on macOS, ALSA on Linux): outputs for notes and
//! clock, and one input for following an external clock.

use std::collections::{HashMap, HashSet};

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

const CLIENT: &str = "Procedural Music";

/// Names of the MIDI input ports currently available.
pub fn input_names() -> Vec<String> {
    match MidiInput::new(CLIENT) {
        Ok(input) => input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Opens an input port and calls `on_realtime` with each clock, start, continue or stop
/// byte it receives (other messages are ignored). The port stays open while the
/// connection is kept.
pub fn listen_clock(
    name: &str,
    mut on_realtime: impl FnMut(u8) + Send + 'static,
) -> Result<MidiInputConnection<()>, String> {
    let mut input = MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
    // Clock is a timing message, which midir can drop; keep it.
    input.ignore(Ignore::SysexAndActiveSense);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).ok().as_deref() == Some(name))
        .ok_or_else(|| format!("MIDI input '{name}' not found"))?;
    input
        .connect(
            &port,
            "clock-in",
            move |_, bytes, _| {
                for &b in bytes {
                    if matches!(b, 0xf8 | 0xfa | 0xfb | 0xfc) {
                        on_realtime(b);
                    }
                }
            },
            (),
        )
        .map_err(|e| format!("couldn't open '{name}': {e} (is another program using it?)"))
}

/// Names of the MIDI output ports currently available.
pub fn port_names() -> Vec<String> {
    match MidiOutput::new(CLIENT) {
        Ok(out) => out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect(),
        Err(_) => Vec::new(),
    }
}

fn connect(name: &str) -> Result<MidiOutputConnection, String> {
    let out = MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
    let port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).ok().as_deref() == Some(name))
        .ok_or_else(|| format!("MIDI port '{name}' not found"))?;
    out.connect(&port, "out").map_err(|e| format!("couldn't open '{name}': {e} (is another program using it?)"))
}

/// Open connections, opened on first use and kept until [`Midi::reset`].
#[derive(Default)]
pub struct Midi {
    connections: HashMap<String, MidiOutputConnection>,
    failed: HashSet<String>,
    default_port: Option<String>,
    pub last_error: Option<String>,
}

impl Midi {
    /// Closes every connection so ports are looked up again (after plugging in a device).
    pub fn reset(&mut self) {
        self.connections.clear();
        self.failed.clear();
        self.default_port = None;
        self.last_error = None;
    }

    /// Sends bytes to a port by name; an empty name means the first port found.
    pub fn send(&mut self, port: &str, bytes: &[u8]) {
        let name = if port.is_empty() {
            if self.default_port.is_none() {
                self.default_port = port_names().into_iter().next();
            }
            match &self.default_port {
                Some(n) => n.clone(),
                None => {
                    self.last_error = Some("No MIDI output ports found".into());
                    return;
                }
            }
        } else {
            port.to_string()
        };
        if self.failed.contains(&name) {
            return;
        }
        if !self.connections.contains_key(&name) {
            match connect(&name) {
                Ok(c) => {
                    self.connections.insert(name.clone(), c);
                }
                Err(e) => {
                    self.failed.insert(name);
                    self.last_error = Some(e);
                    return;
                }
            }
        }
        if let Some(conn) = self.connections.get_mut(&name) {
            if let Err(e) = conn.send(bytes) {
                self.last_error = Some(format!("{name}: {e}"));
            }
        }
    }
}
