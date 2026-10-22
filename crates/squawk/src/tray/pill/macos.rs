//! Shows the pill's pixels in a borderless, non-activating `NSPanel`.
//!
//! It must never take focus or become key, or the paste would land in the pill instead of the
//! app being dictated into. A non-activating borderless panel can't become key, it ignores the
//! mouse, and it's brought up with `orderFrontRegardless`, which doesn't activate Squawk. It
//! floats at status-bar level on every Space and over full-screen apps, and stays out of ⌘Tab
//! and Mission Control. The image is a layer's contents, the cheapest way to put a bitmap on
//! screen. Everything here runs on the main thread, where tao's event loop is.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSPanel, NSScreen, NSStatusWindowLevel, NSView, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_foundation::{CFData, CFRetained, CFType};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::{CALayer, CATransaction};

use super::render::{Image, PANEL_HEIGHT, PANEL_WIDTH};

/// How far above the bottom of the visible frame (so above the Dock) the panel sits.
const BOTTOM_MARGIN: f64 = 28.0;

pub struct Window {
    panel: Retained<NSPanel>,
    layer: Retained<CALayer>,
    color_space: Option<CFRetained<CGColorSpace>>,
}

impl Window {
    pub fn new() -> Option<Self> {
        let Some(mtm) = MainThreadMarker::new() else {
            log::error!("The pill has to be made on the main thread");
            return None;
        };
        let rect = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(PANEL_WIDTH as f64, PANEL_HEIGHT as f64),
        );
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            NSWindowStyleMask::NonactivatingPanel | NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            true,
        );
        // SAFETY: we keep our own reference, so AppKit mustn't also release it on close.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setFloatingPanel(true);
        panel.setLevel(NSStatusWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setOpaque(false);
        // The shadow is drawn into the image (see `render.rs`) rather than by the window.
        panel.setHasShadow(false);
        panel.setIgnoresMouseEvents(true);
        panel.setHidesOnDeactivate(false);

        // A layer-hosting view: set the layer before asking for one, so AppKit leaves its
        // contents to us.
        let layer = CALayer::new();
        let view = NSView::initWithFrame(NSView::alloc(mtm), rect);
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        panel.setContentView(Some(&view));

        Some(Self {
            panel,
            layer,
            color_space: CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })),
        })
    }

    /// Moves the panel to the bottom centre of the screen the mouse is on, just above the Dock,
    /// and returns that screen's backing scale for drawing.
    pub fn place(&mut self) -> f32 {
        let mtm = MainThreadMarker::from(&*self.panel);
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let screen = screens
            .iter()
            .find(|screen| contains(screen.frame(), mouse))
            .or_else(|| NSScreen::mainScreen(mtm));
        let Some(screen) = screen else { return 2.0 };
        let visible = screen.visibleFrame();
        let origin = NSPoint::new(
            visible.origin.x + (visible.size.width - PANEL_WIDTH as f64) / 2.0,
            visible.origin.y + BOTTOM_MARGIN,
        );
        self.panel.setFrameOrigin(origin);
        let scale = screen.backingScaleFactor();
        self.layer.setContentsScale(scale);
        scale as f32
    }

    pub fn show(&self) {
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }

    pub fn set_alpha(&self, alpha: f32) {
        self.panel.setAlphaValue(alpha as f64);
    }

    pub fn set_image(&self, image: &Image) {
        let Some(image) = cg_image(image, self.color_space.as_deref()) else {
            log::warn!("Couldn't make an image for the pill");
            return;
        };
        let contents: &CFType = &image;
        // Without this, Core Animation cross-fades every new frame over a quarter second, which
        // smears the meter and costs far more than drawing it.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // SAFETY: a CGImage is one of the types a layer's contents may be.
        unsafe { self.layer.setContents(Some(contents.as_ref())) };
        CATransaction::commit();
    }

    /// The panel's frame in points from the top left of the main display (the coordinates
    /// `screencapture -R` takes).
    pub fn screen_rect(&self) -> (f64, f64, f64, f64) {
        let mtm = MainThreadMarker::from(&*self.panel);
        let frame = self.panel.frame();
        let main_height = NSScreen::screens(mtm)
            .firstObject()
            .map_or(0.0, |screen| screen.frame().size.height);
        (
            frame.origin.x,
            main_height - frame.origin.y - frame.size.height,
            frame.size.width,
            frame.size.height,
        )
    }
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.x < rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y < rect.origin.y + rect.size.height
}

/// Wraps premultiplied RGBA pixels in a CGImage (copying them, which at 720×144 is nothing).
fn cg_image(image: &Image, color_space: Option<&CGColorSpace>) -> Option<CFRetained<CGImage>> {
    let data = CFData::from_bytes(&image.data);
    let provider = CGDataProvider::with_cf_data(Some(&data))?;
    // SAFETY: the provider holds width × height × 4 bytes, in rows of width × 4.
    unsafe {
        CGImage::new(
            image.width,
            image.height,
            8,
            32,
            image.width * 4,
            color_space,
            CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}
