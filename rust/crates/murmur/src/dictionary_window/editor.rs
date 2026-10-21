//! Linux: no window. "Dictionary…" opens `dictionary.json` in the user's text editor, creating an
//! empty one first so there's something to open.

use murmur_core::Dictionary;
use tao::event::WindowEvent;
use tao::window::WindowId;

use crate::state::{Proxy, Target};
use crate::system;

pub struct DictionaryWindow;

impl DictionaryWindow {
    pub fn new(_proxy: Proxy) -> Self {
        Self
    }

    pub fn show(&mut self, _target: &Target) {
        let path = super::path();
        if !path.exists()
            && let Err(e) = Dictionary::default().save(&path)
        {
            log::error!("Couldn't create {}: {e:#}", path.display());
            murmur_platform::notify("Couldn't create the dictionary file");
            return;
        }
        system::open_in_editor(&path);
    }

    pub fn is_open(&self) -> bool {
        false
    }

    pub fn message(&mut self, _body: &str) {}

    pub fn window_event(&mut self, _id: WindowId, _event: &WindowEvent) {}

    pub fn tick(&mut self) {}
}
