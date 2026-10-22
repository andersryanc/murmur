//! The dictation pipeline after the key comes up: transcribe → Dictionary replacements →
//! cleanup → paste, a port of the Swift `AppModel.process` and `Daemon.finish`.
//!
//! Finished recordings go into a queue served by one worker thread, so a new dictation can start
//! while earlier ones are still processing, and they paste in the order they were spoken.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use murmur_core::{ClaudeCleaner, CleanupMode, Dictionary, OllamaCleaner, TargetContext, TextCleaner};
use murmur_platform::PasteOutcome;
use murmur_stt::Transcriber;

use crate::{Event, Shared, Timing};

/// What to say when the text couldn't be pasted for the user, only copied.
#[cfg(target_os = "macos")]
const COPIED_ONLY: &str = "Copied. Press ⌘V to paste";
#[cfg(not(target_os = "macos"))]
const COPIED_ONLY: &str = "Copied. Press Ctrl+V to paste";

/// One finished recording waiting its turn.
pub(crate) struct Job {
    pub samples: Vec<f32>,
    pub context: TargetContext,
    /// The cleanup choice when the key came up, so changing it mid-queue doesn't affect
    /// dictations already spoken.
    pub cleanup: CleanupMode,
    pub ollama_model: String,
}

/// Starts the worker that processes jobs one at a time, in order.
pub(crate) fn spawn(transcriber: Arc<Transcriber>, shared: Arc<Shared>) -> Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("murmur-dictation".into())
        .spawn(move || {
            for job in rx {
                process(&transcriber, job, &shared);
                let mut state = shared.lock();
                state.queued = state.queued.saturating_sub(1);
                shared.emit(Event::Queue(state.queued));
            }
        })
        .expect("starting the dictation thread");
    tx
}

fn process(transcriber: &Transcriber, job: Job, shared: &Shared) {
    let speech_start = Instant::now();
    let raw = match transcriber.transcribe(&job.samples) {
        Ok(raw) => raw,
        Err(e) => {
            log::error!("Transcription failed: {e:#}");
            shared.notice("Transcription failed");
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

    // Replacements run first and always, so they work with cleanup off and the cleanup model
    // sees the corrected terms.
    let dictionary = load_dictionary();
    let corrected = dictionary.apply_replacements(&raw);

    let mut text = corrected.clone();
    let mut cleanup = None;
    let chosen = match cleaner(job.cleanup, &job.ollama_model, dictionary.vocabulary()) {
        Ok(chosen) => {
            if job.cleanup == CleanupMode::Claude {
                shared.emit(Event::ApiKey(true));
            }
            chosen
        }
        Err(MissingApiKey) => {
            shared.emit(Event::ApiKey(false));
            shared.notice("No API key, pasted raw text");
            None
        }
    };
    if let Some(cleaner) = chosen {
        let start = Instant::now();
        match cleaner.clean(&corrected, job.context) {
            Ok(cleaned) => {
                cleanup = Some((start.elapsed(), cleaner.id()));
                text = cleaned;
            }
            Err(e) => {
                log::error!("Cleanup with {} failed: {e:#}", cleaner.id());
                shared.notice("Cleanup failed, pasted raw text");
            }
        }
    }
    let timing = Timing { speech, cleanup };
    log::info!("{} [{}]", timing.summary(), job.context.as_str());
    shared.emit(Event::Timing(timing));
    if text.is_empty() {
        return;
    }

    match murmur_platform::paste(&text) {
        Ok(PasteOutcome::Pasted) => {}
        Ok(PasteOutcome::CopiedOnly) => shared.notice(COPIED_ONLY),
        Err(e) => {
            log::error!("Paste failed: {e:#}");
            shared.notice("Couldn't paste the dictation");
            return;
        }
    }
    shared.emit(Event::Dictated(text));
}

/// The Dictionary, re-read every time so edits to `dictionary.json` apply to the next dictation
/// without a restart. A file that can't be read is logged and treated as empty.
pub fn load_dictionary() -> Dictionary {
    let path = Dictionary::default_path();
    Dictionary::load(&path).unwrap_or_else(|e| {
        log::warn!("Can't read {}: {e:#}; using an empty dictionary", path.display());
        Dictionary::default()
    })
}

/// Cleanup is Claude, but there's no Anthropic API key in the environment or the key store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingApiKey;

impl fmt::Display for MissingApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("No Anthropic API key")
    }
}

impl std::error::Error for MissingApiKey {}

/// The cleaner for `mode`, or `None` to paste the transcript as it is.
pub fn cleaner(
    mode: CleanupMode,
    ollama_model: &str,
    vocabulary: Vec<String>,
) -> Result<Option<Box<dyn TextCleaner>>, MissingApiKey> {
    Ok(match mode {
        CleanupMode::Off => None,
        CleanupMode::Claude => {
            let key = murmur_core::api_key::load().ok_or(MissingApiKey)?;
            Some(Box::new(ClaudeCleaner::new(key, vocabulary)))
        }
        CleanupMode::Ollama => Some(Box::new(OllamaCleaner::new(ollama_model.to_string(), vocabulary))),
    })
}
