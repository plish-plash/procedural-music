//! Audio output: renders the built-in instruments in the sound card's callback.
//!
//! The scheduler sends [`AudioMsg`]s (rack changes, tempo, timed notes) over a channel;
//! the callback applies them and renders. Note times use the app's shared clock, so the
//! callback keeps a mapping from its sample counter to that clock and re-anchors it if
//! the two drift apart by more than 30 ms.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use pm_synth::{AudioMsg, Rack};

#[derive(Clone, Debug, Default)]
pub struct AudioStatus {
    /// "Speakers · 48 kHz"
    pub device: Option<String>,
    pub error: Option<String>,
}

pub struct Audio {
    pub tx: Sender<AudioMsg>,
    pub status: Arc<Mutex<AudioStatus>>,
}

/// Opens the default output device on its own thread (the stream must stay on the thread
/// that created it on some platforms).
pub fn start(clock: Instant) -> Audio {
    let (tx, rx) = mpsc::channel();
    let status = Arc::new(Mutex::new(AudioStatus::default()));
    let st = status.clone();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || match open(clock, rx, st.clone()) {
            Ok(_stream) => loop {
                // Keep the stream alive for the life of the app.
                std::thread::park();
            },
            Err(e) => st.lock().unwrap().error = Some(e),
        })
        .expect("start audio thread");
    Audio { tx, status }
}

fn open(clock: Instant, rx: Receiver<AudioMsg>, status: Arc<Mutex<AudioStatus>>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no audio output device found")?;
    let supported = device.default_output_config().map_err(|e| format!("audio device: {e}"))?;
    let sample_rate = supported.sample_rate().0;
    let name = device.name().unwrap_or_else(|_| "audio device".into());
    status.lock().unwrap().device = Some(format!("{name} · {} kHz", sample_rate as f32 / 1000.0));
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, clock, rx, status),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, clock, rx, status),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, clock, rx, status),
        other => return Err(format!("unsupported sample format {other:?}")),
    }?;
    stream.play().map_err(|e| format!("couldn't start audio: {e}"))?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    clock: Instant,
    rx: Receiver<AudioMsg>,
    status: Arc<Mutex<AudioStatus>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sr = config.sample_rate.0 as f64;
    let mut rack = Rack::new(sr as f32);
    let mut scratch = vec![0.0f32; 2 * 4096];
    let mut frames_done: u64 = 0;
    let mut anchor: Option<f64> = None;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                while let Ok(msg) = rx.try_recv() {
                    rack.handle(msg);
                }
                let frames = data.len() / channels.max(1);
                if scratch.len() < 2 * frames {
                    scratch.resize(2 * frames, 0.0);
                }
                // Map this buffer onto the shared clock.
                let now = clock.elapsed().as_secs_f64();
                let mut t = anchor.map_or(now, |a| a + frames_done as f64 / sr);
                if anchor.is_none() || (t - now).abs() > 0.03 {
                    anchor = Some(now - frames_done as f64 / sr);
                    t = now;
                }
                rack.render(t, &mut scratch[..2 * frames]);
                frames_done += frames as u64;
                for (f, frame) in data.chunks_mut(channels.max(1)).enumerate() {
                    let (l, r) = (scratch[2 * f], scratch[2 * f + 1]);
                    if channels == 1 {
                        frame[0] = T::from_sample((l + r) * 0.5);
                        continue;
                    }
                    for (c, s) in frame.iter_mut().enumerate() {
                        *s = T::from_sample(match c {
                            0 => l,
                            1 => r,
                            _ => 0.0,
                        });
                    }
                }
            },
            move |e| status.lock().unwrap().error = Some(e.to_string()),
            None,
        )
        .map_err(|e| format!("couldn't open audio output: {e}"))
}
