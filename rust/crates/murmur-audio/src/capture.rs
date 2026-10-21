//! Microphone capture.

use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, StreamConfig};
use murmur_core::voice::{SAMPLE_RATE, level};

use crate::convert::{mix_to_mono, resample};

/// How long `Recorder::start` waits for the first buffer before giving up on waiting. Long
/// enough for a slow Bluetooth headset to switch profiles, short enough not to hang the app.
const FIRST_BUFFER_TIMEOUT: Duration = Duration::from_secs(2);

/// Names of the connected input devices, for the Microphone menu. Names are what settings store,
/// because they survive unplugging and replugging where per-session IDs may not.
pub fn input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for name in devices.filter_map(|d| device_name(&d)) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The system's current default input device, by name.
pub fn default_input_device() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| device_name(&d))
}

fn device_name(device: &Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

/// The named device if it's connected, otherwise the system default. Falling back silently
/// matches the Mac app: a chosen mic that's unplugged shouldn't stop dictation working.
fn find_device(name: Option<&str>) -> Result<Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        let found = host
            .input_devices()
            .ok()
            .and_then(|mut ds| ds.find(|d| device_name(d).as_deref() == Some(name)));
        if let Some(device) = found {
            return Ok(device);
        }
        log::info!("microphone {name:?} not connected; using the default");
    }
    host.default_input_device()
        .ok_or_else(|| anyhow!("no microphone found"))
}

/// A recording in progress.
///
/// cpal's stream isn't `Send` on every backend, so it lives on its own thread for the length of
/// the recording; this handle only holds channels and the shared sample buffer, so the app can
/// pass it between threads freely.
pub struct Recorder {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
    /// Mono samples at `rate`, appended by the audio callback.
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

impl Recorder {
    /// Opens `device` (or the default when it's `None` or not connected) and starts recording.
    /// Returns once audio is actually arriving, so anything said after this returns is captured.
    /// `on_level` gets a 0...1 meter value about every 50 ms, called on the audio thread.
    pub fn start(device: Option<&str>, on_level: impl Fn(f32) + Send + 'static) -> Result<Self> {
        let device = find_device(device)?;
        let config = device
            .default_input_config()
            .context("reading the microphone's format")?;
        let format = config.sample_format();
        let config: StreamConfig = config.config();
        let rate = config.sample_rate;
        let samples = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));

        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let (first_tx, first_rx) = mpsc::sync_channel::<()>(1);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let sink = Sink {
            samples: samples.clone(),
            channels: config.channels as usize,
            on_level: Box::new(on_level),
            meter: Vec::new(),
            meter_len: (rate / 20) as usize,
            first: Some(first_tx),
        };

        let thread = std::thread::Builder::new()
            .name("murmur-capture".into())
            .spawn(move || run_stream(device, config, format, sink, ready_tx, stop_rx))
            .context("starting the capture thread")?;

        // Built before waiting so an early return still stops and joins the thread via Drop.
        let recorder = Recorder {
            stop: stop_tx,
            thread: Some(thread),
            samples,
            rate,
        };
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(anyhow!("capture thread exited")),
        }
        if first_rx.recv_timeout(FIRST_BUFFER_TIMEOUT).is_err() {
            // Keep going: some backends deliver late, and the stream may still come alive.
            log::warn!("microphone opened but no audio after {FIRST_BUFFER_TIMEOUT:?}");
        }
        Ok(recorder)
    }

    /// Stops recording and returns everything captured as 16 kHz mono.
    pub fn stop(mut self) -> Vec<f32> {
        self.shutdown();
        let raw = std::mem::take(&mut *self.samples.lock().unwrap_or_else(|e| e.into_inner()));
        resample(&raw, self.rate, SAMPLE_RATE).unwrap_or_else(|e| {
            log::error!("resampling the recording failed: {e:#}");
            Vec::new()
        })
    }

    /// Ends the stream and waits for its thread, so no callback can append after this returns.
    fn shutdown(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Owns the stream for the length of the recording.
fn run_stream(
    device: Device,
    config: StreamConfig,
    format: SampleFormat,
    sink: Sink,
    ready: Sender<Result<()>>,
    stop: Receiver<()>,
) {
    let stream = match open_stream(&device, &config, format, sink) {
        Ok(stream) => stream,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    // Returns on stop() or when the Recorder is dropped (the sender goes away).
    let _ = stop.recv();
    drop(stream);
}

fn open_stream(device: &Device, config: &StreamConfig, format: SampleFormat, sink: Sink) -> Result<cpal::Stream> {
    let stream = match format {
        SampleFormat::I8 => build::<i8>(device, config, sink),
        SampleFormat::I16 => build::<i16>(device, config, sink),
        SampleFormat::I24 => build::<cpal::I24>(device, config, sink),
        SampleFormat::I32 => build::<i32>(device, config, sink),
        SampleFormat::I64 => build::<i64>(device, config, sink),
        SampleFormat::U8 => build::<u8>(device, config, sink),
        SampleFormat::U16 => build::<u16>(device, config, sink),
        SampleFormat::U24 => build::<cpal::U24>(device, config, sink),
        SampleFormat::U32 => build::<u32>(device, config, sink),
        SampleFormat::U64 => build::<u64>(device, config, sink),
        SampleFormat::F32 => build::<f32>(device, config, sink),
        SampleFormat::F64 => build::<f64>(device, config, sink),
        other => return Err(anyhow!("unsupported microphone sample format {other}")),
    }
    .context("opening the microphone")?;
    stream.play().context("starting the microphone")?;
    Ok(stream)
}

fn build<T>(device: &Device, config: &StreamConfig, mut sink: Sink) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut scratch: Vec<f32> = Vec::new();
    let stream = device.build_input_stream::<T, _, _>(
        *config,
        move |data: &[T], _| {
            scratch.clear();
            scratch.extend(data.iter().map(|&s| s.to_sample::<f32>()));
            sink.push(&scratch);
        },
        |e| log::warn!("microphone stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

/// Everything the audio callback needs, moved onto the audio thread.
struct Sink {
    samples: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    on_level: Box<dyn Fn(f32) + Send>,
    /// Mono samples since the last level report.
    meter: Vec<f32>,
    /// 50 ms at the device rate.
    meter_len: usize,
    /// Signals `start` on the first buffer, then is dropped.
    first: Option<SyncSender<()>>,
}

impl Sink {
    fn push(&mut self, interleaved: &[f32]) {
        let mono = mix_to_mono(interleaved, self.channels);
        if let Some(first) = self.first.take() {
            let _ = first.try_send(());
        }
        if let Ok(mut samples) = self.samples.lock() {
            samples.extend_from_slice(&mono);
        }
        for chunk in mono.chunks(self.meter_len.max(1)) {
            let room = self.meter_len - self.meter.len();
            let take = room.min(chunk.len());
            self.meter.extend_from_slice(&chunk[..take]);
            if self.meter.len() >= self.meter_len {
                (self.on_level)(level(&self.meter));
                self.meter.clear();
                self.meter.extend_from_slice(&chunk[take..]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn assert_send<T: Send>() {}

    #[test]
    fn recorder_is_send() {
        assert_send::<Recorder>();
    }

    #[test]
    fn sink_reports_level_every_50_ms_and_mixes_down() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let mut sink = Sink {
            samples: samples.clone(),
            channels: 2,
            on_level: Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
            meter: Vec::new(),
            meter_len: 2_400, // 50 ms at 48 kHz
            first: None,
        };
        // 1 s of 48 kHz stereo in odd-sized buffers.
        for _ in 0..(48_000 / 480) {
            sink.push(&[0.25; 960]);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 20);
        let samples = samples.lock().unwrap();
        assert_eq!(samples.len(), 48_000);
        assert!(samples.iter().all(|&s| s == 0.25));
    }
}
