//! The channel's socket: connections, lines, the tick.
//!
//! One thread accepts, one reads each connection, and the caller's thread
//! owns the plugin: every `desired` is converged and every action answered
//! before the next line is taken, and `state` is sent to every connection
//! when it differs from the last one sent.
//!
//! A ping is answered by the thread that reads its connection, not by the
//! plugin's thread: a long `converge` is not a dead plugin, and a process
//! that was stopped answers nothing at all. Lines of the two threads never
//! mix: each is written whole under the connection's [`Writer`].
use crate::{Desired, Log, Plugin, State, Stop};
use routeh_plugin_protocol::channel::{
    LINE_LIMIT, done_line, hello_line_answering_pings, is_ping, pong_line, read_action,
};
use routeh_plugin_protocol::names::PluginId;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a stop may wait to be noticed.
const STOP_CHECK: Duration = Duration::from_millis(100);
/// How often the acceptor looks for a connection or a stop.
const ACCEPT_CHECK: Duration = Duration::from_millis(20);
/// How long a write to a connection may take: a router that reads
/// nothing is let go, so it holds up nobody else.
const WRITE_WITHIN: Duration = Duration::from_secs(5);

/// A plugin's `main`: `<program> <socket>`, as procd starts the service.
/// Logs to the system log, stops on SIGTERM and SIGINT. Exits 0 when
/// stopped, 1 when it cannot serve, 2 when not given exactly its socket.
pub fn run<P: Plugin>(plugin: P) -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(socket), None) = (args.next(), args.next()) else {
        eprintln!("usage: {} <socket>", P::ID);
        return ExitCode::from(2);
    };
    let socket = PathBuf::from(socket);
    let log = Log::open(P::ID);
    match serve(&socket, plugin, &log, &Stop::on_signals()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log.error(format!("cannot serve {}: {error}", socket.display()));
            ExitCode::FAILURE
        }
    }
}

/// Serves `plugin` on `socket` until `stop`. The socket is bound as
/// `<socket>.new`, made its owner's alone and renamed into place, so the
/// name answers from the moment it appears; it is removed when stopped.
pub fn serve<P: Plugin>(socket: &Path, plugin: P, log: &Log, stop: &Stop) -> io::Result<()> {
    PluginId::new(P::ID).map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("{}: {error}", P::ID))
    })?;
    let listener = listen(socket)?;
    // Which file is this run's: another run may take the name meanwhile.
    let own = std::fs::metadata(socket).map(|m| (m.dev(), m.ino()))?;
    let (events, received) = mpsc::channel();
    let closing = Arc::new(AtomicBool::new(false));
    let acceptor = {
        let (events, closing) = (events.clone(), closing.clone());
        std::thread::spawn(move || accept(listener, events, &closing))
    };
    log.info(format!("listening on {}", socket.display()));
    let mut service = Service {
        plugin,
        log,
        hello: hello_line_answering_pings(P::ID, P::VERSION),
        current: State::default().line(None),
        refused: None,
        connections: BTreeMap::new(),
        desired: None,
    };
    let mut tick: Option<Instant> = None;
    while !stop.is_stopped() {
        let wait = tick.map_or(STOP_CHECK, |at| {
            at.saturating_duration_since(Instant::now()).min(STOP_CHECK)
        });
        let converged = match received.recv_timeout(wait) {
            Ok(Event::Connected(id, stream)) => {
                service.connect(id, stream);
                false
            }
            Ok(Event::Line(id, line)) => service.take(id, &line),
            Ok(Event::Gone(id)) => {
                service.connections.remove(&id);
                false
            }
            Err(_) => match tick {
                Some(at) if Instant::now() >= at => service.converge(),
                _ => false,
            },
        };
        if converged {
            tick = service.plugin.tick().map(|period| Instant::now() + period);
        }
    }
    service.plugin.shutdown(log);
    for writer in service.connections.values() {
        writer.end();
    }
    // The acceptor looks at `closing` at least every `STOP_CHECK`. It is
    // not woken by a connection to the name: another run may have it.
    closing.store(true, Ordering::Relaxed);
    let _ = acceptor.join();
    // The name is removed only while it is still this run's socket.
    if std::fs::metadata(socket).is_ok_and(|m| (m.dev(), m.ino()) == own) {
        let _ = std::fs::remove_file(socket);
    }
    log.info("stopped");
    Ok(())
}

enum Event {
    Connected(u64, Arc<Writer>),
    /// A line of the connection `id`.
    Line(u64, Vec<u8>),
    Gone(u64),
}

struct Service<'a, P: Plugin> {
    plugin: P,
    log: &'a Log,
    hello: String,
    /// The last state sent: also what a new connection is told.
    current: String,
    /// The last state that was too long to send, logged once.
    refused: Option<String>,
    connections: BTreeMap<u64, Arc<Writer>>,
    desired: Option<Desired<P::Settings, P::Instance>>,
}

impl<P: Plugin> Service<'_, P> {
    fn connect(&mut self, id: u64, writer: Arc<Writer>) {
        if writer.send(&self.hello) && writer.send(&self.current) {
            self.connections.insert(id, writer);
        }
    }

    /// Takes a line from the router on the connection `id`: `desired`, or
    /// an action; whether it converged.
    fn take(&mut self, id: u64, line: &[u8]) -> bool {
        let action = serde_json::from_slice::<serde_json::Value>(line)
            .is_ok_and(|value| value.get("action").is_some());
        let read = if action {
            read_action(line)
                .map(|action| self.act(id, &action))
                .map_err(|error| error.to_string())
        } else {
            Desired::read(line)
                .map(|desired| {
                    self.desired = Some(desired);
                    self.converge()
                })
                .map_err(|error| error.to_string())
        };
        read.unwrap_or_else(|error| {
            self.log
                .error(format!("a line from the router was not read: {error}"));
            false
        })
    }

    /// Does an action and answers the connection that asked, then
    /// converges again: whether it did.
    fn act(&mut self, id: u64, action: &crate::Action) -> bool {
        let answer = match &self.desired {
            None => Err("not told what to be yet".to_owned()),
            Some(desired) => self.plugin.action(action, Some(desired), self.log),
        };
        let line = match &answer {
            Ok(message) => done_line(action.id, true, message),
            Err(message) => done_line(action.id, false, message),
        };
        if let Some(writer) = self.connections.get(&id)
            && !writer.send(&line)
        {
            self.connections.remove(&id);
        }
        self.converge()
    }

    /// Converges to the last `desired`, if there is one, and sends the
    /// state when it changed.
    fn converge(&mut self) -> bool {
        let Some(desired) = &self.desired else {
            return false;
        };
        let state = self.plugin.converge(desired, self.log);
        let line = state.line(Some(&desired.digest));
        self.publish(line);
        true
    }

    fn publish(&mut self, line: String) {
        if line == self.current {
            return;
        }
        let length = line.len() - 1;
        if length > LINE_LIMIT {
            // The router ends a connection on a longer line: none is sent.
            if self.refused.as_ref() != Some(&line) {
                self.log.error(format!(
                    "state is {length} bytes, over the {LINE_LIMIT} bytes a line may be; not sent"
                ));
                self.refused = Some(line);
            }
            return;
        }
        self.refused = None;
        self.connections.retain(|_, writer| writer.send(&line));
        self.current = line;
    }
}

/// The writing side of a connection, shared by the plugin's thread and
/// the thread that reads it (which answers pings): one line at a time.
struct Writer {
    stream: UnixStream,
    turn: Mutex<()>,
}

impl Writer {
    /// Whether `text` was written whole; a connection that did not take
    /// it is ended. Each write is bounded by [`WRITE_WITHIN`].
    fn send(&self, text: &str) -> bool {
        let turn = self.turn.lock().unwrap_or_else(|e| e.into_inner());
        let written = (&self.stream).write_all(text.as_bytes()).is_ok();
        drop(turn);
        if !written {
            self.end();
        }
        written
    }

    fn end(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

fn listen(socket: &Path) -> io::Result<UnixListener> {
    let mut staging = OsString::from(socket.as_os_str());
    staging.push(".new");
    let staging = PathBuf::from(staging);
    let _ = std::fs::remove_file(&staging);
    let listener = UnixListener::bind(&staging)?;
    let placed = std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))
        .and_then(|()| std::fs::rename(&staging, socket));
    if let Err(error) = placed {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    Ok(listener)
}

/// Accepts until `closing`, which it looks at every `STOP_CHECK`: the
/// listener does not block, so nothing has to connect to end it.
fn accept(listener: UnixListener, events: mpsc::Sender<Event>, closing: &AtomicBool) {
    let mut id = 0u64;
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    while !closing.load(Ordering::Relaxed) {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_CHECK);
                continue;
            }
            Err(_) => continue,
        };
        // Accepted streams inherit no mode of the listener on Linux, but
        // do on some systems: reads and writes block.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let Ok(writer) = stream.try_clone() else {
            continue;
        };
        let _ = writer.set_write_timeout(Some(WRITE_WITHIN));
        let writer = Arc::new(Writer {
            stream: writer,
            turn: Mutex::new(()),
        });
        id += 1;
        if events
            .send(Event::Connected(id, writer.clone()))
            .is_err()
        {
            return;
        }
        let events = events.clone();
        std::thread::spawn(move || read_lines(id, stream, &writer, &events));
    }
}

/// Lines from the router. `desired` has no limit of its own (the router
/// bounds the document it is made from) and is read whole. A ping is
/// answered here, whatever the plugin's thread is doing.
fn read_lines(id: u64, stream: UnixStream, writer: &Writer, events: &mpsc::Sender<Event>) {
    let mut reader = BufReader::new(stream);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            // The end, or a line cut off by it.
            Ok(0) | Err(_) => break,
            Ok(_) if buffer.last() != Some(&b'\n') => break,
            Ok(_) => {
                let line = buffer.trim_ascii_end();
                if is_ping(line) {
                    if !writer.send(pong_line()) {
                        break;
                    }
                    continue;
                }
                if !line.is_empty() && events.send(Event::Line(id, line.to_vec())).is_err() {
                    return;
                }
            }
        }
    }
    let _ = events.send(Event::Gone(id));
}
