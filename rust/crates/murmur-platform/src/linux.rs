//! Linux: not built yet. Same API as the macOS module so the app compiles; the hotkey and paste
//! report an error and the rest are harmless defaults.
//!
//! The plan: read key events from evdev (or bind the key in Hyprland) feeding [`HoldTracker`],
//! `wl-copy` + `wtype` for the paste, and `hyprctl activewindow` for the window class.

use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::bail;
use murmur_core::{Hotkey, TargetContext};

use crate::{HoldTracker, HotkeyEvent, PasteOutcome};

pub struct HotkeyMonitor {
    key: Mutex<Hotkey>,
    _tracker: HoldTracker,
}

impl HotkeyMonitor {
    pub fn start(key: Hotkey, on_event: impl Fn(HotkeyEvent) + Send + 'static) -> anyhow::Result<Self> {
        let _ = (key, on_event);
        bail!("the push-to-talk hotkey isn't supported on Linux yet")
    }

    pub fn set_key(&self, key: Hotkey) {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = key;
    }

    /// Always true: there is no permission to wait for on Linux.
    pub fn is_listening(&self) -> bool {
        true
    }
}

/// The focused window's class, e.g. "Slack". Not built yet.
pub fn frontmost_app() -> Option<String> {
    None
}

pub(crate) fn context_for(app: Option<&str>) -> TargetContext {
    TargetContext::from_window_class(app)
}

pub fn paste(text: &str) -> anyhow::Result<PasteOutcome> {
    let _ = text;
    bail!("pasting isn't supported on Linux yet")
}

/// Nothing to grant on Linux; access to input devices is set up at install time.
pub fn has_input_permission(prompt: bool) -> bool {
    let _ = prompt;
    true
}

/// Shows a desktop notification with `notify-send` if it's installed (mako on Omarchy picks it
/// up). Arguments go straight to the process, so the message needs no escaping.
pub fn notify(message: &str) {
    log::info!("notify: {message}");
    let child = Command::new("notify-send")
        .args(["--app-name=Murmur", "Murmur", message])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        // Reap it off-thread so a slow notification daemon never blocks the caller.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => log::debug!("notify-send unavailable: {e}"),
    }
}
