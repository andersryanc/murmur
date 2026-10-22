//! The tray menu.
//!
//! The menu is rebuilt from a [`MenuView`] snapshot whenever that changes, rather than patched
//! item by item: it is small, and rebuilding keeps the radio groups and optional rows simple.
//! The status line changes far more often (download progress), so it is updated in place.
//!
//! muda has no radio items, so each picker is a submenu of check items with exactly one
//! checked.

use std::collections::HashMap;

use squawk_core::{CleanupMode, Hotkey, SoundTheme};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

/// Something a menu item does when clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    OpenAccessibilitySettings,
    OpenMicrophoneSettings,
    RecheckApiKey,
    /// Index into the recent dictations, newest first.
    CopyRecent(usize),
    Hotkey(Hotkey),
    Cleanup(CleanupMode),
    LocalModel(String),
    RefreshLocalModels,
    Microphone(Option<String>),
    Sounds(Option<SoundTheme>),
    /// Turn Launch at Login on (true) or off.
    LaunchAtLogin(bool),
    Dictionary,
    Quit,
}

/// Everything the menu shows apart from the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuView {
    pub timing: Option<String>,
    pub needs_accessibility: bool,
    pub needs_microphone: bool,
    pub needs_api_key: bool,
    /// Recent dictations, newest first.
    pub recent: Vec<String>,
    pub hotkey: Hotkey,
    pub cleanup: CleanupMode,
    pub ollama_model: String,
    pub local_models: Vec<String>,
    pub microphone: Option<String>,
    pub devices: Vec<String>,
    pub default_device: Option<String>,
    pub sounds: Option<SoundTheme>,
    /// Whether Squawk opens at login; `None` where it can't, which leaves the item out.
    pub launch_at_login: Option<bool>,
}

/// How many recent dictations the menu lists.
pub const RECENT_SHOWN: usize = 5;

/// A built menu and what each of its items does.
pub struct BuiltMenu {
    pub menu: Menu,
    status: MenuItem,
    actions: HashMap<MenuId, Action>,
}

impl BuiltMenu {
    pub fn set_status(&self, text: &str) {
        self.status.set_text(text);
    }

    pub fn action(&self, id: &MenuId) -> Option<Action> {
        self.actions.get(id).cloned()
    }

    pub fn build(view: &MenuView, status: &str) -> Self {
        let mut b = Builder::default();
        let menu = Menu::new();
        let status_item = MenuItem::new(status, false, None);
        let mut items: Vec<Box<dyn IsMenuItem>> = vec![Box::new(status_item.clone())];

        if let Some(timing) = &view.timing {
            items.push(Box::new(MenuItem::new(timing, false, None)));
        }
        if view.needs_accessibility {
            items.push(b.item("Grant Accessibility Access…", Action::OpenAccessibilitySettings));
        }
        if view.needs_microphone {
            items.push(b.item("Grant Microphone Access…", Action::OpenMicrophoneSettings));
        }
        if view.needs_api_key {
            items.push(b.item("No Anthropic API key found. Check again", Action::RecheckApiKey));
        }
        items.push(separator());

        if view.recent.is_empty() {
            items.push(Box::new(MenuItem::new("No dictations yet", false, None)));
        } else {
            items.push(Box::new(MenuItem::new("Recent: click to copy", false, None)));
            for (i, text) in view.recent.iter().take(RECENT_SHOWN).enumerate() {
                items.push(b.item(&preview(text), Action::CopyRecent(i)));
            }
        }
        items.push(separator());

        let hotkeys: Vec<_> = Hotkey::available()
            .iter()
            .map(|&key| b.check(key.label(), view.hotkey == key, Action::Hotkey(key)))
            .collect();
        items.push(submenu("Push-to-Talk Key", hotkeys));

        let modes = [
            (CleanupMode::Claude, "Claude Haiku (Cloud)"),
            (CleanupMode::Ollama, "Local Model (Ollama)"),
            (CleanupMode::Off, "Off: Paste Raw Transcript"),
        ];
        let modes = modes
            .into_iter()
            .map(|(mode, label)| b.check(label, view.cleanup == mode, Action::Cleanup(mode)))
            .collect();
        items.push(submenu("Cleanup", modes));

        if view.cleanup == CleanupMode::Ollama {
            // Keep the current choice listed even if Ollama doesn't have it (yet).
            let mut names = view.local_models.clone();
            if !names.contains(&view.ollama_model) {
                names.push(view.ollama_model.clone());
                names.sort();
            }
            let mut models: Vec<Box<dyn IsMenuItem>> = names
                .into_iter()
                .map(|name| {
                    let label = if view.local_models.contains(&name) {
                        name.clone()
                    } else {
                        format!("{name} (not installed)")
                    };
                    b.check(&label, view.ollama_model == name, Action::LocalModel(name))
                })
                .collect();
            models.push(separator());
            models.push(b.item("Refresh Local Models", Action::RefreshLocalModels));
            items.push(submenu("Local Model", models));
        }

        let default_label = match &view.default_device {
            Some(name) => format!("System Default ({name})"),
            None => "System Default".into(),
        };
        let mut mics = vec![
            b.check(&default_label, view.microphone.is_none(), Action::Microphone(None)),
            separator(),
        ];
        for device in &view.devices {
            let chosen = view.microphone.as_ref() == Some(device);
            mics.push(b.check(device, chosen, Action::Microphone(Some(device.clone()))));
        }
        // A saved mic that's unplugged right now stays selected, so it's used again on reconnect.
        if let Some(name) = &view.microphone
            && !view.devices.contains(name)
        {
            let label = format!("{name} (disconnected, using system default)");
            mics.push(b.check(&label, true, Action::Microphone(Some(name.clone()))));
        }
        items.push(submenu("Microphone", mics));

        let mut sounds: Vec<Box<dyn IsMenuItem>> = SoundTheme::ALL
            .iter()
            .map(|&theme| b.check(theme.label(), view.sounds == Some(theme), Action::Sounds(Some(theme))))
            .collect();
        sounds.push(separator());
        sounds.push(b.check("Off", view.sounds.is_none(), Action::Sounds(None)));
        items.push(submenu("Sound Effects", sounds));
        if let Some(on) = view.launch_at_login {
            items.push(b.check("Launch at Login", on, Action::LaunchAtLogin(!on)));
        }

        items.push(separator());
        items.push(b.item("Dictionary…", Action::Dictionary));
        items.push(separator());
        items.push(b.item("Quit Squawk", Action::Quit));

        let refs: Vec<&dyn IsMenuItem> = items.iter().map(|item| item.as_ref()).collect();
        if let Err(e) = menu.append_items(&refs) {
            log::error!("Couldn't build the menu: {e}");
        }
        Self {
            menu,
            status: status_item,
            actions: b.actions,
        }
    }
}

/// Hands out item ids and remembers what each one does.
#[derive(Default)]
struct Builder {
    actions: HashMap<MenuId, Action>,
}

impl Builder {
    fn item(&mut self, text: &str, action: Action) -> Box<dyn IsMenuItem> {
        let item = MenuItem::new(text, true, None);
        self.actions.insert(item.id().clone(), action);
        Box::new(item)
    }

    fn check(&mut self, text: &str, checked: bool, action: Action) -> Box<dyn IsMenuItem> {
        let item = CheckMenuItem::new(text, true, checked, None);
        self.actions.insert(item.id().clone(), action);
        Box::new(item)
    }
}

fn separator() -> Box<dyn IsMenuItem> {
    Box::new(PredefinedMenuItem::separator())
}

fn submenu(text: &str, items: Vec<Box<dyn IsMenuItem>>) -> Box<dyn IsMenuItem> {
    let refs: Vec<&dyn IsMenuItem> = items.iter().map(|item| item.as_ref()).collect();
    let submenu = Submenu::with_items(text, true, &refs).unwrap_or_else(|e| {
        log::error!("Couldn't build the {text} submenu: {e}");
        Submenu::new(text, false)
    });
    Box::new(submenu)
}

/// One line, at most 48 characters, for the Recent list.
fn preview(text: &str) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() > 48 {
        flat.chars().take(48).collect::<String>() + "…"
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_one_short_line() {
        assert_eq!(preview("a\nb"), "a b");
        let long = "x".repeat(60);
        assert_eq!(preview(&long).chars().count(), 49);
    }
}
