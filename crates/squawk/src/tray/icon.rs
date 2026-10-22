//! The tray icon, drawn in code so there are no image assets to ship: a waveform while idle, and
//! the same waveform knocked out of a filled circle while recording (like the SF Symbols
//! `waveform` and `waveform.circle.fill`).
//!
//! On macOS the icon is a template image: only its alpha matters, and the menu bar tints it to
//! match light or dark mode. Windows shows the colours as drawn, so there it's white,
//! which reads on its usual dark taskbar.

use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Pixels per side. macOS shows tray icons at most 22 pt tall, so this is drawn at 2x and the
/// glyph itself kept to about 17 pt, the weight of the system's own menu bar items.
const SIZE: usize = 44;

/// Bar heights as a fraction of the glyph's height, left to right.
const BARS: [f32; 7] = [0.32, 0.62, 1.0, 0.72, 0.42, 0.82, 0.36];
const BAR_WIDTH: f32 = 3.4;
const BAR_GAP: f32 = 1.9;

/// Samples per pixel along each axis, for anti-aliased edges.
const SUPERSAMPLE: usize = 4;

pub fn idle() -> Icon {
    render(false)
}

pub fn recording() -> Icon {
    render(true)
}

/// Whether the platform tints the icon itself (macOS template images).
const IS_TEMPLATE: bool = cfg!(target_os = "macos");

/// Gives the builder its icon, as a template image on macOS.
pub fn with_icon(builder: TrayIconBuilder, icon: Icon) -> TrayIconBuilder {
    #[cfg(target_os = "macos")]
    return builder.with_icon_templated(icon);
    #[cfg(not(target_os = "macos"))]
    return builder.with_icon(icon);
}

/// Swaps the tray's icon, as a template image on macOS.
pub fn set(tray: &TrayIcon, icon: Icon) {
    #[cfg(target_os = "macos")]
    let result = tray.set_icon_templated(Some(icon));
    #[cfg(not(target_os = "macos"))]
    let result = tray.set_icon(Some(icon));
    if let Err(e) = result {
        log::warn!("Couldn't change the tray icon: {e}");
    }
}

fn render(recording: bool) -> Icon {
    let colour: [u8; 3] = if IS_TEMPLATE { [0, 0, 0] } else { [255, 255, 255] };
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut covered = 0;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let px = x as f32 + (sx as f32 + 0.5) / SUPERSAMPLE as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SUPERSAMPLE as f32;
                    if inside(px, py, recording) {
                        covered += 1;
                    }
                }
            }
            let alpha = covered * 255 / (SUPERSAMPLE * SUPERSAMPLE);
            let i = (y * SIZE + x) * 4;
            rgba[i..i + 3].copy_from_slice(&colour);
            rgba[i + 3] = alpha as u8;
        }
    }
    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("the icon buffer matches its size")
}

/// Whether the point (in pixels from the top left) is painted.
fn inside(x: f32, y: f32, recording: bool) -> bool {
    let centre = SIZE as f32 / 2.0;
    if recording {
        // A filled circle with smaller bars cut out of it.
        let radius = 17.0;
        let in_circle = (x - centre).powi(2) + (y - centre).powi(2) <= radius * radius;
        in_circle && !on_bar(x, y, 0.62, 18.0)
    } else {
        on_bar(x, y, 1.0, 30.0)
    }
}

/// Whether the point is on one of the waveform's rounded bars, with the whole waveform scaled
/// by `scale` around the centre and the tallest bar `height` pixels tall.
fn on_bar(x: f32, y: f32, scale: f32, height: f32) -> bool {
    let centre = SIZE as f32 / 2.0;
    let width = BAR_WIDTH * scale;
    let gap = BAR_GAP * scale;
    let total = BARS.len() as f32 * width + (BARS.len() - 1) as f32 * gap;
    let left = centre - total / 2.0;
    BARS.iter().enumerate().any(|(i, fraction)| {
        let bar_left = left + i as f32 * (width + gap);
        let half = (height * fraction / 2.0).max(width / 2.0);
        // A vertical capsule: a line segment from centre-half to centre+half, thickened.
        let cx = bar_left + width / 2.0;
        let r = width / 2.0;
        let dy = (y - centre).abs() - (half - r);
        let dist = if dy > 0.0 {
            ((x - cx).powi(2) + dy * dy).sqrt()
        } else {
            (x - cx).abs()
        };
        dist <= r
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_something_and_leaves_the_corners_clear() {
        for recording in [false, true] {
            let painted = (0..SIZE * SIZE)
                .filter(|&i| inside((i % SIZE) as f32 + 0.5, (i / SIZE) as f32 + 0.5, recording))
                .count();
            assert!(painted > 100, "recording {recording}: {painted} pixels");
            assert!(!inside(0.5, 0.5, recording));
        }
    }
}
