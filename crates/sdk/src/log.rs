//! The system log: syslog lines over `/dev/log`, the plugin's id as the
//! program and an instance's key after it (`awg/vpn`); standard error
//! where there is no `/dev/log`; or memory, for tests.
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Where the system log listens, on the router and in a plugin's jail.
pub const DEV_LOG: &str = "/dev/log";
/// syslog's facility `daemon`.
const DAEMON: u8 = 3;

/// A line's severity, as syslog numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

/// A line kept by a log in memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub level: Level,
    /// `awg`, or `awg/vpn` for an instance's line.
    pub program: String,
    pub text: String,
}

#[derive(Clone, Debug)]
enum Sink {
    Syslog(Arc<UnixDatagram>),
    Stderr,
    Memory(Arc<Mutex<Vec<Entry>>>),
}

/// Where the plugin writes what it does. Clones and instances share it.
#[derive(Clone, Debug)]
pub struct Log {
    program: String,
    sink: Sink,
}

impl Log {
    /// The system log at `/dev/log`, else standard error.
    pub fn open(id: &str) -> Self {
        Self::open_at(Path::new(DEV_LOG), id)
    }

    /// The system log at `path`, else standard error.
    pub fn open_at(path: &Path, id: &str) -> Self {
        Self::to_socket(path, id).unwrap_or_else(|_| Self::stderr(id))
    }

    /// The system log at `path`.
    pub fn to_socket(path: &Path, id: &str) -> std::io::Result<Self> {
        let socket = UnixDatagram::unbound()?;
        socket.connect(path)?;
        Ok(Self::with(id, Sink::Syslog(Arc::new(socket))))
    }

    /// Standard error, one line each: `awg/vpn: text`.
    pub fn stderr(id: &str) -> Self {
        Self::with(id, Sink::Stderr)
    }

    /// Memory, read by [`Log::entries`]: for tests.
    pub fn memory(id: &str) -> Self {
        Self::with(id, Sink::Memory(Arc::default()))
    }

    fn with(id: &str, sink: Sink) -> Self {
        Self {
            program: id.to_owned(),
            sink,
        }
    }

    /// The log of the instance `key`: its lines' program is `<id>/<key>`.
    pub fn instance(&self, key: &str) -> Self {
        let id = self.program.split('/').next().unwrap_or_default();
        Self {
            program: format!("{id}/{key}"),
            sink: self.sink.clone(),
        }
    }

    pub fn write(&self, level: Level, text: impl AsRef<str>) {
        let text = text.as_ref();
        match &self.sink {
            Sink::Syslog(socket) => {
                let priority = DAEMON * 8 + level as u8;
                let line = format!(
                    "<{priority}>{}[{}]: {text}",
                    self.program,
                    std::process::id()
                );
                // A log that cannot take a line loses it: nothing to tell.
                let _ = socket.send(line.as_bytes());
            }
            Sink::Stderr => eprintln!("{}: {text}", self.program),
            Sink::Memory(entries) => lock(entries).push(Entry {
                level,
                program: self.program.clone(),
                text: text.to_owned(),
            }),
        }
    }

    pub fn error(&self, text: impl AsRef<str>) {
        self.write(Level::Error, text);
    }
    pub fn warning(&self, text: impl AsRef<str>) {
        self.write(Level::Warning, text);
    }
    pub fn notice(&self, text: impl AsRef<str>) {
        self.write(Level::Notice, text);
    }
    pub fn info(&self, text: impl AsRef<str>) {
        self.write(Level::Info, text);
    }
    pub fn debug(&self, text: impl AsRef<str>) {
        self.write(Level::Debug, text);
    }

    /// The lines a log in memory kept, of every instance; none for any
    /// other log.
    pub fn entries(&self) -> Vec<Entry> {
        match &self.sink {
            Sink::Memory(entries) => lock(entries).clone(),
            _ => Vec::new(),
        }
    }

    /// Whether a line a log in memory kept contains `text`.
    pub fn contains(&self, text: &str) -> bool {
        self.entries().iter().any(|entry| entry.text.contains(text))
    }
}

/// The entries, also after a thread that held them panicked.
fn lock(entries: &Mutex<Vec<Entry>>) -> std::sync::MutexGuard<'_, Vec<Entry>> {
    entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
