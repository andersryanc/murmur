use std::path::PathBuf;

/// Where settings and the dictionary live: `~/Library/Application Support/Squawk` on macOS,
/// `%APPDATA%\Squawk` on Windows, `$XDG_CONFIG_HOME/squawk` (usually `~/.config/squawk`) on Linux.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = xdg("XDG_CONFIG_HOME", ".config").join("squawk");
    #[cfg(not(target_os = "linux"))]
    let dir = dirs::config_dir().unwrap_or_else(home).join("Squawk");
    dir
}

/// Speech models. Under `data_dir()` on macOS and Windows; `$XDG_DATA_HOME/squawk/models` on
/// Linux.
pub fn models_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = xdg("XDG_DATA_HOME", ".local/share").join("squawk/models");
    #[cfg(not(target_os = "linux"))]
    let dir = data_dir().join("models");
    dir
}

/// `~/Library/Logs/Squawk/squawk.log` on macOS, `$XDG_STATE_HOME/squawk/squawk.log` on Linux,
/// `%LOCALAPPDATA%\Squawk\squawk.log` on Windows.
pub fn log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    let dir = home().join("Library/Logs/Squawk");
    #[cfg(target_os = "linux")]
    let dir = xdg("XDG_STATE_HOME", ".local/state").join("squawk");
    #[cfg(target_os = "windows")]
    let dir = dirs::data_local_dir().unwrap_or_else(home).join("Squawk");
    dir.join("squawk.log")
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "linux")]
fn xdg(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}
