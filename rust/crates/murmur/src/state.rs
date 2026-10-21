//! What the main loop hears about and the small value types it keeps.
//!
//! Everything that happens off the main thread (the hotkey tap, menu clicks, model loading,
//! dictations finishing) arrives as an [`AppEvent`] through the event loop's proxy, so all of
//! the app's state lives on the main thread and needs no locks.

use std::sync::Arc;
use std::time::Duration;

use murmur_platform::HotkeyEvent;
use murmur_stt::Transcriber;
use tray_icon::menu::MenuId;

pub type Proxy = tao::event_loop::EventLoopProxy<AppEvent>;

pub enum AppEvent {
    Hotkey(HotkeyEvent),
    Menu(MenuId),
    /// Startup progress for the status line, e.g. "Downloading speech model… 40%".
    Loading(String),
    ModelReady(Arc<Transcriber>),
    ModelFailed(String),
    /// Whether an Anthropic API key was found.
    ApiKey(bool),
    /// Models pulled into Ollama, sorted.
    LocalModels(Vec<String>),
    /// The connected microphones and the system default, when either changes.
    Devices {
        devices: Vec<String>,
        default: Option<String>,
    },
    /// A dictation was pasted (or left on the clipboard).
    Dictated(String),
    Timing(Timing),
    /// A queued dictation finished, whether or not anything was pasted.
    JobDone,
}

/// Where startup has got to. Recording and processing are tracked separately (see `App`),
/// since a recording can run while earlier dictations are still processing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading(String),
    Ready,
    Failed(String),
}

/// How long each stage of the last dictation took, shown in the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    pub speech: Duration,
    /// How long cleanup took and which cleaner did it; `None` when cleanup was off or failed.
    pub cleanup: Option<(Duration, String)>,
}

impl Timing {
    pub fn summary(&self) -> String {
        let mut text = format!("Last: speech {} ms", self.speech.as_millis());
        if let Some((took, cleaner)) = &self.cleanup {
            text += &format!(" · cleanup {} ms ({cleaner})", took.as_millis());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_summary() {
        let timing = Timing {
            speech: Duration::from_millis(212),
            cleanup: Some((Duration::from_millis(840), "claude-haiku".into())),
        };
        assert_eq!(timing.summary(), "Last: speech 212 ms · cleanup 840 ms (claude-haiku)");
        let raw = Timing {
            cleanup: None,
            ..timing
        };
        assert_eq!(raw.summary(), "Last: speech 212 ms");
    }
}
