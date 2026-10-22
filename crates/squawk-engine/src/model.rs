//! Loading the speech model, downloading it first if it isn't there yet.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::bail;
use squawk_stt::Transcriber;

use crate::warmup::spawn;
use crate::{Event, ModelState, Options, Shared, worker};

/// How often download progress is reported.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// The model folder `options` points at: its override, or the default under the models folder.
pub fn default_model_dir(options: &Options) -> PathBuf {
    options.model_dir.clone().unwrap_or_else(squawk_stt::default_model_dir)
}

/// Downloads the model if it's the default one and isn't there yet, loads it, and runs one
/// second of silence through it so the first real dictation doesn't pay for onnxruntime's lazy
/// setup. Blocking. `progress` gets status lines ("Downloading speech model… 40% of 640 MB",
/// "Loading speech model…"), at most about twice a second.
pub fn load_model(options: &Options, mut progress: impl FnMut(String)) -> anyhow::Result<Transcriber> {
    let dir = default_model_dir(options);
    if !squawk_stt::model_present(&dir) {
        if options.model_dir.is_some() {
            // Someone else's folder: don't download over it.
            bail!(
                "no speech model in {} (set by \"model\" in the settings)",
                dir.display()
            );
        }
        log::info!("Downloading the speech model to {}", dir.display());
        progress("Downloading speech model…".into());
        let mut last = Instant::now();
        squawk_stt::download_model(&dir, |received, total| {
            if last.elapsed() < PROGRESS_INTERVAL {
                return;
            }
            last = Instant::now();
            progress(download_status(received, total));
        })?;
        log::info!("Speech model downloaded");
    }

    progress("Loading speech model…".into());
    let start = Instant::now();
    let threads = options.threads.unwrap_or_else(squawk_stt::default_threads);
    let transcriber = Transcriber::new(&dir, threads)?;
    let loaded = start.elapsed();
    transcriber.transcribe(&vec![0.0; squawk_core::SAMPLE_RATE as usize])?;
    log::info!(
        "Speech model loaded in {} ms on {threads} threads, warmed up in {} ms",
        loaded.as_millis(),
        (start.elapsed() - loaded).as_millis()
    );
    Ok(transcriber)
}

/// [`load_model`] on its own thread; on success starts the dictation worker and marks the
/// engine ready.
pub(crate) fn load_in_background(shared: Arc<Shared>, options: Options) {
    spawn("squawk-model", move || {
        let set = |state: ModelState| {
            shared.lock().model = state.clone();
            shared.emit(Event::Model(state));
        };
        match load_model(&options, |status| set(ModelState::Loading(status))) {
            Ok(transcriber) => {
                let jobs = worker::spawn(Arc::new(transcriber), shared.clone());
                let mut state = shared.lock();
                // Together, so a press that sees Ready always finds the queue.
                state.jobs = Some(jobs);
                state.model = ModelState::Ready;
                shared.emit(Event::Model(ModelState::Ready));
            }
            Err(e) => {
                log::error!("Speech model failed to load: {e:#}");
                set(ModelState::Failed(format!("Speech model failed to load: {e}")));
            }
        }
    });
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

    #[test]
    fn a_chosen_model_folder_is_never_downloaded_into() {
        let options = Options {
            model_dir: Some(PathBuf::from("/nonexistent/squawk-model")),
            threads: Some(1),
        };
        let error = load_model(&options, |_| panic!("no progress expected")).err().unwrap();
        assert!(error.to_string().contains("/nonexistent/squawk-model"), "{error}");
    }
}
