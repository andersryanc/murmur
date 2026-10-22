//! Launch at Login. macOS only: the app bundle is
//! registered as a login item with `SMAppService.mainApp`.
//!
//! The menu's checkmark always shows what the system says rather than a saved setting, since the
//! user can also switch it off in System Settings › General › Login Items.

/// What the system says about Squawk as a login item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    On,
    /// Registered, but the user has to allow it in System Settings first.
    NeedsApproval,
    Off,
}

/// `None` where Squawk can't be a login item (anywhere but macOS, for now), so the menu leaves
/// the item out.
#[cfg(target_os = "macos")]
pub fn status() -> Option<Status> {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    // SAFETY: mainAppService and status are plain queries with no preconditions.
    let status = unsafe { SMAppService::mainAppService().status() };
    Some(match status {
        SMAppServiceStatus::Enabled => Status::On,
        SMAppServiceStatus::RequiresApproval => Status::NeedsApproval,
        _ => Status::Off,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> Option<Status> {
    None
}

/// Registers or unregisters the app. Registering only works from an app bundle (not
/// `cargo run`), and macOS shows a "Login Item Added" notification when it does.
#[cfg(target_os = "macos")]
pub fn set(on: bool) -> anyhow::Result<()> {
    use objc2_service_management::SMAppService;
    let service = unsafe { SMAppService::mainAppService() };
    // SAFETY: register/unregister take no arguments and report failure through the NSError.
    let result = unsafe {
        if on {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|e| anyhow::anyhow!("{}", e.localizedDescription()))
}

#[cfg(not(target_os = "macos"))]
pub fn set(_on: bool) -> anyhow::Result<()> {
    anyhow::bail!("Launch at Login isn't supported here")
}

/// System Settings › General › Login Items, where a login item waiting for approval is allowed.
#[cfg(target_os = "macos")]
pub fn open_settings() {
    // SAFETY: a class method that only opens System Settings.
    unsafe { objc2_service_management::SMAppService::openSystemSettingsLoginItems() };
}

#[cfg(not(target_os = "macos"))]
pub fn open_settings() {}
