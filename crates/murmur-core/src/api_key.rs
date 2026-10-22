//! Where the Anthropic API key lives.

/// Key store item (service, account). On macOS, created with
/// `security add-generic-password -s com.plyhdrn.murmur -a anthropic-api-key -w`; on Linux, with
/// `secret-tool store --label='Murmur' service com.plyhdrn.murmur account anthropic-api-key`.
pub const SERVICE: &str = "com.plyhdrn.murmur";
pub const ACCOUNT: &str = "anthropic-api-key";

/// `ANTHROPIC_API_KEY` wins if set; otherwise read the OS key store.
pub fn load() -> Option<String> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY")
        && !key.is_empty()
    {
        return Some(key);
    }
    let key = from_store()?.trim().to_string();
    (!key.is_empty()).then_some(key)
}

/// Goes through `/usr/bin/security` because the item is created with that tool, so its access
/// list already trusts it and a rebuilt binary won't trigger a fresh Keychain prompt each time.
#[cfg(target_os = "macos")]
fn from_store() -> Option<String> {
    run(
        "/usr/bin/security",
        &["find-generic-password", "-s", SERVICE, "-a", ACCOUNT, "-w"],
    )
}

/// The Secret Service (GNOME Keyring, KWallet).
#[cfg(all(unix, not(target_os = "macos")))]
fn from_store() -> Option<String> {
    run("secret-tool", &["lookup", "service", SERVICE, "account", ACCOUNT])
}

/// Windows Credential Manager.
#[cfg(windows)]
fn from_store() -> Option<String> {
    keyring::Entry::new(SERVICE, ACCOUNT).ok()?.get_password().ok()
}

#[cfg(unix)]
fn run(program: &str, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
