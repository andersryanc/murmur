//! What the main loop hears about.
//!
//! Everything that happens off the main thread (the hotkey tap, menu clicks, the engine's
//! threads) arrives as an [`AppEvent`] through the event loop's proxy, so all of the app's state
//! lives on the main thread and needs no locks.

use murmur_platform::HotkeyEvent;
use tray_icon::menu::MenuId;

pub type Proxy = tao::event_loop::EventLoopProxy<AppEvent>;
/// What windows are made with; only lent out while the event loop is handling an event.
pub type Target = tao::event_loop::EventLoopWindowTarget<AppEvent>;

pub enum AppEvent {
    Hotkey(HotkeyEvent),
    Menu(MenuId),
    /// Something from the dictation engine: model progress, levels, notices, dictations.
    Engine(murmur_engine::Event),
    /// Whether an Anthropic API key was found, from the menu's own check.
    ApiKey(bool),
    /// Models pulled into Ollama, sorted.
    LocalModels(Vec<String>),
    /// The connected microphones and the system default, when either changes.
    Devices {
        devices: Vec<String>,
        default: Option<String>,
    },
    /// A message (JSON) from the Dictionary window's page.
    Dictionary(String),
}
