//! The app's state and what it does with each event: the push-to-talk loop (key down → record →
//! key up → queue), the menu's choices, and keeping the tray icon and menu current. A port of
//! the Swift `AppModel`. Lives on the main thread; see `state.rs` for how other threads reach it.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use murmur_audio::Recorder;
use murmur_core::{CleanupMode, Settings, SoundTheme};
use murmur_platform::{HotkeyEvent, HotkeyMonitor};
use tao::event::WindowEvent;
use tao::window::WindowId;
use tray_icon::menu::MenuId;
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::background;
use crate::demo::{self, Demo};
use crate::dictionary_window::{self, DictionaryWindow};
use crate::icon;
use crate::login_item;
use crate::menu::{Action, BuiltMenu, MenuView, RECENT_SHOWN};
use crate::pill::{self, Pill};
use crate::pipeline::{self, Job, MINIMUM_HOLD, Recording};
use crate::state::{AppEvent, Phase, Proxy, Target, Timing};
use crate::system::{self, MicAccess};

/// How often to re-check permissions, which are granted in System Settings while we run.
pub const TICK: Duration = Duration::from_secs(1);

/// How many dictations to remember for "Recent".
const HISTORY: usize = 10;

/// Start/stop sounds play at this volume; they're cues, not alerts.
const SOUND_VOLUME: f32 = 0.6;

pub struct App {
    proxy: Proxy,
    settings: Settings,
    phase: Phase,
    /// The recording in progress while the key is held.
    recording: Option<Recording>,
    /// Dictations handed to the worker and not finished yet.
    queued: usize,
    /// The worker's queue; `None` until the speech model has loaded.
    jobs: Option<Sender<Job>>,
    /// Pasted dictations, newest first.
    history: Vec<String>,
    timing: Option<Timing>,
    accessibility: bool,
    microphone_access: MicAccess,
    /// `None` until the key store has been checked, so the warning doesn't flash at launch.
    has_api_key: Option<bool>,
    local_models: Vec<String>,
    devices: Vec<String>,
    default_device: Option<String>,
    hotkeys: Option<HotkeyMonitor>,
    tray: Option<TrayIcon>,
    menu: Option<(MenuView, BuiltMenu)>,
    shown_status: String,
    shows_recording: bool,
    pill: Pill,
    /// Set when running the pill demo instead of the app.
    demo: Option<Demo>,
    dictionary: DictionaryWindow,
    /// Running only the Dictionary window (`MURMUR_OPEN_DICTIONARY=1`): quit when it closes.
    dictionary_only: bool,
    /// What the system says; `None` where there's no such thing.
    launch_at_login: Option<login_item::Status>,
    next_tick: Instant,
    pub quitting: bool,
}

impl App {
    /// Called once the event loop is running: on macOS the tray must be created then, on the
    /// main thread.
    pub fn start(proxy: Proxy, target: &Target) -> Self {
        if demo::requested() {
            return Self::demo(proxy);
        }
        if dictionary_window::requested() {
            return Self::dictionary_only(proxy, target);
        }
        let settings = Settings::load();
        log::info!(
            "Settings: hotkey {:?}, cleanup {:?} ({}), microphone {:?}, sounds {:?}",
            settings.hotkey,
            settings.cleanup,
            settings.ollama_model,
            settings.microphone,
            settings.sounds
        );

        let accessibility = murmur_platform::has_input_permission(false);
        if !accessibility {
            log::info!("Accessibility not granted yet; asking");
            murmur_platform::has_input_permission(true);
        }

        let hotkeys = {
            let proxy = proxy.clone();
            // The tap thread only forwards; macOS switches off a tap that stalls.
            match HotkeyMonitor::start(settings.hotkey, move |event| {
                let _ = proxy.send_event(AppEvent::Hotkey(event));
            }) {
                Ok(monitor) => {
                    if monitor.is_listening() && accessibility {
                        log::info!("Hotkey tap installed for {}", settings.hotkey.label());
                    } else {
                        log::info!("Hotkey waiting for Accessibility permission");
                    }
                    Some(monitor)
                }
                Err(e) => {
                    log::error!("Couldn't start the hotkey: {e:#}");
                    None
                }
            }
        };

        background::load_model(proxy.clone());
        background::warm_up_audio(settings.microphone.clone());
        background::check_api_key(proxy.clone());
        background::refresh_local_models(proxy.clone());
        background::watch_devices(proxy.clone());
        if settings.cleanup == CleanupMode::Ollama {
            background::prepare_local_model(settings.ollama_model.clone());
        }

        let mut app = Self {
            settings,
            phase: Phase::Loading("Starting…".into()),
            recording: None,
            queued: 0,
            jobs: None,
            history: Vec::new(),
            timing: None,
            accessibility,
            microphone_access: system::microphone_access(),
            has_api_key: None,
            local_models: Vec::new(),
            devices: Vec::new(),
            default_device: None,
            hotkeys,
            tray: None,
            menu: None,
            shown_status: String::new(),
            shows_recording: false,
            pill: Pill::new(),
            demo: None,
            dictionary: DictionaryWindow::new(proxy.clone()),
            dictionary_only: false,
            launch_at_login: login_item::status(),
            next_tick: Instant::now() + TICK,
            quitting: false,
            proxy,
        };
        app.create_tray();
        app
    }

    /// Just the pill, run through its states (see `demo.rs`). No hotkey tap, tray, microphone
    /// or model, so it can run beside a real Murmur without interfering.
    fn demo(proxy: Proxy) -> Self {
        Self {
            demo: Some(Demo::new()),
            ..Self::idle(proxy, "Pill demo")
        }
    }

    /// Just the Dictionary window, with no hotkey tap, tray or model, like the pill demo.
    fn dictionary_only(proxy: Proxy, target: &Target) -> Self {
        log::info!("Dictionary window only; quits when it closes");
        let mut app = Self {
            dictionary_only: true,
            ..Self::idle(proxy, "Dictionary")
        };
        app.dictionary.show(target);
        app
    }

    /// An app that does nothing by itself, for the modes above to build on.
    fn idle(proxy: Proxy, status: &str) -> Self {
        Self {
            settings: Settings::load(),
            phase: Phase::Loading(status.into()),
            recording: None,
            queued: 0,
            jobs: None,
            history: Vec::new(),
            timing: None,
            accessibility: true,
            microphone_access: MicAccess::Granted,
            has_api_key: None,
            local_models: Vec::new(),
            devices: Vec::new(),
            default_device: None,
            hotkeys: None,
            tray: None,
            menu: None,
            shown_status: String::new(),
            shows_recording: false,
            pill: Pill::new(),
            demo: None,
            dictionary: DictionaryWindow::new(proxy.clone()),
            dictionary_only: false,
            launch_at_login: None,
            next_tick: Instant::now() + TICK,
            quitting: false,
            proxy,
        }
    }

    fn create_tray(&mut self) {
        let view = self.view();
        let status = self.status_text();
        let built = BuiltMenu::build(&view, &status);
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(built.menu.clone()))
            .with_tooltip("Murmur");
        let tray = icon::with_icon(builder, icon::idle()).build();
        match tray {
            Ok(tray) => {
                log::info!("Tray icon created");
                self.tray = Some(tray);
            }
            Err(e) => log::error!("Couldn't create the tray icon: {e}"),
        }
        self.menu = Some((view, built));
        self.shown_status = status;
    }

    // MARK: - Events

    pub fn handle(&mut self, event: AppEvent, target: &Target) {
        match event {
            AppEvent::Hotkey(HotkeyEvent::Pressed) => self.start_recording(),
            AppEvent::Hotkey(HotkeyEvent::Released) => self.finish_recording(),
            AppEvent::Hotkey(HotkeyEvent::Cancelled) => self.cancel_recording(),
            AppEvent::Menu(id) => self.menu_clicked(&id, target),
            AppEvent::Level(level) => {
                // Readings can still be in flight just after the recording stops.
                if self.recording.is_some() {
                    self.pill.push_level(level);
                }
            }
            AppEvent::Notice(message) => self.flash(&message),
            AppEvent::Loading(message) => {
                if matches!(self.phase, Phase::Loading(_)) {
                    self.phase = Phase::Loading(message);
                }
            }
            AppEvent::ModelReady(transcriber) => {
                self.jobs = Some(pipeline::spawn_worker(transcriber, self.proxy.clone()));
                self.phase = Phase::Ready;
                log::info!("Ready: hold {} to dictate", self.settings.hotkey.label());
            }
            AppEvent::ModelFailed(message) => self.phase = Phase::Failed(message),
            AppEvent::ApiKey(found) => self.has_api_key = Some(found),
            AppEvent::LocalModels(models) => self.local_models = models,
            AppEvent::Devices { devices, default } => {
                self.devices = devices;
                self.default_device = default;
            }
            AppEvent::Dictated(text) => {
                self.history.insert(0, text);
                self.history.truncate(HISTORY);
            }
            AppEvent::Timing(timing) => self.timing = Some(timing),
            AppEvent::JobDone => self.queued = self.queued.saturating_sub(1),
            AppEvent::Dictionary(message) => {
                self.dictionary.message(&message);
                self.quit_if_dictionary_closed();
            }
        }
    }

    /// An event for one of our tao windows (only the Dictionary window is one).
    pub fn window_event(&mut self, id: WindowId, event: &WindowEvent) {
        self.dictionary.window_event(id, event);
        self.quit_if_dictionary_closed();
    }

    fn quit_if_dictionary_closed(&mut self) {
        if self.dictionary_only && !self.dictionary.is_open() {
            self.quitting = true;
        }
    }

    /// When the event loop should next wake: the permission poll, or the pill's next frame
    /// while it's on screen.
    pub fn wake_at(&self) -> Instant {
        match self.pill.next_frame() {
            Some(frame) => frame.min(self.next_tick),
            None => self.next_tick,
        }
    }

    /// The loop woke at `wake_at`. Pill frames are drawn by `refresh`, which runs next.
    pub fn timer_fired(&mut self) {
        if Instant::now() >= self.next_tick {
            self.tick();
        }
    }

    /// Permissions are granted in System Settings while the app runs, so poll them. The hotkey
    /// monitor makes its own tap once Accessibility is granted.
    fn tick(&mut self) {
        self.next_tick = Instant::now() + TICK;
        let accessibility = murmur_platform::has_input_permission(false);
        if accessibility != self.accessibility {
            log::info!("Accessibility {}", if accessibility { "granted" } else { "revoked" });
            self.accessibility = accessibility;
        }
        self.microphone_access = system::microphone_access();
        if self.launch_at_login.is_some() {
            // Can be switched off in System Settings while we run.
            self.launch_at_login = login_item::status();
        }
        self.dictionary.tick();
    }

    // MARK: - Push to talk

    fn start_recording(&mut self) {
        if self.phase != Phase::Ready || self.recording.is_some() {
            return;
        }
        if self.microphone_access == MicAccess::Denied {
            self.flash("Murmur needs microphone access");
            return;
        }
        // Note the target first: it's whatever was in front when the key went down.
        let context = murmur_platform::target_context();
        // Blocks until audio is arriving (tens of ms once warmed up), so nothing said right
        // after the start sound is lost. The Swift app also started capture on the main thread.
        // Levels arrive on the audio thread; the pill lives on this one.
        let proxy = self.proxy.clone();
        let on_level = move |level| {
            let _ = proxy.send_event(AppEvent::Level(level));
        };
        let recorder = match Recorder::start(self.settings.microphone.as_deref(), on_level) {
            Ok(recorder) => recorder,
            Err(e) => {
                log::error!("Couldn't start the microphone: {e:#}");
                self.flash(&format!("Couldn't start the microphone: {e}"));
                return;
            }
        };
        self.pill.start_recording();
        self.recording = Some(Recording {
            recorder,
            started: Instant::now(),
            context,
        });
        if let Some(theme) = self.settings.sounds {
            murmur_audio::play_wav(theme.start_sound(), SOUND_VOLUME);
        }
    }

    /// Shows a short message in the pill, or as a system notification where there's no pill.
    fn flash(&mut self, message: &str) {
        if self.pill.available() {
            log::info!("Notice: {message}");
            self.pill.flash(message, Instant::now());
        } else {
            murmur_platform::notify(message);
        }
    }

    /// What the pill should show: recording wins, then a notice, then queued dictations.
    fn pill_mode(&mut self, now: Instant) -> Option<pill::Mode> {
        if let Some(demo) = &mut self.demo {
            return match demo.drive(&mut self.pill, now) {
                Ok(mode) => mode,
                Err(()) => {
                    log::info!("Pill demo finished");
                    self.quitting = true;
                    None
                }
            };
        }
        if self.recording.is_some() {
            Some(pill::Mode::Recording)
        } else if self.pill.has_notice(now) {
            Some(pill::Mode::Notice)
        } else if self.queued > 0 {
            Some(pill::Mode::Processing)
        } else {
            None
        }
    }

    /// Recording just ended, whether it will be transcribed or thrown away.
    fn end_chime(&self) {
        if let Some(theme) = self.settings.sounds {
            murmur_audio::play_wav(theme.stop_sound(), SOUND_VOLUME);
        }
    }

    /// Another key was typed while the hotkey was held (a shortcut, not dictation).
    fn cancel_recording(&mut self) {
        let Some(recording) = self.recording.take() else { return };
        self.end_chime();
        drop(recording.recorder.stop());
        log::info!("Recording cancelled by a chord");
    }

    fn finish_recording(&mut self) {
        let Some(recording) = self.recording.take() else { return };
        let held = recording.started.elapsed();
        self.end_chime();
        let samples = recording.recorder.stop();

        // Too short, or no voice in it (a tap of the key): speech models invent words from
        // silence, so don't transcribe at all.
        if held < MINIMUM_HOLD || !murmur_core::voice::contains_speech(&samples) {
            log::info!("Skipped a {} ms recording with no speech", held.as_millis());
            return;
        }
        let Some(jobs) = &self.jobs else { return };
        let job = Job {
            samples,
            context: recording.context,
            cleanup: self.settings.cleanup,
            ollama_model: self.settings.ollama_model.clone(),
        };
        if jobs.send(job).is_ok() {
            self.queued += 1;
        } else {
            log::error!("The dictation thread has stopped");
        }
    }

    // MARK: - Menu

    fn menu_clicked(&mut self, id: &MenuId, target: &Target) {
        let Some(action) = self.menu.as_ref().and_then(|(_, built)| built.action(id)) else {
            return;
        };
        match action {
            Action::OpenAccessibilitySettings => system::open_accessibility_settings(),
            Action::OpenMicrophoneSettings => system::open_microphone_settings(),
            Action::RecheckApiKey => background::check_api_key(self.proxy.clone()),
            Action::CopyRecent(i) => {
                if let Some(text) = self.history.get(i) {
                    system::copy(text);
                }
            }
            Action::Hotkey(key) => {
                self.settings.hotkey = key;
                if let Some(monitor) = &self.hotkeys {
                    monitor.set_key(key);
                }
                self.save();
            }
            Action::Cleanup(mode) => {
                self.settings.cleanup = mode;
                match mode {
                    CleanupMode::Ollama => {
                        background::refresh_local_models(self.proxy.clone());
                        background::prepare_local_model(self.settings.ollama_model.clone());
                    }
                    CleanupMode::Claude => background::check_api_key(self.proxy.clone()),
                    CleanupMode::Off => {}
                }
                self.save();
            }
            Action::LocalModel(name) => {
                self.settings.ollama_model = name.clone();
                background::prepare_local_model(name);
                self.save();
            }
            Action::RefreshLocalModels => background::refresh_local_models(self.proxy.clone()),
            Action::Microphone(name) => {
                self.settings.microphone = name;
                self.save();
            }
            Action::Sounds(theme) => {
                self.settings.sounds = theme;
                // Picking a theme plays its start sound as a preview.
                if let Some(theme) = theme {
                    murmur_audio::play_wav(SoundTheme::start_sound(theme), SOUND_VOLUME);
                }
                self.save();
            }
            Action::LaunchAtLogin(on) => self.set_launch_at_login(on),
            Action::Dictionary => self.dictionary.show(target),
            Action::Quit => {
                log::info!("Quitting");
                self.quitting = true;
            }
        }
    }

    fn set_launch_at_login(&mut self, on: bool) {
        match login_item::set(on) {
            Ok(()) => log::info!("Launch at Login {}", if on { "on" } else { "off" }),
            Err(e) => {
                log::error!("Couldn't change Login Items: {e:#}");
                self.flash(&format!("Couldn't change Login Items: {e}"));
            }
        }
        self.launch_at_login = login_item::status();
        if on && self.launch_at_login == Some(login_item::Status::NeedsApproval) {
            self.flash("Allow Murmur in Login Items to finish");
            login_item::open_settings();
        }
        // muda flips the checkmark itself when clicked; rebuild so it shows what the system says.
        self.menu = None;
    }

    fn save(&self) {
        if let Err(e) = self.settings.save() {
            log::error!("Couldn't save settings: {e:#}");
        }
    }

    fn view(&self) -> MenuView {
        MenuView {
            timing: self.timing.as_ref().map(Timing::summary),
            needs_accessibility: !self.accessibility,
            needs_microphone: self.microphone_access == MicAccess::Denied,
            needs_api_key: self.settings.cleanup == CleanupMode::Claude && self.has_api_key == Some(false),
            recent: self.history.iter().take(RECENT_SHOWN).cloned().collect(),
            hotkey: self.settings.hotkey,
            cleanup: self.settings.cleanup,
            ollama_model: self.settings.ollama_model.clone(),
            local_models: self.local_models.clone(),
            microphone: self.settings.microphone.clone(),
            devices: self.devices.clone(),
            default_device: self.default_device.clone(),
            sounds: self.settings.sounds,
            launch_at_login: self.launch_at_login.map(|status| status == login_item::Status::On),
        }
    }

    fn status_text(&self) -> String {
        if self.recording.is_some() {
            return "Listening…".into();
        }
        if self.queued > 1 {
            return format!("Transcribing… ({} queued)", self.queued - 1);
        }
        if self.queued == 1 {
            return "Transcribing…".into();
        }
        match &self.phase {
            Phase::Loading(message) | Phase::Failed(message) => message.clone(),
            Phase::Ready => format!("Hold {} to dictate", self.settings.hotkey.label()),
        }
    }

    /// Brings the pill, tray icon and menu up to date after any event. Cheap when nothing
    /// changed.
    pub fn refresh(&mut self) {
        let now = Instant::now();
        let mode = self.pill_mode(now);
        self.pill.update(mode, now);

        let Some(tray) = &self.tray else { return };

        let recording = self.recording.is_some();
        if recording != self.shows_recording {
            let icon = if recording { icon::recording() } else { icon::idle() };
            icon::set(tray, icon);
            self.shows_recording = recording;
        }

        let view = self.view();
        let status = self.status_text();
        match &self.menu {
            Some((shown, _)) if *shown == view => {
                if status != self.shown_status
                    && let Some((_, built)) = &self.menu
                {
                    built.set_status(&status);
                }
            }
            _ => {
                let built = BuiltMenu::build(&view, &status);
                tray.set_menu(Some(Box::new(built.menu.clone())));
                self.menu = Some((view, built));
            }
        }
        self.shown_status = status;
    }

    pub fn shut_down(&mut self) {
        // Stop listening first so no key event arrives mid-teardown.
        self.hotkeys = None;
        self.recording = None;
        self.tray = None;
    }
}
