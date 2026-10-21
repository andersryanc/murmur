use serde::{Deserialize, Serialize};

/// The kind of app the dictation is headed for. The cleanup pass formats differently for each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetContext {
    #[default]
    Plain,
    Chat,
    Email,
    Notes,
    Code,
}

impl TargetContext {
    pub const ALL: [TargetContext; 5] = [Self::Plain, Self::Chat, Self::Email, Self::Notes, Self::Code];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Chat => "chat",
            Self::Email => "email",
            Self::Notes => "notes",
            Self::Code => "code",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// Best guess from the frontmost app's bundle identifier on macOS.
    pub fn from_bundle_id(id: Option<&str>) -> Self {
        let Some(id) = id.map(str::to_lowercase) else { return Self::Plain };
        KNOWN_APPS.iter().find(|(prefix, _)| id.starts_with(prefix)).map_or(Self::Plain, |&(_, c)| c)
    }

    /// Best guess from the focused window's class on Linux (`hyprctl activewindow -j`), e.g.
    /// "Slack", "Alacritty", "obsidian". Matches by substring, which also catches Chromium web
    /// apps, whose class carries the site's address (e.g. "chrome-app.slack.com__client-Default").
    pub fn from_window_class(class: Option<&str>) -> Self {
        Self::by_fragment(class, KNOWN_WINDOW_CLASSES)
    }

    /// Best guess from the foreground window's executable on Windows, e.g. "slack.exe".
    pub fn from_process_name(exe: Option<&str>) -> Self {
        Self::by_fragment(exe, KNOWN_PROCESSES)
    }

    fn by_fragment(name: Option<&str>, table: &[(&str, TargetContext)]) -> Self {
        let Some(name) = name.map(str::to_lowercase).filter(|n| !n.is_empty()) else { return Self::Plain };
        table.iter().find(|(fragment, _)| name.contains(fragment)).map_or(Self::Plain, |&(_, c)| c)
    }
}

use TargetContext::{Chat, Code, Email, Notes};

const KNOWN_APPS: &[(&str, TargetContext)] = &[
    // Chat
    ("com.tinyspeck.slackmacgap", Chat),
    ("com.apple.mobilesms", Chat),
    ("com.hnc.discord", Chat),
    ("net.whatsapp.whatsapp", Chat),
    ("ru.keepcoder.telegram", Chat),
    ("com.microsoft.teams", Chat),
    // Email
    ("com.apple.mail", Email),
    ("com.microsoft.outlook", Email),
    ("com.superhuman.electron", Email),
    ("com.readdle.smartemail", Email),
    // Notes and docs
    ("com.apple.notes", Notes),
    ("md.obsidian", Notes),
    ("notion.id", Notes),
    ("net.shinyfrog.bear", Notes),
    ("com.lukilabs.lukiapp", Notes), // Craft
    // Code: editors, terminals, AI coding tools
    ("com.apple.dt.xcode", Code),
    ("com.microsoft.vscode", Code),
    ("com.todesktop.230313mzl4w4u92", Code), // Cursor
    ("dev.zed.zed", Code),
    ("com.apple.terminal", Code),
    ("com.googlecode.iterm2", Code),
    ("com.mitchellh.ghostty", Code),
    ("dev.warp.warp-stable", Code),
    ("com.jetbrains", Code),
    ("com.anthropic.claudefordesktop", Code),
];

/// Checked in order, so a more specific fragment must come before a shorter one inside it.
const KNOWN_WINDOW_CLASSES: &[(&str, TargetContext)] = &[
    // Chat
    ("slack", Chat), ("discord", Chat), ("vesktop", Chat), ("signal", Chat),
    ("telegram", Chat), ("whatsapp", Chat), ("element", Chat), ("teams", Chat),
    // Email
    ("thunderbird", Email), ("mail.google.com", Email), ("hey.com", Email), ("geary", Email),
    ("evolution", Email), ("outlook", Email), ("superhuman", Email), ("betterbird", Email),
    // Notes and docs
    ("obsidian", Notes), ("notion", Notes), ("logseq", Notes), ("typora", Notes),
    ("joplin", Notes), ("docs.google.com", Notes),
    // Code: editors, terminals, AI coding tools
    ("code", Code), ("cursor", Code), ("zed", Code), ("jetbrains", Code), ("neovide", Code),
    ("alacritty", Code), ("kitty", Code), ("ghostty", Code), ("foot", Code),
    ("wezterm", Code), ("konsole", Code), ("gnome-terminal", Code), ("claude", Code),
];

/// Checked in order, like the window classes.
const KNOWN_PROCESSES: &[(&str, TargetContext)] = &[
    // Chat
    ("slack", Chat), ("discord", Chat), ("signal", Chat), ("telegram", Chat),
    ("whatsapp", Chat), ("ms-teams", Chat), ("teams", Chat),
    // Email
    ("outlook", Email), ("olk", Email), ("thunderbird", Email), ("superhuman", Email),
    // Notes and docs
    ("obsidian", Notes), ("notion", Notes), ("logseq", Notes), ("onenote", Notes), ("winword", Notes),
    // Code: editors, terminals, AI coding tools
    ("code", Code), ("cursor", Code), ("zed", Code), ("devenv", Code), ("idea", Code),
    ("pycharm", Code), ("rider", Code), ("windowsterminal", Code), ("wezterm", Code),
    ("alacritty", Code), ("powershell", Code), ("pwsh", Code), ("cmd.exe", Code), ("claude", Code),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_ids() {
        assert_eq!(TargetContext::from_bundle_id(Some("com.tinyspeck.slackmacgap")), Chat);
        assert_eq!(TargetContext::from_bundle_id(Some("com.jetbrains.intellij")), Code);
        assert_eq!(TargetContext::from_bundle_id(Some("com.example.unknown")), TargetContext::Plain);
        assert_eq!(TargetContext::from_bundle_id(None), TargetContext::Plain);
    }

    #[test]
    fn window_classes_and_processes() {
        assert_eq!(TargetContext::from_window_class(Some("chrome-app.slack.com__client-Default")), Chat);
        assert_eq!(TargetContext::from_window_class(Some("Alacritty")), Code);
        assert_eq!(TargetContext::from_process_name(Some("Slack.exe")), Chat);
        assert_eq!(TargetContext::from_process_name(Some("WindowsTerminal.exe")), Code);
        assert_eq!(TargetContext::from_process_name(Some("explorer.exe")), TargetContext::Plain);
    }
}
