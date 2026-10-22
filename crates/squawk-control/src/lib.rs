//! The control socket between Squawk's daemon and everything that drives or watches it.
//!
//! On Linux (Hyprland) Squawk runs as a daemon: Wayland doesn't let apps watch keys globally,
//! so Hyprland key bindings run `squawkctl start` / `squawkctl stop`, which send one command over this
//! Unix socket and print the one-line reply. The Omarchy shell plugin (`linux/omarchy-plugin`)
//! connects to the same socket, sends `watch`, and stays connected to receive one line per event
//! to draw the on-screen pill.
//!
//! The wire format is plain text, one line each way,
//! and must stay as it is: the plugin parses it. Nothing here is Linux-specific, so it builds and
//! is tested on macOS too.

#![cfg(unix)]

mod client;
mod server;

use std::fmt;
use std::path::PathBuf;

pub use client::{Watch, send, send_to, watch};
pub use server::Server;

/// The socket the daemon listens on: `$XDG_RUNTIME_DIR/squawk/squawk.sock`, or
/// `/tmp/squawk-<uid>/squawk/squawk.sock` without a runtime directory. The Omarchy plugin
/// computes the same path, so the two must change together.
pub fn socket_path() -> PathBuf {
    let runtime = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        // SAFETY: getuid has no preconditions and can't fail.
        _ => PathBuf::from(format!("/tmp/squawk-{}", unsafe { libc::getuid() })),
    };
    runtime.join("squawk").join("squawk.sock")
}

/// A command a client sends. Each gets a one-line reply from the daemon's handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Start recording (a Hyprland binding on the key's press).
    Start,
    /// Finish recording and dictate it (the key's release).
    Stop,
    /// Start, or stop if already recording, for a single-key binding.
    Toggle,
    /// Throw the current recording away.
    Cancel,
    /// Reply with the current [`Phase`].
    Status,
    /// Reply with the latest dictations, newest first, as a JSON array of strings (for the
    /// Omarchy bar menu's Recent list).
    Recent,
    /// Re-read the settings file now rather than at the next dictation, after the bar menu
    /// changed it. Replies `ok`.
    Reload,
}

impl Command {
    pub const ALL: [Command; 7] = [
        Self::Start,
        Self::Stop,
        Self::Toggle,
        Self::Cancel,
        Self::Status,
        Self::Recent,
        Self::Reload,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Toggle => "toggle",
            Self::Cancel => "cancel",
            Self::Status => "status",
            Self::Recent => "recent",
            Self::Reload => "reload",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a client sends instead of a [`Command`] to stay connected and receive [`Event`]s. The
/// server handles it itself; the daemon's handler never sees it.
pub const WATCH: &str = "watch";

/// What the daemon is doing, as the pill shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Phase {
    #[default]
    Ready,
    Recording,
    /// Recordings are waiting to be transcribed, cleaned up and pasted.
    Processing,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Recording => "recording",
            Self::Processing => "processing",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Ready, Self::Recording, Self::Processing]
            .into_iter()
            .find(|p| p.as_str() == s)
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One line sent to watchers, e.g. `state recording`, `queue 2`, `level 0.42`, `notice …`.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The daemon's phase changed.
    State(Phase),
    /// How many dictations are waiting to paste, including the one being processed. Sent before
    /// the `State` it goes with, so a watcher already knows it when the state changes.
    Queue(usize),
    /// The mic level, 0...1, about every 50 ms while recording. Sent with two decimals.
    Level(f32),
    /// A problem to show for a moment ("Cleanup failed, pasted raw text"). Newlines become
    /// spaces, since each event is one line.
    Notice(String),
}

impl Event {
    /// Parses a line as the plugin would. Unknown or malformed lines give `None`, so older
    /// watchers can skip events added later.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
        match kind {
            "state" => Phase::parse(rest).map(Self::State),
            "queue" => rest.parse().ok().map(Self::Queue),
            "level" => rest.parse().ok().map(Self::Level),
            "notice" => Some(Self::Notice(rest.to_string())),
            _ => None,
        }
    }
}

impl fmt::Display for Event {
    /// The line without its newline.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(phase) => write!(f, "state {phase}"),
            Self::Queue(count) => write!(f, "queue {count}"),
            Self::Level(level) => {
                let level = if level.is_nan() { 0.0 } else { level.clamp(0.0, 1.0) };
                write!(f, "level {level:.2}")
            }
            Self::Notice(text) => write!(f, "notice {}", text.replace(['\r', '\n'], " ")),
        }
    }
}

/// The errors a caller may want to tell apart (find them with `anyhow::Error::downcast_ref`).
#[derive(Debug)]
pub enum ControlError {
    /// Nothing is listening on the socket.
    NotRunning,
    /// Another daemon already answers on the socket.
    AlreadyRunning,
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunning => {
                f.write_str("Squawk isn't running (or is still loading). Start it with `squawkctl daemon`.")
            }
            Self::AlreadyRunning => f.write_str("Squawk is already running."),
        }
    }
}

impl std::error::Error for ControlError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip() {
        for command in Command::ALL {
            assert_eq!(Command::parse(command.as_str()), Some(command));
        }
        assert_eq!(Command::parse("watch"), None);
        assert_eq!(Command::parse("Start"), None);
    }

    #[test]
    fn events_format_as_the_plugin_expects() {
        assert_eq!(Event::State(Phase::Recording).to_string(), "state recording");
        assert_eq!(Event::Queue(2).to_string(), "queue 2");
        assert_eq!(Event::Level(0.4213).to_string(), "level 0.42");
        assert_eq!(Event::Level(1.7).to_string(), "level 1.00");
        assert_eq!(Event::Level(f32::NAN).to_string(), "level 0.00");
        assert_eq!(Event::Notice("two\nlines".into()).to_string(), "notice two lines");
    }

    #[test]
    fn events_round_trip() {
        for event in [
            Event::State(Phase::Ready),
            Event::State(Phase::Processing),
            Event::Queue(0),
            Event::Level(0.5),
            Event::Notice("Copied. Paste with Ctrl+V".into()),
            Event::Notice(String::new()),
        ] {
            assert_eq!(Event::parse(&format!("{event}\n")), Some(event));
        }
        assert_eq!(Event::parse("state sleeping"), None);
        assert_eq!(Event::parse("queue -1"), None);
        assert_eq!(Event::parse("hello"), None);
    }

    #[test]
    fn socket_path_ends_where_the_plugin_looks() {
        assert!(socket_path().ends_with("squawk/squawk.sock"));
    }
}
