//! Output nodes.

use super::{ControlMessage, Inputs, NodeImpl, Sink};
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

pub static MIDI_CC: NodeSpec = NodeSpec {
    kind: "midi_cc",
    title: "MIDI CC out",
    category: "Output",
    help: "Sends a control to a MIDI port as control changes, pitch bend or \
           channel pressure, every sixteenth while it changes. CC and pressure take values 0–127 (so \
           set the control's range to match); pitch bend takes −1 to 1, with 0 the centre.",
    inputs: &[port("value", PortType::Control)],
    outputs: &[],
    params: &[
        ParamSpec { name: "port", label: "Port", kind: ParamKind::MidiPort },
        ParamSpec { name: "channel", label: "Channel", kind: ParamKind::Number { min: 1.0, max: 16.0, step: 1.0, default: 1.0 } },
        ParamSpec {
            name: "message",
            label: "Message",
            kind: ParamKind::Choice {
                options: &["Control change", "Pitch bend", "Channel pressure"],
                default: "Control change",
            },
        },
        ParamSpec { name: "cc", label: "CC number", kind: ParamKind::Number { min: 0.0, max: 127.0, step: 1.0, default: 74.0 } },
        ParamSpec { name: "mute", label: "Mute", kind: ParamKind::Bool { default: false } },
    ],
};

struct MidiCc {
    sink: Sink,
    mute: bool,
}

impl NodeImpl for MidiCc {
    fn eval(&self, _: &Inputs, _: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        Ok(Vec::new())
    }

    fn sink(&self) -> Option<Sink> {
        (!self.mute).then(|| self.sink.clone())
    }

    fn is_sink(&self) -> bool {
        true
    }
}

pub fn build_midi_cc(p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let message = match p.text("message").as_str() {
        "Pitch bend" => ControlMessage::PitchBend,
        "Channel pressure" => ControlMessage::Pressure,
        _ => ControlMessage::Cc(p.int("cc").clamp(0, 127) as u8),
    };
    Ok(Box::new(MidiCc {
        sink: Sink::Control {
            port: p.text("port"),
            channel: (p.int("channel").clamp(1, 16) - 1) as u8,
            message,
        },
        mute: p.flag("mute"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_bytes() {
        assert_eq!(ControlMessage::Cc(74).bytes(2, 64.4), [0xb2, 74, 64]);
        assert_eq!(ControlMessage::Cc(1).bytes(0, 300.0), [0xb0, 1, 127]);
        assert_eq!(ControlMessage::Pressure.bytes(15, -3.0), [0xdf, 0]);
        assert_eq!(ControlMessage::PitchBend.bytes(0, 0.0), [0xe0, 0, 64]);
        assert_eq!(ControlMessage::PitchBend.bytes(0, 1.0), [0xe0, 0x7f, 0x7f]);
        assert_eq!(ControlMessage::PitchBend.bytes(0, -1.0), [0xe0, 0, 0]);
    }
}
