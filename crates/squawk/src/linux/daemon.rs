//! `squawkctl daemon`: owns the dictation engine, serves control-socket commands into it, and
//! sends its events to watchers (the Omarchy pill).
//!
//! Three threads matter. The control server's thread owns the [`Engine`] and runs each command
//! on it, so the engine has one owner as it expects. The main thread receives everything else
//! (engine events, notices from commands, and SIGINT/SIGTERM) on one channel and does all the
//! broadcasting, so watchers see events in the order they happened. A small thread waits for
//! the signals, which are blocked everywhere else.

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use squawk_control::{Command, ControlError, Event as Line, Phase, Server};
use squawk_core::{CleanupMode, Settings};
use squawk_engine::{Engine, Event, ModelState, Press, Release, SOUND_VOLUME};

use super::config;
use crate::logging;

/// What the main thread hears about.
enum Message {
    Engine(Event),
    /// A notice from a command (e.g. "still loading"), shown like the engine's.
    Notice(String),
    /// SIGINT or SIGTERM.
    Stop(i32),
}

/// How many dictations `recent` replies with, as the Mac menu lists.
const RECENT_KEPT: usize = 5;

/// The latest dictations, newest first. The main thread adds them; the server thread reads them.
type Recent = Arc<Mutex<VecDeque<String>>>;

pub fn run() -> ExitCode {
    logging::init(logging::Output::FileAndStderr);
    // Before any thread starts, so every thread inherits the mask and only `sigwait` sees them.
    let signals = match block_shutdown_signals() {
        Ok(set) => set,
        Err(e) => {
            log::error!("Couldn't set up signal handling: {e}");
            return ExitCode::FAILURE;
        }
    };
    let path = squawk_control::socket_path();
    // Checked before loading anything, so a second daemon goes away at once.
    if squawk_control::send_to(&path, Command::Status).is_ok() {
        eprintln!("{}", ControlError::AlreadyRunning);
        return ExitCode::FAILURE;
    }
    log::info!(
        "Squawk {} daemon starting (pid {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    let config = config::load();
    let settings = &config.settings;
    log::info!(
        "Settings from {}: cleanup {:?} ({}), microphone {:?}, sounds {:?}, model {}, threads {:?}",
        squawk_core::Settings::path().display(),
        settings.cleanup,
        settings.ollama_model,
        settings.microphone,
        settings.sounds,
        squawk_engine::default_model_dir(&config.options).display(),
        config.options.threads,
    );

    let (tx, rx) = mpsc::channel::<Message>();
    wait_for_signals(signals, tx.clone());
    let mut engine = {
        let tx = tx.clone();
        Engine::start(config.settings, config.options, move |event| {
            let _ = tx.send(Message::Engine(event));
        })
    };
    let recent = Recent::default();
    let server = {
        let tx = tx.clone();
        let recent = recent.clone();
        Server::start(&path, move |command| handle(&mut engine, command, &tx, &recent))
    };
    let server = match server {
        Ok(server) => server,
        Err(e) => {
            eprintln!("{e:#}");
            return ExitCode::FAILURE;
        }
    };
    log::info!("Listening on {}", path.display());

    let mut watchers = Watchers {
        server,
        recording: false,
        queued: 0,
        recent,
    };
    for message in rx {
        match message {
            Message::Engine(event) => watchers.engine_event(event),
            Message::Notice(text) => watchers.notice(&text),
            Message::Stop(signal) => {
                log::info!("Stopping (signal {signal})");
                break;
            }
        }
    }
    // Removes the socket file.
    drop(watchers);
    log::info!("Squawk stopped");
    // Skips the libraries' exit handlers, which can crash while onnxruntime's worker threads
    // are still running.
    // SAFETY: _exit ends the process at once; nothing after it runs.
    unsafe { libc::_exit(0) }
}

/// Runs one command on the engine (on the server thread) and returns the reply line.
fn handle(engine: &mut Engine, command: Command, tx: &Sender<Message>, recent: &Recent) -> String {
    match command {
        Command::Start => start(engine, tx),
        Command::Stop => stop(engine),
        Command::Toggle if engine.is_recording() => stop(engine),
        Command::Toggle => start(engine, tx),
        Command::Cancel => {
            if engine.cancel() {
                log::info!("Recording cancelled");
                "cancelled".into()
            } else {
                "not recording".into()
            }
        }
        Command::Status => match engine.model_state() {
            ModelState::Loading(_) => "loading".into(),
            ModelState::Failed(message) => format!("error: {message}"),
            ModelState::Ready => phase(engine.is_recording(), engine.queued()).to_string(),
        },
        Command::Recent => {
            let recent = recent.lock().unwrap_or_else(|e| e.into_inner());
            serde_json::to_string(&*recent).unwrap_or_else(|_| "[]".into())
        }
        Command::Reload => {
            reload(engine, config::load().settings);
            "ok".into()
        }
    }
}

/// Applies settings changed in the bar menu, doing what the Mac menu does on the same change:
/// load a newly picked local model into Ollama, and play a newly picked sound as a preview.
fn reload(engine: &mut Engine, settings: Settings) {
    let old = engine.settings();
    let new_model = settings.cleanup == CleanupMode::Ollama
        && (old.cleanup != CleanupMode::Ollama || old.ollama_model != settings.ollama_model);
    if new_model {
        squawk_engine::prepare_local_model(settings.ollama_model.clone());
    }
    if settings.sounds != old.sounds
        && let Some(theme) = settings.sounds
    {
        squawk_audio::play_wav(theme.start_sound(), SOUND_VOLUME);
    }
    if settings != *old {
        log::info!(
            "Settings changed: cleanup {:?} ({}), microphone {:?}, sounds {:?}",
            settings.cleanup,
            settings.ollama_model,
            settings.microphone,
            settings.sounds,
        );
    }
    engine.set_settings(settings);
}

fn start(engine: &mut Engine, tx: &Sender<Message>) -> String {
    // Re-read every time, so edits to config.json apply to the next
    // dictation without a restart. (`model` and `threads` only apply when the daemon starts.)
    engine.set_settings(config::load().settings);
    match engine.press(squawk_platform::target_context()) {
        Press::Started => "recording".into(),
        Press::AlreadyRecording => "already recording".into(),
        Press::NotReady => {
            let message = match engine.model_state() {
                ModelState::Loading(status) => format!("Squawk is still starting: {status}"),
                ModelState::Failed(message) => message,
                ModelState::Ready => "Squawk isn't ready".into(),
            };
            let _ = tx.send(Message::Notice(message.clone()));
            format!("error: {message}")
        }
        // The engine has already sent the notice.
        Press::Failed(message) => format!("error: {message}"),
    }
}

fn stop(engine: &mut Engine) -> String {
    match engine.release() {
        Release::Queued(1) => "processing".into(),
        Release::Queued(_) => "queued".into(),
        Release::NoSpeech => "no speech".into(),
        Release::NotRecording => "not recording".into(),
    }
}

fn phase(recording: bool, queued: usize) -> Phase {
    if recording {
        Phase::Recording
    } else if queued > 0 {
        Phase::Processing
    } else {
        Phase::Ready
    }
}

/// The socket's watchers, and what they've been told the daemon is doing.
struct Watchers {
    server: Server,
    recording: bool,
    queued: usize,
    recent: Recent,
}

impl Watchers {
    fn engine_event(&mut self, event: Event) {
        match event {
            Event::Recording(recording) => {
                self.recording = recording;
                self.publish();
            }
            Event::Queue(queued) => {
                self.queued = queued;
                self.publish();
            }
            // Readings can arrive while the microphone starts, before the state says so.
            Event::Level(level) if self.recording => {
                self.server.broadcast(&Line::Level(level));
            }
            Event::Notice(text) => self.notice(&text),
            Event::Model(ModelState::Ready) => log::info!("Ready"),
            Event::Model(ModelState::Failed(message)) => self.notice(&message),
            Event::Dictated(text) => {
                log::info!("Dictated: {text}");
                let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
                recent.push_front(text);
                recent.truncate(RECENT_KEPT);
            }
            Event::Level(_) | Event::Model(ModelState::Loading(_)) | Event::Timing(_) | Event::ApiKey(_) => {}
        }
    }

    /// The queue length goes first, so a watcher already knows it when the state changes.
    fn publish(&self) {
        self.server.broadcast(&Line::Queue(self.queued));
        self.server.broadcast(&Line::State(phase(self.recording, self.queued)));
    }

    /// Shows `text` in the pill when the Omarchy plugin is watching, or as a notification.
    /// Broadcasting first drops watchers that have gone away (e.g. the shell restarted).
    fn notice(&self, text: &str) {
        log::info!("Notice: {text}");
        if self.server.broadcast(&Line::Notice(text.to_string())) == 0 {
            squawk_platform::notify(text);
        }
    }
}

fn shutdown_signals() -> libc::sigset_t {
    // SAFETY: sigemptyset initialises the set before sigaddset touches it.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        set
    }
}

/// Blocks SIGINT and SIGTERM on this thread and so every thread started after it, so they
/// stay pending for [`wait_for_signals`] instead of killing the process mid-write.
fn block_shutdown_signals() -> std::io::Result<libc::sigset_t> {
    let set = shutdown_signals();
    // SAFETY: `set` is a valid, initialised signal set.
    let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
    if result == 0 {
        Ok(set)
    } else {
        Err(std::io::Error::from_raw_os_error(result))
    }
}

/// Waits for SIGINT or SIGTERM on its own thread and tells the main thread.
fn wait_for_signals(set: libc::sigset_t, tx: Sender<Message>) {
    let spawned = std::thread::Builder::new()
        .name("squawk-signals".into())
        .spawn(move || {
            let mut signal: libc::c_int = 0;
            // SAFETY: `set` is a valid signal set, blocked on every thread, and `signal` outlives
            // the call.
            let result = unsafe { libc::sigwait(&set, &mut signal) };
            let _ = tx.send(Message::Stop(if result == 0 { signal } else { libc::SIGTERM }));
        });
    if let Err(e) = spawned {
        log::error!("Couldn't start the signal thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_follows_recording_then_the_queue() {
        assert_eq!(phase(true, 2), Phase::Recording);
        assert_eq!(phase(false, 1), Phase::Processing);
        assert_eq!(phase(false, 0), Phase::Ready);
    }

    #[test]
    fn idle_engine_answers_commands() {
        let (tx, rx) = mpsc::channel();
        let recent = Recent::default();
        let mut engine = Engine::idle(Settings::default(), "Downloading speech model…");
        assert_eq!(handle(&mut engine, Command::Status, &tx, &recent), "loading");
        assert_eq!(handle(&mut engine, Command::Stop, &tx, &recent), "not recording");
        assert_eq!(handle(&mut engine, Command::Cancel, &tx, &recent), "not recording");
        assert_eq!(handle(&mut engine, Command::Recent, &tx, &recent), "[]");
        recent.lock().unwrap().push_front("two\nlines".into());
        assert_eq!(handle(&mut engine, Command::Recent, &tx, &recent), r#"["two\nlines"]"#);
        let reply = handle(&mut engine, Command::Toggle, &tx, &recent);
        assert!(reply.starts_with("error: Squawk is still starting"), "{reply}");
        assert!(matches!(rx.try_recv(), Ok(Message::Notice(_))));
    }
}
