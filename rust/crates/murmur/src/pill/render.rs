//! Draws the recording pill into an RGBA image, a port of the Swift `HUDView`.
//!
//! Plain Rust with no OS calls, so every platform's window shows the same pixels and only
//! putting them on screen differs (see `macos.rs`). Every shape in the pill is a circle or a
//! capsule, so they're drawn from their distance functions, which gives exact anti-aliasing and
//! the soft shadow without a 2D graphics library. Text uses the system UI font through
//! `ab_glyph`, so nothing is bundled and the pill reads like the rest of the OS.

use std::cell::RefCell;

use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont, VariableFont, point};

/// The panel's size in points. The pill sits in the middle with room around it for its shadow,
/// and grows or shrinks to fit what it shows, like the Swift one.
pub const PANEL_WIDTH: f32 = 360.0;
pub const PANEL_HEIGHT: f32 = 72.0;

const PILL_HEIGHT: f32 = 40.0;
const PADDING: f32 = 16.0;
const SPACING: f32 = 10.0;
const FONT_SIZE: f32 = 13.0;

const DOT: f32 = 8.0;
const BAR_WIDTH: f32 = 3.0;
const BAR_SPACING: f32 = 3.0;
const THINKING_DOT: f32 = 6.0;
const THINKING_SPACING: f32 = 4.0;
/// `exclamationmark.circle.fill` at 13 pt is about this wide.
const ICON: f32 = 15.0;

/// SwiftUI's shadow: black at 35 %, radius 12 (a Gaussian with σ of half that), 4 pt down.
const SHADOW_ALPHA: f32 = 0.35;
const SHADOW_SIGMA: f32 = 6.0;
const SHADOW_OFFSET: f32 = 4.0;

/// Colours as straight (not premultiplied) sRGB.
type Rgba = [f32; 4];
const BLACK: Rgba = [0.0, 0.0, 0.0, 1.0];
const BORDER: Rgba = [1.0, 1.0, 1.0, 0.12];
const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
/// `Murmur.recording` in the Swift palette.
const RED: Rgba = [1.0, 0.23, 0.19, 1.0];
/// SwiftUI's `.orange`.
const ORANGE: Rgba = [1.0, 0.584, 0.0, 1.0];

/// What the pill shows.
#[derive(Clone, Copy, Debug)]
pub enum Content<'a> {
    /// The red dot and the microphone level, oldest first.
    Recording(&'a [f32]),
    /// Thinking dots and "Murmuring…"; `time` in seconds drives the dots.
    Processing { time: f64 },
    /// An orange warning icon and a short message.
    Notice(&'a str),
}

const PROCESSING_TEXT: &str = "Murmuring…";

/// Premultiplied RGBA, `width × height` pixels, rows top to bottom.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

pub struct Renderer {
    font: Option<FontVec>,
    /// The shadow, capsule and border for the last width and scale. They change only while
    /// the width animates, and the shadow is most of the drawing.
    background: RefCell<Option<Background>>,
}

struct Background {
    width: f32,
    scale: f32,
    data: Vec<f32>,
}

impl Renderer {
    /// `font` is the UI font's file; without one the pill still draws, minus its text.
    pub fn new(font: Option<FontVec>) -> Self {
        Self {
            font,
            background: RefCell::new(None),
        }
    }

    /// Loads the system UI font at medium weight, as the Swift pill uses.
    pub fn with_system_font() -> Self {
        let font = system_font();
        if font.is_none() {
            log::warn!("No system font found; the pill will have no text");
        }
        Self::new(font)
    }

    /// How wide the pill is for this content, in points.
    pub fn pill_width(&self, content: &Content) -> f32 {
        let inner = match content {
            Content::Recording(levels) => DOT + SPACING + bars_width(levels.len()),
            Content::Processing { .. } => thinking_width() + SPACING + self.text_width(PROCESSING_TEXT),
            Content::Notice(text) => ICON + SPACING + self.text_width(&self.fit(text)),
        };
        inner + 2.0 * PADDING
    }

    /// Draws the pill, `pill_width` points wide (animated by the caller), at `scale` pixels per
    /// point so it's sharp on Retina screens.
    pub fn draw(&self, content: &Content, pill_width: f32, scale: f32) -> Image {
        let mut canvas = Canvas::new(scale);
        let cy = PANEL_HEIGHT / 2.0;
        {
            let mut cached = self.background.borrow_mut();
            match &*cached {
                Some(bg) if bg.width == pill_width && bg.scale == scale => canvas.data.clone_from(&bg.data),
                _ => {
                    let pill = Rect {
                        x: (PANEL_WIDTH - pill_width) / 2.0,
                        y: cy - PILL_HEIGHT / 2.0,
                        w: pill_width,
                        h: PILL_HEIGHT,
                    };
                    canvas.shadow(pill.offset(0.0, SHADOW_OFFSET));
                    canvas.capsule(pill, BLACK);
                    canvas.capsule_border(pill, 0.5, BORDER);
                    *cached = Some(Background {
                        width: pill_width,
                        scale,
                        data: canvas.data.clone(),
                    });
                }
            }
        }

        // Content is centred in the pill, so it stays put while the width animates.
        let natural = self.pill_width(content) - 2.0 * PADDING;
        let mut x = (PANEL_WIDTH - natural) / 2.0;
        match content {
            Content::Recording(levels) => {
                canvas.circle(x + DOT / 2.0, cy, DOT / 2.0, RED);
                x += DOT + SPACING;
                for level in *levels {
                    let h = 4.0 + level.clamp(0.0, 1.0) * 18.0;
                    let bar = Rect {
                        x,
                        y: cy - h / 2.0,
                        w: BAR_WIDTH,
                        h,
                    };
                    canvas.capsule(bar, WHITE);
                    x += BAR_WIDTH + BAR_SPACING;
                }
            }
            Content::Processing { time } => {
                for i in 0..3 {
                    let pulse = (time * 6.0 - i as f64 * 0.8).sin().max(0.0) as f32;
                    let mut color = WHITE;
                    color[3] = 0.35 + 0.65 * pulse;
                    canvas.circle(x + THINKING_DOT / 2.0, cy, THINKING_DOT / 2.0, color);
                    x += THINKING_DOT + THINKING_SPACING;
                }
                x += SPACING - THINKING_SPACING;
                self.text(&mut canvas, PROCESSING_TEXT, x, cy, [1.0, 1.0, 1.0, 0.9]);
            }
            Content::Notice(text) => {
                warning_icon(&mut canvas, x, cy);
                x += ICON + SPACING;
                self.text(&mut canvas, &self.fit(text), x, cy, WHITE);
            }
        }
        canvas.into_image()
    }

    /// The text cut short with "…" if it would make the pill wider than the panel (the Swift
    /// pill's `lineLimit(1)`).
    fn fit(&self, text: &str) -> String {
        let room = PANEL_WIDTH - 2.0 * (PADDING + SHADOW_SIGMA * 2.0) - ICON - SPACING;
        if self.text_width(text) <= room {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
            if self.text_width(&candidate) <= room {
                return candidate;
            }
        }
        "…".into()
    }

    fn text_width(&self, text: &str) -> f32 {
        let Some(font) = &self.font else { return 0.0 };
        let font = font.as_scaled(px_scale(font, FONT_SIZE));
        let mut width = 0.0;
        let mut previous: Option<GlyphId> = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(previous) = previous {
                width += font.kern(previous, id);
            }
            width += font.h_advance(id);
            previous = Some(id);
        }
        width
    }

    /// Draws one line of text starting at `x`, its line box centred on `cy` like SwiftUI does.
    fn text(&self, canvas: &mut Canvas, text: &str, x: f32, cy: f32, color: Rgba) {
        let Some(font) = &self.font else { return };
        let s = canvas.scale;
        let font = font.as_scaled(px_scale(font, FONT_SIZE * s));
        let baseline = cy * s + (font.ascent() + font.descent()) / 2.0;
        let mut pen = x * s;
        let mut previous: Option<GlyphId> = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(previous) = previous {
                pen += font.kern(previous, id);
            }
            let glyph = id.with_scale_and_position(font.scale(), point(pen, baseline));
            pen += font.h_advance(id);
            previous = Some(id);
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i64 + gx as i64;
                let py = bounds.min.y as i64 + gy as i64;
                canvas.blend(px, py, color, coverage);
            });
        }
    }
}

/// `exclamationmark.circle.fill`: an orange disc with the mark cut out of it.
fn warning_icon(canvas: &mut Canvas, x: f32, cy: f32) {
    let r = ICON / 2.0;
    let cx = x + r;
    canvas.circle(cx, cy, r, ORANGE);
    let stroke = 1.9;
    let stem = Rect {
        x: cx - stroke / 2.0,
        y: cy - r * 0.58,
        w: stroke,
        h: r * 0.82,
    };
    canvas.capsule(stem, BLACK);
    canvas.circle(cx, cy + r * 0.48, stroke * 0.58, BLACK);
}

fn bars_width(count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    count as f32 * BAR_WIDTH + (count - 1) as f32 * BAR_SPACING
}

fn thinking_width() -> f32 {
    3.0 * THINKING_DOT + 2.0 * THINKING_SPACING
}

/// ab_glyph scales by line height rather than by em, so convert from a point size.
fn px_scale(font: &FontVec, size: f32) -> PxScale {
    let em = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(size * font.height_unscaled() / em)
}

/// The OS's UI font, set to medium weight where it's a variable font. These ship with every
/// install, so nothing needs bundling: SF Pro on macOS and Segoe UI on Windows.
fn system_font() -> Option<FontVec> {
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[&str] = &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc"];
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[&str] = &["C:\\Windows\\Fonts\\segoeuisb.ttf", "C:\\Windows\\Fonts\\segoeui.ttf"];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    CANDIDATES.iter().find_map(|path| {
        let bytes = std::fs::read(path).ok()?;
        let mut font = FontVec::try_from_vec(bytes).ok()?;
        // SwiftUI's .medium: SF's weight axis puts Regular at 400, Medium at 510 and Bold at 700.
        // Its optical size axis starts at 17, the Text cut AppKit uses for small sizes (the
        // default, 28, is the wider-set Display cut). Fonts without these axes ignore them.
        font.set_variation(b"wght", 510.0);
        font.set_variation(b"opsz", 17.0);
        Some(font)
    })
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    fn scaled(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            w: self.w * s,
            h: self.h * s,
        }
    }

    /// Signed distance from (px, py) to the capsule filling this rect: negative inside.
    fn capsule_distance(&self, px: f32, py: f32) -> f32 {
        let r = self.w.min(self.h) / 2.0;
        // The capsule is every point within r of the segment between its two end centres.
        let (ax, ay, bx, by) = if self.w >= self.h {
            let cy = self.y + self.h / 2.0;
            (self.x + r, cy, self.x + self.w - r, cy)
        } else {
            let cx = self.x + self.w / 2.0;
            (cx, self.y + r, cx, self.y + self.h - r)
        };
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (qx, qy) = (ax + t * dx - px, ay + t * dy - py);
        (qx * qx + qy * qy).sqrt() - r
    }
}

/// A premultiplied RGBA buffer the size of the panel at `scale`, drawn into in points.
struct Canvas {
    scale: f32,
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Canvas {
    fn new(scale: f32) -> Self {
        let width = (PANEL_WIDTH * scale).round() as usize;
        let height = (PANEL_HEIGHT * scale).round() as usize;
        Self {
            scale,
            width,
            height,
            data: vec![0.0; width * height * 4],
        }
    }

    /// Source-over with `color` at `coverage` (0...1).
    fn blend(&mut self, x: i64, y: i64, color: Rgba, coverage: f32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height || coverage <= 0.0 {
            return;
        }
        let a = color[3] * coverage.min(1.0);
        let i = (y as usize * self.width + x as usize) * 4;
        let pixel = &mut self.data[i..i + 4];
        for c in 0..3 {
            pixel[c] = color[c] * a + pixel[c] * (1.0 - a);
        }
        pixel[3] = a + pixel[3] * (1.0 - a);
    }

    /// Fills the pixels around `rect` (in points, grown by `margin`) with `color` at whatever
    /// coverage `cover` gives for each pixel centre's position in pixels.
    fn fill(&mut self, rect: Rect, margin: f32, color: Rgba, cover: impl Fn(f32, f32) -> f32) {
        let s = self.scale;
        let x0 = ((rect.x - margin) * s).floor().max(0.0) as i64;
        let y0 = ((rect.y - margin) * s).floor().max(0.0) as i64;
        let x1 = ((rect.x + rect.w + margin) * s).ceil() as i64;
        let y1 = ((rect.y + rect.h + margin) * s).ceil() as i64;
        for y in y0..y1.min(self.height as i64) {
            for x in x0..x1.min(self.width as i64) {
                let coverage = cover(x as f32 + 0.5, y as f32 + 0.5);
                self.blend(x, y, color, coverage);
            }
        }
    }

    fn capsule(&mut self, rect: Rect, color: Rgba) {
        let px = rect.scaled(self.scale);
        self.fill(rect, 1.0, color, |x, y| {
            (0.5 - px.capsule_distance(x, y)).clamp(0.0, 1.0)
        });
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Rgba) {
        let rect = Rect {
            x: cx - r,
            y: cy - r,
            w: 2.0 * r,
            h: 2.0 * r,
        };
        self.capsule(rect, color);
    }

    /// A line `width` points wide just inside the capsule's edge (SwiftUI's `strokeBorder`).
    fn capsule_border(&mut self, rect: Rect, width: f32, color: Rgba) {
        let px = rect.scaled(self.scale);
        let w = width * self.scale;
        self.fill(rect, 1.0, color, |x, y| {
            let d = px.capsule_distance(x, y);
            (0.5 - d).clamp(0.0, 1.0) - (0.5 - (d + w)).clamp(0.0, 1.0)
        });
    }

    /// The capsule blurred: at distance d from the edge, a Gaussian blur of a large shape leaves
    /// Φ(−d/σ) of its ink, which is close enough for a 40 pt pill and a 6 pt blur.
    fn shadow(&mut self, rect: Rect) {
        let px = rect.scaled(self.scale);
        let sigma = SHADOW_SIGMA * self.scale;
        let color = [0.0, 0.0, 0.0, SHADOW_ALPHA];
        self.fill(rect, SHADOW_SIGMA * 3.0, color, |x, y| {
            let d = px.capsule_distance(x, y);
            0.5 * erfc(d / (sigma * std::f32::consts::SQRT_2))
        });
    }

    fn into_image(self) -> Image {
        let data = self
            .data
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        Image {
            width: self.width,
            height: self.height,
            data,
        }
    }
}

/// Complementary error function (Abramowitz and Stegun 7.1.26, error below 1.5e-7).
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t * (0.254_829_6 + t * (-0.284_496_7 + t * (1.421_413_8 + t * (-1.453_152 + t * 1.061_405_4))));
    let r = poly * (-z * z).exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_follow_the_swift_layout() {
        let renderer = Renderer::new(None);
        let levels = [0.0; 14];
        // Dot, spacing, 14 bars with 13 gaps, and the padding.
        assert_eq!(
            renderer.pill_width(&Content::Recording(&levels)),
            8.0 + 10.0 + 81.0 + 32.0
        );
    }

    #[test]
    fn draws_at_the_backing_scale() {
        let renderer = Renderer::new(None);
        let image = renderer.draw(&Content::Processing { time: 0.0 }, 120.0, 2.0);
        assert_eq!((image.width, image.height), (720, 144));
        assert_eq!(image.data.len(), 720 * 144 * 4);
        // The middle of the pill is opaque black; the corner is clear.
        let at = |x: usize, y: usize| &image.data[(y * 720 + x) * 4..(y * 720 + x) * 4 + 4];
        assert_eq!(at(360, 40), [0, 0, 0, 255]);
        assert_eq!(at(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn erfc_matches_known_values() {
        assert!((erfc(0.0) - 1.0).abs() < 1e-6);
        assert!((erfc(1.0) - 0.157_299_2).abs() < 1e-6);
        assert!((erfc(-1.0) - 1.842_700_8).abs() < 1e-6);
    }
}
