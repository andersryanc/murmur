//! What the menu shows that's slow to find out, checked off the main thread and reported back
//! through the event loop: the API key, Ollama's models and the connected microphones. (Loading
//! the speech model and the warm-ups belong to `squawk-engine`.)

use std::time::Duration;

use squawk_core::OllamaCleaner;

use crate::tray::state::{AppEvent, Proxy};

/// How often to look for microphones being plugged in or out.
const DEVICE_POLL: Duration = Duration::from_secs(3);

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::error!("Couldn't start the {name} thread: {e}");
    }
}

/// Reads the key store off the main thread, since on macOS that runs `/usr/bin/security`.
pub fn check_api_key(proxy: Proxy) {
    spawn("squawk-api-key", move || {
        let found = squawk_core::api_key::load().is_some();
        log::info!("Anthropic API key {}", if found { "found" } else { "not found" });
        let _ = proxy.send_event(AppEvent::ApiKey(found));
    });
}

pub fn refresh_local_models(proxy: Proxy) {
    spawn("squawk-ollama-models", move || {
        let _ = proxy.send_event(AppEvent::LocalModels(OllamaCleaner::installed_models()));
    });
}

/// Keeps the Microphone menu current as devices come and go. Polls, since cpal has no change
/// notifications; enumerating is cheap.
pub fn watch_devices(proxy: Proxy) {
    spawn("squawk-devices", move || {
        let mut last = None;
        loop {
            let current = (squawk_audio::input_devices(), squawk_audio::default_input_device());
            if last.as_ref() != Some(&current) {
                let (devices, default) = current.clone();
                if proxy.send_event(AppEvent::Devices { devices, default }).is_err() {
                    return; // The event loop has exited.
                }
                last = Some(current);
            }
            std::thread::sleep(DEVICE_POLL);
        }
    });
}
