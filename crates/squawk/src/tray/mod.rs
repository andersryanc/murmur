//! The tray app (menu bar on macOS, notification area on Windows): a front end for
//! `squawk-engine`. Its windows are the recording pill, which never takes focus, and the
//! Dictionary window. The event loop runs on the main thread, as AppKit requires; the hotkey tap
//! and the engine's threads report back through it.

mod app;
mod background;
mod demo;
mod dictionary_window;
mod icon;
mod login_item;
mod menu;
mod pill;
mod state;
mod system;

use std::sync::Mutex;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::MenuEvent;

use crate::logging;
use app::App;
use state::AppEvent;

pub fn run() {
    logging::init(logging::Output::FileAndStderr);
    log::info!(
        "Squawk {} starting (pid {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    #[allow(unused_mut)] // Only changed on macOS.
    let mut event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        // A menu-bar app: no Dock icon and no app menu (LSUIElement does the same when bundled).
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        // tao activates the app at launch by default, taking focus from whatever is in front.
        // Squawk has nothing to focus, and should never take it from the app being dictated into.
        event_loop.set_activate_ignoring_other_apps(false);
    }

    // Menu clicks arrive on muda's callback; forward them so the loop wakes for each one.
    let proxy = Mutex::new(event_loop.create_proxy());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Ok(proxy) = proxy.lock() {
            let _ = proxy.send_event(AppEvent::Menu(event.id));
        }
    }));

    let proxy = event_loop.create_proxy();
    let mut app: Option<App> = None;
    event_loop.run(move |event, target, control_flow| {
        match event {
            // The tray has to be made once the loop is running (macOS needs the app launched).
            Event::NewEvents(StartCause::Init) => app = Some(App::start(proxy.clone(), target)),
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if let Some(app) = &mut app {
                    app.timer_fired();
                }
            }
            Event::UserEvent(event) => {
                if let Some(app) = &mut app {
                    app.handle(event, target);
                }
            }
            Event::WindowEvent { window_id, event, .. } => {
                if let Some(app) = &mut app {
                    app.window_event(window_id, &event);
                }
            }
            Event::LoopDestroyed => {
                if let Some(app) = &mut app {
                    app.shut_down();
                }
                log::info!("Squawk stopped");
            }
            _ => {}
        }

        let Some(app) = &mut app else { return };
        if app.quitting {
            *control_flow = ControlFlow::Exit;
            return;
        }
        // Cheap when nothing changed. The pill's only window ignores the mouse, so nothing
        // floods the loop with events.
        app.refresh();
        *control_flow = ControlFlow::WaitUntil(app.wake_at());
    });
}
