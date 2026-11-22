//! The router in memory, so a plugin is tested without one: [`Harness`]
//! serves a plugin on a socket in a directory of its own, and [`Router`]
//! connects to it, sends `desired` and reads `hello` and `state` as the
//! router reads them (the router's own reader, [`channel::read`], against
//! the plugin's manifest).
//!
//! Always built: it is small, uses nothing but the standard library and
//! the protocol, and a plugin's tests reach it without a feature.
//!
//! ```no_run
//! # use routeh_plugin_sdk::{Desired, Log, Plugin, State};
//! # struct Minimal;
//! # impl Plugin for Minimal {
//! #     const ID: &'static str = "minimal";
//! #     const VERSION: &'static str = "1";
//! #     type Settings = serde_json::Value;
//! #     type Instance = serde_json::Value;
//! #     fn converge(&mut self, _: &Desired, _: &Log) -> State { State::settled() }
//! # }
//! use routeh_plugin_sdk::protocol::{Manifest, Settings, channel};
//! use routeh_plugin_sdk::testing::Harness;
//!
//! // The plugin's own manifest, as the router installs it.
//! let manifest = Manifest::parse(include_str!("../manifest.toml")).unwrap();
//! let harness = Harness::start(Minimal).unwrap();
//! let mut router = harness.connect(&manifest).unwrap();
//! let desired = channel::desired(&Settings::default(), &[]);
//! router.send(&desired).unwrap();
//! assert!(router.settled(&desired.digest).unwrap().settled);
//! ```
use crate::{Log, Plugin, Stop, serve};
use routeh_plugin_protocol::Manifest;
use routeh_plugin_protocol::channel::{self, Done, Hello, LINE_LIMIT, Lines, Message, Report};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// How long a [`Router`] waits for a line, and for a socket to answer.
pub const WAIT: Duration = Duration::from_secs(5);

/// A plugin served on a socket of its own, in memory's log, until it is
/// stopped or dropped.
pub struct Harness {
    dir: TempDir,
    socket: PathBuf,
    log: Log,
    stop: Stop,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl Harness {
    /// Serves `plugin` on `<dir>/<id>.sock` in a thread.
    pub fn start<P: Plugin + Send + 'static>(plugin: P) -> io::Result<Self> {
        let dir = TempDir::new()?;
        let socket = dir.path().join(format!("{}.sock", P::ID));
        let log = Log::memory(P::ID);
        let stop = Stop::new();
        let thread = {
            let (socket, log, stop) = (socket.clone(), log.clone(), stop.clone());
            std::thread::spawn(move || serve(&socket, plugin, &log, &stop))
        };
        Ok(Self {
            dir,
            socket,
            log,
            stop,
            thread: Some(thread),
        })
    }

    /// The socket the plugin listens on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The directory the socket is in, removed with the harness.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// A connection as the router makes it.
    pub fn connect(&self, manifest: &Manifest) -> io::Result<Router> {
        Router::connect(&self.socket, manifest)
    }

    /// What the plugin logged (every instance's lines): a log in memory.
    pub fn log(&self) -> Log {
        self.log.clone()
    }

    /// Stops the plugin and waits for it: what `serve` returned.
    pub fn stop(mut self) -> io::Result<()> {
        self.halt()
    }

    fn halt(&mut self) -> io::Result<()> {
        self.stop.stop();
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("the plugin panicked"))),
            None => Ok(()),
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.halt();
    }
}

/// One connection to a plugin, as the router makes it: the plugin's
/// lines are cut at the channel's limit and read against its manifest.
#[derive(Debug)]
pub struct Router {
    stream: UnixStream,
    lines: Lines,
    id: String,
    manifest: Manifest,
    /// What came while an action's answer was awaited, read next.
    pending: VecDeque<Message>,
    /// The id of the last action sent.
    actions: u64,
}

impl Router {
    /// Connects to `socket`, waiting up to [`WAIT`] for it to answer.
    pub fn connect(socket: &Path, manifest: &Manifest) -> io::Result<Self> {
        Self::connect_within(socket, manifest, WAIT)
    }

    /// Connects to `socket`, waiting up to `within` for it to answer.
    pub fn connect_within(
        socket: &Path,
        manifest: &Manifest,
        within: Duration,
    ) -> io::Result<Self> {
        let deadline = Instant::now() + within;
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(stream) => break stream,
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        stream.set_read_timeout(Some(WAIT))?;
        Ok(Self {
            stream,
            lines: Lines::new(LINE_LIMIT),
            id: manifest.id.to_string(),
            manifest: manifest.clone(),
            pending: VecDeque::new(),
            actions: 0,
        })
    }

    /// Sends a `desired` made as the router makes it
    /// ([`channel::desired`]).
    pub fn send(&mut self, desired: &channel::Desired) -> io::Result<()> {
        self.send_text(&desired.text)
    }

    /// Sends text as it is: lines of one's own.
    pub fn send_text(&mut self, text: &str) -> io::Result<()> {
        self.stream.write_all(text.as_bytes())
    }

    /// Asks for the action `key` of the manifest, done to `row` (for an
    /// action of a group), and waits for its answer. What the plugin sends
    /// meanwhile is read next. An action the manifest does not declare
    /// as asked is not sent: `InvalidInput`.
    pub fn action(&mut self, key: &str, row: Option<&str>) -> io::Result<Done> {
        self.actions += 1;
        let id = self.actions;
        let line = channel::action_line(&self.manifest, id, key, row)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        self.send_text(&line)?;
        loop {
            match self.read_message()? {
                Message::Done(done) if done.id == id => return Ok(done),
                other => self.pending.push_back(other),
            }
        }
    }

    /// Pings the plugin and waits for its `pong`. What the plugin sends
    /// meanwhile is read next.
    pub fn ping(&mut self) -> io::Result<()> {
        self.send_text(channel::ping_line())?;
        loop {
            match self.read_message()? {
                Message::Pong => return Ok(()),
                other => self.pending.push_back(other),
            }
        }
    }

    /// The next line the plugin sent, read as the router reads it.
    pub fn message(&mut self) -> io::Result<Message> {
        match self.pending.pop_front() {
            Some(message) => Ok(message),
            None => self.read_message(),
        }
    }

    fn read_message(&mut self) -> io::Result<Message> {
        let line = self.next_line()?;
        channel::read(&line, &self.manifest).map_err(invalid)
    }

    /// The next line, which must be `hello`.
    pub fn hello(&mut self) -> io::Result<Hello> {
        match self.message()? {
            Message::Hello(hello) => Ok(hello),
            other => Err(self.unexpected("hello", &other)),
        }
    }

    /// The next line, which must be `state`. Its rows are in `rows`.
    pub fn state(&mut self) -> io::Result<Report> {
        match self.message()? {
            Message::State(report) => Ok(report),
            other => Err(self.unexpected("state", &other)),
        }
    }

    fn unexpected(&self, wanted: &str, got: &Message) -> io::Error {
        let got = match got {
            Message::Hello(_) => "hello",
            Message::State(_) => "state",
            Message::Done(_) => "done",
            Message::Pong => "pong",
        };
        invalid(format!("{} sent {got}, not {wanted}", self.id))
    }

    /// Lines until a state converged to `digest` and settled, as an apply
    /// waits for it (a `hello` on the way included).
    pub fn settled(&mut self, digest: &str) -> io::Result<Report> {
        loop {
            if let Message::State(report) = self.message()?
                && report.digest == digest
                && report.settled
            {
                return Ok(report);
            }
        }
    }

    fn next_line(&mut self) -> io::Result<Vec<u8>> {
        let mut buffer = [0u8; 8192];
        loop {
            if let Some(line) = self.lines.next_line().map_err(invalid)? {
                return Ok(line);
            }
            match self.stream.read(&mut buffer)? {
                0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!("{} ended the connection", self.id),
                    ));
                }
                read => self.lines.push(&buffer[..read]),
            }
        }
    }
}

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

/// A directory of its own under the system's temporary directory,
/// removed with what is in it when dropped. Its path is short enough for
/// a socket in it.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> io::Result<Self> {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let name = format!(
            "routeh-{}-{}-{nanos}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
