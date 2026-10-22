//! macOS: a Quartz event tap for the hotkey, the general pasteboard plus a synthetic ⌘V for
//! pasting, Accessibility for permission, and NSWorkspace for the frontmost app.

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::{Command, Stdio};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSWorkspace};
use objc2_application_services::{AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFMachPort, CFRetained, CFRunLoop, CFString, kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGKeyCode,
};
use objc2_foundation::NSString;
use squawk_core::{Hotkey, TargetContext};

use crate::{HoldTracker, HotkeyEvent, KeyInput, PasteOutcome};

// MARK: - Hotkey

/// The hotkey as the event tap sees it: the key code in its flagsChanged events and the
/// modifier flag that says whether it's down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MacKey {
    code: CGKeyCode,
    flag: CGEventFlags,
}

impl MacKey {
    /// All the hotkeys are modifier keys, so holding them never types anything. Right Alt is
    /// the same key as Right Option; Right Ctrl isn't offered on the Mac.
    fn new(key: Hotkey) -> anyhow::Result<Self> {
        let (code, flag) = match key {
            Hotkey::Fn => (63, CGEventFlags::MaskSecondaryFn),
            Hotkey::RightOption | Hotkey::RightAlt => (61, CGEventFlags::MaskAlternate),
            Hotkey::RightCommand => (54, CGEventFlags::MaskCommand),
            Hotkey::RightCtrl => {
                bail!("Right Ctrl can't be the hotkey on macOS; use fn, Right Option or Right Command")
            }
        };
        Ok(Self { code, flag })
    }
}

/// Reduces a tapped event to what the [`HoldTracker`] needs, or `None` to ignore it.
fn classify(event_type: CGEventType, key_code: CGKeyCode, flags: CGEventFlags, key: MacKey) -> Option<KeyInput> {
    if event_type == CGEventType::KeyDown {
        Some(KeyInput::OtherKey)
    } else if event_type == CGEventType::FlagsChanged && key_code == key.code {
        Some(if flags.contains(key.flag) {
            KeyInput::HotkeyDown
        } else {
            KeyInput::HotkeyUp
        })
    } else {
        None
    }
}

/// How long the tap thread sleeps between checks when nothing is pending. It is woken early for
/// a key change or shutdown, so this only bounds how stale a missed wake-up can get.
const IDLE_WAIT: Duration = Duration::from_secs(1);

/// Watches for the push-to-talk key going down and up anywhere in the system.
///
/// Listens with a Quartz event tap at the HID level, the first stop for keyboard events. Other
/// dictation apps (Wispr Flow, for one) install *active* taps at the later session level that
/// swallow fn, and an ordinary NSEvent global monitor sits behind those, so it would never see
/// the key. This tap is listen-only: it never blocks or changes events for anyone else.
///
/// The tap runs on its own thread with its own CFRunLoop, and `on_event` is called on that
/// thread, so it should hand the event off (e.g. to a channel) rather than do work there: a tap
/// that stalls gets switched off by macOS.
///
/// Creating the tap needs Accessibility permission, which Squawk needs anyway to paste. Without
/// it `start` still succeeds: the thread checks `AXIsProcessTrusted` once a second and makes a
/// fresh tap when it turns true, so granting permission in System Settings while the app is
/// running starts the hotkey on its own.
/// [`is_listening`](Self::is_listening) says whether a tap is in place; macOS can hand out a
/// listen-only tap before permission is granted, so pair it with [`has_input_permission`].
pub struct HotkeyMonitor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

/// State the app's thread and the tap thread both touch.
struct Shared {
    /// The key to watch; `None` from `set_key` means unchanged. Taken by the tap thread.
    new_key: Mutex<Option<MacKey>>,
    stop: AtomicBool,
    listening: AtomicBool,
    /// The tap thread's run loop, so it can be woken. `None` until the thread has started.
    run_loop: Mutex<Option<RunLoopHandle>>,
}

/// A CFRunLoop reference that can be sent to other threads to wake or stop the loop.
struct RunLoopHandle(CFRetained<CFRunLoop>);

// SAFETY: only `CFRunLoopWakeUp` and `CFRunLoopStop` are called through this from other
// threads, and Core Foundation documents both as safe to call from any thread.
unsafe impl Send for RunLoopHandle {}

impl Shared {
    fn wake(&self, thread: Option<&JoinHandle<()>>) {
        if let Some(thread) = thread {
            thread.thread().unpark();
        }
        if let Some(RunLoopHandle(run_loop)) = &*lock(&self.run_loop) {
            run_loop.stop();
            run_loop.wake_up();
        }
    }
}

impl HotkeyMonitor {
    /// Starts listening on its own thread. `Pressed` fires after the key has been held 200 ms
    /// (a quick tap is left to macOS, so fn still opens the emoji picker); `Cancelled` when
    /// another key is typed while held.
    pub fn start(key: Hotkey, on_event: impl Fn(HotkeyEvent) + Send + 'static) -> anyhow::Result<Self> {
        let key = MacKey::new(key)?;
        let shared = Arc::new(Shared {
            new_key: Mutex::new(None),
            stop: AtomicBool::new(false),
            listening: AtomicBool::new(false),
            run_loop: Mutex::new(None),
        });
        let thread = thread::Builder::new()
            .name("squawk-hotkey".into())
            .spawn({
                let shared = shared.clone();
                move || tap_thread(shared, key, Box::new(on_event))
            })
            .context("couldn't start the hotkey thread")?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Switches to a different key. A recording in progress on the old key gets `Cancelled`,
    /// since its release will never be seen. An unsupported key (Right Ctrl) is logged and the
    /// old key kept, so the hotkey never silently stops working.
    pub fn set_key(&self, key: Hotkey) {
        match MacKey::new(key) {
            Ok(key) => {
                *lock(&self.shared.new_key) = Some(key);
                self.shared.wake(self.thread.as_ref());
            }
            Err(e) => log::error!("hotkey not changed: {e:#}"),
        }
    }

    /// Whether an event tap is in place. macOS may create one before Accessibility is granted
    /// that sees no keys, so check `has_input_permission(false)` too.
    pub fn is_listening(&self) -> bool {
        self.shared.listening.load(Ordering::Relaxed)
    }
}

impl Drop for HotkeyMonitor {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake(self.thread.as_ref());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::error!("the hotkey thread panicked");
        }
    }
}

/// What the tap callback works with. Lives on the tap thread for as long as the tap does.
struct TapState {
    key: MacKey,
    tracker: HoldTracker,
    on_event: Box<dyn Fn(HotkeyEvent) + Send>,
    tap: Option<CFRetained<CFMachPort>>,
}

impl TapState {
    fn emit(&self, event: HotkeyEvent) {
        log::debug!("hotkey: {event:?}");
        (self.on_event)(event);
    }

    fn handle(&mut self, event_type: CGEventType, key_code: CGKeyCode, flags: CGEventFlags) {
        if event_type == CGEventType::TapDisabledByTimeout || event_type == CGEventType::TapDisabledByUserInput {
            // macOS switches a tap off if it stalls; turn it straight back on.
            log::warn!("hotkey tap was disabled ({}), re-enabling", event_type.0);
            if let Some(tap) = &self.tap {
                CGEvent::tap_enable(tap, true);
            }
            return;
        }
        if let Some(input) = classify(event_type, key_code, flags, self.key) {
            let mut events = Vec::with_capacity(2);
            self.tracker.handle(input, Instant::now(), |e| events.push(e));
            for event in events {
                self.emit(event);
            }
        }
    }
}

fn tap_thread(shared: Arc<Shared>, key: MacKey, on_event: Box<dyn Fn(HotkeyEvent) + Send>) {
    let Some(run_loop) = CFRunLoop::current() else {
        log::error!("hotkey thread has no run loop");
        return;
    };
    *lock(&shared.run_loop) = Some(RunLoopHandle(run_loop.clone()));

    let state = RefCell::new(TapState {
        key,
        tracker: HoldTracker::new(),
        on_event,
        tap: None,
    });
    // The callback gets this pointer back as its user info. `state` outlives the tap: the tap
    // is invalidated below before `state` goes out of scope.
    let user_info = &state as *const RefCell<TapState> as *mut c_void;

    let mut trusted = has_input_permission(false);
    let mut next_trust_check = Instant::now() + IDLE_WAIT;
    let mut tried_once = false;
    let mut logged_failure = false;
    while !shared.stop.load(Ordering::Relaxed) {
        if let Some(key) = lock(&shared.new_key).take() {
            let mut state = state.borrow_mut();
            state.key = key;
            if let Some(event) = state.tracker.reset() {
                state.emit(event);
            }
        }

        // Accessibility gets granted in System Settings while the app is running. macOS may hand
        // out a tap before that which never sees a key, so make a fresh one
        // when permission flips on.
        if Instant::now() >= next_trust_check {
            next_trust_check = Instant::now() + IDLE_WAIT;
            let now_trusted = has_input_permission(false);
            if now_trusted && !trusted {
                log::info!("Accessibility granted; reinstalling the hotkey tap");
                remove_tap(&mut state.borrow_mut(), &shared);
                tried_once = false;
            }
            trusted = now_trusted;
        }

        if state.borrow().tap.is_none() {
            // Try once at launch, then again only once Accessibility has been granted, rather
            // than asking macOS for a tap every second regardless.
            if !tried_once || trusted {
                tried_once = true;
                match install_tap(&run_loop, user_info) {
                    Some(tap) => {
                        state.borrow_mut().tap = Some(tap);
                        shared.listening.store(true, Ordering::Relaxed);
                        log::info!("hotkey tap installed");
                    }
                    None if !logged_failure => {
                        logged_failure = true;
                        log::warn!("couldn't create the hotkey event tap; waiting for Accessibility permission");
                    }
                    None => {}
                }
            }
            if state.borrow().tap.is_none() {
                thread::park_timeout(IDLE_WAIT);
                continue;
            }
        }

        let wait = state
            .borrow()
            .tracker
            .deadline()
            .map_or(IDLE_WAIT, |at| at.saturating_duration_since(Instant::now()))
            .min(IDLE_WAIT);
        // SAFETY: reading an immutable CF constant.
        let mode = unsafe { kCFRunLoopDefaultMode };
        // Return after each event so a new deadline is picked up straight away.
        CFRunLoop::run_in_mode(mode, wait.as_secs_f64(), true);

        let mut state = state.borrow_mut();
        if let Some(event) = state.tracker.poll(Instant::now()) {
            state.emit(event);
        }
    }

    remove_tap(&mut state.borrow_mut(), &shared);
}

/// Takes the tap down and forgets the current hold, cancelling a recording in progress since
/// its release will never be seen.
fn remove_tap(state: &mut TapState, shared: &Shared) {
    shared.listening.store(false, Ordering::Relaxed);
    if let Some(tap) = state.tap.take() {
        CGEvent::tap_enable(&tap, false);
        // Invalidating the port also removes its source from the run loop, so the callback
        // can't run again with a dangling `user_info`.
        tap.invalidate();
    }
    if let Some(event) = state.tracker.reset() {
        state.emit(event);
    }
}

/// Creates the listen-only HID tap and adds it to `run_loop`. `None` means no permission yet.
fn install_tap(run_loop: &CFRunLoop, user_info: *mut c_void) -> Option<CFRetained<CFMachPort>> {
    let mask = (1u64 << CGEventType::FlagsChanged.0) | (1u64 << CGEventType::KeyDown.0);
    // SAFETY: `tap_callback` matches `CGEventTapCallBack` and only reads `user_info` as the
    // `RefCell<TapState>` that `tap_thread` keeps alive until it invalidates this tap.
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(tap_callback),
            user_info,
        )
    }?;
    let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)?;
    // SAFETY: reading an immutable CF constant.
    let common = unsafe { kCFRunLoopCommonModes };
    run_loop.add_source(Some(&source), common);
    CGEvent::tap_enable(&tap, true);
    Some(tap)
}

/// C callback for the event tap. The tap's run loop source is on the tap thread's run loop, so
/// this always runs on that thread, inside `CFRunLoop::run_in_mode`.
unsafe extern "C-unwind" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: the tap only lives while the `RefCell<TapState>` that `user_info` points to does,
    // and this runs on the thread that owns it.
    let state = unsafe { &*(user_info as *const RefCell<TapState>) };
    // SAFETY: Quartz hands us a valid event for the duration of the callback.
    let event_ref = unsafe { event.as_ref() };
    let key_code = CGEvent::integer_value_field(Some(event_ref), CGEventField::KeyboardEventKeycode) as CGKeyCode;
    let flags = CGEvent::flags(Some(event_ref));
    // A panic must not unwind into Quartz. `try_borrow_mut` guards against the app's callback
    // somehow re-entering the run loop.
    let result = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(mut state) = state.try_borrow_mut() {
            state.handle(event_type, key_code, flags);
        }
    }));
    if result.is_err() {
        log::error!("panic in the hotkey handler");
    }
    // Listen-only: pass the event on untouched.
    event.as_ptr()
}

// MARK: - Paste

/// Puts text at the cursor of whatever app is in front, by pasting it.
///
/// Pasting works in nearly every text field (native, Electron, browsers, terminals), unlike
/// typing characters or writing through the Accessibility API. The text stays on the clipboard
/// afterwards, so if no text field had focus you can still paste it yourself.
pub fn paste(text: &str) -> anyhow::Result<PasteOutcome> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // SAFETY: reading an immutable AppKit constant.
        let string_type = unsafe { NSPasteboardTypeString };
        if !pasteboard.setString_forType(&NSString::from_str(text), string_type) {
            bail!("couldn't put the text on the clipboard");
        }
        Ok(())
    })?;

    if !has_input_permission(false) {
        return Ok(PasteOutcome::CopiedOnly);
    }
    send_command_v()?;
    Ok(PasteOutcome::Pasted)
}

fn send_command_v() -> anyhow::Result<()> {
    const V_KEY: CGKeyCode = 9; // "V" on ANSI layouts
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
    for key_down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.as_deref(), V_KEY, key_down)
            .context("couldn't create the ⌘V keystroke")?;
        CGEvent::set_flags(Some(&event), CGEventFlags::MaskCommand);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    Ok(())
}

// MARK: - Permissions

/// Whether Squawk may watch and send keystrokes (Accessibility). With `prompt`, macOS also
/// shows its dialog that sends you to Privacy & Security › Accessibility; the answer comes
/// later, so poll this (without prompting) to see it change.
pub fn has_input_permission(prompt: bool) -> bool {
    if prompt {
        // SAFETY: reading an immutable ApplicationServices constant.
        let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
        let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
        // SAFETY: the options dictionary maps CFString keys to CFBoolean values, as documented.
        unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
    } else {
        // SAFETY: takes no arguments and only reads TCC state.
        unsafe { AXIsProcessTrusted() }
    }
}

// MARK: - Frontmost app

/// The bundle ID of the app in front, e.g. "com.tinyspeck.slackmacgap".
///
/// NSWorkspace keeps this up to date from notifications delivered on the main run loop, so it
/// only changes while the main thread is running one (as the app's event loop does). In a bare
/// command-line program it stays at whatever was in front when it was first asked.
pub fn frontmost_app() -> Option<String> {
    autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        app.bundleIdentifier().map(|id| id.to_string())
    })
}

pub(crate) fn context_for(app: Option<&str>) -> TargetContext {
    TargetContext::from_bundle_id(app)
}

// MARK: - Notifications

/// Shows a notification through `osascript`, which works from an unbundled binary (it appears
/// as coming from Script Editor). The message goes in as an argument to the script's run
/// handler rather than spliced into the source, so quotes and backslashes need no escaping.
pub fn notify(message: &str) {
    log::info!("notify: {message}");
    let child = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "display notification (item 1 of argv) with title \"Squawk\"",
            "-e",
            "end run",
            message,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        // Reap it off-thread so the caller never waits on AppleScript.
        Ok(mut child) => {
            thread::spawn(move || child.wait());
        }
        Err(e) => log::warn!("couldn't run osascript for a notification: {e}"),
    }
}

/// Locks a mutex, carrying on if another thread panicked while holding it: the data here is
/// always left consistent.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_key() -> MacKey {
        MacKey::new(Hotkey::Fn).unwrap()
    }

    #[test]
    fn key_codes() {
        assert_eq!(
            fn_key(),
            MacKey {
                code: 63,
                flag: CGEventFlags::MaskSecondaryFn
            }
        );
        let right_option = MacKey {
            code: 61,
            flag: CGEventFlags::MaskAlternate,
        };
        assert_eq!(MacKey::new(Hotkey::RightOption).unwrap(), right_option);
        assert_eq!(MacKey::new(Hotkey::RightAlt).unwrap(), right_option);
        assert_eq!(
            MacKey::new(Hotkey::RightCommand).unwrap(),
            MacKey {
                code: 54,
                flag: CGEventFlags::MaskCommand
            }
        );
        assert!(MacKey::new(Hotkey::RightCtrl).is_err());
    }

    #[test]
    fn classify_hotkey_down_and_up() {
        let down = CGEventFlags::MaskSecondaryFn;
        let up = CGEventFlags::empty();
        assert_eq!(
            classify(CGEventType::FlagsChanged, 63, down, fn_key()),
            Some(KeyInput::HotkeyDown)
        );
        assert_eq!(
            classify(CGEventType::FlagsChanged, 63, up, fn_key()),
            Some(KeyInput::HotkeyUp)
        );
    }

    #[test]
    fn classify_ignores_other_modifiers() {
        // Left Shift going down while fn is watched.
        assert_eq!(
            classify(CGEventType::FlagsChanged, 56, CGEventFlags::MaskShift, fn_key()),
            None
        );
    }

    #[test]
    fn classify_any_key_down_as_other_key() {
        assert_eq!(
            classify(CGEventType::KeyDown, 0, CGEventFlags::empty(), fn_key()),
            Some(KeyInput::OtherKey)
        );
    }

    #[test]
    fn classify_ignores_tap_disabled_events() {
        assert_eq!(
            classify(CGEventType::TapDisabledByTimeout, 0, CGEventFlags::empty(), fn_key()),
            None
        );
    }

    #[test]
    fn notification_script_takes_the_message_as_an_argument() {
        // Runs the same handler shape as `notify`, but returns the argument instead of
        // showing anything, to check odd characters reach AppleScript intact.
        let message = r#"He said "hi" \ and left"#;
        let out = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "return item 1 of argv",
                "-e",
                "end run",
                message,
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), message);
    }
}
