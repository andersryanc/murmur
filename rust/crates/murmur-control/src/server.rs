//! The daemon's side of the socket.

use std::fs::{self, DirBuilder};
use std::io::{self, PipeReader, PipeWriter, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Context;

use crate::{Command, ControlError, Event, Phase, WATCH};

/// Longest command line read from a client. Real commands are a few bytes.
const MAX_COMMAND: usize = 256;

/// How long a client gets to send its command and take its reply. Clients are handled one at a
/// time on the server thread, so one that connects and goes quiet must not hold up the rest.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(1);

/// Listens on the control socket on its own thread, so the daemon itself can stay synchronous.
///
/// Each client sends one line. A [`Command`] goes to the handler and its return value goes back
/// as the reply; then the connection closes. A client that sends `watch` stays connected and
/// gets every [`Server::broadcast`] from then on.
///
/// Dropping the server stops the thread, disconnects the watchers and removes the socket file.
pub struct Server {
    path: PathBuf,
    shared: Arc<Mutex<Watchers>>,
    /// Dropping this wakes the server thread to stop.
    wake: Option<PipeWriter>,
    thread: Option<JoinHandle<()>>,
    /// The socket file's identity, so dropping only removes it if it's still ours.
    file_id: (u64, u64),
}

impl Server {
    /// Starts listening on `path` (normally [`socket_path`](crate::socket_path)), creating its
    /// folder (private to this user) if need be.
    ///
    /// Fails with [`ControlError::AlreadyRunning`] if a server already answers there. A socket
    /// file nobody answers on is left over from a daemon that didn't shut down cleanly, and is
    /// replaced.
    ///
    /// `handler` runs on the server thread, one command at a time; it should return quickly,
    /// since other clients wait meanwhile. It may call [`broadcast`](Self::broadcast).
    pub fn start(path: &Path, handler: impl FnMut(Command) -> String + Send + 'static) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .with_context(|| format!("couldn't create {}", dir.display()))?;
        }
        if UnixStream::connect(path).is_ok() {
            return Err(ControlError::AlreadyRunning.into());
        }
        match fs::remove_file(path) {
            Ok(()) => log::info!("Removed a stale socket at {}", path.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("couldn't remove the old {}", path.display())),
        }

        let listener = UnixListener::bind(path).with_context(|| format!("couldn't listen on {}", path.display()))?;
        // Non-blocking, so a client that gives up between poll and accept can't stall the loop.
        listener.set_nonblocking(true)?;
        let meta = fs::metadata(path)?;
        let (wake_reader, wake_writer) = io::pipe()?;
        let shared = Arc::new(Mutex::new(Watchers::default()));
        let thread = thread::Builder::new().name("murmur-control".into()).spawn({
            let shared = shared.clone();
            move || serve(listener, wake_reader, shared, handler)
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            shared,
            wake: Some(wake_writer),
            thread: Some(thread),
            file_id: (meta.dev(), meta.ino()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sends `event` to every watcher, from any thread, and returns how many got it.
    ///
    /// Never blocks: a watcher that has gone away, or isn't reading and has a full buffer, is
    /// dropped instead of waited for. The latest `Queue` and `State` are remembered and sent to
    /// each new watcher first, so it starts out knowing what the daemon is doing.
    ///
    /// The count lets the daemon fall back to a desktop notification for a notice nobody saw.
    pub fn broadcast(&self, event: &Event) -> usize {
        let mut watchers = lock(&self.shared);
        match event {
            Event::Queue(count) => watchers.queue = *count,
            Event::State(phase) => watchers.phase = *phase,
            _ => {}
        }
        watchers.send(event);
        watchers.clients.len()
    }

    /// How many clients are watching, as of the last broadcast (one that has disconnected since
    /// is only noticed when it's next sent something).
    pub fn watcher_count(&self) -> usize {
        lock(&self.shared).clients.len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.wake.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        lock(&self.shared).clients.clear();
        // Another daemon may have replaced a socket it thought was stale; leave its file alone.
        if fs::metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.file_id) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Default)]
struct Watchers {
    clients: Vec<UnixStream>,
    queue: usize,
    phase: Phase,
}

impl Watchers {
    fn add(&mut self, client: UnixStream) {
        if let Err(e) = client.set_nonblocking(true) {
            log::warn!("Dropping a watcher: {e}");
            return;
        }
        // The greeting goes to this client alone; everyone else already knows.
        let greeting = format!("{}\n{}\n", Event::Queue(self.queue), Event::State(self.phase));
        if send(&client, greeting.as_bytes(), false).is_ok_and(|n| n == greeting.len()) {
            self.clients.push(client);
        }
    }

    /// Drops any client the whole line can't be written to right now. A partial write would
    /// leave half a line in its stream, so that client is dropped too.
    fn send(&mut self, event: &Event) {
        let line = format!("{event}\n");
        self.clients
            .retain(|client| send(client, line.as_bytes(), false).is_ok_and(|n| n == line.len()));
    }
}

fn lock(shared: &Mutex<Watchers>) -> MutexGuard<'_, Watchers> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes without raising SIGPIPE if the other end has gone: a daemon killed by SIGPIPE because
/// the shell restarted would be a poor trade for one event. Linux has MSG_NOSIGNAL per call;
/// Apple systems set SO_NOSIGPIPE on the socket instead, in [`no_sigpipe`]. With `wait` false it
/// also never blocks (MSG_DONTWAIT), for watchers; with it true it waits up to the socket's
/// write timeout, for replies.
fn send(stream: &UnixStream, bytes: &[u8], wait: bool) -> io::Result<usize> {
    #[cfg(not(target_vendor = "apple"))]
    let mut flags = libc::MSG_NOSIGNAL;
    #[cfg(target_vendor = "apple")]
    let mut flags = 0;
    if !wait {
        flags |= libc::MSG_DONTWAIT;
    }
    // SAFETY: the pointer and length describe `bytes`, and the fd is open for `stream`'s lifetime.
    let sent = unsafe { libc::send(stream.as_raw_fd(), bytes.as_ptr().cast(), bytes.len(), flags) };
    match sent {
        n if n > 0 => Ok(n as usize),
        0 => Err(io::ErrorKind::WriteZero.into()),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(target_vendor = "apple")]
fn no_sigpipe(stream: &UnixStream) -> io::Result<()> {
    let on: libc::c_int = 1;
    // SAFETY: SO_NOSIGPIPE takes a c_int, passed by pointer with its size.
    let result = unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_NOSIGPIPE,
            (&on as *const libc::c_int).cast(),
            size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_vendor = "apple"))]
fn no_sigpipe(_: &UnixStream) -> io::Result<()> {
    Ok(())
}

/// The server thread: waits for a client or the wake pipe, whichever comes first.
fn serve(
    listener: UnixListener,
    wake: PipeReader,
    shared: Arc<Mutex<Watchers>>,
    mut handler: impl FnMut(Command) -> String,
) {
    loop {
        match wait_readable(listener.as_raw_fd(), wake.as_raw_fd()) {
            Ok(Ready::Stop) => return,
            Ok(Ready::Client) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::error!("Control socket stopped: {e}");
                return;
            }
        }
        match listener.accept() {
            Ok((client, _)) => {
                if let Err(e) = handle(client, &shared, &mut handler) {
                    log::debug!("Control client: {e}");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => log::warn!("Couldn't accept a control client: {e}"),
        }
    }
}

enum Ready {
    Client,
    Stop,
}

fn wait_readable(listener: RawFd, wake: RawFd) -> io::Result<Ready> {
    let mut fds = [listener, wake].map(|fd| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    });
    // SAFETY: `fds` is a valid array of two pollfds for the duration of the call.
    if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // The writer is only ever dropped, never written to, so any wake-up there means stop.
    if fds[1].revents != 0 {
        Ok(Ready::Stop)
    } else {
        Ok(Ready::Client)
    }
}

fn handle(client: UnixStream, shared: &Mutex<Watchers>, handler: &mut impl FnMut(Command) -> String) -> io::Result<()> {
    // Accepted sockets inherit the listener's non-blocking flag on some systems (macOS).
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    client.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    no_sigpipe(&client)?;

    let line = read_line(&client)?;
    let line = line.trim();
    if line == WATCH {
        lock(shared).add(client);
        return Ok(());
    }
    let reply = match Command::parse(line) {
        Some(command) => handler(command),
        None => format!("unknown command: {line}"),
    };
    let reply = format!("{}\n", reply.trim_end());
    let mut written = 0;
    while written < reply.len() {
        written += send(&client, &reply.as_bytes()[written..], true)?;
    }
    Ok(())
}

/// Reads up to the first newline, the end of the stream, or [`MAX_COMMAND`] bytes. Reads one
/// byte at a time so nothing after the newline is consumed; the lines are tiny.
fn read_line(mut client: &UnixStream) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < MAX_COMMAND {
        match client.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => line.push(byte[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}
