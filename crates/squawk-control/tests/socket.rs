//! The control socket over real Unix sockets in a temporary folder.

#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use squawk_control::{Command, ControlError, Event, Phase, Server, send_to, watch};

/// A fresh folder per test, removed when dropped. Kept short: socket paths are limited to about
/// 100 bytes, and macOS's temp folder is already long.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mc-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("squawk").join("squawk.sock")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn echo_server(dir: &TempDir) -> Server {
    Server::start(&dir.socket(), |command| format!("got {command}")).unwrap()
}

fn next(watch: &mut squawk_control::Watch) -> Event {
    watch.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    watch.next().expect("an event")
}

#[test]
fn commands_get_the_handlers_reply() {
    let dir = TempDir::new();
    let (seen_tx, seen_rx) = mpsc::channel();
    let _server = Server::start(&dir.socket(), move |command| {
        seen_tx.send(command).unwrap();
        match command {
            Command::Status => "ready".into(),
            other => format!("did {other}"),
        }
    })
    .unwrap();

    assert_eq!(send_to(&dir.socket(), Command::Status).unwrap(), "ready");
    for command in [Command::Start, Command::Stop, Command::Toggle, Command::Cancel] {
        assert_eq!(send_to(&dir.socket(), command).unwrap(), format!("did {command}"));
    }
    let seen: Vec<_> = seen_rx.try_iter().collect();
    assert_eq!(
        seen,
        [
            Command::Status,
            Command::Start,
            Command::Stop,
            Command::Toggle,
            Command::Cancel
        ]
    );
}

#[test]
fn unknown_commands_are_refused_without_the_handler() {
    let dir = TempDir::new();
    let _server = Server::start(&dir.socket(), |_| panic!("handler called")).unwrap();
    let mut stream = UnixStream::connect(dir.socket()).unwrap();
    stream.write_all(b"dance\n").unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert_eq!(reply, "unknown command: dance\n");
}

#[test]
fn the_plugins_way_of_writing_commands_works() {
    // The plugin writes "watch\n" and leaves the connection open; `socat` and `nc` users might
    // write a command with no newline and close their side. Both must work.
    let dir = TempDir::new();
    let _server = echo_server(&dir);
    let mut stream = UnixStream::connect(dir.socket()).unwrap();
    stream.write_all(b"status").unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert_eq!(reply, "got status\n");
}

#[test]
fn not_running_is_a_typed_error() {
    let dir = TempDir::new();
    let error = send_to(&dir.socket(), Command::Status).unwrap_err();
    assert!(matches!(error.downcast_ref(), Some(ControlError::NotRunning)));
}

#[test]
fn watchers_get_the_current_state_then_every_broadcast() {
    let dir = TempDir::new();
    let server = echo_server(&dir);
    let mut first = watch(&dir.socket()).unwrap();
    assert_eq!(next(&mut first), Event::Queue(0));
    assert_eq!(next(&mut first), Event::State(Phase::Ready));

    server.broadcast(&Event::Queue(2));
    server.broadcast(&Event::State(Phase::Processing));
    let mut second = watch(&dir.socket()).unwrap();
    // A late watcher starts from the latest queue and state.
    assert_eq!(next(&mut second), Event::Queue(2));
    assert_eq!(next(&mut second), Event::State(Phase::Processing));

    // `watch` returns once the request is written, before the server has taken the watcher on.
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.watcher_count() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(server.broadcast(&Event::Level(0.42)), 2);
    assert_eq!(server.broadcast(&Event::Notice("Copied. Paste with Ctrl+V".into())), 2);

    assert_eq!(next(&mut first), Event::Queue(2));
    assert_eq!(next(&mut first), Event::State(Phase::Processing));
    for watch in [&mut first, &mut second] {
        assert_eq!(next(watch), Event::Level(0.42));
        assert_eq!(next(watch), Event::Notice("Copied. Paste with Ctrl+V".into()));
    }

    // Commands still work while clients are watching.
    assert_eq!(send_to(&dir.socket(), Command::Status).unwrap(), "got status");
}

#[test]
fn a_watcher_that_disconnects_is_dropped() {
    let dir = TempDir::new();
    let server = echo_server(&dir);
    let mut stays = watch(&dir.socket()).unwrap();
    let mut leaves = watch(&dir.socket()).unwrap();
    for w in [&mut stays, &mut leaves] {
        next(w);
        next(w);
    }
    assert_eq!(server.watcher_count(), 2);
    drop(leaves);

    // The first send after a disconnect can still land in the kernel's buffer; the next fails.
    let mut count = 2;
    for i in 0..5 {
        count = server.broadcast(&Event::Level(i as f32 / 10.0));
        if count == 1 {
            break;
        }
    }
    assert_eq!(count, 1);
    assert_eq!(next(&mut stays), Event::Level(0.0));
}

#[test]
fn a_watcher_that_stops_reading_never_blocks_broadcasts() {
    let dir = TempDir::new();
    let server = echo_server(&dir);
    // Connected and watching, but never reads a byte.
    let mut stuck = UnixStream::connect(dir.socket()).unwrap();
    stuck.write_all(b"watch\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.watcher_count() < 1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }

    // Far more than any socket buffer holds.
    let notice = Event::Notice("x".repeat(1000));
    let started = Instant::now();
    let mut sent = 0;
    while server.broadcast(&notice) > 0 {
        sent += 1;
        assert!(sent < 100_000, "the stuck watcher was never dropped");
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "broadcasts waited on a stuck watcher"
    );

    // The daemon carries on: commands work and a new watcher gets events.
    assert_eq!(send_to(&dir.socket(), Command::Status).unwrap(), "got status");
    let mut fresh = watch(&dir.socket()).unwrap();
    next(&mut fresh);
    next(&mut fresh);
    server.broadcast(&Event::State(Phase::Recording));
    assert_eq!(next(&mut fresh), Event::State(Phase::Recording));
    drop(stuck);
}

#[test]
fn a_stale_socket_file_is_replaced() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.socket().parent().unwrap()).unwrap();
    // A listener that's gone leaves its file behind, as a crashed daemon would.
    drop(UnixListener::bind(dir.socket()).unwrap());
    assert!(dir.socket().exists());
    assert!(send_to(&dir.socket(), Command::Status).is_err());

    let _server = echo_server(&dir);
    assert_eq!(send_to(&dir.socket(), Command::Status).unwrap(), "got status");
}

#[test]
fn a_second_server_refuses_to_start() {
    let dir = TempDir::new();
    let _first = echo_server(&dir);
    let error = Server::start(&dir.socket(), |_| "second".into()).err().unwrap();
    assert!(matches!(error.downcast_ref(), Some(ControlError::AlreadyRunning)));
    assert_eq!(send_to(&dir.socket(), Command::Status).unwrap(), "got status");
}

#[test]
fn dropping_the_server_removes_the_socket_and_disconnects_watchers() {
    let dir = TempDir::new();
    let server = echo_server(&dir);
    let mut watcher = watch(&dir.socket()).unwrap();
    next(&mut watcher);
    next(&mut watcher);
    drop(server);
    assert!(!dir.socket().exists());
    assert_eq!(watcher.next(), None);
    let error = send_to(&dir.socket(), Command::Status).unwrap_err();
    assert!(matches!(error.downcast_ref(), Some(ControlError::NotRunning)));
}

#[test]
fn the_socket_folder_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let _server = echo_server(&dir);
    let mode = std::fs::metadata(dir.socket().parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}
