//! Output nodes.

use super::{Inputs, NodeImpl, Sink};
use crate::graph::EvalContext;
use crate::spec::{port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;

pub static MIDI_OUT: NodeSpec = NodeSpec {
    kind: "midi_out",
    title: "MIDI out",
    category: "Output",
    help: "Sends notes to a MIDI port and channel. Leave the port on 'Default' to use the first port found \
           (on Windows, usually Microsoft GS Wavetable Synth). Program (1-128) selects a General MIDI \
           instrument when playback starts; 0 leaves the synth's sound alone.",
    inputs: &[port("notes", PortType::Notes)],
    outputs: &[],
    params: &[
        ParamSpec { name: "port", label: "Port", kind: ParamKind::MidiPort },
        ParamSpec { name: "channel", label: "Channel", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 1.0 } },
        ParamSpec {
            name: "program",
            label: "Program",
            kind: ParamKind::Number { min: 0.0, max: 128.0, step: 1.0, default: 0.0 },
        },
        ParamSpec { name: "mute", label: "Mute", kind: ParamKind::Bool { default: false } },
    ],
};

struct MidiOut {
    port: String,
    channel: u8,
    program: Option<u8>,
    mute: bool,
}

impl NodeImpl for MidiOut {
    fn eval(&self, _: &Inputs, _: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        Ok(Vec::new())
    }

    fn sink(&self) -> Option<Sink> {
        (!self.mute).then(|| Sink::Midi {
            port: self.port.clone(),
            channel: self.channel,
        })
    }

    fn is_sink(&self) -> bool {
        true
    }

    fn program(&self) -> Option<(String, u8, u8)> {
        self.program.map(|p| (self.port.clone(), self.channel, p))
    }
}

pub fn build_midi_out(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    Ok(Box::new(MidiOut {
        port: p.text("port"),
        channel: (p.int("channel").clamp(1, 16) - 1) as u8,
        program: match p.int("program") {
            0 => None,
            n => Some((n - 1).clamp(0, 127) as u8),
        },
        mute: p.flag("mute"),
    }))
}
