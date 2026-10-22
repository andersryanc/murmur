//! The dictation engine every front end shares: press → record → release → queue → transcribe →
//! Dictionary replacements → cleanup → paste, plus loading the speech model and the start-up
//! warm-ups. It's one crate so every change to the queue is made once, not once per platform.
//!
//! The front ends only translate. The macOS tray app turns the hotkey into [`Engine::press`] and
//! [`Engine::release`] and engine [`Event`]s into the menu, pill and icon; the Linux daemon turns
//! control-socket commands into the same calls and events into lines for its watchers.
//!
//! An [`Engine`] is owned by one thread (the one that receives key presses or commands), which
//! makes every call. Its own threads (model loading, the dictation worker, the audio callback)
//! report through the `on_event` callback, so events arrive on any thread and the front end
//! forwards them to wherever it keeps its state.

mod model;
mod timing;
mod warmup;
mod worker;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use squawk_audio::Recorder;
use squawk_core::{CleanupMode, Settings, TargetContext};

pub use model::{default_model_dir, load_model};
pub use timing::Timing;
pub use warmup::prepare_local_model;
pub use worker::{MissingApiKey, cleaner, load_dictionary};

use worker::Job;

/// Holds shorter than this (counted from when recording starts, after the hotkey's own 200 ms
/// hold) are treated as accidental taps and never transcribed.
pub const MINIMUM_HOLD: Duration = Duration::from_millis(300);

/// Start/stop sounds play at this volume; they're cues, not alerts.
pub const SOUND_VOLUME: f32 = 0.6;

/// Where the speech model comes from. The defaults are what the Mac app uses; the Linux daemon
/// passes `config.json`'s `model` and `threads` overrides through here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// A model folder to load instead of the default. Never downloaded into: if it's missing
    /// files, loading fails and says so.
    pub model_dir: Option<PathBuf>,
    /// CPU threads for speech-to-text; `None` is `squawk_stt::default_threads()`.
    pub threads: Option<usize>,
}

/// Where loading the speech model has got to. Dictation only starts once it's `Ready`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelState {
    /// Progress for a status line, e.g. "Downloading speech model… 40% of 640 MB".
    Loading(String),
    Ready,
    /// "Speech model failed to load: …"
    Failed(String),
}

/// What the engine tells its front end. Sent from whichever thread it happened on.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The speech model's state changed (including download progress, about twice a second).
    Model(ModelState),
    /// A recording started (true) or ended, whether it was queued, skipped or cancelled.
    Recording(bool),
    /// Dictations waiting to paste, including the one being processed. Sent in the order the
    /// count changed, so the latest one received is the current count.
    Queue(usize),
    /// The microphone level, 0...1, about every 50 ms while recording, from the audio thread.
    /// A few can arrive just before `Recording(true)`, while the microphone is starting.
    Level(f32),
    /// A problem to show the user for a moment, e.g. "Cleanup failed, pasted raw text".
    Notice(String),
    /// A dictation was pasted (or left on the clipboard).
    Dictated(String),
    /// How long the last dictation's stages took.
    Timing(Timing),
    /// Whether an Anthropic API key was found when a dictation needed one.
    ApiKey(bool),
}

/// What [`Engine::press`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    Started,
    AlreadyRecording,
    /// The speech model isn't loaded (yet, or it failed); nothing was recorded.
    NotReady,
    /// The microphone couldn't start. The message has also gone out as a [`Event::Notice`].
    Failed(String),
}

/// What [`Engine::release`] did with the recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// Queued; the number is the queue length including it (1 means it's being processed now).
    Queued(usize),
    /// Too short or no voice in it (a tap of the key), so it was thrown away.
    NoSpeech,
    NotRecording,
}

/// The dictation engine. See the crate docs for how front ends use it.
pub struct Engine {
    shared: Arc<Shared>,
    settings: Settings,
    recording: Option<Recording>,
}

/// What the engine's own threads share with it.
struct Shared {
    state: Mutex<State>,
    on_event: Box<dyn Fn(Event) + Send + Sync>,
}

struct State {
    model: ModelState,
    /// The worker's queue; `None` until the speech model has loaded.
    jobs: Option<Sender<Job>>,
    queued: usize,
}

impl Shared {
    fn emit(&self, event: Event) {
        (self.on_event)(event);
    }

    fn notice(&self, message: impl Into<String>) {
        self.emit(Event::Notice(message.into()));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A recording in progress, and what was in front when it started: that's where the text will
/// go, so its tone is fixed then rather than when the key comes up.
struct Recording {
    recorder: Recorder,
    started: Instant,
    context: TargetContext,
}

impl Engine {
    /// Starts loading (and if need be downloading) the speech model, warms up the microphone,
    /// and loads the Ollama model if that's the cleanup, all on their own threads. Returns at
    /// once; [`Event::Model`] says when dictation is ready.
    ///
    /// `on_event` is called from the engine's threads and the audio thread. It must return
    /// quickly and must not call back into the engine: forward the event and return.
    pub fn start(settings: Settings, options: Options, on_event: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let engine = Self::new(settings, ModelState::Loading("Starting…".into()), on_event);
        model::load_in_background(engine.shared.clone(), options);
        warmup::warm_up_audio(engine.settings.microphone.clone());
        if engine.settings.cleanup == CleanupMode::Ollama {
            prepare_local_model(engine.settings.ollama_model.clone());
        }
        engine
    }

    /// An engine that loads nothing and never becomes ready, for front ends that run without
    /// dictation (the Mac app's pill demo and Dictionary-only modes). `status` is its model
    /// state's message.
    pub fn idle(settings: Settings, status: &str) -> Self {
        Self::new(settings, ModelState::Loading(status.into()), |_| {})
    }

    fn new(settings: Settings, model: ModelState, on_event: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    model,
                    jobs: None,
                    queued: 0,
                }),
                on_event: Box::new(on_event),
            }),
            settings,
            recording: None,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Takes effect from the next recording; dictations already queued keep the cleanup they
    /// were spoken with.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn model_state(&self) -> ModelState {
        self.shared.lock().model.clone()
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Dictations waiting to paste, including the one being processed.
    pub fn queued(&self) -> usize {
        self.shared.lock().queued
    }

    /// Starts recording. Blocks until audio is arriving (tens of ms once warmed up), so nothing
    /// said right after the start sound is lost. `context` is the kind of app in front now,
    /// which is where the text will go.
    pub fn press(&mut self, context: TargetContext) -> Press {
        if self.recording.is_some() {
            return Press::AlreadyRecording;
        }
        if self.model_state() != ModelState::Ready {
            return Press::NotReady;
        }
        let shared = self.shared.clone();
        let on_level = move |level| shared.emit(Event::Level(level));
        let recorder = match Recorder::start(self.settings.microphone.as_deref(), on_level) {
            Ok(recorder) => recorder,
            Err(e) => {
                log::error!("Couldn't start the microphone: {e:#}");
                let message = format!("Couldn't start the microphone: {e}");
                self.shared.notice(message.clone());
                return Press::Failed(message);
            }
        };
        self.recording = Some(Recording {
            recorder,
            started: Instant::now(),
            context,
        });
        self.shared.emit(Event::Recording(true));
        if let Some(theme) = self.settings.sounds {
            squawk_audio::play_wav(theme.start_sound(), SOUND_VOLUME);
        }
        Press::Started
    }

    /// Stops recording and queues the dictation, unless it was a tap with no speech in it.
    pub fn release(&mut self) -> Release {
        let Some(recording) = self.recording.take() else {
            return Release::NotRecording;
        };
        let held = recording.started.elapsed();
        self.end_chime();
        let samples = recording.recorder.stop();

        // Too short, or no voice in it (a tap of the key): speech models invent words from
        // silence, so don't transcribe at all.
        let outcome = if held < MINIMUM_HOLD || !squawk_core::voice::contains_speech(&samples) {
            log::info!("Skipped a {} ms recording with no speech", held.as_millis());
            Release::NoSpeech
        } else {
            self.queue(Job {
                samples,
                context: recording.context,
                cleanup: self.settings.cleanup,
                ollama_model: self.settings.ollama_model.clone(),
            })
        };
        // After the queue count, so a watcher never sees "ready" between recording and
        // processing.
        self.shared.emit(Event::Recording(false));
        outcome
    }

    fn queue(&self, job: Job) -> Release {
        let mut state = self.shared.lock();
        let Some(jobs) = &state.jobs else {
            return Release::NoSpeech;
        };
        if jobs.send(job).is_err() {
            log::error!("The dictation thread has stopped");
            return Release::NoSpeech;
        }
        state.queued += 1;
        // Sent under the lock, so counts from this thread and the worker arrive in order.
        self.shared.emit(Event::Queue(state.queued));
        Release::Queued(state.queued)
    }

    /// Throws the recording away (another key was typed while the hotkey was held, or the
    /// user asked). Returns whether there was one; the front end logs why.
    pub fn cancel(&mut self) -> bool {
        let Some(recording) = self.recording.take() else {
            return false;
        };
        self.end_chime();
        drop(recording.recorder.stop());
        self.shared.emit(Event::Recording(false));
        true
    }

    /// Recording just ended, whether it will be transcribed or thrown away.
    fn end_chime(&self) {
        if let Some(theme) = self.settings.sounds {
            squawk_audio::play_wav(theme.stop_sound(), SOUND_VOLUME);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn engine_can_move_to_the_thread_that_drives_it() {
        assert_send::<Engine>();
    }

    #[test]
    fn idle_engine_never_records() {
        let mut engine = Engine::idle(Settings::default(), "Pill demo");
        assert_eq!(engine.model_state(), ModelState::Loading("Pill demo".into()));
        assert_eq!(engine.press(TargetContext::Plain), Press::NotReady);
        assert!(!engine.is_recording());
        assert_eq!(engine.release(), Release::NotRecording);
        assert!(!engine.cancel());
        assert_eq!(engine.queued(), 0);
    }
}
