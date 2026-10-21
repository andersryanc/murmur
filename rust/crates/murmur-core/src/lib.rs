//! Everything in Murmur that doesn't depend on the OS: the cleanup prompt, the cleaners (Claude,
//! Ollama), the personal dictionary, target context, voice activity, sound synthesis, settings
//! and where files live. A port of `Core/Sources/MurmurKit`, which stays the reference until the
//! Swift app is retired.
//!
//! Network calls are blocking (no async runtime): the app runs each dictation on a worker thread.

pub mod api_key;
pub mod cleaner;
pub mod context;
pub mod dictionary;
pub mod paths;
pub mod prompt;
pub mod settings;
pub mod sounds;
pub mod voice;

pub use cleaner::{ClaudeCleaner, OllamaCleaner, TextCleaner};
pub use context::TargetContext;
pub use dictionary::{Dictionary, Replacement};
pub use settings::{CleanupMode, Hotkey, Settings};
pub use sounds::SoundTheme;
pub use voice::SAMPLE_RATE;
