//! Slow work that runs off the main thread and reports back through the event loop: loading
//! (and if need be downloading) the speech model, warming up audio and Ollama, and checking for
//! the API key, Ollama's models and the connected microphones.

use std::sync::Arc;
use std::time::{Duration, Instant};

use murmur_core::{OllamaCleaner, TextCleaner};
use murmur_stt::Transcriber;

use crate::state::{AppEvent, Proxy};

/// How often the download progress in the status line updates.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// How often to look for microphones being plugged in or out.
const DEVICE_POLL: Duration = Duration::from_secs(3);

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::error!("Couldn't start the {name} thread: {e}");
    }
}

/// Downloads the model if it isn't there yet, loads it, and runs one second of silence through
/// it so the first real dictation doesn't pay for onnxruntime's lazy setup.
pub fn load_model(proxy: Proxy) {
    spawn("murmur-model", move || {
        let event = match load(&proxy) {
            Ok(transcriber) => AppEvent::ModelReady(Arc::new(transcriber)),
            Err(e) => {
                log::error!("Speech model failed to load: {e:#}");
                AppEvent::ModelFailed(format!("Speech model failed to load: {e}"))
            }
        };
        let _ = proxy.send_event(event);
    });
}

fn load(proxy: &Proxy) -> anyhow::Result<Transcriber> {
    let dir = murmur_stt::default_model_dir();
    if !murmur_stt::model_present(&dir) {
        log::info!("Downloading the speech model to {}", dir.display());
        let _ = proxy.send_event(AppEvent::Loading("Downloading speech model…".into()));
        let mut last = Instant::now();
        murmur_stt::download_model(&dir, |received, total| {
            if last.elapsed() < PROGRESS_INTERVAL {
                return;
            }
            last = Instant::now();
            let _ = proxy.send_event(AppEvent::Loading(download_status(received, total)));
        })?;
        log::info!("Speech model downloaded");
    }

    let _ = proxy.send_event(AppEvent::Loading("Loading speech model…".into()));
    let start = Instant::now();
    let threads = murmur_stt::default_threads();
    let transcriber = Transcriber::new(&dir, threads)?;
    let loaded = start.elapsed();
    transcriber.transcribe(&vec![0.0; murmur_core::SAMPLE_RATE as usize])?;
    log::info!(
        "Speech model loaded in {} ms on {threads} threads, warmed up in {} ms",
        loaded.as_millis(),
        (start.elapsed() - loaded).as_millis()
    );
    Ok(transcriber)
}

fn download_status(received: u64, total: Option<u64>) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    match total {
        Some(total) if total > 0 => format!(
            "Downloading speech model… {}% of {:.0} MB",
            received * 100 / total,
            total as f64 / MB
        ),
        _ => format!("Downloading speech model… {:.0} MB", received as f64 / MB),
    }
}

/// Opens and closes the microphone once. The first start in a process took over 2 s in testing
/// (CoreAudio loading its drivers); doing it now keeps that off the first dictation. On macOS
/// this is also what brings up the microphone permission prompt on first launch.
pub fn warm_up_audio(microphone: Option<String>) {
    spawn("murmur-audio-warmup", move || {
        let start = Instant::now();
        match murmur_audio::Recorder::start(microphone.as_deref(), |_| {}) {
            Ok(recorder) => {
                recorder.stop();
                log::info!("Audio warmed up in {} ms", start.elapsed().as_millis());
            }
            Err(e) => log::warn!("Couldn't open the microphone to warm up audio: {e:#}"),
        }
    });
}

/// Reads the key store off the main thread, since on macOS that runs `/usr/bin/security`.
pub fn check_api_key(proxy: Proxy) {
    spawn("murmur-api-key", move || {
        let found = murmur_core::api_key::load().is_some();
        log::info!("Anthropic API key {}", if found { "found" } else { "not found" });
        let _ = proxy.send_event(AppEvent::ApiKey(found));
    });
}

pub fn refresh_local_models(proxy: Proxy) {
    spawn("murmur-ollama-models", move || {
        let _ = proxy.send_event(AppEvent::LocalModels(OllamaCleaner::installed_models()));
    });
}

/// Loads the local model into Ollama's memory now, so the next dictation doesn't wait for it.
pub fn prepare_local_model(model: String) {
    spawn("murmur-ollama-prepare", move || {
        let start = Instant::now();
        match OllamaCleaner::new(model.clone(), Vec::new()).prepare() {
            Ok(()) => log::info!("Local model {model} ready in {} ms", start.elapsed().as_millis()),
            Err(e) => {
                log::warn!("Local model {model} isn't ready: {e:#}");
                murmur_platform::notify(&format!("Local model {model} isn't ready: {e}"));
            }
        }
    });
}

/// Keeps the Microphone menu current as devices come and go. Polls, since cpal has no change
/// notifications; enumerating is cheap.
pub fn watch_devices(proxy: Proxy) {
    spawn("murmur-devices", move || {
        let mut last = None;
        loop {
            let current = (murmur_audio::input_devices(), murmur_audio::default_input_device());
            if last.as_ref() != Some(&current) {
                let (devices, default) = current.clone();
                if proxy.send_event(AppEvent::Devices { devices, default }).is_err() {
                    return; // The event loop has exited.
                }
                last = Some(current);
            }
            std::thread::sleep(DEVICE_POLL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_status_text() {
        assert_eq!(
            download_status(50 * 1024 * 1024, Some(200 * 1024 * 1024)),
            "Downloading speech model… 25% of 200 MB"
        );
        assert_eq!(download_status(3 * 1024 * 1024, None), "Downloading speech model… 3 MB");
    }
}
