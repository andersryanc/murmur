use std::path::PathBuf;

/// Where settings and the dictionary live. The same folders the Swift apps use, so an existing
/// `dictionary.json` carries over: `~/Library/Application Support/Murmur` on macOS,
/// `%APPDATA%\Murmur` on Windows, `$XDG_CONFIG_HOME/murmur` (usually `~/.config/murmur`) on Linux.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = xdg("XDG_CONFIG_HOME", ".config").join("murmur");
    #[cfg(not(target_os = "linux"))]
    let dir = dirs::config_dir().unwrap_or_else(home).join("Murmur");
    dir
}

/// Speech models. Under `data_dir()` on macOS and Windows; `$XDG_DATA_HOME/murmur/models` on
/// Linux, where the Swift Linux build already downloads them.
pub fn models_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = xdg("XDG_DATA_HOME", ".local/share").join("murmur/models");
    #[cfg(not(target_os = "linux"))]
    let dir = data_dir().join("models");
    dir
}

/// `~/Library/Logs/Murmur/murmur.log` on macOS, `$XDG_STATE_HOME/murmur/murmur.log` on Linux,
/// `%LOCALAPPDATA%\Murmur\murmur.log` on Windows.
pub fn log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    let dir = home().join("Library/Logs/Murmur");
    #[cfg(target_os = "linux")]
    let dir = xdg("XDG_STATE_HOME", ".local/state").join("murmur");
    #[cfg(target_os = "windows")]
    let dir = dirs::data_local_dir().unwrap_or_else(home).join("Murmur");
    dir.join("murmur.log")
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "linux")]
fn xdg(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home().join(fallback))
}
