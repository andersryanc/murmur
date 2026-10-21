//! The dictation pipeline after the key comes up: transcribe → dictionary replacements → cleanup
//! → paste, a port of the Swift `AppModel.process`.
//!
//! Finished recordings go into a queue served by one worker thread, so a new dictation can start
//! while earlier ones are still processing, and they paste in the order they were spoken.

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use murmur_audio::Recorder;
use murmur_core::{ClaudeCleaner, CleanupMode, Dictionary, OllamaCleaner, TargetContext, TextCleaner};
use murmur_platform::PasteOutcome;
use murmur_stt::Transcriber;

use crate::state::{AppEvent, Proxy, Timing};

/// Holds shorter than this (counted from when recording starts, after the hotkey's own 200 ms
/// hold) are treated as accidental taps and never transcribed.
pub const MINIMUM_HOLD: Duration = Duration::from_millis(300);

/// A recording in progress, and what was in front when it started: that's where the text will
/// go, so its tone is fixed then rather than when the key comes up.
pub struct Recording {
    pub recorder: Recorder,
    pub started: Instant,
    pub context: TargetContext,
}

/// One finished recording waiting its turn.
pub struct Job {
    pub samples: Vec<f32>,
    pub context: TargetContext,
    /// The cleanup choice when the key came up, so changing it mid-queue doesn't affect
    /// dictations already spoken.
    pub cleanup: CleanupMode,
    pub ollama_model: String,
}

/// Starts the worker that processes jobs one at a time, in order.
pub fn spawn_worker(transcriber: Arc<Transcriber>, proxy: Proxy) -> Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("murmur-dictation".into())
        .spawn(move || {
            for job in rx {
                process(&transcriber, job, &proxy);
                let _ = proxy.send_event(AppEvent::JobDone);
            }
        })
        .expect("starting the dictation thread");
    tx
}

fn process(transcriber: &Transcriber, job: Job, proxy: &Proxy) {
    let speech_start = Instant::now();
    let raw = match transcriber.transcribe(&job.samples) {
        Ok(raw) => raw,
        Err(e) => {
            log::error!("Transcription failed: {e:#}");
            notice(proxy, "Transcription failed");
            return;
        }
    };
    let speech = speech_start.elapsed();
    log::info!(
        "Transcribed {:.1} s of audio in {} ms",
        job.samples.len() as f64 / murmur_core::SAMPLE_RATE as f64,
        speech.as_millis()
    );
    if raw.is_empty() {
        return;
    }

    // Re-read every time so edits to dictionary.json apply to the next dictation without a
    // restart. Replacements run first and always, so they work with cleanup off and the cleanup
    // model sees the corrected terms.
    let path = Dictionary::default_path();
    let dictionary = Dictionary::load(&path).unwrap_or_else(|e| {
        log::warn!("Can't read {}: {e:#}; using an empty dictionary", path.display());
        Dictionary::default()
    });
    let corrected = dictionary.apply_replacements(&raw);

    let mut text = corrected.clone();
    let mut cleanup = None;
    if let Some(cleaner) = make_cleaner(&job, dictionary.vocabulary(), proxy) {
        let start = Instant::now();
        match cleaner.clean(&corrected, job.context) {
            Ok(cleaned) => {
                cleanup = Some((start.elapsed(), cleaner.id()));
                text = cleaned;
            }
            Err(e) => {
                log::error!("Cleanup with {} failed: {e:#}", cleaner.id());
                notice(proxy, "Cleanup failed, pasted raw text");
            }
        }
    }
    let timing = Timing { speech, cleanup };
    log::info!("{} [{}]", timing.summary(), job.context.as_str());
    let _ = proxy.send_event(AppEvent::Timing(timing));
    if text.is_empty() {
        return;
    }

    match murmur_platform::paste(&text) {
        Ok(PasteOutcome::Pasted) => {}
        Ok(PasteOutcome::CopiedOnly) => notice(proxy, "Copied. Press ⌘V to paste"),
        Err(e) => {
            log::error!("Paste failed: {e:#}");
            notice(proxy, "Couldn't paste the dictation");
            return;
        }
    }
    let _ = proxy.send_event(AppEvent::Dictated(text));
}

/// The cleaner for the job's mode, or `None` to paste the transcript as it is.
fn make_cleaner(job: &Job, vocabulary: Vec<String>, proxy: &Proxy) -> Option<Box<dyn TextCleaner>> {
    match job.cleanup {
        CleanupMode::Off => None,
        CleanupMode::Claude => {
            let Some(key) = murmur_core::api_key::load() else {
                let _ = proxy.send_event(AppEvent::ApiKey(false));
                notice(proxy, "No API key, pasted raw text");
                return None;
            };
            let _ = proxy.send_event(AppEvent::ApiKey(true));
            Some(Box::new(ClaudeCleaner::new(key, vocabulary)))
        }
        CleanupMode::Ollama => Some(Box::new(OllamaCleaner::new(job.ollama_model.clone(), vocabulary))),
    }
}

/// Shown in the pill for a moment (or as a system notification where there's no pill).
fn notice(proxy: &Proxy, message: &str) {
    let _ = proxy.send_event(AppEvent::Notice(message.to_string()));
}
