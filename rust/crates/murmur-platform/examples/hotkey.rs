//! Prints hotkey events so the push-to-talk key can be tried by hand.
//!
//!     cargo run -p murmur-platform --example hotkey            # fn
//!     cargo run -p murmur-platform --example hotkey rightOption
//!
//! Keys: fn, rightOption, rightCommand, rightAlt, rightCtrl. On macOS the terminal running this
//! needs Accessibility (System Settings › Privacy & Security › Accessibility); without it the
//! example waits and starts listening as soon as it's granted. Ctrl+C to quit.

use std::sync::mpsc;
use std::time::Duration;

use murmur_core::Hotkey;
use murmur_platform::{HotkeyMonitor, frontmost_app, has_input_permission, target_context};

fn main() -> anyhow::Result<()> {
    let key = match std::env::args().nth(1).as_deref() {
        None | Some("fn") => Hotkey::Fn,
        Some("rightOption") => Hotkey::RightOption,
        Some("rightCommand") => Hotkey::RightCommand,
        Some("rightAlt") => Hotkey::RightAlt,
        Some("rightCtrl") => Hotkey::RightCtrl,
        Some(other) => anyhow::bail!("unknown key {other:?}"),
    };

    if !has_input_permission(true) {
        println!("No input permission yet: grant it in System Settings, then hold the key.");
    }

    let (tx, rx) = mpsc::channel();
    let monitor = HotkeyMonitor::start(key, move |event| {
        let _ = tx.send(event);
    })?;
    println!("Hold {} to see events. Ctrl+C to quit.", key.label());

    let mut was_listening = false;
    loop {
        // Keep the main run loop turning so NSWorkspace notices app switches, as it would in
        // the real app.
        pump_main_run_loop(Duration::from_millis(50));
        while let Ok(event) = rx.try_recv() {
            println!(
                "{event:?}  (front: {}, context: {})",
                frontmost_app().as_deref().unwrap_or("?"),
                target_context().as_str()
            );
        }
        let listening = monitor.is_listening();
        if listening != was_listening {
            println!("listening: {listening}");
            was_listening = listening;
        }
    }
}

#[cfg(target_os = "macos")]
fn pump_main_run_loop(for_: Duration) {
    use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};
    // SAFETY: reading an immutable CF constant.
    let mode = unsafe { kCFRunLoopDefaultMode };
    CFRunLoop::run_in_mode(mode, for_.as_secs_f64(), false);
}

#[cfg(not(target_os = "macos"))]
fn pump_main_run_loop(for_: Duration) {
    std::thread::sleep(for_);
}
