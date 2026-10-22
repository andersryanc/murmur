//! The OS-facing pieces of Squawk: the push-to-talk hotkey, pasting, the frontmost app and
//! notifications. One module per OS sits behind `cfg`, all with the same API (see CONTRACTS.md);
//! the hold/release timing they share lives in [`hold`].

pub mod hold;

pub use hold::{HOLD_DELAY, HoldTracker, HotkeyEvent, KeyInput};

/// What [`paste`] managed to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteOutcome {
    /// The text was put on the clipboard and the paste keystroke sent.
    Pasted,
    /// No permission to send keystrokes, so the text was left on the clipboard for a manual paste.
    CopiedOnly,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

// The stubs are plain Rust, so build them on every OS's test run too: that is the only check
// they get on a Mac without the Windows and Linux targets installed.
#[cfg(all(test, not(target_os = "linux")))]
#[allow(dead_code)]
#[path = "linux.rs"]
mod linux_check;
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "windows.rs"]
mod windows_check;

pub use imp::{HotkeyMonitor, frontmost_app, has_input_permission, notify, paste};

/// The kind of app text is about to land in, so the cleanup can match its tone.
pub fn target_context() -> squawk_core::TargetContext {
    imp::context_for(frontmost_app().as_deref())
}
