//! The recording pill: the small overlay near the bottom of the screen that shows Murmur
//! listening (a red dot and the microphone level), working ("Murmuring…") or warning about
//! something for a moment.
//!
//! The drawing (`render.rs`) is plain Rust shared by every platform; only the window that shows
//! it is OS-specific. The window never takes focus: if it did, the paste would land in the
//! wrong place. Where there's no window yet (Windows), [`Pill`]
//! does nothing and the app falls back to system notifications for notices.
//!
//! It animates only while visible, at about 30 fps, so the idle app does no work for it.

mod render;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::Window;

#[cfg(not(target_os = "macos"))]
mod none;
#[cfg(not(target_os = "macos"))]
use none::Window;

use std::time::{Duration, Instant};

use render::{Content, Renderer};

/// How many level bars the meter shows.
pub const LEVELS: usize = 14;

const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// How long a notice stays up.
const NOTICE: Duration = Duration::from_millis(2500);
const FADE: Duration = Duration::from_millis(150);
/// The width eases toward its new size over roughly 0.25 s.
const WIDTH_EASING: f32 = 0.06;
/// The level bars ease toward each new reading over about 0.08 s.
const LEVEL_EASING: f32 = 0.08;

/// What the app wants the pill to show; `None` hides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Recording,
    Processing,
    Notice,
}

pub struct Pill {
    window: Option<Window>,
    /// Made on first show, so the font is only read if the pill is ever used.
    renderer: Option<Renderer>,
    /// Latest readings, newest last.
    levels: [f32; LEVELS],
    /// What the bars show, easing toward `levels`.
    shown_levels: [f32; LEVELS],
    /// The message and when it stops showing. Kept after that so it can fade out.
    notice: Option<(String, Instant)>,
    shown: Option<Shown>,
    /// The thinking dots' clock.
    epoch: Instant,
}

/// The pill while it's on screen (or fading out).
struct Shown {
    mode: Mode,
    width: f32,
    scale: f32,
    fading_since: Option<Instant>,
    last_frame: Instant,
}

impl Pill {
    /// Call on the main thread.
    pub fn new() -> Self {
        Self {
            window: Window::new(),
            renderer: None,
            levels: [0.0; LEVELS],
            shown_levels: [0.0; LEVELS],
            notice: None,
            shown: None,
            epoch: Instant::now(),
        }
    }

    /// Whether there's a pill on this platform, so notices can go to it rather than to a
    /// system notification.
    pub fn available(&self) -> bool {
        self.window.is_some()
    }

    /// A new recording: flat bars, and any notice from the last dictation is done with.
    pub fn start_recording(&mut self) {
        self.levels = [0.0; LEVELS];
        self.shown_levels = [0.0; LEVELS];
        self.notice = None;
    }

    pub fn push_level(&mut self, level: f32) {
        self.levels.rotate_left(1);
        self.levels[LEVELS - 1] = level;
    }

    /// Shows `message` for a couple of seconds, e.g. when cleanup failed.
    pub fn flash(&mut self, message: &str, now: Instant) {
        self.notice = Some((message.to_string(), now + NOTICE));
    }

    pub fn has_notice(&self, now: Instant) -> bool {
        self.notice.as_ref().is_some_and(|(_, until)| now < *until)
    }

    /// When the next frame is due, while the pill is on screen.
    pub fn next_frame(&self) -> Option<Instant> {
        self.shown.as_ref().map(|shown| shown.last_frame + FRAME)
    }

    /// Where the pill is on screen, in points from the top left of the main display, while
    /// it's showing.
    pub fn screen_rect(&self) -> Option<(f64, f64, f64, f64)> {
        self.shown.as_ref()?;
        self.window.as_ref().map(Window::screen_rect)
    }

    /// Brings the pill up to date with what the app wants shown. Cheap to call after every
    /// event: it only draws when the mode changes or a frame is due.
    pub fn update(&mut self, wanted: Option<Mode>, now: Instant) {
        let Some(window) = &mut self.window else { return };
        match (wanted, &mut self.shown) {
            (None, None) => return,
            (Some(mode), None) => {
                // Placed when it appears, on the screen the mouse is on, and left there.
                let scale = window.place();
                let renderer = self.renderer.get_or_insert_with(Renderer::with_system_font);
                let width = renderer.pill_width(&content(mode, &self.levels, &self.notice, 0.0));
                self.shown_levels = self.levels;
                self.shown = Some(Shown {
                    mode,
                    width,
                    scale,
                    fading_since: None,
                    last_frame: now,
                });
                self.draw(now, 0.0);
                if let Some(window) = &self.window {
                    window.set_alpha(1.0);
                    window.show();
                }
                return;
            }
            (Some(mode), Some(shown)) => {
                let changed = shown.mode != mode || shown.fading_since.is_some();
                shown.mode = mode;
                if shown.fading_since.take().is_some() {
                    window.set_alpha(1.0);
                }
                if !changed && now < shown.last_frame + FRAME {
                    return;
                }
            }
            (None, Some(shown)) => {
                let since = *shown.fading_since.get_or_insert(now);
                let t = now.duration_since(since).as_secs_f32() / FADE.as_secs_f32();
                if t >= 1.0 {
                    window.hide();
                    self.shown = None;
                    return;
                }
                if now < shown.last_frame + FRAME && t > 0.0 {
                    return;
                }
                window.set_alpha(1.0 - t);
            }
        }
        let dt = self
            .shown
            .as_ref()
            .map_or(0.0, |shown| now.duration_since(shown.last_frame).as_secs_f32());
        self.draw(now, dt);
    }

    /// Advances the animations by `dt` seconds and puts a new frame on screen.
    fn draw(&mut self, now: Instant, dt: f32) {
        let (Some(window), Some(renderer), Some(shown)) = (&self.window, &self.renderer, &mut self.shown) else {
            return;
        };
        let level_step = (dt / LEVEL_EASING).min(1.0);
        for (shown_level, level) in self.shown_levels.iter_mut().zip(self.levels) {
            *shown_level += (level - *shown_level) * level_step;
        }
        let time = now.duration_since(self.epoch).as_secs_f64();
        let content = content(shown.mode, &self.shown_levels, &self.notice, time);
        let target = renderer.pill_width(&content);
        shown.width += (target - shown.width) * (1.0 - (-dt / WIDTH_EASING).exp());
        if (target - shown.width).abs() < 0.25 {
            shown.width = target;
        }
        shown.last_frame = now;
        window.set_image(&renderer.draw(&content, shown.width, shown.scale));
    }
}

fn content<'a>(mode: Mode, levels: &'a [f32], notice: &'a Option<(String, Instant)>, time: f64) -> Content<'a> {
    match (mode, notice) {
        (Mode::Recording, _) => Content::Recording(levels),
        (Mode::Notice, Some((message, _))) => Content::Notice(message),
        _ => Content::Processing { time },
    }
}
