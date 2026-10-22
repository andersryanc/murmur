//! The app's state and what it does with each event: hotkey presses into the dictation engine,
//! engine events into the pill, icon and menu, and the menu's choices. The dictation loop itself
//! is in `murmur-engine`. Lives on the main thread; see
//! `state.rs` for how other threads reach it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use murmur_core::{CleanupMode, Settings, SoundTheme};
use murmur_engine::{Engine, ModelState, Press, SOUND_VOLUME, Timing};
use murmur_platform::{HotkeyEvent, HotkeyMonitor};
use tao::event::WindowEvent;
use tao::window::WindowId;
use tray_icon::menu::MenuId;
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::tray::background;
use crate::tray::demo::{self, Demo};
use crate::tray::dictionary_window::{self, DictionaryWindow};
use crate::tray::icon;
use crate::tray::login_item;
use crate::tray::menu::{Action, BuiltMenu, MenuView, RECENT_SHOWN};
use crate::tray::pill::{self, Pill};
use crate::tray::state::{AppEvent, Proxy, Target};
use crate::tray::system::{self, MicAccess};

/// How often to re-check permissions, which are granted in System Settings while we run.
pub const TICK: Duration = Duration::from_secs(1);

/// How many dictations to remember for "Recent".
const HISTORY: usize = 10;

pub struct App {
    proxy: Proxy,
    /// What the menu shows and saves. The engine gets a copy whenever it changes.
    settings: Settings,
    /// Recording, the queue and the speech model.
    engine: Engine,
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

        // Engine events come from its threads and the audio thread; the loop handles them.
        let engine = {
            let proxy = Mutex::new(proxy.clone());
            Engine::start(settings.clone(), Default::default(), move |event| {
                if let Ok(proxy) = proxy.lock() {
                    let _ = proxy.send_event(AppEvent::Engine(event));
                }
            })
        };
        background::check_api_key(proxy.clone());
        background::refresh_local_models(proxy.clone());
        background::watch_devices(proxy.clone());

        let mut app = Self {
            settings,
            engine,
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
        let settings = Settings::load();
        Self {
            engine: Engine::idle(settings.clone(), status),
            settings,
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
            AppEvent::Engine(event) => self.engine_event(event),
            AppEvent::ApiKey(found) => self.has_api_key = Some(found),
            AppEvent::LocalModels(models) => self.local_models = models,
            AppEvent::Devices { devices, default } => {
                self.devices = devices;
                self.default_device = default;
            }
            AppEvent::Dictionary(message) => {
                self.dictionary.message(&message);
                self.quit_if_dictionary_closed();
            }
        }
    }

    /// The engine's state (recording, queue, model) is read from it directly when drawing, so
    /// most events only need to wake the loop; these are the ones that carry something to keep.
    fn engine_event(&mut self, event: murmur_engine::Event) {
        use murmur_engine::Event;
        match event {
            Event::Level(level) => {
                // Readings can still be in flight just after the recording stops.
                if self.engine.is_recording() {
                    self.pill.push_level(level);
                }
            }
            Event::Notice(message) => self.flash(&message),
            Event::Model(ModelState::Ready) => {
                log::info!("Ready: hold {} to dictate", self.settings.hotkey.label());
            }
            Event::ApiKey(found) => self.has_api_key = Some(found),
            Event::Dictated(text) => {
                self.history.insert(0, text);
                self.history.truncate(HISTORY);
            }
            Event::Timing(timing) => self.timing = Some(timing),
            Event::Model(_) | Event::Recording(_) | Event::Queue(_) => {}
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
        if self.engine.model_state() != ModelState::Ready || self.engine.is_recording() {
            return;
        }
        if self.microphone_access == MicAccess::Denied {
            self.flash("Murmur needs microphone access");
            return;
        }
        // Note the target first: it's whatever was in front when the key went down. The engine
        // then blocks until audio is arriving, so nothing said after the start sound is lost.
        let context = murmur_platform::target_context();
        if self.engine.press(context) == Press::Started {
            self.pill.start_recording();
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
        if self.engine.is_recording() {
            Some(pill::Mode::Recording)
        } else if self.pill.has_notice(now) {
            Some(pill::Mode::Notice)
        } else if self.engine.queued() > 0 {
            Some(pill::Mode::Processing)
        } else {
            None
        }
    }

    /// Another key was typed while the hotkey was held (a shortcut, not dictation).
    fn cancel_recording(&mut self) {
        if self.engine.cancel() {
            log::info!("Recording cancelled by a chord");
        }
    }

    fn finish_recording(&mut self) {
        self.engine.release();
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
                        murmur_engine::prepare_local_model(self.settings.ollama_model.clone());
                    }
                    CleanupMode::Claude => background::check_api_key(self.proxy.clone()),
                    CleanupMode::Off => {}
                }
                self.save();
            }
            Action::LocalModel(name) => {
                self.settings.ollama_model = name.clone();
                murmur_engine::prepare_local_model(name);
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

    /// Hands the new settings to the engine (they apply from the next recording) and saves them.
    fn save(&mut self) {
        self.engine.set_settings(self.settings.clone());
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
        if self.engine.is_recording() {
            return "Listening…".into();
        }
        let queued = self.engine.queued();
        if queued > 1 {
            return format!("Transcribing… ({} queued)", queued - 1);
        }
        if queued == 1 {
            return "Transcribing…".into();
        }
        match self.engine.model_state() {
            ModelState::Loading(message) | ModelState::Failed(message) => message,
            ModelState::Ready => format!("Hold {} to dictate", self.settings.hotkey.label()),
        }
    }

    /// Brings the pill, tray icon and menu up to date after any event. Cheap when nothing
    /// changed.
    pub fn refresh(&mut self) {
        let now = Instant::now();
        let mode = self.pill_mode(now);
        self.pill.update(mode, now);

        let Some(tray) = &self.tray else { return };

        let recording = self.engine.is_recording();
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
        self.tray = None;
    }
}
