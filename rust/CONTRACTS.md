# Crate contracts

The interfaces between the crates in this workspace. Each crate is built against this file, so
change it (and its callers) together. Every crate keeps to its own folder.

All audio is 16 kHz mono `f32` in -1...1. Errors are `anyhow::Result`. No async runtime: blocking
calls, run off the main thread by the app.

## murmur-core (no OS-specific code beyond file locations and the key store)

```rust
// context.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetContext { Plain, Chat, Email, Notes, Code }
impl TargetContext {
    pub const ALL: [TargetContext; 5];
    pub fn as_str(self) -> &'static str;                    // "plain", "chat", …
    pub fn parse(s: &str) -> Option<Self>;
    pub fn from_bundle_id(id: Option<&str>) -> Self;        // macOS, prefix match
    pub fn from_window_class(class: Option<&str>) -> Self;  // Linux, substring match
    pub fn from_process_name(exe: Option<&str>) -> Self;    // Windows, e.g. "slack.exe"
}

// prompt.rs
pub const SYSTEM: &str;            // byte-for-byte the Swift CleanupPrompt.system
pub const EMPTY_MARKER: &str;      // "<empty/>"
pub fn finalize(output: &str, transcript: &str) -> String;
pub fn is_only_filler(text: &str) -> bool;
pub fn user_message(transcript: &str, context: TargetContext, vocabulary: &[String]) -> String;

// cleaner.rs
pub trait TextCleaner: Send + Sync {
    fn id(&self) -> String;
    fn prepare(&self) -> anyhow::Result<()> { Ok(()) }
    fn clean(&self, transcript: &str, context: TargetContext) -> anyhow::Result<String>;
}
pub struct ClaudeCleaner { … }   // ClaudeCleaner::new(api_key: String, vocabulary: Vec<String>)
pub struct OllamaCleaner { … }   // OllamaCleaner::new(model: String, vocabulary: Vec<String>)
impl OllamaCleaner { pub fn installed_models() -> Vec<String>; }

// api_key.rs
pub const SERVICE: &str = "com.plyhdrn.murmur";
pub const ACCOUNT: &str = "anthropic-api-key";
pub fn load() -> Option<String>;  // ANTHROPIC_API_KEY, then the OS key store

// dictionary.rs  (same JSON as the Swift app: {"words": [..], "replacements": [{"from","to"}]})
pub struct Dictionary { pub words: Vec<String>, pub replacements: Vec<Replacement> }
pub struct Replacement { pub from: String, pub to: String }
impl Dictionary {
    pub fn vocabulary(&self) -> Vec<String>;
    pub fn apply_replacements(&self, text: &str) -> String;
    pub fn default_path() -> std::path::PathBuf;
    pub fn load(path: &Path) -> anyhow::Result<Self>;   // missing file → empty
    pub fn save(&self, path: &Path) -> anyhow::Result<()>;
}

// voice.rs
pub const SAMPLE_RATE: u32 = 16_000;
pub fn voiced_duration(samples: &[f32]) -> f64;
pub fn contains_speech(samples: &[f32]) -> bool;
pub fn level(samples: &[f32]) -> f32;   // 0...1 meter, -50 dB...0 dB

// sounds.rs
pub enum SoundTheme { Wood, Soft, Chime, Bubble }   // serde lowercase; ALL, label()
impl SoundTheme { pub fn start_sound(self) -> Vec<u8>; pub fn stop_sound(self) -> Vec<u8>; } // 16-bit mono WAV

// paths.rs
pub fn data_dir() -> PathBuf;    // macOS ~/Library/Application Support/Murmur, Windows %APPDATA%\Murmur,
                                 // Linux $XDG_CONFIG_HOME/murmur (matches the Swift dictionary location)
pub fn models_dir() -> PathBuf;  // macOS/Windows data_dir()/models, Linux $XDG_DATA_HOME/murmur/models
pub fn log_path() -> PathBuf;

// settings.rs  (data_dir()/settings.json, every field optional)
pub enum Hotkey { Fn, RightOption, RightCommand, RightAlt, RightCtrl }  // serde camelCase; label()
pub enum CleanupMode { Claude, Ollama, Off }
pub struct Settings {
    pub hotkey: Hotkey,                  // Fn on macOS, RightAlt elsewhere
    pub cleanup: CleanupMode,            // Claude
    pub ollama_model: String,            // "qwen3.5:9b"
    pub microphone: Option<String>,      // device name; None = system default
    pub sounds: Option<SoundTheme>,      // Some(Wood); None = off
}
impl Settings { pub fn load() -> Self; pub fn save(&self) -> anyhow::Result<()>; }
```

## murmur-stt

```rust
pub const DEFAULT_MODEL: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
pub fn default_model_dir() -> PathBuf;                 // murmur_core::paths::models_dir()/DEFAULT_MODEL
pub fn model_present(dir: &Path) -> bool;
pub fn download_model(dir: &Path, progress: impl FnMut(u64, Option<u64>)) -> anyhow::Result<()>;
pub fn default_threads() -> usize;                     // min(cores, 8): faster than 4 on big chips, flat after 8
pub struct Transcriber;                                // Send + Sync
impl Transcriber {
    pub fn new(model_dir: &Path, threads: usize) -> anyhow::Result<Self>;
    pub fn transcribe(&self, samples: &[f32]) -> anyhow::Result<String>;
}
```

## murmur-audio

```rust
pub fn input_devices() -> Vec<String>;                // names, for the Microphone menu
pub fn default_input_device() -> Option<String>;
pub struct Recorder;
impl Recorder {
    /// device None or not connected → system default. on_level gets 0...1 about every 50 ms,
    /// from the audio thread.
    pub fn start(device: Option<&str>, on_level: impl Fn(f32) + Send + 'static) -> anyhow::Result<Self>;
    /// Stops and returns everything recorded, resampled to 16 kHz mono.
    pub fn stop(self) -> Vec<f32>;
}
pub fn play_wav(wav: Vec<u8>, volume: f32);           // fire and forget
```

## murmur-platform (one module per OS behind cfg; same API everywhere)

```rust
pub enum HotkeyEvent { Pressed, Released, Cancelled }
pub struct HotkeyMonitor;
impl HotkeyMonitor {
    /// Starts listening on its own thread. Pressed fires after the key has been held 200 ms
    /// (a quick tap is left to the OS); Cancelled when another key is typed while held.
    pub fn start(key: murmur_core::Hotkey, on_event: impl Fn(HotkeyEvent) + Send + 'static) -> anyhow::Result<Self>;
    pub fn set_key(&self, key: murmur_core::Hotkey);
}
pub fn frontmost_app() -> Option<String>;       // macOS bundle ID, Windows exe name, Linux window class
pub fn target_context() -> murmur_core::TargetContext;  // frontmost_app() mapped with the right from_*
pub enum PasteOutcome { Pasted, CopiedOnly }
pub fn paste(text: &str) -> anyhow::Result<PasteOutcome>;  // clipboard + paste keystroke; text stays on clipboard
pub fn has_input_permission(prompt: bool) -> bool;          // macOS Accessibility; true elsewhere
pub fn notify(message: &str);
```

## murmur (the app)

Tray menu, recording pill, the dictation queue, settings. Uses only the APIs above.
