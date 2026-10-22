//! `squawkctl doctor`: checks everything Squawk needs on this machine and prints the fix for
//! anything missing.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use squawk_control::Command;
use squawk_core::{CleanupMode, OllamaCleaner, Settings};

use super::config;

/// Command-line tools Squawk runs, what for, and the Arch package that has them.
const TOOLS: [(&str, &str, &str); 4] = [
    ("wl-copy", "copies to the clipboard", "wl-clipboard"),
    ("wtype", "sends the paste keystroke", "wtype"),
    ("hyprctl", "finds the focused window", "hyprland"),
    ("notify-send", "shows notices when the pill isn't running", "libnotify"),
];

/// Where pipewire-alsa installs the `pipewire` ALSA device, which is how capture reaches
/// PipeWire (and how a `microphone` node name is opened).
const PIPEWIRE_ALSA: [&str; 2] = [
    "/usr/share/alsa/alsa.conf.d/50-pipewire.conf",
    "/etc/alsa/conf.d/50-pipewire.conf",
];

/// The Omarchy shell plugin's folder name, as `linux/install.sh` installs it.
const PLUGIN: &str = "plyhdrn.squawk";

pub fn run() -> ExitCode {
    let config = config::load();
    let settings = &config.settings;
    let mut problems = 0;
    let mut check = |ok: bool, label: &str, fix: &str| {
        if ok {
            println!("✓ {label}");
        } else {
            println!("✗ {label}\n    {fix}");
            problems += 1;
        }
    };

    for (tool, purpose, package) in TOOLS {
        check(
            on_path(tool).is_some(),
            &format!("{tool}: {purpose}"),
            &format!("Install it: sudo pacman -S --needed {package}"),
        );
    }
    check(
        PIPEWIRE_ALSA.iter().any(|path| Path::new(path).exists()),
        "pipewire-alsa: records the microphone through PipeWire",
        "Install it: sudo pacman -S --needed pipewire-alsa",
    );
    match &settings.microphone {
        Some(node) => println!("· Microphone: PipeWire node {node} (falls back to the default if it can't be opened)"),
        None => println!("· Microphone: the default input"),
    }

    let model = squawk_engine::default_model_dir(&config.options);
    let fix = if config.options.model_dir.is_some() {
        "Put a sherpa-onnx Parakeet model there, or remove \"model\" from the settings".to_string()
    } else {
        "`squawkctl daemon` downloads it (~480 MB) when it starts".to_string()
    };
    check(
        squawk_stt::model_present(&model),
        &format!("Speech model at {}", model.display()),
        &fix,
    );

    match settings.cleanup {
        CleanupMode::Claude => check(
            squawk_core::api_key::load().is_some(),
            "Anthropic API key (cleanup: claude)",
            &format!(
                "secret-tool store --label='Squawk' service {} account {}",
                squawk_core::api_key::SERVICE,
                squawk_core::api_key::ACCOUNT
            ),
        ),
        CleanupMode::Ollama => check(
            OllamaCleaner::installed_models().contains(&settings.ollama_model),
            &format!("Ollama model {} (cleanup: ollama)", settings.ollama_model),
            &format!("Start Ollama and run: ollama pull {}", settings.ollama_model),
        ),
        CleanupMode::Off => println!("· Cleanup is off"),
    }

    let status = squawk_control::send(Command::Status);
    let label = match &status {
        Ok(reply) => format!("Daemon running ({reply})"),
        Err(_) => "Daemon running".into(),
    };
    check(
        status.as_ref().is_ok_and(|reply| !reply.starts_with("error:")),
        &label,
        "Run `squawkctl daemon`, or start it from Hyprland's autostart (see the README)",
    );

    if on_path("omarchy-shell").is_some() {
        let plugin = plugin_dir();
        check(
            plugin.join("manifest.json").is_file(),
            &format!("On-screen pill (Omarchy plugin) at {}", plugin.display()),
            "Run linux/install.sh, then: omarchy plugin enable plyhdrn.squawk",
        );
    } else {
        println!("· Not Omarchy: no on-screen pill, so notices go to notify-send");
    }

    println!("\nSettings: {}", Settings::path().display());
    if problems == 0 {
        println!("\nAll good.");
        ExitCode::SUCCESS
    } else {
        println!("\n{problems} problem(s).");
        ExitCode::FAILURE
    }
}

fn plugin_dir() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_default();
    config.join("omarchy/plugins").join(PLUGIN)
}

/// The executable `name` on `$PATH`, if there is one.
fn on_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| {
            path.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tools_on_the_path() {
        assert!(on_path("sh").is_some());
        assert!(on_path("squawk-no-such-tool").is_none());
    }
}
