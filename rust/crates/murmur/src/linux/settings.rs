//! `murmur settings` and `murmur set`: what the Omarchy bar menu reads and changes.
//!
//! The Mac app has a tray menu; on Omarchy the same menu is a bar widget in the pill's plugin
//! (`linux/omarchy-plugin/MurmurBar.qml`). It runs `murmur settings` when it opens, which prints
//! everything the menu shows as one JSON object, and `murmur set <key> <value>` when something is
//! picked. `set` edits `config.json` in place, keeping keys it doesn't know (`model`, `threads`),
//! then tells a running daemon to `reload`, so the change applies at once.

use std::process::Command as Process;

use anyhow::{Context, bail};
use murmur_control::{Command, ControlError};
use murmur_core::{CleanupMode, OllamaCleaner, Settings, SoundTheme};
use serde::Serialize;
use serde_json::{Map, Value};

/// What `murmur settings` prints.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct View {
    /// The daemon's `status` reply (`loading`, `ready`, `recording`, `processing`, `error: …`),
    /// or `not running`.
    status: String,
    cleanup: CleanupMode,
    ollama_model: String,
    /// Models pulled into Ollama; only looked up when cleanup is Ollama.
    local_models: Vec<String>,
    /// Cleanup is Claude and no API key was found.
    needs_api_key: bool,
    /// The chosen PipeWire node name; `None` is the default input.
    microphone: Option<String>,
    microphones: Vec<Source>,
    default_microphone: Option<String>,
    /// A theme's `as_str`, or `off`.
    sounds: &'static str,
    /// Latest dictations, newest first.
    recent: Vec<String>,
    dictionary: String,
    settings_file: String,
}

/// A PipeWire audio source, as the Microphone list shows it.
#[derive(Debug, PartialEq, Serialize)]
struct Source {
    /// The node name, which is what `microphone` stores.
    name: String,
    description: String,
}

pub fn show() -> anyhow::Result<()> {
    let settings = Settings::load();
    let status = match murmur_control::send(Command::Status) {
        Ok(reply) => reply,
        Err(_) => "not running".into(),
    };
    let recent = murmur_control::send(Command::Recent)
        .ok()
        .and_then(|reply| serde_json::from_str(&reply).ok())
        .unwrap_or_default();
    let (microphones, default_microphone) = sources();
    let view = View {
        status,
        cleanup: settings.cleanup,
        local_models: if settings.cleanup == CleanupMode::Ollama {
            OllamaCleaner::installed_models()
        } else {
            Vec::new()
        },
        needs_api_key: settings.cleanup == CleanupMode::Claude && murmur_core::api_key::load().is_none(),
        ollama_model: settings.ollama_model,
        microphone: settings.microphone,
        microphones,
        default_microphone,
        sounds: settings.sounds.map_or("off", SoundTheme::as_str),
        recent,
        dictionary: murmur_core::Dictionary::default_path().display().to_string(),
        settings_file: Settings::path().display().to_string(),
    };
    println!("{}", serde_json::to_string(&view)?);
    Ok(())
}

/// `murmur set <key> <value>`.
pub fn set(args: &[String]) -> anyhow::Result<()> {
    let [key, value] = args else {
        bail!(
            "Usage: murmur set cleanup claude|ollama|off\n       murmur set ollamaModel <name>\n       \
             murmur set microphone <PipeWire node name>|default\n       murmur set sounds wood|soft|chime|bubble|off"
        );
    };
    let path = Settings::path();
    let mut fields = match std::fs::read(&path) {
        Ok(data) => match serde_json::from_slice::<Value>(&data) {
            Ok(Value::Object(fields)) => fields,
            // Rewriting it would lose whatever the user meant to write there.
            _ => bail!("{} isn't a JSON object; fix it by hand first", path.display()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(e) => return Err(e).with_context(|| format!("couldn't read {}", path.display())),
    };
    apply(&mut fields, key, value)?;
    let mut data = serde_json::to_vec_pretty(&fields)?;
    data.push(b'\n');
    murmur_core::dictionary::write_atomically(&path, &data)
        .with_context(|| format!("couldn't write {}", path.display()))?;
    match murmur_control::send(Command::Reload) {
        Ok(_) => {}
        // It reads the file when it starts.
        Err(e) if e.downcast_ref::<ControlError>().is_some() => {}
        Err(e) => log::warn!("Couldn't tell the daemon: {e:#}"),
    }
    Ok(())
}

/// Checks `value` the way `Settings::load` would read it, then sets (or, for the default
/// microphone, removes) `key`.
fn apply(fields: &mut Map<String, Value>, key: &str, value: &str) -> anyhow::Result<()> {
    match key {
        "cleanup" => {
            if !["claude", "ollama", "off"].contains(&value) {
                bail!("cleanup is claude, ollama or off");
            }
        }
        "sounds" => {
            if value != "off" && SoundTheme::parse(value).is_none() {
                bail!("sounds is wood, soft, chime, bubble or off");
            }
        }
        "ollamaModel" => {
            if value.trim().is_empty() {
                bail!("ollamaModel needs a model name");
            }
        }
        "microphone" => {
            if value == "default" || value.is_empty() {
                fields.remove(key);
                return Ok(());
            }
        }
        _ => bail!("Unknown setting {key}: use cleanup, ollamaModel, microphone or sounds"),
    }
    fields.insert(key.into(), Value::String(value.into()));
    Ok(())
}

/// The audio sources PipeWire has, and the default one's node name, from `pw-dump`. Empty if
/// it can't be run.
fn sources() -> (Vec<Source>, Option<String>) {
    let dump = Process::new("pw-dump")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok());
    dump.as_ref().map(parse_sources).unwrap_or_default()
}

fn parse_sources(dump: &Value) -> (Vec<Source>, Option<String>) {
    let objects = dump.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut sources: Vec<Source> = objects
        .iter()
        .filter_map(|object| {
            let props = &object["info"]["props"];
            let class = props["media.class"].as_str()?;
            if class != "Audio/Source" && class != "Audio/Source/Virtual" {
                return None;
            }
            let name = props["node.name"].as_str()?.to_string();
            let description = props["node.description"]
                .as_str()
                .or_else(|| props["node.nick"].as_str())
                .unwrap_or(&name)
                .to_string();
            Some(Source { name, description })
        })
        .collect();
    sources.sort_by(|a, b| a.description.cmp(&b.description));
    let default = objects
        .iter()
        .filter(|object| object["type"] == "PipeWire:Interface:Metadata")
        .filter_map(|object| object["metadata"].as_array())
        .flatten()
        .find(|entry| entry["key"] == "default.audio.source")
        .and_then(|entry| entry["value"]["name"].as_str())
        .map(String::from);
    (sources, default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_in(json: Value, key: &str, value: &str) -> anyhow::Result<Value> {
        let Value::Object(mut fields) = json else {
            unreachable!()
        };
        apply(&mut fields, key, value)?;
        Ok(Value::Object(fields))
    }

    #[test]
    fn set_keeps_other_keys() {
        let json = serde_json::json!({"model": "/opt/parakeet", "threads": 4, "cleanup": "off"});
        let json = set_in(json, "cleanup", "ollama").unwrap();
        assert_eq!(
            json,
            serde_json::json!({"model": "/opt/parakeet", "threads": 4, "cleanup": "ollama"})
        );
    }

    #[test]
    fn default_microphone_removes_the_key() {
        let json = serde_json::json!({"microphone": "alsa_input.usb"});
        assert_eq!(set_in(json, "microphone", "default").unwrap(), serde_json::json!({}));
    }

    #[test]
    fn set_rejects_bad_values() {
        let json = serde_json::json!({});
        assert!(set_in(json.clone(), "cleanup", "gpt").is_err());
        assert!(set_in(json.clone(), "sounds", "loud").is_err());
        assert!(set_in(json.clone(), "ollamaModel", " ").is_err());
        assert!(set_in(json.clone(), "hotkey", "rightAlt").is_err());
        assert_eq!(
            set_in(json, "sounds", "off").unwrap(),
            serde_json::json!({"sounds": "off"})
        );
    }

    #[test]
    fn reads_sources_and_the_default_from_pw_dump() {
        let dump = serde_json::json!([
            {"type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Source", "node.name": "t2-mic-fix", "node.description": "MacBook Microphone"}}},
            {"type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Sink", "node.name": "speakers", "node.description": "Speakers"}}},
            {"type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Source", "node.name": "alsa_input.usb", "node.nick": "Yeti"}}},
            {"type": "PipeWire:Interface:Metadata", "metadata": [
                {"subject": 0, "key": "default.audio.sink", "value": {"name": "speakers"}},
                {"subject": 0, "key": "default.audio.source", "value": {"name": "t2-mic-fix"}}]},
        ]);
        let (sources, default) = parse_sources(&dump);
        assert_eq!(
            sources,
            [
                Source {
                    name: "t2-mic-fix".into(),
                    description: "MacBook Microphone".into()
                },
                Source {
                    name: "alsa_input.usb".into(),
                    description: "Yeti".into()
                },
            ]
        );
        assert_eq!(default.as_deref(), Some("t2-mic-fix"));
    }
}
