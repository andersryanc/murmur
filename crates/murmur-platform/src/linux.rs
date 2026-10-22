//! Linux, for Hyprland on Wayland (built for Omarchy).
//!
//! Wayland doesn't let an app watch keys globally, so there is no hotkey monitor here: Hyprland
//! key bindings run `murmur start` / `murmur stop`, which reach the daemon over its control
//! socket (the `murmur-control` crate). The rest goes through the desktop's command-line tools:
//! `hyprctl` for the focused window, `wl-copy` + `wtype` to paste, `notify-send` to notify.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::{Context, bail};
use murmur_core::{Hotkey, TargetContext};

use crate::{HoldTracker, HotkeyEvent, PasteOutcome};

/// Never constructed on Linux; it exists so the app compiles against one API everywhere.
pub struct HotkeyMonitor {
    key: Mutex<Hotkey>,
    _tracker: HoldTracker,
}

impl HotkeyMonitor {
    /// Always fails: on Wayland the push-to-talk key is bound in Hyprland, not watched by Murmur.
    pub fn start(key: Hotkey, on_event: impl Fn(HotkeyEvent) + Send + 'static) -> anyhow::Result<Self> {
        let _ = (key, on_event);
        bail!(
            "Linux has no global hotkey monitor: Wayland doesn't let apps watch keys, so the push-to-talk \
             key is a Hyprland binding that runs `murmur start` and `murmur stop`"
        )
    }

    pub fn set_key(&self, key: Hotkey) {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = key;
    }

    /// Always true: there is no permission to wait for on Linux.
    pub fn is_listening(&self) -> bool {
        true
    }
}

/// The focused window's class, e.g. "Slack" or "Alacritty", from `hyprctl activewindow -j`.
/// `None` when nothing has focus, `hyprctl` is missing, or this isn't Hyprland.
pub fn frontmost_app() -> Option<String> {
    // hyprctl exits once it has printed, so reading its output to the end is safe (unlike
    // wl-copy below).
    let output = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    window_class_from_json(&output.stdout)
}

/// Pulls `class` out of `hyprctl activewindow -j`. With no focused window hyprctl prints `{}`.
fn window_class_from_json(json: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(json).ok()?;
    let class = value.get("class")?.as_str()?;
    (!class.is_empty()).then(|| class.to_string())
}

pub(crate) fn context_for(app: Option<&str>) -> TargetContext {
    TargetContext::from_window_class(app)
}

/// Terminals paste with Ctrl+Shift+V; Ctrl+V would send a literal ^V.
fn is_terminal(window_class: Option<&str>) -> bool {
    const TERMINALS: &[&str] = &[
        "alacritty",
        "kitty",
        "ghostty",
        "foot",
        "wezterm",
        "konsole",
        "gnome-terminal",
        "terminator",
        "tilix",
        "xterm",
        "rio",
        "warp",
    ];
    window_class.is_some_and(|class| {
        let class = class.to_lowercase();
        TERMINALS.iter().any(|t| class.contains(t))
    })
}

/// wtype's arguments for the paste chord: press the modifiers, tap V, release them in reverse.
fn paste_keys(terminal: bool) -> Vec<&'static str> {
    let modifiers: &[&str] = if terminal { &["ctrl", "shift"] } else { &["ctrl"] };
    let mut args = Vec::new();
    for m in modifiers {
        args.extend(["-M", m]);
    }
    args.extend(["-k", "v"]);
    for m in modifiers.iter().rev() {
        args.extend(["-m", m]);
    }
    args
}

/// Puts `text` on the clipboard with `wl-copy` and pastes it into the focused window by sending
/// Ctrl+V (Ctrl+Shift+V in a terminal) with `wtype`. The text stays on the clipboard afterwards,
/// like on the Mac, so if wtype is missing or fails it can still be pasted by hand.
///
/// The window is looked up here rather than when recording started: the keystroke goes to
/// whatever has focus now, so that's the window whose paste chord matters.
pub fn paste(text: &str) -> anyhow::Result<PasteOutcome> {
    copy_to_clipboard(text)?;
    let keys = paste_keys(is_terminal(frontmost_app().as_deref()));
    let typed = Command::new("wtype")
        .args(keys)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match typed {
        Ok(status) if status.success() => Ok(PasteOutcome::Pasted),
        Ok(status) => {
            log::warn!("wtype failed ({status}), text left on the clipboard");
            Ok(PasteOutcome::CopiedOnly)
        }
        Err(e) => {
            log::warn!("couldn't run wtype ({e}), text left on the clipboard");
            Ok(PasteOutcome::CopiedOnly)
        }
    }
}

/// Runs `wl-copy` with `text` on its stdin.
///
/// wl-copy forks: the parent exits once the text is read, and a child stays behind to serve the
/// clipboard until something else is copied. That child inherits whatever the parent was given,
/// so anything that waits for those handles to close would hang until the next copy.
///
/// Here wl-copy gets a pipe on stdin only, which is written and then closed (the child has
/// already read it), and null stdout and stderr, so there is nothing for its child to hold open
/// that we read from. `wait` is waitpid on wl-copy itself, which returns as soon as the parent
/// exits. Never use `output()` here: it reads stdout to the end, which the child would hold off.
/// std opens every descriptor close-on-exec, so other programs started at the same moment can't
/// inherit this stdin pipe and keep it open either.
fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    copy_with("wl-copy".as_ref(), text)
}

/// [`copy_to_clipboard`] with the program passed in, so a test can stand in for wl-copy.
fn copy_with(program: &std::ffi::OsStr, text: &str) -> anyhow::Result<()> {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("couldn't run wl-copy (install wl-clipboard)")?;
    let written = child
        .stdin
        .take()
        .context("wl-copy has no stdin")?
        .write_all(text.as_bytes());
    // The stdin handle was dropped above, closing the pipe, so wl-copy sees the end of the text.
    let status = child.wait().context("couldn't wait for wl-copy")?;
    written.context("couldn't write to wl-copy")?;
    if !status.success() {
        bail!("wl-copy failed ({status})");
    }
    Ok(())
}

/// Nothing to grant on Linux: Hyprland delivers the key, and wtype needs no permission.
pub fn has_input_permission(prompt: bool) -> bool {
    let _ = prompt;
    true
}

/// Shows a desktop notification with `notify-send` if it's installed (mako on Omarchy picks it
/// up). Arguments go straight to the process, so the message needs no escaping.
pub fn notify(message: &str) {
    log::info!("notify: {message}");
    let child = Command::new("notify-send")
        .args(["--app-name=Murmur", "--expire-time=3000", "Murmur", message])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_class_comes_from_hyprctl_json() {
        let json = br#"{"address": "0x55d1", "class": "Alacritty", "title": "~", "pid": 1234}"#;
        assert_eq!(window_class_from_json(json).as_deref(), Some("Alacritty"));
        assert_eq!(window_class_from_json(b"{}"), None);
        assert_eq!(window_class_from_json(br#"{"class": ""}"#), None);
        assert_eq!(window_class_from_json(b"Invalid"), None);
    }

    #[test]
    fn terminals_are_recognised_by_fragment() {
        assert!(is_terminal(Some("Alacritty")));
        assert!(is_terminal(Some("com.mitchellh.ghostty")));
        assert!(is_terminal(Some("org.wezfurlong.wezterm")));
        assert!(!is_terminal(Some("Slack")));
        assert!(!is_terminal(Some("chromium")));
        assert!(!is_terminal(None));
    }

    #[test]
    fn paste_chord_releases_modifiers_in_reverse() {
        assert_eq!(paste_keys(false), ["-M", "ctrl", "-k", "v", "-m", "ctrl"]);
        assert_eq!(
            paste_keys(true),
            ["-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"]
        );
    }

    #[test]
    fn hotkey_monitor_explains_the_hyprland_bindings() {
        let error = HotkeyMonitor::start(Hotkey::RightAlt, |_| {}).err().unwrap();
        assert!(error.to_string().contains("Hyprland"));
    }

    /// Stands in for wl-copy: a script that forks a child which keeps the inherited stdout and
    /// stderr open for a long time, then exits. `wl-copy`'s own behaviour, minus the clipboard.
    #[test]
    fn copying_returns_while_a_forked_child_lives_on() {
        let dir = std::env::temp_dir().join(format!("murmur-wl-copy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("wl-copy");
        std::fs::write(&script, "#!/bin/sh\ncat > /dev/null\n(sleep 30) &\nexit 0\n").unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        let started = std::time::Instant::now();
        copy_with(script.as_os_str(), &"hello ".repeat(100_000)).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::fs::remove_dir_all(&dir).ok();
    }
}
