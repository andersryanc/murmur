//! Small OS chores the menu needs that the platform crate doesn't cover: opening settings pages
//! and files, copying text without pasting it, and the microphone permission state.

use std::path::Path;
use std::process::{Command, Stdio};

/// Whether the system lets Murmur hear the microphone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicAccess {
    Granted,
    /// Never asked: the first recording (the warm-up at launch) brings up the system prompt.
    NotAsked,
    Denied,
}

#[cfg(target_os = "macos")]
pub fn microphone_access() -> MicAccess {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: reads an immutable AVFoundation constant and a class method with no side effects.
    let status = unsafe {
        let Some(audio) = AVMediaTypeAudio else {
            return MicAccess::Granted;
        };
        AVCaptureDevice::authorizationStatusForMediaType(audio)
    };
    match status {
        AVAuthorizationStatus::Authorized => MicAccess::Granted,
        AVAuthorizationStatus::NotDetermined => MicAccess::NotAsked,
        _ => MicAccess::Denied,
    }
}

/// Windows and Linux don't gate the microphone per app in a way we can query; a blocked mic
/// shows up as silence.
#[cfg(not(target_os = "macos"))]
pub fn microphone_access() -> MicAccess {
    MicAccess::Granted
}

/// Privacy & Security › Accessibility, where Murmur is allowed to watch and send keystrokes.
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    open("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
}

/// Privacy & Security › Microphone.
pub fn open_microphone_settings() {
    #[cfg(target_os = "macos")]
    open("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone");
    #[cfg(target_os = "windows")]
    open("ms-settings:privacy-microphone");
}

/// Opens a text file in the user's editor. On macOS `open -t` picks the default *text* editor,
/// since the default app for `.json` is often Xcode or a browser.
#[allow(dead_code)] // Only used where there's no Dictionary window (Linux).
pub fn open_in_editor(path: &Path) {
    #[cfg(target_os = "macos")]
    spawn(Command::new("/usr/bin/open").arg("-t").arg(path));
    #[cfg(not(target_os = "macos"))]
    open(&path.to_string_lossy());
}

/// Shows a file selected in Finder or Explorer; elsewhere, opens the folder it's in.
#[allow(dead_code)] // Linux has no Dictionary window to call it.
pub fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    spawn(Command::new("/usr/bin/open").arg("-R").arg(path));
    #[cfg(target_os = "windows")]
    spawn(Command::new("explorer").arg(format!("/select,{}", path.display())));
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(dir) = path.parent() {
        open(&dir.to_string_lossy());
    }
}

/// Opens a URL or file with whatever the OS associates with it.
#[allow(dead_code)] // Unused on Linux, which has no settings pages to link to.
fn open(target: &str) {
    #[cfg(target_os = "macos")]
    spawn(Command::new("/usr/bin/open").arg(target));
    #[cfg(target_os = "windows")]
    spawn(Command::new("explorer").arg(target));
    #[cfg(all(unix, not(target_os = "macos")))]
    spawn(Command::new("xdg-open").arg(target));
}

fn spawn(command: &mut Command) {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        // Reap it off the main thread so a slow opener never stalls the menu.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => log::warn!("Couldn't run {:?}: {e}", command.get_program()),
    }
}

/// Puts text on the clipboard without pasting it, for "Recent: click to copy".
pub fn copy(text: &str) {
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => {}
        Err(e) => log::warn!("Couldn't copy to the clipboard: {e}"),
    }
}
