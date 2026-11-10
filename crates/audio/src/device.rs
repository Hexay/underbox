//! Real devices through cpal: the output that drains a [`Queue`], microphone capture for voice
//! chat, and device enumeration. A cpal stream isn't `Send` everywhere, so each lives on its own
//! thread until its handle drops.

use crate::convert::Converter;
use crate::output::Queue;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

fn info(d: &cpal::Device, default: Option<&str>) -> DeviceInfo {
    let id = d.id().map(|i| i.to_string()).unwrap_or_default();
    let name = d.description().map(|n| n.name().to_string()).unwrap_or_else(|_| id.clone());
    DeviceInfo { is_default: default == Some(id.as_str()), id, name }
}

pub fn outputs() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
    host.output_devices().map(|ds| ds.map(|d| info(&d, default.as_deref())).collect()).unwrap_or_default()
}

pub fn inputs() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
    host.input_devices().map(|ds| ds.map(|d| info(&d, default.as_deref())).collect()).unwrap_or_default()
}

/// A running cpal stream; dropping it stops the stream.
pub struct Running {
    _stop: mpsc::Sender<()>,
}

/// Build a stream on its own thread and keep it there; `build` returns the stream and a value
/// to hand back.
fn run<T: Send + 'static>(build: impl FnOnce() -> Result<(cpal::Stream, T), String> + Send + 'static) -> Result<(Running, T), String> {
    let (stop, stopped) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("audio device".into())
        .spawn(move || match build().and_then(|(s, v)| s.play().map(|_| (s, v)).map_err(|e| e.to_string())) {
            Ok((stream, v)) => {
                let _ = tx.send(Ok(v));
                let _ = stopped.recv();
                drop(stream);
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    let v = rx.recv().map_err(|e| e.to_string())??;
    Ok((Running { _stop: stop }, v))
}

pub struct Output {
    pub name: String,
    pub queue: Arc<Queue>,
    _running: Running,
}

fn find(devices: impl Iterator<Item = cpal::Device>, id_or_name: &str) -> Option<cpal::Device> {
    let wanted = |d: &cpal::Device| {
        let i = info(d, None);
        i.id == id_or_name || i.name == id_or_name
    };
    devices.into_iter().find(wanted)
}

/// Open `id` (or the default output), mixing into a queue of its channel count (at most 8).
pub fn open_output(id: Option<&str>) -> Result<Output, String> {
    let id = id.filter(|s| !s.is_empty()).map(str::to_owned);
    let (running, (name, queue)) = run(move || {
        let host = cpal::default_host();
        let device = match &id {
            Some(id) => host.output_devices().ok().and_then(|ds| find(ds, id)),
            None => host.default_output_device(),
        }
        .ok_or("no output device")?;
        let default = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = (default.channels() as usize).clamp(1, crate::MAX_CHANNELS);
        let rate = device
            .supported_output_configs()
            .ok()
            .and_then(|mut c| c.find(|c| c.channels() == default.channels() && c.sample_format() == cpal::SampleFormat::F32 && c.min_sample_rate() <= crate::RATE && crate::RATE <= c.max_sample_rate()))
            .map_or(default.sample_rate(), |_| crate::RATE);
        let config = cpal::StreamConfig { channels: default.channels(), sample_rate: rate, buffer_size: cpal::BufferSize::Default };
        let queue = Arc::new(Queue::new(channels));
        let (q, device_channels) = (queue.clone(), default.channels() as usize);
        let mut conv = Converter::new(channels, rate);
        let mut scratch = Vec::new();
        let stream = device
            .build_output_stream::<f32, _, _>(
                &config,
                move |data, _| {
                    let frames = data.len() / device_channels;
                    scratch.resize(frames * channels, 0.0);
                    if rate == crate::RATE {
                        q.pull(&mut scratch);
                    } else {
                        conv.fill(&q, &mut scratch);
                    }
                    for (out, frame) in data.chunks_exact_mut(device_channels).zip(scratch.chunks_exact(channels)) {
                        out.iter_mut().enumerate().for_each(|(c, o)| *o = frame.get(c).copied().unwrap_or(0.0));
                    }
                },
                |e| eprintln!("audio output: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok((stream, (info(&device, None).name, queue)))
    })?;
    Ok(Output { name, queue, _running: running })
}

/// Microphone capture, mono at the rate asked for. Holds at most a fifth of a second; older
/// audio is dropped (managed reads at most that much per tick).
pub struct Capture {
    buffer: Arc<Mutex<VecDeque<f32>>>,
    _running: Running,
}

impl Capture {
    pub fn read(&self, out: &mut [f32]) -> usize {
        let mut b = self.buffer.lock().unwrap();
        let n = out.len().min(b.len());
        out.iter_mut().zip(b.drain(..n)).for_each(|(o, s)| *o = s);
        n
    }
}

pub fn open_capture(name: Option<&str>, rate: u32) -> Result<Capture, String> {
    let name = name.filter(|s| !s.is_empty()).map(str::to_owned);
    let buffer = Arc::new(Mutex::new(VecDeque::new()));
    let sink = buffer.clone();
    let (running, ()) = run(move || {
        let host = cpal::default_host();
        let device = match &name {
            Some(n) => host.input_devices().ok().and_then(|ds| find(ds, n)),
            None => host.default_input_device(),
        }
        .ok_or("no input device")?;
        let config: cpal::StreamConfig = device.default_input_config().map_err(|e| e.to_string())?.into();
        let (channels, step) = (config.channels.max(1) as usize, config.sample_rate as f64 / rate.max(1) as f64);
        let cap = rate as usize / 5;
        let mut pos = 0.0f64;
        let stream = device
            .build_input_stream::<f32, _, _>(
                &config,
                move |data, _| {
                    let mono: Vec<f32> = data.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect();
                    let mut b = sink.lock().unwrap();
                    while (pos as usize) < mono.len() {
                        b.push_back(mono[pos as usize]);
                        pos += step;
                    }
                    pos -= mono.len() as f64;
                    let excess = b.len().saturating_sub(cap);
                    b.drain(..excess);
                },
                |e| eprintln!("audio capture: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok((stream, ()))
    })?;
    Ok(Capture { buffer, _running: running })
}
