//! The push-to-talk timing, kept free of any OS code so every platform shares it and it can be
//! tested without a keyboard.
//!
//! Each OS module turns its raw key events into [`KeyInput`]s, feeds them to a [`HoldTracker`]
//! with a timestamp, and calls [`HoldTracker::poll`] when [`HoldTracker::deadline`] comes round
//! so the press can fire while the key is still held.

use std::time::{Duration, Instant};

/// How long the key has to stay down before it counts as push-to-talk. A quicker tap is left to
/// the OS, so tapping fn on a Mac still opens the emoji picker without starting a recording.
pub const HOLD_DELAY: Duration = Duration::from_millis(200);

/// What the hotkey monitor reports to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// The key has been held for [`HOLD_DELAY`]: start recording.
    Pressed,
    /// The key came up after a `Pressed`: finish the recording.
    Released,
    /// Another key was typed while the hotkey was held after a `Pressed`, e.g. Option+Arrow to
    /// jump words. That's a shortcut, not dictation, so the recording should be thrown away.
    Cancelled,
}

/// A key event, already reduced to what the tracker cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyInput {
    /// The hotkey went down. Repeats while it is already down are ignored.
    HotkeyDown,
    /// The hotkey came up.
    HotkeyUp,
    /// Any other key was typed.
    OtherKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// The hotkey is up.
    Idle,
    /// The hotkey is down but hasn't been held long enough yet.
    Pending { press_at: Instant },
    /// `Pressed` has fired and the hotkey is still down.
    Held,
    /// The hotkey is down but this hold has been written off: it was part of a shortcut. Nothing
    /// more is reported until the key comes up.
    Spent,
}

/// Turns hotkey down/up and other-key events into [`HotkeyEvent`]s.
///
/// Every hold produces either nothing (a quick tap, or a shortcut typed before the delay ran out)
/// or `Pressed` followed by exactly one `Released` or `Cancelled`.
///
/// A written-off hold (one that became a shortcut) stays silent until the key comes up, so the
/// app never sees a release or repeated chord it has to ignore.
#[derive(Clone, Debug)]
pub struct HoldTracker {
    state: State,
    delay: Duration,
}

impl Default for HoldTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldTracker {
    pub fn new() -> Self {
        Self::with_delay(HOLD_DELAY)
    }

    pub fn with_delay(delay: Duration) -> Self {
        Self {
            state: State::Idle,
            delay,
        }
    }

    /// When [`poll`](Self::poll) should next be called, or `None` when no press is waiting.
    pub fn deadline(&self) -> Option<Instant> {
        match self.state {
            State::Pending { press_at } => Some(press_at),
            _ => None,
        }
    }

    /// Whether the hotkey is currently down, as far as the tracker knows.
    pub fn is_down(&self) -> bool {
        self.state != State::Idle
    }

    /// Fires `Pressed` once the hold delay has run out.
    pub fn poll(&mut self, now: Instant) -> Option<HotkeyEvent> {
        match self.state {
            State::Pending { press_at } if now >= press_at => {
                self.state = State::Held;
                Some(HotkeyEvent::Pressed)
            }
            _ => None,
        }
    }

    /// Feeds one key event in, passing whatever it produces to `emit` in order.
    ///
    /// A late event can produce two: if the key comes up after the delay but before anyone
    /// polled, that is still a hold, so it reports `Pressed` then `Released`.
    pub fn handle(&mut self, input: KeyInput, now: Instant, mut emit: impl FnMut(HotkeyEvent)) {
        if let Some(event) = self.poll(now) {
            emit(event);
        }
        match (self.state, input) {
            (State::Idle, KeyInput::HotkeyDown) => {
                self.state = State::Pending {
                    press_at: now + self.delay,
                };
            }
            (State::Idle, _) => {}
            // Already down: an auto-repeat or a duplicate flags event.
            (_, KeyInput::HotkeyDown) => {}

            // A quick tap: recording never started, so there's nothing to finish.
            (State::Pending { .. }, KeyInput::HotkeyUp) => self.state = State::Idle,
            // A shortcut typed before the hold delay ran out: never start recording at all.
            (State::Pending { .. }, KeyInput::OtherKey) => self.state = State::Spent,

            (State::Held, KeyInput::HotkeyUp) => {
                self.state = State::Idle;
                emit(HotkeyEvent::Released);
            }
            (State::Held, KeyInput::OtherKey) => {
                self.state = State::Spent;
                emit(HotkeyEvent::Cancelled);
            }

            (State::Spent, KeyInput::HotkeyUp) => self.state = State::Idle,
            (State::Spent, KeyInput::OtherKey) => {}
        }
    }

    /// Forgets the current hold, e.g. when the hotkey changes or the listener is torn down.
    /// Returns `Cancelled` if a recording had started, so the app isn't left recording with no
    /// release ever coming.
    pub fn reset(&mut self) -> Option<HotkeyEvent> {
        let was_held = self.state == State::Held;
        self.state = State::Idle;
        was_held.then_some(HotkeyEvent::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyEvent::*;
    use KeyInput::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Runs a script of (milliseconds, input) pairs, polling at every millisecond in between
    /// the way a real event loop would, and returns what came out with the time it fired.
    fn run(script: &[(u64, KeyInput)]) -> Vec<(u64, HotkeyEvent)> {
        let t0 = Instant::now();
        let mut tracker = HoldTracker::new();
        let mut out = Vec::new();
        let end = script.last().map_or(0, |(t, _)| *t) + 1000;
        let mut next = script.iter().peekable();
        for t in 0..=end {
            let now = t0 + ms(t);
            while let Some((_, input)) = next.next_if(|(at, _)| *at == t) {
                tracker.handle(*input, now, |e| out.push((t, e)));
            }
            if let Some(e) = tracker.poll(now) {
                out.push((t, e));
            }
        }
        out
    }

    #[test]
    fn quick_tap_produces_nothing() {
        assert_eq!(run(&[(0, HotkeyDown), (100, HotkeyUp)]), vec![]);
    }

    #[test]
    fn tap_just_under_the_delay_produces_nothing() {
        assert_eq!(run(&[(0, HotkeyDown), (199, HotkeyUp)]), vec![]);
    }

    #[test]
    fn hold_presses_at_the_delay_then_releases() {
        assert_eq!(
            run(&[(0, HotkeyDown), (1500, HotkeyUp)]),
            vec![(200, Pressed), (1500, Released)]
        );
    }

    #[test]
    fn other_key_while_held_cancels_once() {
        assert_eq!(
            run(&[(0, HotkeyDown), (500, OtherKey), (600, OtherKey), (900, HotkeyUp)]),
            vec![(200, Pressed), (500, Cancelled)]
        );
    }

    #[test]
    fn other_key_during_the_delay_never_presses() {
        assert_eq!(run(&[(0, HotkeyDown), (50, OtherKey), (1000, HotkeyUp)]), vec![]);
    }

    #[test]
    fn other_key_during_the_delay_then_more_keys_stays_silent() {
        assert_eq!(
            run(&[(0, HotkeyDown), (50, OtherKey), (400, OtherKey), (1000, HotkeyUp)]),
            vec![]
        );
    }

    #[test]
    fn other_key_while_up_is_ignored() {
        assert_eq!(run(&[(0, OtherKey), (10, HotkeyUp)]), vec![]);
    }

    #[test]
    fn repeated_down_does_not_restart_the_delay() {
        assert_eq!(
            run(&[(0, HotkeyDown), (150, HotkeyDown), (300, HotkeyUp)]),
            vec![(200, Pressed), (300, Released)]
        );
    }

    #[test]
    fn next_hold_works_after_a_cancelled_one() {
        assert_eq!(
            run(&[
                (0, HotkeyDown),
                (300, OtherKey),
                (400, HotkeyUp),
                (1000, HotkeyDown),
                (1500, HotkeyUp),
            ]),
            vec![(200, Pressed), (300, Cancelled), (1200, Pressed), (1500, Released)]
        );
    }

    #[test]
    fn next_hold_works_after_a_shortcut_in_the_delay() {
        assert_eq!(
            run(&[
                (0, HotkeyDown),
                (100, OtherKey),
                (150, HotkeyUp),
                (1000, HotkeyDown),
                (1300, HotkeyUp),
            ]),
            vec![(1200, Pressed), (1300, Released)]
        );
    }

    #[test]
    fn late_release_without_a_poll_still_presses_first() {
        let t0 = Instant::now();
        let mut tracker = HoldTracker::new();
        let mut out = Vec::new();
        tracker.handle(HotkeyDown, t0, |e| out.push(e));
        tracker.handle(HotkeyUp, t0 + ms(250), |e| out.push(e));
        assert_eq!(out, vec![Pressed, Released]);
    }

    #[test]
    fn late_other_key_without_a_poll_presses_then_cancels() {
        let t0 = Instant::now();
        let mut tracker = HoldTracker::new();
        let mut out = Vec::new();
        tracker.handle(HotkeyDown, t0, |e| out.push(e));
        tracker.handle(OtherKey, t0 + ms(200), |e| out.push(e));
        assert_eq!(out, vec![Pressed, Cancelled]);
    }

    #[test]
    fn deadline_is_only_set_while_pending() {
        let t0 = Instant::now();
        let mut tracker = HoldTracker::new();
        assert_eq!(tracker.deadline(), None);
        tracker.handle(HotkeyDown, t0, |_| {});
        assert_eq!(tracker.deadline(), Some(t0 + HOLD_DELAY));
        assert_eq!(tracker.poll(t0 + ms(199)), None);
        assert_eq!(tracker.poll(t0 + ms(200)), Some(Pressed));
        assert_eq!(tracker.deadline(), None);
        assert_eq!(tracker.poll(t0 + ms(400)), None, "Pressed fires only once");
    }

    #[test]
    fn reset_cancels_only_a_started_recording() {
        let t0 = Instant::now();
        let mut tracker = HoldTracker::new();
        tracker.handle(HotkeyDown, t0, |_| {});
        assert_eq!(tracker.reset(), None);
        assert!(!tracker.is_down());

        tracker.handle(HotkeyDown, t0, |_| {});
        tracker.poll(t0 + ms(200));
        assert_eq!(tracker.reset(), Some(Cancelled));
        // The release of the old hold is now a stray key-up and does nothing.
        let mut out = Vec::new();
        tracker.handle(HotkeyUp, t0 + ms(300), |e| out.push(e));
        assert_eq!(out, vec![]);
    }

    #[test]
    fn custom_delay() {
        let t0 = Instant::now();
        let mut tracker = HoldTracker::with_delay(ms(50));
        tracker.handle(HotkeyDown, t0, |_| {});
        assert_eq!(tracker.poll(t0 + ms(50)), Some(Pressed));
    }
}
