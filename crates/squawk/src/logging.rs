//! Logging to `squawk_core::paths::log_path()` and stderr. A menu-bar app or a daemon started by
//! the compositor has no console, so the file is where to look when something goes wrong;
//! stderr is for running the binary directly. One-shot commands (`squawkctl transcribe`) log only
//! warnings, to stderr, so their own output stays readable.

use std::fs::{self, OpenOptions};

use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, SharedLogger, TermLogger, TerminalMode};

/// Past this size the log is moved aside to `squawk.log.old` at launch, so it never grows
/// without bound but the previous run's log is still there to read.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Where the log goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// Everything from Info up, to the log file and stderr: the tray app and the daemon.
    FileAndStderr,
    /// Warnings and errors to stderr only: one-shot commands.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Stderr,
}

/// Call first thing in `main`, before any other thread starts: finding the local time zone
/// offset is only allowed while the process is single-threaded.
pub fn init(output: Output) {
    let mut builder = ConfigBuilder::new();
    builder
        .set_target_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        // Their debug output is noise; warnings and errors still come through at Info.
        .add_filter_ignore_str("ureq")
        .add_filter_ignore_str("rustls")
        .add_filter_ignore_str("tao")
        .add_filter_ignore_str("tray_icon");
    if builder.set_time_offset_to_local().is_err() {
        // Falls back to UTC; the timestamps are still consistent within the file.
    }
    let config = builder.build();

    let terminal_level = match output {
        Output::FileAndStderr => LevelFilter::Info,
        Output::Stderr => LevelFilter::Warn,
    };
    let mut loggers: Vec<Box<dyn SharedLogger>> = vec![TermLogger::new(
        terminal_level,
        config.clone(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )];
    if output == Output::Stderr {
        let _ = CombinedLogger::init(loggers);
        return;
    }

    let path = squawk_core::paths::log_path();
    let file = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| {
        if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
            fs::rename(&path, path.with_extension("log.old"))?;
        }
        OpenOptions::new().create(true).append(true).open(&path)
    });
    let file_error = match file {
        Ok(file) => {
            loggers.push(simplelog::WriteLogger::new(LevelFilter::Info, config, file));
            None
        }
        Err(e) => Some(e),
    };

    let _ = CombinedLogger::init(loggers);
    if let Some(e) = file_error {
        log::warn!("Can't write the log file {}: {e}", path.display());
    }
}
