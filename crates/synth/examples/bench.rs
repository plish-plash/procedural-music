//! Rough CPU check: renders 10 s of a busy rack and reports the real-time factor.
use pm_synth::dsp::Wave;
use pm_synth::fx::{ChorusParams, DelayParams, ReverbParams};
use pm_synth::synth::SynthParams;
use pm_synth::*;
use std::time::Instant;

fn main() {
    let sr = 48_000.0;
    let mut rack = Rack::new(sr);
    rack.handle(AudioMsg::Rack(RackDesc {
        modules: vec![
            ModuleDesc {
                id: 1,
                kind: ModuleKind::Poly(SynthParams {
                    wave: Wave::Saw,
                    ..Default::default()
                }),
                inputs: vec![],
            },
            ModuleDesc {
                id: 2,
                kind: ModuleKind::Drums(Default::default()),
                inputs: vec![],
            },
            ModuleDesc {
                id: 3,
                kind: ModuleKind::Chorus(ChorusParams::default()),
                inputs: vec![Some(1)],
            },
            ModuleDesc {
                id: 4,
                kind: ModuleKind::Delay(DelayParams::default()),
                inputs: vec![Some(3)],
            },
            ModuleDesc {
                id: 5,
                kind: ModuleKind::Reverb(ReverbParams::default()),
                inputs: vec![Some(4), Some(2)],
            },
            ModuleDesc {
                id: 6,
                kind: ModuleKind::Output { level: 0.8 },
                inputs: vec![Some(5)],
            },
        ],
    }));
    let mut events = Vec::new();
    for i in 0..16 {
        events.push(TimedEvent {
            at: 0.0,
            module: 1,
            event: NoteEvent::On {
                id: i,
                freq: 110.0 * (1.0 + i as f32 * 0.25),
                key: 60,
                velocity: 100,
            },
        });
    }
    for b in 0..40 {
        for key in [36u8, 42, 38] {
            events.push(TimedEvent {
                at: b as f64 * 0.25,
                module: 2,
                event: NoteEvent::On {
                    id: 0,
                    freq: 0.0,
                    key,
                    velocity: 100,
                },
            });
        }
    }
    rack.handle(AudioMsg::Events(events));
    let mut buf = vec![0.0f32; 2 * 512];
    let t = Instant::now();
    let blocks = (10.0 * sr / 512.0) as usize;
    for b in 0..blocks {
        rack.render(b as f64 * 512.0 / sr as f64, &mut buf);
    }
    let secs = t.elapsed().as_secs_f64();
    println!(
        "10 s of audio in {secs:.3} s: {:.0}x real time",
        10.0 / secs
    );
}
