//! `MURMUR_PILL_DEMO=1`: cycles the pill through recording (with a made-up level), processing
//! and a notice, then quits. For checking how the pill looks without pressing the hotkey; the
//! app skips the hotkey tap, the tray, the microphone and the speech model in this mode, so it
//! can run beside a real Murmur.

use std::time::{Duration, Instant};

use crate::pill::{Mode, Pill};

const RECORDING_UNTIL: Duration = Duration::from_secs(4);
const PROCESSING_UNTIL: Duration = Duration::from_secs(8);
/// The notice shows for its usual time, then the pill fades; quit a moment after that.
const QUIT_AT: Duration = Duration::from_millis(11_500);
const LEVEL_EVERY: Duration = Duration::from_millis(50);

pub fn requested() -> bool {
    std::env::var_os("MURMUR_PILL_DEMO").is_some_and(|value| value == "1")
}

pub struct Demo {
    started: Instant,
    last_level: Instant,
    flashed: bool,
    logged: Option<Mode>,
}

impl Demo {
    pub fn new() -> Self {
        log::info!("Pill demo: recording, processing, a notice, then quit");
        let now = Instant::now();
        Self {
            started: now,
            last_level: now,
            flashed: false,
            logged: None,
        }
    }

    /// What the pill should show now, feeding it levels and the notice as the app would.
    /// `Err(())` once the demo is over.
    pub fn drive(&mut self, pill: &mut Pill, now: Instant) -> Result<Option<Mode>, ()> {
        let elapsed = now.duration_since(self.started);
        let mode = if elapsed < RECORDING_UNTIL {
            if now.duration_since(self.last_level) >= LEVEL_EVERY {
                self.last_level = now;
                pill.push_level(fake_level(elapsed.as_secs_f32()));
            }
            Some(Mode::Recording)
        } else if elapsed < PROCESSING_UNTIL {
            Some(Mode::Processing)
        } else if elapsed < QUIT_AT {
            if !self.flashed {
                self.flashed = true;
                pill.flash("Cleanup failed, pasted raw text", now);
            }
            pill.has_notice(now).then_some(Mode::Notice)
        } else {
            return Err(());
        };
        // Logged once the pill is up, with where to point `screencapture -R`.
        if mode.is_some()
            && mode != self.logged
            && let Some((x, y, w, h)) = pill.screen_rect()
        {
            self.logged = mode;
            log::info!("Pill demo: {mode:?} at {x},{y},{w},{h}");
        }
        Ok(mode)
    }
}

/// Something speech-like: syllables of varying loudness with short gaps.
fn fake_level(t: f32) -> f32 {
    let syllable = ((t * 7.0).sin() * 0.5 + 0.5).powf(1.5);
    let phrase = ((t * 1.3).sin() * 0.5 + 0.5) * 0.8 + 0.2;
    let jitter = ((t * 53.0).sin() * (t * 31.0).cos()).abs() * 0.25;
    (syllable * phrase + jitter).min(1.0)
}
