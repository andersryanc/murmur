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

// settings.rs  (data_dir()/settings.json, every field optional; on Linux data_dir()/config.json,
//               the hand-edited file the Swift Linux build already wrote, read in place)
pub enum Hotkey { Fn, RightOption, RightCommand, RightAlt, RightCtrl }  // serde camelCase; label()
pub enum CleanupMode { Claude, Ollama, Off }
pub struct Settings {
    pub hotkey: Hotkey,                  // Fn on macOS, RightAlt elsewhere
    pub cleanup: CleanupMode,            // Claude
    pub ollama_model: String,            // "qwen3.5:9b"
    pub microphone: Option<String>,      // device name; None = system default
    pub sounds: Option<SoundTheme>,      // Some(Wood); None = off
}
impl Settings { pub fn path() -> PathBuf; pub fn load() -> Self; pub fn save(&self) -> anyhow::Result<()>; }

// bench.rs
pub const CASES: [(TargetContext, &str); 5];   // MurmurKit's benchCases, for `murmur bench`
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
    /// from the audio thread. On Linux a name that isn't an ALSA device is a PipeWire node
    /// name, opened as ALSA `pipewire:NODE="<name>"` (pipewire-alsa); failing that, the default.
    pub fn start(device: Option<&str>, on_level: impl Fn(f32) + Send + 'static) -> anyhow::Result<Self>;
    /// Stops and returns everything recorded, resampled to 16 kHz mono.
    pub fn stop(self) -> Vec<f32>;
}
pub fn play_wav(wav: Vec<u8>, volume: f32);           // fire and forget
pub fn load_wav(path: &Path) -> anyhow::Result<Vec<f32>>;  // any WAV → 16 kHz mono
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

## murmur-control (Unix only; the Linux daemon's control socket)

Hyprland key bindings run `murmur start` / `murmur stop`, which send one command and print the
reply; the Omarchy plugin (`linux/omarchy-plugin`) sends `watch` and stays connected for events.
Plain text, one line each way. The plugin parses it, so the wire format doesn't change.

```rust
pub fn socket_path() -> PathBuf;   // $XDG_RUNTIME_DIR/murmur/murmur.sock, else /tmp/murmur-<uid>/murmur/murmur.sock
                                   // (the same path the plugin computes)
pub enum Command { Start, Stop, Toggle, Cancel, Status }   // "start", …; ALL, as_str(), parse(), Display
// The daemon's replies (free text, for people): start → "recording" | "already recording" |
// "error: …"; stop → "processing" | "queued" | "no speech" | "not recording"; cancel →
// "cancelled" | "not recording"; status → "loading" | "ready" | "recording" | "processing" |
// "error: …" (the model failed to load).
pub const WATCH: &str = "watch";   // handled by the server, never reaches the handler
pub enum Phase { Ready, Recording, Processing }            // as_str(), parse(), Display
pub enum Event {                   // Display gives the line (no newline); parse() reads one
    State(Phase),                  // "state recording"
    Queue(usize),                  // "queue 2": waiting to paste, including the one being processed
    Level(f32),                    // "level 0.42": 0...1, two decimals, ~every 50 ms while recording
    Notice(String),                // "notice <text>": newlines become spaces
}
pub enum ControlError { NotRunning, AlreadyRunning }       // std Error; downcast from anyhow

// Client
pub fn send(command: Command) -> anyhow::Result<String>;  // to socket_path(); the reply, trimmed
pub fn send_to(path: &Path, command: Command) -> anyhow::Result<String>;
pub fn watch(path: &Path) -> anyhow::Result<Watch>;       // Iterator<Item = Event>; set_read_timeout()

// Server: its own thread; dropping it stops the thread and removes the socket file
pub struct Server;
impl Server {
    /// Fails with AlreadyRunning if something answers on `path`; replaces a stale socket file.
    /// `handler` runs on the server thread, one command at a time; unknown commands get
    /// "unknown command: <line>" without reaching it.
    pub fn start(path: &Path, handler: impl FnMut(Command) -> String + Send + 'static) -> anyhow::Result<Self>;
    /// From any thread; never blocks. Drops watchers that are gone or not reading. Returns how
    /// many got it (0 → nobody saw a notice, so fall back to murmur_platform::notify).
    /// The latest Queue and State are sent to each new watcher first.
    pub fn broadcast(&self, event: &Event) -> usize;
    pub fn watcher_count(&self) -> usize;
    pub fn path(&self) -> &Path;
}
```

## murmur-engine (the dictation loop every front end shares)

Recording, the queue and its worker, model loading, the warm-ups and notices. Owned by one
thread, which makes every call; its own threads report through `on_event`, which must return
quickly and never call back into the engine.

```rust
pub const MINIMUM_HOLD: Duration;   // 300 ms: shorter recordings are taps, never transcribed
pub const SOUND_VOLUME: f32;        // 0.6, the start/stop sounds (and the menu's preview)

pub struct Options {                // where the speech model comes from; Default = the Mac's
    pub model_dir: Option<PathBuf>, // never downloaded into
    pub threads: Option<usize>,     // None → murmur_stt::default_threads()
}
pub enum ModelState { Loading(String), Ready, Failed(String) }   // Loading carries progress text
pub enum Event {
    Model(ModelState),      // including download progress, ~2/s
    Recording(bool),        // started, or ended (queued, skipped or cancelled)
    Queue(usize),           // waiting to paste, incl. the one processing; sent in order
    Level(f32),             // 0...1, ~every 50 ms while recording (a few may precede Recording(true))
    Notice(String),         // "Cleanup failed, pasted raw text", "Couldn't start the microphone: …"
    Dictated(String),       // pasted (or left on the clipboard)
    Timing(Timing),
    ApiKey(bool),           // whether a key was found when a dictation needed one
}
pub enum Press { Started, AlreadyRecording, NotReady, Failed(String) }  // Failed also sent as a Notice
pub enum Release { Queued(usize), NoSpeech, NotRecording }             // Queued(n): n incl. this one
pub struct Timing { pub speech: Duration, pub cleanup: Option<(Duration, String)> }  // summary()

pub struct Engine;                  // Send
impl Engine {
    /// Loads (downloads) the model, warms up the mic, and prepares Ollama if that's the cleanup,
    /// each on its own thread; returns at once.
    pub fn start(settings: Settings, options: Options, on_event: impl Fn(Event) + Send + Sync + 'static) -> Self;
    pub fn idle(settings: Settings, status: &str) -> Self;   // never ready (demo modes, tests)
    pub fn settings(&self) -> &Settings;
    pub fn set_settings(&mut self, settings: Settings);     // from the next recording on
    pub fn model_state(&self) -> ModelState;
    pub fn is_recording(&self) -> bool;
    pub fn queued(&self) -> usize;
    /// Blocks until audio is arriving; plays the start sound. `context` = what's in front now.
    pub fn press(&mut self, context: TargetContext) -> Press;
    pub fn release(&mut self) -> Release;   // stop sound; queues unless it was a tap
    pub fn cancel(&mut self) -> bool;       // stop sound; throws the recording away
}

pub fn default_model_dir(options: &Options) -> PathBuf;
/// Blocking: download if need be (default folder only), load, warm up.
pub fn load_model(options: &Options, progress: impl FnMut(String)) -> anyhow::Result<Transcriber>;
pub fn prepare_local_model(model: String);   // own thread; failure → murmur_platform::notify
pub fn load_dictionary() -> Dictionary;      // re-read per dictation; unreadable → empty
pub struct MissingApiKey;                    // std Error
pub fn cleaner(mode: CleanupMode, ollama_model: &str, vocabulary: Vec<String>)
    -> Result<Option<Box<dyn TextCleaner>>, MissingApiKey>;   // None = cleanup off
```

## murmur (the binary)

`main` picks the front end by OS; both use only the APIs above.

- **macOS (and later Windows): the tray app** (`src/tray`). The hotkey's Pressed, Released and
  Cancelled become `press`, `release` and `cancel`; menu choices become `set_settings` (and
  `prepare_local_model`). Engine events arrive through the tao event loop; the pill, icon and
  status line read `is_recording`, `queued` and `model_state` when they redraw.
- **Linux: the CLI and daemon** (`src/linux`). The daemon's control-socket handler owns the
  engine and turns `start`/`stop`/`toggle`/`cancel`/`status` into its calls. Engine events go to
  the main thread, which broadcasts `queue`, `state`, `level` and `notice` lines to watchers;
  a notice nobody watched goes to `notify()`. `config.json`'s `model` and `threads` become
  `Options`.
