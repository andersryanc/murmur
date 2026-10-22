//! The Dictionary window, a port of the Swift `DictionaryView`: your Words (spellings for the
//! cleanup step) and Replacements (exact "when I say X, write Y" rules).
//!
//! On macOS and Windows it's a small web page (`dictionary.html`) in a system web view, so one
//! page serves both. The page does all the editing; the Rust side only reads, writes and watches
//! `dictionary.json`, which stays the source of truth: the dictation pipeline re-reads it every
//! time, and the user can still edit it by hand. (Linux has no tray app; there the Dictionary is
//! a card in the Omarchy plugin, `linux/omarchy-plugin/Dictionary.qml`, ported from this page.)
//!
//! `MURMUR_OPEN_DICTIONARY=1` runs just this window (no hotkey tap, tray or speech model) and
//! quits when it closes, for working on it beside a real Murmur. `MURMUR_DICTIONARY=<file>` edits
//! another file instead of the real one, and `MURMUR_DICTIONARY_SCRIPT=<js>` runs a script in the
//! page once it has loaded, for driving it without typing.

use std::path::PathBuf;

use murmur_core::Dictionary;

mod webview;
pub use webview::DictionaryWindow;

/// Whether to run only the Dictionary window (see the module docs).
pub fn requested() -> bool {
    std::env::var_os("MURMUR_OPEN_DICTIONARY").is_some_and(|value| value == "1")
}

/// The file the window edits: `MURMUR_DICTIONARY` if set, so the window can be tried without
/// touching the real dictionary, otherwise the one the pipeline reads.
pub fn path() -> PathBuf {
    std::env::var_os("MURMUR_DICTIONARY")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(Dictionary::default_path)
}
