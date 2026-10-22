//! The Dictionary window on macOS and Windows: `dictionary.html` in a wry web view, in a tao
//! window on the app's event loop.
//!
//! The page and the app talk in JSON. The page posts `ready`, `save` (the whole dictionary after
//! each change), `reveal`, `close` and `error` through `window.ipc`; they arrive here as
//! `AppEvent::Dictionary`. The app answers with `squawk.load(…)` and `squawk.saved(…)`.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde_json::json;
use squawk_core::Dictionary;
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::WindowEvent;
use tao::window::{Window, WindowBuilder, WindowId};
use wry::{WebView, WebViewBuilder};

use crate::tray::state::{AppEvent, Proxy, Target};
use crate::tray::system;

const PAGE: &str = include_str!("dictionary.html");

/// The window stays hidden until the page has drawn, so it never flashes an empty frame. If the
/// page somehow never says it's ready, show it anyway after this long.
const READY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Message {
    Ready,
    Save {
        dictionary: Dictionary,
    },
    Reveal,
    Close,
    /// A script error in the page, for the log.
    Error {
        message: String,
    },
}

pub struct DictionaryWindow {
    proxy: Proxy,
    open: Option<Open>,
    /// The Edit menu that gives the page copy, paste and undo; installed on first open.
    #[cfg(target_os = "macos")]
    edit_menu: Option<tray_icon::menu::Menu>,
}

/// An open window. The web view is declared first so it's dropped before its window.
struct Open {
    webview: WebView,
    window: Window,
    /// The file's modification time when it was last read or written here. Any other time
    /// means someone else wrote it (a text editor, a sync service), so it's read again.
    seen: Option<SystemTime>,
    /// The file on disk couldn't be parsed. Saving would overwrite the user's half-done hand
    /// edit, so saves are refused until it reads cleanly again.
    unreadable: bool,
    opened: Instant,
    shown: bool,
    /// The app that was in front before the window opened; it gets the focus back on close.
    #[cfg(target_os = "macos")]
    previous_app: Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>,
}

impl DictionaryWindow {
    pub fn new(proxy: Proxy) -> Self {
        Self {
            proxy,
            open: None,
            #[cfg(target_os = "macos")]
            edit_menu: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens the window, or brings it forward if it's already open.
    pub fn show(&mut self, target: &Target) {
        if let Some(open) = &self.open {
            open.bring_forward();
            return;
        }
        #[cfg(target_os = "macos")]
        if self.edit_menu.is_none() {
            self.edit_menu = Some(macos::install_edit_menu());
        }
        match self.create(target) {
            Ok(open) => {
                log::info!("Dictionary window opened for {}", super::path().display());
                self.open = Some(open);
            }
            Err(e) => {
                log::error!("Couldn't open the Dictionary window: {e:#}");
                squawk_platform::notify("Couldn't open the Dictionary window");
            }
        }
    }

    fn create(&self, target: &Target) -> anyhow::Result<Open> {
        #[cfg(target_os = "macos")]
        let previous_app = macos::frontmost_other_app();

        let window = WindowBuilder::new()
            .with_title("Dictionary")
            .with_inner_size(LogicalSize::new(560.0, 520.0))
            .with_min_inner_size(LogicalSize::new(440.0, 360.0))
            .with_visible(false)
            .build(target)?;
        center(&window);

        let proxy = self.proxy.clone();
        let webview = WebViewBuilder::new()
            .with_html(PAGE)
            // The page paints no background, so the native window colour shows through and
            // follows light and dark mode like any other window.
            .with_transparent(true)
            .with_ipc_handler(move |request| {
                let _ = proxy.send_event(AppEvent::Dictionary(request.into_body()));
            })
            .build(&window)?;

        Ok(Open {
            webview,
            window,
            seen: None,
            unreadable: false,
            opened: Instant::now(),
            shown: false,
            #[cfg(target_os = "macos")]
            previous_app,
        })
    }

    /// A message from the page.
    pub fn message(&mut self, body: &str) {
        let Some(open) = &mut self.open else { return };
        let message = match serde_json::from_str::<Message>(body) {
            Ok(message) => message,
            Err(e) => {
                log::warn!("Dictionary window: unexpected message {body:?}: {e}");
                return;
            }
        };
        match message {
            Message::Ready => {
                open.load();
                open.show();
                if let Ok(script) = std::env::var("SQUAWK_DICTIONARY_SCRIPT") {
                    open.run(&script);
                }
            }
            Message::Save { dictionary } => open.save(&dictionary),
            Message::Reveal => reveal(&super::path()),
            Message::Close => self.close(),
            Message::Error { message } => log::error!("Dictionary window script error: {message}"),
        }
    }

    pub fn window_event(&mut self, id: WindowId, event: &WindowEvent) {
        let Some(open) = &mut self.open else { return };
        if open.window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.close(),
            WindowEvent::Focused(true) => {
                // Clicking back into the window is when a hand edit is most likely to be new.
                open.reload_if_changed();
                let _ = open.webview.focus();
            }
            _ => {}
        }
    }

    /// Called about once a second: picks up edits made to the file by hand while the window is
    /// open, so the window never saves over them.
    pub fn tick(&mut self) {
        let Some(open) = &mut self.open else { return };
        if !open.shown && open.opened.elapsed() > READY_TIMEOUT {
            log::warn!("Dictionary page didn't report ready; showing the window anyway");
            open.show();
        }
        open.reload_if_changed();
    }

    fn close(&mut self) {
        let Some(open) = self.open.take() else { return };
        open.window.set_visible(false);
        // A menu-bar app has no other window to hand the focus to, so without this the keyboard
        // goes nowhere until the user clicks something.
        #[cfg(target_os = "macos")]
        if let Some(app) = &open.previous_app {
            macos::give_focus_back(app);
        }
        drop(open);
        log::info!("Dictionary window closed");
    }
}

impl Open {
    /// Shows the window once the page is ready and makes it the key window, activating the app
    /// (a menu-bar app is otherwise never active, so the window would open behind others).
    fn show(&mut self) {
        self.shown = true;
        self.bring_forward();
    }

    fn bring_forward(&self) {
        self.window.set_visible(true);
        self.window.set_minimized(false);
        self.window.set_focus();
        let _ = self.webview.focus();
    }

    fn run(&self, script: &str) {
        if let Err(e) = self.webview.evaluate_script(script) {
            log::warn!("Dictionary window: couldn't run a script: {e}");
        }
    }

    /// Reads the file and hands it to the page.
    fn load(&mut self) {
        let path = super::path();
        self.seen = modified(&path);
        let file = path.display().to_string();
        let message = match Dictionary::load(&path) {
            Ok(dictionary) => {
                self.unreadable = false;
                log::info!(
                    "Dictionary loaded: {} words, {} replacements",
                    dictionary.words.len(),
                    dictionary.replacements.len()
                );
                json!({ "dictionary": dictionary, "file": file, "error": null })
            }
            Err(e) => {
                self.unreadable = true;
                log::warn!("Can't read {file}: {e:#}");
                let error = format!(
                    "{} has a mistake in it ({e}). Fix it in a text editor first.",
                    name(&path)
                );
                json!({ "dictionary": null, "file": file, "error": error })
            }
        };
        self.run(&format!("squawk.load({message})"));
    }

    fn reload_if_changed(&mut self) {
        if self.shown && modified(&super::path()) != self.seen {
            log::info!("Dictionary changed on disk; reloading");
            self.load();
        }
    }

    fn save(&mut self, dictionary: &Dictionary) {
        let path = super::path();
        // Something else may have written the file since the page last heard from it; those
        // changes win, and the page gets them instead of saving over them.
        if modified(&path) != self.seen {
            log::info!("Dictionary changed on disk before a save; reloading instead");
            self.load();
            return;
        }
        let error = if self.unreadable {
            Some(format!(
                "Not saved: {} has a mistake in it. Fix it in a text editor first.",
                name(&path)
            ))
        } else {
            match dictionary.save(&path) {
                Ok(()) => {
                    self.seen = modified(&path);
                    log::info!(
                        "Dictionary saved: {} words, {} replacements",
                        dictionary.words.len(),
                        dictionary.replacements.len()
                    );
                    None
                }
                Err(e) => {
                    log::error!("Couldn't save {}: {e:#}", path.display());
                    Some(format!("Couldn't save {}: {e}", name(&path)))
                }
            }
        };
        self.run(&format!("squawk.saved({})", json!({ "error": error })));
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Shows the file in Finder or Explorer, creating an empty one first so there's something to
/// show.
fn reveal(path: &Path) {
    if !path.exists()
        && let Err(e) = Dictionary::default().save(path)
    {
        log::error!("Couldn't create {}: {e:#}", path.display());
        return;
    }
    system::reveal(path);
}

/// Horizontally centred and a little above the middle, where macOS puts new windows.
fn center(window: &Window) {
    let Some(monitor) = window.current_monitor().or_else(|| window.primary_monitor()) else {
        return;
    };
    let scale = monitor.scale_factor();
    let screen = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position().to_logical::<f64>(scale);
    let size = window.outer_size().to_logical::<f64>(scale);
    let x = origin.x + (screen.width - size.width) / 2.0;
    let y = origin.y + ((screen.height - size.height) * 0.4).max(0.0);
    window.set_outer_position(LogicalPosition::new(x, y));
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
    use tray_icon::menu::{Menu, PredefinedMenuItem, Submenu};

    /// Text fields only get ⌘C, ⌘V, ⌘X, ⌘A and ⌘Z through the app's main menu, which a
    /// menu-bar app doesn't have. It's never shown (the app has no menu bar), but its key
    /// equivalents still work. The first submenu is the app menu, so it comes first.
    pub fn install_edit_menu() -> Menu {
        let menu = Menu::new();
        let app = Submenu::with_items("Squawk", true, &[&PredefinedMenuItem::hide(None)]);
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &PredefinedMenuItem::undo(None),
                &PredefinedMenuItem::redo(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::cut(None),
                &PredefinedMenuItem::copy(None),
                &PredefinedMenuItem::paste(None),
                &PredefinedMenuItem::select_all(None),
            ],
        );
        match (app, edit) {
            (Ok(app), Ok(edit)) => {
                if let Err(e) = menu.append_items(&[&app, &edit]) {
                    log::warn!("Couldn't build the Edit menu: {e}");
                }
                menu.init_for_nsapp();
            }
            (Err(e), _) | (_, Err(e)) => log::warn!("Couldn't build the Edit menu: {e}"),
        }
        menu
    }

    /// The app in front, unless it's Squawk itself.
    pub fn frontmost_other_app() -> Option<Retained<NSRunningApplication>> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        (app != NSRunningApplication::currentApplication()).then_some(app)
    }

    /// Re-activates `app`, but only if Squawk is still the active app: if the user already
    /// switched elsewhere, leave them there.
    pub fn give_focus_back(app: &NSRunningApplication) {
        if NSRunningApplication::currentApplication().isActive() && !app.isTerminated() {
            app.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }
}
