//! `desired` as the plugin reads it.
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

/// What the plugin is to be, whole: its settings and every instance, by
/// key in order. `S` and `I` are the plugin's own types.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(bound(deserialize = "S: DeserializeOwned, I: DeserializeOwned"))]
pub struct Desired<S = Value, I = Value> {
    /// `sha256:` and the hex of the rest: what `state` says it converged to.
    pub digest: String,
    /// `[plugins.<id>]` of the document, each value as the document
    /// writes it (a duration `"25s"`, bytes in base64, a range `"1-2"`).
    pub settings: S,
    pub instances: BTreeMap<String, Instance<I>>,
}

/// One instance: a tunnel of the plugin's kind.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(bound(deserialize = "I: DeserializeOwned"))]
pub struct Instance<I = Value> {
    /// What it is: `tunnel`.
    pub kind: String,
    /// The device to make, named by the router.
    pub device: String,
    /// The mark to put on its outer socket.
    pub mark: u32,
    /// Its settings, without the fields of role `address`.
    pub settings: I,
}

/// Why a line from the router is no `desired` the plugin takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadError(String);

impl fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for ReadError {}

impl<S: DeserializeOwned, I: DeserializeOwned> Desired<S, I> {
    /// Reads one line the router sent, without its end.
    pub fn read(line: &[u8]) -> Result<Self, ReadError> {
        let refused = |why: String| Err(ReadError(why));
        let Ok(Value::Object(mut object)) = serde_json::from_slice::<Value>(line) else {
            return refused("not a JSON object".into());
        };
        let Some(desired) = object.remove("desired") else {
            let keys: Vec<&str> = object.keys().map(String::as_str).collect();
            return refused(format!("not desired: {}", keys.join(", ")));
        };
        serde_json::from_value(desired).or_else(|error| refused(format!("desired: {error}")))
    }
}
