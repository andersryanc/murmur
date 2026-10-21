//! The Linux front end: a command-line tool and a daemon, for Hyprland (built for Omarchy). A
//! port of the Swift `Linux/Sources/murmur`, with the dictation loop now in `murmur-engine`.
//!
//! Wayland doesn't let an app watch keys globally, so the push-to-talk key is a Hyprland binding
//! that runs `murmur start` on press and `murmur stop` on release. Those send one command over
//! the daemon's control socket (`murmur-control`) and print its reply. The daemon owns the
//! engine; the Omarchy shell plugin watches the same socket to draw the pill.

mod bench;
mod config;
mod daemon;
mod doctor;
mod transcribe;

use std::process::ExitCode;

use murmur_control::Command;

use crate::logging;

fn usage() -> String {
    format!(
        "\
murmur: push-to-talk dictation for Linux (Hyprland)

  murmur daemon                Load the speech model and wait for commands. Run it from
                               Hyprland's autostart. Downloads the model (~480 MB) the
                               first time.
  murmur start | stop          Start or finish a dictation. Bind to a key's press and release.
  murmur toggle                Start, or finish if already recording. For a single key binding.
  murmur cancel                Throw away the current recording.
  murmur status                loading, ready, recording or processing.
  murmur transcribe <file.wav> [--context plain|chat|email|notes|code] [--no-cleanup]
                               Transcribe and clean up a WAV file, without the daemon.
  murmur bench [--model <ollama model>] [--runs 3]
                               Time the cleanup on fixed test dictations and print the results.
  murmur doctor                Check the tools, model, API key, daemon and on-screen pill.

Settings: {}
Dictionary: {}
Log: {}
Socket: {}
",
        murmur_core::Settings::path().display(),
        murmur_core::Dictionary::default_path().display(),
        murmur_core::paths::log_path().display(),
        murmur_control::socket_path().display(),
    )
}

/// Runs the command in `args` (the command line without the program name).
pub fn run(args: Vec<String>) -> ExitCode {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    };
    let rest = &args[1..];
    if command != "daemon" {
        logging::init(logging::Output::Stderr);
    }
    let result = match command {
        "daemon" => return daemon::run(),
        "transcribe" => transcribe::run(rest),
        "bench" => bench::run(rest),
        "doctor" => return doctor::run(),
        "help" | "--help" | "-h" => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        other => match Command::parse(other) {
            Some(command) => return send(command),
            None => {
                eprint!("Unknown command: {other}\n\n{}", usage());
                return ExitCode::from(2);
            }
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `murmur start` and the other socket commands: send one and print the daemon's reply.
fn send(command: Command) -> ExitCode {
    match murmur_control::send(command) {
        Ok(reply) => {
            println!("{reply}");
            if reply.starts_with("error:") {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            // From a key binding nobody sees stderr, so say it on screen when a dictation was
            // meant to start.
            if matches!(command, Command::Start | Command::Toggle) {
                murmur_platform::notify(&e.to_string());
            }
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// The value after `--name` in `args`.
fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_take_the_next_argument() {
        let args: Vec<String> = ["clip.wav", "--context", "chat", "--runs"].map(String::from).into();
        assert_eq!(option(&args, "--context"), Some("chat"));
        assert_eq!(option(&args, "--runs"), None);
        assert_eq!(option(&args, "--model"), None);
    }

    #[test]
    fn usage_lists_every_socket_command() {
        let text = usage();
        for command in Command::ALL {
            assert!(text.contains(&format!("murmur {command}")) || text.contains(&format!("| {command}")));
        }
    }
}
