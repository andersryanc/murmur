use serde::{Deserialize, Serialize};

/// Which key you hold to talk. Modifier keys only, so holding one never types anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Hotkey {
    /// macOS only: the fn (Globe) key.
    Fn,
    /// macOS: Right Option ⌥.
    RightOption,
    /// macOS: Right Command ⌘.
    RightCommand,
    /// Windows and Linux: Right Alt (AltGr on some layouts).
    RightAlt,
    /// Windows and Linux: Right Ctrl.
    RightCtrl,
}

impl Hotkey {
    /// The keys offered on this OS, default first.
    pub fn available() -> &'static [Hotkey] {
        if cfg!(target_os = "macos") {
            &[Self::Fn, Self::RightOption, Self::RightCommand]
        } else {
            &[Self::RightAlt, Self::RightCtrl]
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fn => "fn (Globe)",
            Self::RightOption => "Right Option ⌥",
            Self::RightCommand => "Right Command ⌘",
            Self::RightAlt => "Right Alt",
            Self::RightCtrl => "Right Ctrl",
        }
    }
}

impl Default for Hotkey {
    fn default() -> Self {
        Self::available()[0]
    }
}
