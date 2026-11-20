//! The values of a plugin's settings as they are checked, and their
//! canonical form: what Save writes into the document.
use crate::base64;
use crate::names::FieldKey;
use crate::schema::Reference;
use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One checked value. Each type of `FieldType` gives one variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `text`, `template`.
    Text(String),
    /// `bytes`, decoded.
    Bytes(Vec<u8>),
    Bool(bool),
    /// `u8`, `u16`, `u32`.
    Integer(u32),
    Range {
        lo: u32,
        hi: u32,
    },
    /// In seconds.
    Duration(u32),
    Port(u16),
    /// A host name in lowercase, or an address.
    HostPort {
        host: String,
        port: u16,
    },
    /// `ipv4_prefix`, `ipv6_prefix`: an interface's address and its
    /// length; host bits stay.
    Prefix(IpAddr, u8),
    Address(IpAddr),
    Choice(String),
    /// The key of a section of the router's document.
    Reference(Reference, String),
    List(Vec<Value>),
    Group(Vec<Settings>),
}

impl Value {
    /// The value as the document writes it.
    pub fn to_toml(&self) -> toml::Value {
        match self {
            Value::Text(text) | Value::Choice(text) | Value::Reference(_, text) => {
                toml::Value::String(text.clone())
            }
            Value::Bytes(bytes) => toml::Value::String(base64::encode(bytes, true)),
            Value::Bool(flag) => toml::Value::Boolean(*flag),
            Value::Integer(number) => toml::Value::Integer(i64::from(*number)),
            Value::Port(port) => toml::Value::Integer(i64::from(*port)),
            Value::Range { lo, hi } => toml::Value::String(format!("{lo}-{hi}")),
            Value::Duration(seconds) => toml::Value::String(duration_text(*seconds)),
            Value::HostPort { host, port } if host.contains(':') => {
                toml::Value::String(format!("[{host}]:{port}"))
            }
            Value::HostPort { host, port } => toml::Value::String(format!("{host}:{port}")),
            Value::Prefix(address, length) => toml::Value::String(format!("{address}/{length}")),
            Value::Address(address) => toml::Value::String(address.to_string()),
            Value::List(items) => toml::Value::Array(items.iter().map(Value::to_toml).collect()),
            Value::Group(rows) => toml::Value::Array(
                rows.iter()
                    .map(|row| toml::Value::Table(row.to_toml()))
                    .collect(),
            ),
        }
    }
}

/// The checked settings of one section: only those set. Its `Debug`
/// shows the keys alone: a value may be a secret, and what is secret is
/// the schema's to say.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Settings(pub(crate) BTreeMap<FieldKey, Value>);

impl fmt::Debug for Settings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.0.keys()).finish()
    }
}

impl Settings {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Every reference in the settings, in the order of their paths
    /// (`exit`, `records.0.uplink`, `via.1`): what the router checks is
    /// there, and what it refuses to delete while it is named.
    pub fn references(&self) -> Vec<Referenced> {
        let mut found = Vec::new();
        for (key, value) in &self.0 {
            references_in(key.as_str(), value, &mut found);
        }
        found
    }

    /// The settings as the document's table.
    pub fn to_toml(&self) -> toml::Table {
        self.0
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_toml()))
            .collect()
    }
}

/// A reference found in settings: where it is, what kind of section it
/// names and its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Referenced {
    pub at: String,
    pub kind: Reference,
    pub key: String,
}

fn references_in(at: &str, value: &Value, found: &mut Vec<Referenced>) {
    match value {
        Value::Reference(kind, key) => found.push(Referenced {
            at: at.to_owned(),
            kind: *kind,
            key: key.clone(),
        }),
        Value::List(items) => {
            for (index, item) in items.iter().enumerate() {
                references_in(&format!("{at}.{index}"), item, found);
            }
        }
        Value::Group(rows) => {
            for (index, row) in rows.iter().enumerate() {
                for (key, value) in &row.0 {
                    references_in(&format!("{at}.{index}.{key}"), value, found);
                }
            }
        }
        _ => {}
    }
}

pub(crate) const DURATION_RULE: &str = "a duration is a whole number and s, m or h (\"25s\")";
pub(crate) const RANGE_RULE: &str =
    "a range is two whole numbers with '-' between them (\"100-200\")";
pub(crate) const HOST_PORT_RULE: &str =
    "a host name or address and a port (\"vpn.example.com:51820\", \"[fd00::1]:443\")";

/// A whole number in decimal, with no zero in front of it.
pub(crate) fn decimal(text: &str) -> Option<u64> {
    let plain = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if plain { text.parse().ok() } else { None }
}

/// `25s`, `10m`, `2h` in seconds.
pub(crate) fn duration(text: &str) -> Option<u32> {
    let unit = match text.chars().last()? {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => return None,
    };
    let amount = decimal(&text[..text.len() - 1])?;
    u32::try_from(amount.checked_mul(unit)?).ok()
}

/// Seconds in the largest unit that holds them whole.
pub(crate) fn duration_text(seconds: u32) -> String {
    match seconds {
        0 => "0s".to_owned(),
        _ if seconds.is_multiple_of(3600) => format!("{}h", seconds / 3600),
        _ if seconds.is_multiple_of(60) => format!("{}m", seconds / 60),
        _ => format!("{seconds}s"),
    }
}

/// `100-200` as its two ends, in any order.
pub(crate) fn range(text: &str) -> Option<(u64, u64)> {
    let (lo, hi) = text.split_once('-')?;
    Some((decimal(lo)?, decimal(hi)?))
}

/// An address and its prefix length, of IPv4 (`v4`) or IPv6.
pub(crate) fn prefix(text: &str, v4: bool) -> Option<(IpAddr, u8)> {
    let (address, length) = text.split_once('/')?;
    let length = u8::try_from(decimal(length)?).ok()?;
    let address = if v4 {
        IpAddr::V4(address.parse::<Ipv4Addr>().ok()?)
    } else {
        IpAddr::V6(address.parse::<Ipv6Addr>().ok()?)
    };
    let bits = if v4 { 32 } else { 128 };
    (length <= bits).then_some((address, length))
}

/// `host:port`, `[v6]:port`; a host name is lowercased.
pub(crate) fn host_port(text: &str) -> Option<(String, u16)> {
    let (host, port) = match text.strip_prefix('[') {
        Some(rest) => {
            let (address, port) = rest.split_once("]:")?;
            (address.parse::<Ipv6Addr>().ok()?.to_string(), port)
        }
        None => {
            let (host, port) = text.rsplit_once(':')?;
            match host.parse::<Ipv4Addr>() {
                Ok(address) => (address.to_string(), port),
                Err(_) if host_name(host) && !numeric(host) => (host.to_ascii_lowercase(), port),
                Err(_) => return None,
            }
        }
    };
    let port = u16::try_from(decimal(port)?)
        .ok()
        .filter(|port| *port > 0)?;
    Some((host, port))
}

/// A name whose last label is all digits is no host name (RFC 3696 2):
/// `999.1.1.1` is a mistyped address, not a name.
fn numeric(host: &str) -> bool {
    host.rsplit('.')
        .next()
        .is_some_and(|label| label.bytes().all(|b| b.is_ascii_digit()))
}

/// A DNS name: labels of letters, digits and `-`, not at either end of a
/// label, 1 to 63 bytes each, 253 in all.
fn host_name(text: &str) -> bool {
    text.len() <= 253
        && text.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}
