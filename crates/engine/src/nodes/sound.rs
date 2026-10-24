//! Built-in sound: instruments, effects, the mixer and the audio output. These nodes
//! become modules in the audio rack (`pm-synth`); in the event graph, instruments are
//! note sinks and the rest do nothing.

use pm_synth::drums::DrumParams;
use pm_synth::dsp::{db, FilterMode, Wave};
use pm_synth::fx::{
    ChannelParams, ChorusParams, DelayParams, DriveParams, FilterParams, ReverbParams,
};
use pm_synth::synth::{MonoParams, SynthParams};
use pm_synth::ModuleKind;

use super::{Inputs, NodeImpl, Sink};
use crate::graph::EvalContext;
use crate::spec::{optional, port, NodeSpec, ParamKind, ParamSpec, Params, PortType};
use crate::value::Value;

struct SoundNode {
    kind: ModuleKind,
    instrument: bool,
}

impl NodeImpl for SoundNode {
    fn eval(&self, _: &Inputs, _: &EvalContext) -> Result<Vec<Option<Value>>, String> {
        Ok(Vec::new())
    }

    fn sink(&self) -> Option<Sink> {
        self.instrument.then_some(Sink::Instrument)
    }

    fn is_sink(&self) -> bool {
        self.instrument
    }

    fn module(&self) -> Option<ModuleKind> {
        Some(self.kind.clone())
    }
}

const fn num(
    name: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    step: f64,
    default: f64,
) -> ParamSpec {
    ParamSpec {
        name,
        label,
        kind: ParamKind::Number {
            min,
            max,
            step,
            default,
        },
    }
}

const fn log(
    name: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
) -> ParamSpec {
    ParamSpec {
        name,
        label,
        kind: ParamKind::LogNumber { min, max, default },
    }
}

const WAVES: &[&str] = &["Saw", "Square", "Triangle", "Sine"];

const NOTES_IN: &[crate::spec::PortSpec] = &[port("notes", PortType::Notes)];
const AUDIO_IN: &[crate::spec::PortSpec] = &[port("audio", PortType::Audio)];
const AUDIO_PORT: &[crate::spec::PortSpec] = &[port("audio", PortType::Audio)];

pub static POLY_SYNTH: NodeSpec = NodeSpec {
    kind: "poly_synth",
    title: "Poly synth",
    category: "Sound",
    help: "A 16-voice subtractive synth: two detuned oscillators into a resonant low-pass filter, \
           with an amp envelope and a filter envelope. Good for pads, keys and plucks.",
    inputs: NOTES_IN,
    outputs: AUDIO_PORT,
    params: &[
        ParamSpec {
            name: "wave",
            label: "Wave",
            kind: ParamKind::Choice {
                options: WAVES,
                default: "Saw",
            },
        },
        num("detune", "Detune ¢", 0.0, 50.0, 1.0, 8.0),
        log("cutoff", "Cutoff Hz", 40.0, 16_000.0, 2000.0),
        num("resonance", "Resonance %", 0.0, 100.0, 1.0, 20.0),
        num("env_amount", "Filter env oct", 0.0, 6.0, 0.1, 2.0),
        log("filter_decay", "Filter decay ms", 5.0, 5000.0, 300.0),
        log("attack", "Attack ms", 1.0, 5000.0, 5.0),
        log("decay", "Decay ms", 5.0, 5000.0, 400.0),
        num("sustain", "Sustain %", 0.0, 100.0, 1.0, 70.0),
        log("release", "Release ms", 5.0, 8000.0, 400.0),
        num("level", "Level dB", -40.0, 6.0, 0.5, -6.0),
    ],
};

pub static MONO_SYNTH: NodeSpec = NodeSpec {
    kind: "mono_synth",
    title: "Mono synth",
    category: "Sound",
    help:
        "A one-voice synth for bass and leads. The newest note wins; with Legato on, overlapping \
           notes glide without restarting the envelopes.",
    inputs: NOTES_IN,
    outputs: AUDIO_PORT,
    params: &[
        ParamSpec {
            name: "wave",
            label: "Wave",
            kind: ParamKind::Choice {
                options: WAVES,
                default: "Saw",
            },
        },
        num("detune", "Detune ¢", 0.0, 50.0, 1.0, 5.0),
        log("cutoff", "Cutoff Hz", 40.0, 16_000.0, 700.0),
        num("resonance", "Resonance %", 0.0, 100.0, 1.0, 30.0),
        num("env_amount", "Filter env oct", 0.0, 6.0, 0.1, 2.5),
        log("filter_decay", "Filter decay ms", 5.0, 5000.0, 220.0),
        log("attack", "Attack ms", 1.0, 5000.0, 3.0),
        log("decay", "Decay ms", 5.0, 5000.0, 300.0),
        num("sustain", "Sustain %", 0.0, 100.0, 1.0, 60.0),
        log("release", "Release ms", 5.0, 8000.0, 120.0),
        num("glide", "Glide ms", 0.0, 1000.0, 5.0, 40.0),
        ParamSpec {
            name: "legato",
            label: "Legato",
            kind: ParamKind::Bool { default: true },
        },
        num("level", "Level dB", -40.0, 6.0, 0.5, -6.0),
    ],
};

pub static DRUM_SYNTH: NodeSpec = NodeSpec {
    kind: "drum_synth",
    title: "Drum synth",
    category: "Sound",
    help: "A synthesized drum kit on General MIDI keys (the Drum hit node's sounds): kick, snare, clap, \
           hats, toms, cymbals, rim, cowbell and shaker.",
    inputs: NOTES_IN,
    outputs: AUDIO_PORT,
    params: &[
        num("tune", "Tune st", -12.0, 12.0, 0.5, 0.0),
        num("decay", "Decay %", 25.0, 400.0, 5.0, 100.0),
        num("level", "Level dB", -40.0, 6.0, 0.5, -3.0),
    ],
};

pub static MIXER: NodeSpec = NodeSpec {
    kind: "mixer",
    title: "Mixer",
    category: "Sound",
    help: "Mixes up to four audio signals, each with its own level and pan.",
    inputs: &[
        optional("a", PortType::Audio),
        optional("b", PortType::Audio),
        optional("c", PortType::Audio),
        optional("d", PortType::Audio),
    ],
    outputs: AUDIO_PORT,
    params: &[
        num("level_a", "A level dB", -60.0, 6.0, 0.5, 0.0),
        num("pan_a", "A pan", -100.0, 100.0, 1.0, 0.0),
        num("level_b", "B level dB", -60.0, 6.0, 0.5, 0.0),
        num("pan_b", "B pan", -100.0, 100.0, 1.0, 0.0),
        num("level_c", "C level dB", -60.0, 6.0, 0.5, 0.0),
        num("pan_c", "C pan", -100.0, 100.0, 1.0, 0.0),
        num("level_d", "D level dB", -60.0, 6.0, 0.5, 0.0),
        num("pan_d", "D pan", -100.0, 100.0, 1.0, 0.0),
    ],
};

pub static FX_FILTER: NodeSpec = NodeSpec {
    kind: "fx_filter",
    title: "Filter",
    category: "Effects",
    help: "A resonant stereo filter: low-pass, high-pass or band-pass.",
    inputs: AUDIO_IN,
    outputs: AUDIO_PORT,
    params: &[
        ParamSpec {
            name: "mode",
            label: "Mode",
            kind: ParamKind::Choice {
                options: &["Low-pass", "High-pass", "Band-pass"],
                default: "Low-pass",
            },
        },
        log("cutoff", "Cutoff Hz", 20.0, 18_000.0, 1200.0),
        num("resonance", "Resonance %", 0.0, 100.0, 1.0, 20.0),
    ],
};

pub static FX_DRIVE: NodeSpec = NodeSpec {
    kind: "fx_drive",
    title: "Drive",
    category: "Effects",
    help: "Soft saturation, from warmth to fuzz.",
    inputs: AUDIO_IN,
    outputs: AUDIO_PORT,
    params: &[
        num("amount", "Amount %", 0.0, 100.0, 1.0, 40.0),
        num("mix", "Mix %", 0.0, 100.0, 1.0, 100.0),
    ],
};

pub static FX_CHORUS: NodeSpec = NodeSpec {
    kind: "fx_chorus",
    title: "Chorus",
    category: "Effects",
    help: "Thickens and widens a sound with slowly moving delayed copies.",
    inputs: AUDIO_IN,
    outputs: AUDIO_PORT,
    params: &[
        log("rate", "Rate Hz", 0.05, 8.0, 0.6),
        num("depth", "Depth %", 0.0, 100.0, 1.0, 50.0),
        num("mix", "Mix %", 0.0, 100.0, 1.0, 40.0),
    ],
};

const DELAY_TIMES: &[&str] = &[
    "1/16",
    "1/8",
    "1/8 dotted",
    "1/8 triplet",
    "1/4",
    "1/4 dotted",
    "1/2",
    "1 bar",
];

pub static FX_DELAY: NodeSpec = NodeSpec {
    kind: "fx_delay",
    title: "Delay",
    category: "Effects",
    help: "Tempo-synced echo. Ping-pong bounces the repeats between left and right; Tone darkens them.",
    inputs: AUDIO_IN,
    outputs: AUDIO_PORT,
    params: &[
        ParamSpec { name: "time", label: "Time", kind: ParamKind::Choice { options: DELAY_TIMES, default: "1/8 dotted" } },
        num("feedback", "Feedback %", 0.0, 95.0, 1.0, 35.0),
        num("mix", "Mix %", 0.0, 100.0, 1.0, 25.0),
        ParamSpec { name: "ping_pong", label: "Ping-pong", kind: ParamKind::Bool { default: true } },
        log("tone", "Tone Hz", 500.0, 16_000.0, 4000.0),
    ],
};

pub static FX_REVERB: NodeSpec = NodeSpec {
    kind: "fx_reverb",
    title: "Reverb",
    category: "Effects",
    help: "A room-to-hall reverb (Freeverb).",
    inputs: AUDIO_IN,
    outputs: AUDIO_PORT,
    params: &[
        num("size", "Size %", 0.0, 100.0, 1.0, 75.0),
        num("damping", "Damping %", 0.0, 100.0, 1.0, 40.0),
        num("mix", "Mix %", 0.0, 100.0, 1.0, 25.0),
        num("width", "Width %", 0.0, 100.0, 1.0, 100.0),
    ],
};

pub static AUDIO_OUT: NodeSpec = NodeSpec {
    kind: "audio_out",
    title: "Audio out",
    category: "Output",
    help: "Sends audio to your speakers or headphones. Several Audio out nodes are added together.",
    inputs: AUDIO_IN,
    outputs: &[],
    params: &[num("level", "Level dB", -40.0, 6.0, 0.5, 0.0)],
};

fn wave(name: &str) -> Wave {
    match name {
        "Square" => Wave::Square,
        "Triangle" => Wave::Triangle,
        "Sine" => Wave::Sine,
        _ => Wave::Saw,
    }
}

fn synth_params(p: &Params) -> SynthParams {
    let ms = |name: &str| (p.number(name) / 1000.0) as f32;
    SynthParams {
        wave: wave(&p.text("wave")),
        detune: p.number("detune") as f32,
        cutoff: p.number("cutoff") as f32,
        resonance: (p.number("resonance") / 100.0) as f32,
        env_amount: p.number("env_amount") as f32,
        filter_decay: ms("filter_decay"),
        attack: ms("attack"),
        decay: ms("decay"),
        sustain: (p.number("sustain") / 100.0) as f32,
        release: ms("release"),
        level: db(p.number("level") as f32),
    }
}

fn pct(p: &Params, name: &str) -> f32 {
    (p.number(name) / 100.0) as f32
}

fn delay_beats(name: &str) -> f32 {
    match name {
        "1/16" => 0.25,
        "1/8" => 0.5,
        "1/8 triplet" => 1.0 / 3.0,
        "1/4" => 1.0,
        "1/4 dotted" => 1.5,
        "1/2" => 2.0,
        "1 bar" => 4.0,
        _ => 0.75,
    }
}

pub fn build(kind: &str, p: &Params) -> Result<Box<dyn NodeImpl>, String> {
    let (kind, instrument) = match kind {
        "poly_synth" => (ModuleKind::Poly(synth_params(p)), true),
        "mono_synth" => (
            ModuleKind::Mono(
                synth_params(p),
                MonoParams {
                    glide: (p.number("glide") / 1000.0) as f32,
                    legato: p.flag("legato"),
                },
            ),
            true,
        ),
        "drum_synth" => (
            ModuleKind::Drums(DrumParams {
                tune: p.number("tune") as f32,
                decay: pct(p, "decay"),
                level: db(p.number("level") as f32),
            }),
            true,
        ),
        "mixer" => {
            let ch = |c: &str| ChannelParams {
                level: db(p.number(&format!("level_{c}")) as f32),
                pan: pct(p, &format!("pan_{c}")),
            };
            (
                ModuleKind::Mixer([ch("a"), ch("b"), ch("c"), ch("d")]),
                false,
            )
        }
        "fx_filter" => {
            let mode = match p.text("mode").as_str() {
                "High-pass" => FilterMode::High,
                "Band-pass" => FilterMode::Band,
                _ => FilterMode::Low,
            };
            (
                ModuleKind::Filter(FilterParams {
                    mode,
                    cutoff: p.number("cutoff") as f32,
                    resonance: pct(p, "resonance"),
                }),
                false,
            )
        }
        "fx_drive" => (
            ModuleKind::Drive(DriveParams {
                amount: pct(p, "amount"),
                mix: pct(p, "mix"),
            }),
            false,
        ),
        "fx_chorus" => (
            ModuleKind::Chorus(ChorusParams {
                rate: p.number("rate") as f32,
                depth: pct(p, "depth"),
                mix: pct(p, "mix"),
            }),
            false,
        ),
        "fx_delay" => (
            ModuleKind::Delay(DelayParams {
                beats: delay_beats(&p.text("time")),
                feedback: pct(p, "feedback"),
                mix: pct(p, "mix"),
                ping_pong: p.flag("ping_pong"),
                tone: p.number("tone") as f32,
            }),
            false,
        ),
        "fx_reverb" => (
            ModuleKind::Reverb(ReverbParams {
                size: pct(p, "size"),
                damping: pct(p, "damping"),
                mix: pct(p, "mix"),
                width: pct(p, "width"),
            }),
            false,
        ),
        "audio_out" => (
            ModuleKind::Output {
                level: db(p.number("level") as f32),
            },
            false,
        ),
        other => return Err(format!("unknown sound node '{other}'")),
    };
    Ok(Box::new(SoundNode { kind, instrument }))
}
