//! Node graph evaluation and MIDI scheduling.
//!
//! The graph is a pure function of musical time: [`Graph::evaluate`] takes a
//! window of ticks and returns the note events that start in it. The
//! [`Engine`] drives that function from a clock, keeps a short lookahead queue
//! of timed MIDI messages, and hands them out as they come due.
//!
//! This crate has no dependencies beyond `pm-theory`; the desktop app supplies
//! the clock, the MIDI ports and the JSON layer.

pub mod api;
mod engine;
mod graph;
pub mod json;
mod nodes;
pub mod offline;
mod rng;
mod spec;
mod timeline;
mod value;

pub use engine::{Engine, MidiMessage, DEFAULT_PORT};
pub use graph::{
    module_id, CompileReport, EvalContext, Graph, GraphDoc, NodeDoc, NodeIssue, SinkOutput, WireDoc,
};
pub use nodes::{solve_generation, spec_for, specs, Sink};
pub use spec::{NodeSpec, ParamKind, ParamSpec, ParamValue, PortSpec, PortType};
pub use timeline::{Segment, Timeline};
pub use value::Value;
