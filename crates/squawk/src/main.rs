//! Squawk: hold a key, speak, and cleaned-up text is pasted at the cursor.
//!
//! One binary, two front ends on the shared `squawk-engine`. On macOS (and later Windows) it's a
//! tray app that watches the hotkey itself. On Linux it's a command-line tool and daemon:
//! Wayland doesn't let apps watch keys, so Hyprland key bindings run `squawkctl start` and
//! `squawkctl stop`, which reach the daemon over its control socket.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod logging;

#[cfg(not(target_os = "linux"))]
mod tray;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(target_os = "linux"))]
fn main() {
    tray::run();
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::run(std::env::args().skip(1).collect())
}
