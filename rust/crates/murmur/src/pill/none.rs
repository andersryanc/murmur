//! No pill window on this platform yet: Linux has its own QML pill, and Windows comes later.
//! `Window::new` returns `None`, so `Pill` never draws and notices go to the system instead.

use super::render::Image;

pub enum Window {}

impl Window {
    pub fn new() -> Option<Self> {
        None
    }

    pub fn place(&mut self) -> f32 {
        match *self {}
    }

    pub fn show(&self) {
        match *self {}
    }

    pub fn hide(&self) {
        match *self {}
    }

    pub fn set_alpha(&self, _alpha: f32) {
        match *self {}
    }

    pub fn set_image(&self, _image: &Image) {
        match *self {}
    }

    pub fn screen_rect(&self) -> (f64, f64, f64, f64) {
        match *self {}
    }
}
