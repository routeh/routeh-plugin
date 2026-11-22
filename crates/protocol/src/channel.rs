//! The channel between the router and a plugin's service (docs/PLUGINS.md,
//! The channel): one JSON object a line over a Unix stream socket.
//! The router sends `desired`, whole, and an `action` a person asked for;
//! the plugin sends `hello` once, `state`, whole, whenever it changes, and
//! `done` for each action. A plugin whose `hello` says `pings` also answers
//! the router's `ping` with `pong`, and only such a plugin is asked. Only
//! what is pure lives here: the text of
//! `desired` and of an action, reading a line a plugin sent against its
//! manifest, and cutting a stream into lines. Who connects is the
//! caller's.
use crate::sha256::{hex, sha256};
use crate::{Manifest, Schema, Settings, StatusType, StatusValue};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// The version of the protocol a plugin's `hello` must name.
pub const PROTOCOL: u64 = 1;
/// The most a plugin sends in one line: a longer one ends the connection.
/// `desired` has no limit of its own (decided 2026-11-18).
pub const LINE_LIMIT: usize = 64 * 1024;
/// The most characters of the message of a `done`: a line, not a log.
pub const MESSAGE_LIMIT: usize = 1024;
/// What a plugin is to be, as the line the router sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desired {
    /// `sha256:` and the hash of the object without its digest:
    /// `{"instances":…,"settings":…}`, keys in order.
    pub digest: String,
    /// The whole line, its end included: also the artifact's text.
    pub text: String,
}

/// One instance of what the plugin provides, as `desired` tells it: a
/// tunnel of its kind, by the tunnel's key.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    pub key: String,
    /// The device it is to make, named by the router.
    pub device: String,
    /// The mark to put on its outer socket: the number of the policy the
    /// tunnel follows.
    pub mark: u32,
    /// Its settings as the document writes them, without the fields of
    /// role `address` (netifd sets addresses).
    pub settings: toml::Table,
}

/// `desired` of a plugin with these settings and instances. Each value is
/// as the document writes it; the instances are by key, every key of the
/// line in order.
pub fn desired(settings: &Settings, instances: &[Instance]) -> Desired {
    let object = |table: toml::Table| -> Map<String, Value> {
        table
            .into_iter()
            .map(|(key, value)| (key, json(value)))
            .collect()
    };
    let instances: Map<String, Value> = instances
        .iter()
        .map(|instance| {
            let mut entry = Map::new();
            entry.insert("device".into(), Value::from(instance.device.as_str()));
            entry.insert("kind".into(), Value::from("tunnel"));
            entry.insert("mark".into(), Value::from(instance.mark));
            entry.insert(
                "settings".into(),
                Value::Object(object(instance.settings.clone())),
            );
            (instance.key.clone(), Value::Object(entry))
        })
        .collect();
    sealed(instances, Value::Object(object(settings.to_toml())))
}

/// The line of these instances and settings, with its digest.
fn sealed(instances: Map<String, Value>, settings: Value) -> Desired {
    let mut body = Map::new();
    body.insert("instances".into(), Value::Object(instances));
    body.insert("settings".into(), settings);
    let hashed = Value::Object(body.clone()).to_string();
    let digest = format!("sha256:{}", hex(&sha256(hashed.as_bytes())));
    body.insert("digest".into(), Value::String(digest.clone()));
    let mut whole = Map::new();
    whole.insert("desired".into(), Value::Object(body));
    Desired {
        digest,
        text: format!("{}\n", Value::Object(whole)),
    }
}

/// What a plugin that goes off is told before its service is stopped:
/// the `desired` line `text` with its settings and no instance, so that
/// it takes away what it made (a stopped service leaves its devices
/// behind). `None` for text that is no `desired`.
pub fn emptied(text: &str) -> Option<Desired> {
    let value: Value = serde_json::from_str(text).ok()?;
    let settings = value.get("desired")?.get("settings")?.clone();
    settings.is_object().then(|| sealed(Map::new(), settings))
}

/// Each instance of a `desired` line, by key in order, with the device it
/// is to make: what a plugin reads of it. `None` for text that is no
/// `desired` or an instance without its device.
pub fn instances_of(text: &str) -> Option<Vec<(String, String)>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let instances = value.get("desired")?.get("instances")?.as_object()?;
    instances
        .iter()
        .map(|(key, instance)| Some((key.clone(), instance.get("device")?.as_str()?.to_owned())))
        .collect()
}

/// A value of the document as JSON. The settings' values are strings,
/// integers, booleans and arrays and tables of them.
fn json(value: toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::String(text),
        toml::Value::Integer(number) => Value::from(number),
        toml::Value::Boolean(flag) => Value::Bool(flag),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .into_iter()
                .map(|(key, value)| (key, json(value)))
                .collect(),
        ),
        toml::Value::Float(number) => Value::from(number),
        toml::Value::Datetime(moment) => Value::String(moment.to_string()),
    }
}

/// The digest a `desired` line carries; `None` for text that is none.
pub fn digest_of(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value
        .get("desired")?
        .get("digest")?
        .as_str()
        .map(str::to_owned)
}

/// The `hello` line a plugin sends first.
pub fn hello_line(plugin: &str, version: &str) -> String {
    let mut hello = Map::new();
    hello.insert("plugin".into(), Value::from(plugin));
    hello.insert("protocol".into(), Value::from(PROTOCOL));
    hello.insert("version".into(), Value::from(version));
    let mut whole = Map::new();
    whole.insert("hello".into(), Value::Object(hello));
    format!("{}\n", Value::Object(whole))
}

/// The `hello` line of a plugin that answers pings: what the SDK sends. A
/// plugin that does not say so is never pinged, since it may take a line
/// it does not know for an error.
pub fn hello_line_answering_pings(plugin: &str, version: &str) -> String {
    let mut hello = Map::new();
    hello.insert("pings".into(), Value::Bool(true));
    hello.insert("plugin".into(), Value::from(plugin));
    hello.insert("protocol".into(), Value::from(PROTOCOL));
    hello.insert("version".into(), Value::from(version));
    let mut whole = Map::new();
    whole.insert("hello".into(), Value::Object(hello));
    format!("{}\n", Value::Object(whole))
}

/// The line the router asks whether a plugin is alive with.
pub fn ping_line() -> &'static str {
    "{\"ping\":{}}\n"
}

/// The line a plugin answers a ping with.
pub fn pong_line() -> &'static str {
    "{\"pong\":{}}\n"
}

/// Whether a line from the router, without its end, is a ping.
pub fn is_ping(line: &[u8]) -> bool {
    line == b"{\"ping\":{}}"
}

/// The `state` line of a plugin with no values and no instances.
pub fn state_line(digest: &str, settled: bool) -> String {
    state_line_with(digest, settled, &[])
}

/// The `state` line of a plugin with no values and these instances, each
/// by its key with its phase and message.
pub fn state_line_with(digest: &str, settled: bool, instances: &[(&str, Phase, &str)]) -> String {
    let mut state = Map::new();
    state.insert("digest".into(), Value::from(digest));
    if !instances.is_empty() {
        let instances: Map<String, Value> = instances
            .iter()
            .map(|(key, phase, message)| {
                let mut instance = Map::new();
                instance.insert("message".into(), Value::from(*message));
                instance.insert("phase".into(), Value::from(phase.word()));
                ((*key).to_owned(), Value::Object(instance))
            })
            .collect();
        state.insert("instances".into(), Value::Object(instances));
    }
    state.insert("settled".into(), Value::Bool(settled));
    let mut whole = Map::new();
    whole.insert("state".into(), Value::Object(state));
    format!("{}\n", Value::Object(whole))
}

/// An action a person asked for (`[[actions]]` of the manifest): what
/// The router sends, and what the plugin reads.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Action {
    /// The plugin answers with it.
    pub id: u64,
    pub key: String,
    /// The key of the row it is done to, for an action of a group.
    #[serde(default)]
    pub row: Option<String>,
}

/// The line of the action `key` of the plugin of `manifest`, done to
/// `row`: refused when the manifest declares no such action, or when a
/// row is given to an action of none or missing for one of a group.
pub fn action_line(
    manifest: &Manifest,
    id: u64,
    key: &str,
    row: Option<&str>,
) -> Result<String, ChannelError> {
    let Some(declared) = manifest.actions.iter().find(|action| action.key == key) else {
        return Err(ChannelError::Refused(format!(
            "{} has no action {key}",
            manifest.id
        )));
    };
    match (&declared.of, row) {
        (Some(group), None) => {
            return Err(ChannelError::Refused(format!(
                "{key} is done to a row of {group}"
            )));
        }
        (None, Some(_)) => {
            return Err(ChannelError::Refused(format!("{key} is done to no row")));
        }
        _ => {}
    }
    let mut action = Map::new();
    action.insert("id".into(), Value::from(id));
    action.insert("key".into(), Value::from(key));
    if let Some(row) = row {
        action.insert("row".into(), Value::from(row));
    }
    let mut whole = Map::new();
    whole.insert("action".into(), Value::Object(action));
    Ok(format!("{}\n", Value::Object(whole)))
}

/// Reads an `action` line, as a plugin does.
pub fn read_action(line: &[u8]) -> Result<Action, ChannelError> {
    let Ok(Value::Object(mut object)) = serde_json::from_slice::<Value>(line) else {
        return Err(ChannelError::Refused("not a JSON object".into()));
    };
    let Some(action) = object.remove("action") else {
        return Err(ChannelError::Refused("not an action".into()));
    };
    serde_json::from_value(action)
        .map_err(|error| ChannelError::Refused(format!("action: {error}")))
}

/// The `done` line a plugin answers the action `id` with: whether it did
/// it, and a message for the person, cut to [`MESSAGE_LIMIT`] characters.
pub fn done_line(id: u64, ok: bool, message: &str) -> String {
    let message: String = message.chars().take(MESSAGE_LIMIT).collect();
    let mut done = Map::new();
    done.insert("id".into(), Value::from(id));
    done.insert("message".into(), Value::from(message));
    done.insert("ok".into(), Value::Bool(ok));
    let mut whole = Map::new();
    whole.insert("done".into(), Value::Object(done));
    format!("{}\n", Value::Object(whole))
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ChannelError {
    #[error("a line is longer than {0} bytes")]
    TooLong(usize),
    #[error("{0}")]
    Refused(String),
}

/// A line a plugin sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    State(Report),
    Done(Done),
    /// The answer to a ping.
    Pong,
}

/// A plugin's answer to an action.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Done {
    pub id: u64,
    pub ok: bool,
    #[serde(default)]
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Hello {
    pub protocol: u64,
    pub plugin: String,
    pub version: String,
    /// It answers `ping`. Absent in the `hello` of an older plugin.
    #[serde(default)]
    pub pings: bool,
}

/// What a plugin says it is, keeping only what its manifest declares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The `desired` it converged to; empty before its first one.
    pub digest: String,
    /// It is done with it.
    pub settled: bool,
    /// The plugin's values, by key in order: only declared keys of the
    /// declared type.
    pub values: Vec<(String, Reading)>,
    /// Each instance by its key.
    pub instances: BTreeMap<String, InstanceReport>,
    /// The rows of each group a status value or an action is of, by the
    /// group and the row's key.
    pub rows: BTreeMap<String, BTreeMap<String, RowReport>>,
    /// Keys no manifest declares (`vpn.extra` for an instance's), which
    /// are dropped: the caller logs them once.
    pub dropped: Vec<String>,
    /// Declared keys whose value is of another type: shown as missing.
    pub wrong: Vec<String>,
}

impl Report {
    /// Some instance reports `error`, which fails an apply.
    pub fn in_error(&self) -> bool {
        self.first_error().is_some()
    }
    /// The first instance by key that reports `error`, said by the apply
    /// that it fails.
    pub fn first_error(&self) -> Option<(&String, &InstanceReport)> {
        self.instances
            .iter()
            .find(|(_, instance)| instance.phase == Phase::Error)
    }

    /// Drops the rows `settings` (checked by `schema`, the plugin's own)
    /// do not have, each said in `dropped`: a row of a record that was
    /// deleted, which the plugin may still report.
    pub fn retain_rows(&mut self, schema: &Schema, settings: &Settings) {
        for (group, rows) in &mut self.rows {
            let keys = schema.row_keys(settings, group);
            rows.retain(|key, _| {
                let kept = keys.contains(key);
                if !kept {
                    self.dropped.push(format!("rows.{group}.{key}"));
                }
                kept
            });
        }
    }
}

/// What one row of a group is: an instance without a peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowReport {
    pub phase: Phase,
    pub message: String,
    pub values: Vec<(String, Reading)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceReport {
    pub phase: Phase,
    pub message: String,
    pub peer: Option<String>,
    pub values: Vec<(String, Reading)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Connecting,
    Up,
    Down,
    Error,
}

impl Phase {
    /// The word the channel says it by.
    pub fn word(self) -> &'static str {
        match self {
            Phase::Starting => "starting",
            Phase::Connecting => "connecting",
            Phase::Up => "up",
            Phase::Down => "down",
            Phase::Error => "error",
        }
    }
}

/// A value of a status key, of the type its manifest declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    Text(String),
    Count(u64),
    Bytes(u64),
    /// In seconds.
    Duration(u64),
    /// Seconds since 1970 UTC.
    Time(i64),
    Address(IpAddr),
}

/// Reads one line a plugin sent, without its end.
pub fn read(line: &[u8], manifest: &Manifest) -> Result<Message, ChannelError> {
    let tunnel = manifest
        .tunnel
        .as_ref()
        .map_or(&[][..], |kind| kind.status.as_slice());
    let mut rows: BTreeMap<&str, Vec<StatusValue>> = BTreeMap::new();
    for action in &manifest.actions {
        if let Some(group) = &action.of {
            rows.entry(group.as_str()).or_default();
        }
    }
    let mut plugin = Vec::new();
    for status in &manifest.status {
        match &status.of {
            Some(group) => rows.entry(group.as_str()).or_default().push(status.clone()),
            None => plugin.push(status.clone()),
        }
    }
    read_as(
        line,
        manifest.id.as_str(),
        &Declared {
            status: &plugin,
            tunnel,
            rows: &rows,
        },
    )
}

/// What a manifest declares that `state` may say: the plugin's values, an
/// instance's, and the values of each row of the groups something is of.
struct Declared<'a> {
    status: &'a [StatusValue],
    tunnel: &'a [StatusValue],
    rows: &'a BTreeMap<&'a str, Vec<StatusValue>>,
}

/// Reads one line of the plugin `id` for what an apply needs of it: the
/// digest, whether it settled and each instance's phase. Every value is
/// dropped.
pub fn read_bare(line: &[u8], id: &str) -> Result<Message, ChannelError> {
    read_as(
        line,
        id,
        &Declared {
            status: &[],
            tunnel: &[],
            rows: &BTreeMap::new(),
        },
    )
}

fn read_as(line: &[u8], id: &str, declared: &Declared<'_>) -> Result<Message, ChannelError> {
    let refused = |reason: String| ChannelError::Refused(reason);
    let Ok(Value::Object(mut object)) = serde_json::from_slice::<Value>(line) else {
        return Err(refused("not a JSON object".into()));
    };
    if let Some(hello) = object.remove("hello") {
        let hello: Hello =
            serde_json::from_value(hello).map_err(|error| refused(format!("hello: {error}")))?;
        if hello.protocol != PROTOCOL {
            return Err(refused(format!(
                "protocol {} is not {PROTOCOL}",
                hello.protocol
            )));
        }
        if hello.plugin != id {
            return Err(refused(format!(
                "hello names plugin {}, not {id}",
                hello.plugin
            )));
        }
        return Ok(Message::Hello(hello));
    }
    if object.contains_key("pong") {
        return Ok(Message::Pong);
    }
    if let Some(done) = object.remove("done") {
        let done: Done =
            serde_json::from_value(done).map_err(|error| refused(format!("done: {error}")))?;
        if done.message.chars().count() > MESSAGE_LIMIT {
            return Err(refused(format!(
                "done: a message is at most {MESSAGE_LIMIT} characters"
            )));
        }
        return Ok(Message::Done(done));
    }
    let Some(state) = object.remove("state") else {
        return Err(refused("neither hello, state nor done".into()));
    };
    state_of(state, declared)
        .map(Message::State)
        .map_err(|error| refused(format!("state: {error}")))
}

#[derive(serde::Deserialize)]
struct StateWire {
    /// `null` before the plugin's first `desired`: no digest. Still
    /// required: a state that leaves it out is refused.
    #[serde(deserialize_with = "Option::deserialize")]
    digest: Option<String>,
    settled: bool,
    #[serde(default)]
    values: Map<String, Value>,
    #[serde(default)]
    instances: Map<String, Value>,
    #[serde(default)]
    rows: BTreeMap<String, Map<String, Value>>,
}
#[derive(serde::Deserialize)]
struct RowWire {
    phase: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    values: Map<String, Value>,
}
#[derive(serde::Deserialize)]
struct InstanceWire {
    phase: String,
    #[serde(default)]
    message: String,
    peer: Option<String>,
    #[serde(default)]
    values: Map<String, Value>,
}

fn phase(word: &str) -> Option<Phase> {
    Some(match word {
        "starting" => Phase::Starting,
        "connecting" => Phase::Connecting,
        "up" => Phase::Up,
        "down" => Phase::Down,
        "error" => Phase::Error,
        _ => return None,
    })
}

fn state_of(state: Value, declared: &Declared<'_>) -> Result<Report, String> {
    let wire: StateWire = serde_json::from_value(state).map_err(|error| error.to_string())?;
    let mut report = Report {
        digest: wire.digest.unwrap_or_default(),
        settled: wire.settled,
        ..Report::default()
    };
    report.values = readings(
        "",
        wire.values,
        declared.status,
        &mut report.dropped,
        &mut report.wrong,
    );
    for (key, instance) in wire.instances {
        let instance: InstanceWire =
            serde_json::from_value(instance).map_err(|error| format!("instance {key}: {error}"))?;
        let Some(phase) = phase(&instance.phase) else {
            return Err(format!("instance {key}: unknown phase {}", instance.phase));
        };
        let values = readings(
            &format!("{key}."),
            instance.values,
            declared.tunnel,
            &mut report.dropped,
            &mut report.wrong,
        );
        report.instances.insert(
            key,
            InstanceReport {
                phase,
                message: instance.message,
                peer: instance.peer,
                values,
            },
        );
    }
    for (group, rows) in wire.rows {
        let Some(values_of) = declared.rows.get(group.as_str()) else {
            report.dropped.push(format!("rows.{group}"));
            continue;
        };
        let mut kept = BTreeMap::new();
        for (key, row) in rows {
            let at = format!("row {group}.{key}");
            let row: RowWire =
                serde_json::from_value(row).map_err(|error| format!("{at}: {error}"))?;
            let Some(phase) = phase(&row.phase) else {
                return Err(format!("{at}: unknown phase {}", row.phase));
            };
            let values = readings(
                &format!("rows.{group}.{key}."),
                row.values,
                values_of,
                &mut report.dropped,
                &mut report.wrong,
            );
            kept.insert(
                key,
                RowReport {
                    phase,
                    message: row.message,
                    values,
                },
            );
        }
        report.rows.insert(group, kept);
    }
    Ok(report)
}

/// The values of declared keys that are of their type, in key order.
fn readings(
    at: &str,
    values: Map<String, Value>,
    declared: &[StatusValue],
    dropped: &mut Vec<String>,
    wrong: &mut Vec<String>,
) -> Vec<(String, Reading)> {
    let mut kept = Vec::new();
    for (key, value) in values {
        let Some(status) = declared.iter().find(|status| status.key.as_str() == key) else {
            dropped.push(format!("{at}{key}"));
            continue;
        };
        match reading(status.kind, &value) {
            Some(reading) => kept.push((key, reading)),
            None => wrong.push(format!("{at}{key}")),
        }
    }
    kept
}

fn reading(kind: StatusType, value: &Value) -> Option<Reading> {
    Some(match kind {
        StatusType::Text => Reading::Text(value.as_str()?.to_owned()),
        StatusType::Count => Reading::Count(value.as_u64()?),
        StatusType::Bytes => Reading::Bytes(value.as_u64()?),
        StatusType::Duration => Reading::Duration(value.as_u64()?),
        StatusType::Time => Reading::Time(value.as_i64()?),
        StatusType::Address => Reading::Address(value.as_str()?.parse().ok()?),
    })
}

/// A stream cut into lines, each at most `limit` bytes without its end.
#[derive(Debug, Default)]
pub struct Lines {
    buffer: Vec<u8>,
    limit: Option<usize>,
}

impl Lines {
    pub fn new(limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            limit: Some(limit),
        }
    }
    /// For what the router sends: whole however long.
    pub fn unbounded() -> Self {
        Self::default()
    }
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }
    /// The next whole line without its end (`\n`, or `\r\n`); `None` until
    /// one is there. A line over the limit is an error, also before its
    /// end has come.
    pub fn next_line(&mut self) -> Result<Option<Vec<u8>>, ChannelError> {
        let end = self.buffer.iter().position(|byte| *byte == b'\n');
        let length = end.unwrap_or(self.buffer.len());
        let mut line_length = length;
        if end.is_some() && length > 0 && self.buffer[length - 1] == b'\r' {
            line_length -= 1;
        }
        if let Some(limit) = self.limit
            && line_length > limit
        {
            return Err(ChannelError::TooLong(limit));
        }
        let Some(end) = end else {
            return Ok(None);
        };
        let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
        line.truncate(line_length);
        Ok(Some(line))
    }
}
