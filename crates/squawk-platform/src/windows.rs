//! Windows: not built yet. Same API as the macOS module so the app compiles; the hotkey and
//! paste report an error and the rest are harmless defaults.
//!
//! The plan: a low-level keyboard hook (`WH_KEYBOARD_LL`) feeding [`HoldTracker`], `SendInput`
//! for Ctrl+V, and `GetForegroundWindow` → process name for the frontmost app.

use std::sync::Mutex;

use anyhow::bail;
use squawk_core::{Hotkey, TargetContext};

use crate::{HoldTracker, HotkeyEvent, PasteOutcome};

pub struct HotkeyMonitor {
    key: Mutex<Hotkey>,
    _tracker: HoldTracker,
}

impl HotkeyMonitor {
    pub fn start(key: Hotkey, on_event: impl Fn(HotkeyEvent) + Send + 'static) -> anyhow::Result<Self> {
        let _ = (key, on_event);
        bail!("the push-to-talk hotkey isn't supported on Windows yet")
    }

    pub fn set_key(&self, key: Hotkey) {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = key;
    }

    /// Always true: there is no permission to wait for on Windows.
    pub fn is_listening(&self) -> bool {
        true
    }
}

/// The frontmost app's process name, e.g. "slack.exe". Not built yet.
pub fn frontmost_app() -> Option<String> {
    None
}

pub(crate) fn context_for(app: Option<&str>) -> TargetContext {
    TargetContext::from_process_name(app)
}

pub fn paste(text: &str) -> anyhow::Result<PasteOutcome> {
    let _ = text;
    bail!("pasting isn't supported on Windows yet")
}

/// Windows lets any app send keystrokes, so there's nothing to grant.
pub fn has_input_permission(prompt: bool) -> bool {
    let _ = prompt;
    true
}

pub fn notify(message: &str) {
    log::info!("notify: {message}");
}
