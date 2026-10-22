//! The client side: `squawkctl start` and friends, and watching events (as the Omarchy plugin does).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;

use crate::{Command, ControlError, Event, WATCH, socket_path};

/// How long to wait for the daemon's reply. Commands only flip state and hand work to other
/// threads, so a reply this late means the daemon is stuck; better to say so than to hang the
/// key binding that ran us.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Sends `command` to the daemon on [`socket_path`] and returns its reply, e.g. "recording".
pub fn send(command: Command) -> anyhow::Result<String> {
    send_to(&socket_path(), command)
}

/// [`send`] to the socket at `path`. The write side is shut down after the command, so the
/// server sees its end even without the newline.
pub fn send_to(path: &Path, command: Command) -> anyhow::Result<String> {
    let mut stream = connect(path)?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
    stream.set_write_timeout(Some(REPLY_TIMEOUT))?;
    let io = || format!("Couldn't talk to Squawk over {}", path.display());
    stream.write_all(format!("{command}\n").as_bytes()).with_context(io)?;
    stream.shutdown(Shutdown::Write).with_context(io)?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).with_context(io)?;
    Ok(reply.trim().to_string())
}

/// Connects to the daemon on `path` and asks for events. The first two are the current queue
/// length and state; then one arrives per change until the daemon goes away.
pub fn watch(path: &Path) -> anyhow::Result<Watch> {
    let mut stream = connect(path)?;
    stream.write_all(format!("{WATCH}\n").as_bytes())?;
    Ok(Watch {
        reader: BufReader::new(stream),
    })
}

fn connect(path: &Path) -> anyhow::Result<UnixStream> {
    UnixStream::connect(path).map_err(|_| ControlError::NotRunning.into())
}

/// The events from [`watch`], as an iterator that ends when the daemon closes the connection
/// (or a read times out, if a timeout was set). Lines it doesn't understand are skipped.
pub struct Watch {
    reader: BufReader<UnixStream>,
}

impl Watch {
    /// Gives up waiting for the next event after `timeout`, ending the iterator.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.reader.get_ref().set_read_timeout(timeout)
    }
}

impl Iterator for Watch {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Some(event) = Event::parse(&line) {
                        return Some(event);
                    }
                }
            }
        }
    }
}
