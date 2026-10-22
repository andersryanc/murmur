//! Start-up work that keeps first-use delays off the first dictation.

use std::time::Instant;

use squawk_core::{OllamaCleaner, TextCleaner};

pub(crate) fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::error!("Couldn't start the {name} thread: {e}");
    }
}

/// Opens and closes the microphone once. The first start in a process took over 2 s in testing
/// (CoreAudio loading its drivers); doing it now keeps that off the first dictation. On macOS
/// this is also what brings up the microphone permission prompt on first launch.
pub(crate) fn warm_up_audio(microphone: Option<String>) {
    spawn("squawk-audio-warmup", move || {
        let start = Instant::now();
        match squawk_audio::Recorder::start(microphone.as_deref(), |_| {}) {
            Ok(recorder) => {
                recorder.stop();
                log::info!("Audio warmed up in {} ms", start.elapsed().as_millis());
            }
            Err(e) => log::warn!("Couldn't open the microphone to warm up audio: {e:#}"),
        }
    });
}

/// Loads the local model into Ollama's memory now, on its own thread, so the next dictation
/// doesn't wait for it (that can take 25 s). The engine does this at start when cleanup is
/// Ollama; front ends call it when the user picks Ollama or another model. A failure is a
/// system notification rather than a notice, since no dictation is under way to show it with.
pub fn prepare_local_model(model: String) {
    spawn("squawk-ollama-prepare", move || {
        let start = Instant::now();
        match OllamaCleaner::new(model.clone(), Vec::new()).prepare() {
            Ok(()) => log::info!("Local model {model} ready in {} ms", start.elapsed().as_millis()),
            Err(e) => {
                log::warn!("Local model {model} isn't ready: {e:#}");
                squawk_platform::notify(&format!("Local model {model} isn't ready: {e}"));
            }
        }
    });
}
