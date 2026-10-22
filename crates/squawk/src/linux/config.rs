//! Settings on Linux.
//!
//! `~/.config/squawk/config.json` is read in place:
//! `squawk_core::Settings::path()` points there on Linux and reads its `cleanup`, `ollamaModel`,
//! `sounds` and `microphone` keys, which have the same names and values as the Rust settings.
//! The two speech-model keys, `model` (another model folder) and `threads`, only matter here, so this module reads them itself. Every key is optional.
//!
//! `microphone` is still a PipeWire node name (`pw-record --list-targets`); `squawk-audio`
//! opens it through pipewire-alsa.

use std::path::PathBuf;

use serde_json::Value;
use squawk_core::Settings;
use squawk_engine::Options;

/// Everything the Linux front end reads from the settings file.
pub struct Config {
    pub settings: Settings,
    pub options: Options,
}

pub fn load() -> Config {
    let settings = Settings::load();
    let fields = std::fs::read(Settings::path())
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok());
    Config {
        settings,
        options: fields.as_ref().map(options).unwrap_or_default(),
    }
}

/// `model` (a folder, `~` allowed) and `threads` from the settings JSON. A value of the wrong
/// type is logged and ignored, like the other settings.
fn options(fields: &Value) -> Options {
    let model_dir = match fields.get("model") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) if !path.is_empty() => Some(expand_home(path)),
        Some(other) => {
            log::warn!("Ignoring config.json \"model\": {other}");
            None
        }
    };
    let threads = match fields.get("threads") {
        None | Some(Value::Null) => None,
        Some(value) => match value.as_u64().filter(|&n| n > 0) {
            Some(n) => Some(n as usize),
            None => {
                log::warn!("Ignoring config.json \"threads\": {value}");
                None
            }
        },
    };
    Options { model_dir, threads }
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_model_and_threads() {
        let json: Value =
            serde_json::from_str(r#"{"model": "/opt/parakeet", "threads": 4, "cleanup": "off"}"#).unwrap();
        assert_eq!(
            options(&json),
            Options {
                model_dir: Some(PathBuf::from("/opt/parakeet")),
                threads: Some(4),
            }
        );
    }

    #[test]
    fn missing_or_wrong_values_are_defaults() {
        let json: Value = serde_json::from_str(r#"{"model": 3, "threads": "four"}"#).unwrap();
        assert_eq!(options(&json), Options::default());
        assert_eq!(options(&serde_json::json!({})), Options::default());
        assert_eq!(options(&serde_json::json!({"threads": 0})).threads, None);
    }

    #[test]
    fn tilde_means_home() {
        let Some(home) = std::env::var_os("HOME") else { return };
        assert_eq!(expand_home("~/models/x"), PathBuf::from(home).join("models/x"));
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
    }
}
