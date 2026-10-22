use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::SoundTheme;

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

/// Which cleanup pass runs after transcription.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanupMode {
    #[default]
    Claude,
    Ollama,
    Off,
}

/// Everything the user picks in the menu, stored as `settings.json` in `paths::data_dir()`
/// (`config.json` on Linux; see [`Settings::path`]). Every field is optional in the file, and
/// one that can't be read falls back to its default without losing the others.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: Hotkey,
    pub cleanup: CleanupMode,
    pub ollama_model: String,
    /// Device name; `None` is the system default.
    pub microphone: Option<String>,
    /// `None` is off. Stored as "wood", "soft", "chime", "bubble" or "off".
    #[serde(serialize_with = "sounds_to_str")]
    pub sounds: Option<SoundTheme>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: Hotkey::default(),
            cleanup: CleanupMode::Claude,
            ollama_model: "qwen3.5:9b".into(),
            microphone: None,
            sounds: Some(SoundTheme::Wood),
        }
    }
}

fn sounds_to_str<S: serde::Serializer>(sounds: &Option<SoundTheme>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(sounds.map_or("off", SoundTheme::as_str))
}

impl Settings {
    /// `settings.json` in `paths::data_dir()`. On Linux it's `config.json` there instead: it's
    /// also edited by hand (the Omarchy bar menu changes it through `squawkctl set`, which keeps the
    /// keys it doesn't know), and earlier versions wrote that file with these same keys (plus `model` and `threads`,
    /// which the Linux front end reads itself). Reading it in place keeps one file and its documentation, rather than a second
    /// copy that drifts.
    pub fn path() -> PathBuf {
        let name = if cfg!(target_os = "linux") {
            "config.json"
        } else {
            "settings.json"
        };
        crate::paths::data_dir().join(name)
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::path())
    }

    fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        crate::dictionary::write_atomically(path, &serde_json::to_vec_pretty(self)?)
    }

    fn load_from(path: &Path) -> Self {
        let mut settings = Self::default();
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return settings,
            Err(e) => {
                log::warn!("Can't read {}: {e}; using default settings", path.display());
                return settings;
            }
        };
        let fields = match serde_json::from_slice::<Value>(&data) {
            Ok(Value::Object(fields)) => fields,
            Ok(_) | Err(_) => {
                log::warn!("{} isn't a JSON object; using default settings", path.display());
                return settings;
            }
        };

        fn field<T: DeserializeOwned>(fields: &Map<String, Value>, key: &str, into: &mut T) {
            let Some(value) = fields.get(key) else { return };
            match T::deserialize(value) {
                Ok(value) => *into = value,
                Err(e) => log::warn!("Ignoring settings.json \"{key}\": {e}"),
            }
        }
        field(&fields, "hotkey", &mut settings.hotkey);
        field(&fields, "cleanup", &mut settings.cleanup);
        field(&fields, "ollamaModel", &mut settings.ollama_model);
        field(&fields, "microphone", &mut settings.microphone);
        match fields
            .get("sounds")
            .map(|v| v.as_str().map(|s| (s, SoundTheme::parse(s))))
        {
            None => {}
            Some(Some(("off", _))) => settings.sounds = None,
            Some(Some((_, Some(theme)))) => settings.sounds = Some(theme),
            Some(_) => log::warn!("Ignoring settings.json \"sounds\": {}", fields["sounds"]),
        }
        // A key from another OS (fn on Linux, say) can't be held here.
        if !Hotkey::available().contains(&settings.hotkey) {
            settings.hotkey = Hotkey::default();
        }
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("squawk-settings-{}-{nanos}.json", std::process::id()))
    }

    fn load_json(json: &str) -> Settings {
        let path = temp_path();
        std::fs::write(&path, json).unwrap();
        let settings = Settings::load_from(&path);
        std::fs::remove_file(&path).unwrap();
        settings
    }

    #[test]
    fn round_trips() {
        let path = temp_path();
        let settings = Settings {
            cleanup: CleanupMode::Ollama,
            ollama_model: "gemma3:4b".into(),
            microphone: Some("USB Mic".into()),
            sounds: None,
            ..Settings::default()
        };
        settings.save_to(&path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(
            json.contains(r#""sounds": "off""#) && json.contains(r#""ollamaModel": "gemma3:4b""#),
            "{json}"
        );
        assert_eq!(Settings::load_from(&path), settings);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_or_broken_files_give_defaults() {
        assert_eq!(Settings::load_from(&temp_path()), Settings::default());
        assert_eq!(load_json("{not json"), Settings::default());
        assert_eq!(load_json("[]"), Settings::default());
    }

    #[test]
    fn every_field_is_optional() {
        let settings = load_json(r#"{"sounds": "chime", "cleanup": "nonsense", "microphone": null}"#);
        assert_eq!(
            settings,
            Settings {
                sounds: Some(SoundTheme::Chime),
                ..Settings::default()
            }
        );
        assert_eq!(load_json(r#"{"sounds": "off"}"#).sounds, None);
        assert_eq!(load_json(r#"{"sounds": 3}"#).sounds, Some(SoundTheme::Wood));
    }

    #[test]
    fn hotkey_from_another_os_falls_back() {
        let other = if cfg!(target_os = "macos") { "rightAlt" } else { "fn" };
        assert_eq!(
            load_json(&format!(r#"{{"hotkey": "{other}"}}"#)).hotkey,
            Hotkey::default()
        );
    }
}
