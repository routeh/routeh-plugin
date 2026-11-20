//! `state` as the plugin writes it.
use crate::Phase;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Everything the plugin is, whole. The router keeps only the values its
/// manifest declares (`[[status]]`, `[[tunnel.status]]`) and of their type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Done with the last `desired`: nothing left to make or change,
    /// every instance out of `starting`.
    pub settled: bool,
    /// The plugin's values, by key.
    pub values: BTreeMap<String, Value>,
    /// Each instance by its key.
    pub instances: BTreeMap<String, InstanceState>,
    /// The rows of each group a status value or an action is of
    /// (`[[status]] of`, `[[actions]] of`), by group and the row's key: the
    /// value of the group's key field.
    pub rows: BTreeMap<String, BTreeMap<String, RowState>>,
}

/// What one row of a group is: like an instance, without a peer.
#[derive(Clone, Debug, PartialEq)]
pub struct RowState {
    pub phase: Phase,
    pub message: String,
    pub values: BTreeMap<String, Value>,
}

impl RowState {
    /// In `phase`, with no message or values.
    pub fn new(phase: Phase) -> Self {
        Self {
            phase,
            message: String::new(),
            values: BTreeMap::new(),
        }
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// With the value `key`, declared `of` the row's group.
    pub fn value(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    fn json(&self) -> Value {
        let mut row = Map::new();
        row.insert("message".into(), Value::from(self.message.as_str()));
        row.insert("phase".into(), Value::from(self.phase.word()));
        row.insert("values".into(), object(&self.values));
        Value::Object(row)
    }
}

/// What one instance is.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceState {
    pub phase: Phase,
    /// Short, for a person.
    pub message: String,
    /// The address and port it talks to.
    pub peer: Option<String>,
    pub values: BTreeMap<String, Value>,
}

impl State {
    /// A settled state with no values and no instances.
    pub fn settled() -> Self {
        Self {
            settled: true,
            ..Self::default()
        }
    }

    /// With the value `key`: a count or bytes (a whole number), a
    /// duration (seconds), a time (seconds since 1970 UTC), text or an
    /// address (text).
    pub fn value(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    /// With the instance `key`.
    pub fn instance(mut self, key: impl Into<String>, instance: InstanceState) -> Self {
        self.instances.insert(key.into(), instance);
        self
    }

    /// With the row `key` of the group `group`.
    pub fn row(mut self, group: impl Into<String>, key: impl Into<String>, row: RowState) -> Self {
        self.rows
            .entry(group.into())
            .or_default()
            .insert(key.into(), row);
        self
    }

    /// The line of this state with its end, as converged to `digest`
    /// (`None` before the first `desired`), its keys in order.
    pub fn line(&self, digest: Option<&str>) -> String {
        let instances: Map<String, Value> = self
            .instances
            .iter()
            .map(|(key, instance)| (key.clone(), instance.json()))
            .collect();
        let mut state = Map::new();
        state.insert("digest".into(), digest.map_or(Value::Null, Value::from));
        state.insert("instances".into(), Value::Object(instances));
        let rows: Map<String, Value> = self
            .rows
            .iter()
            .map(|(group, rows)| {
                let rows = rows.iter().map(|(key, row)| (key.clone(), row.json()));
                (group.clone(), Value::Object(rows.collect()))
            })
            .collect();
        state.insert("rows".into(), Value::Object(rows));
        state.insert("settled".into(), Value::Bool(self.settled));
        state.insert("values".into(), object(&self.values));
        let mut whole = Map::new();
        whole.insert("state".into(), Value::Object(state));
        format!("{}\n", Value::Object(whole))
    }
}

impl InstanceState {
    /// In `phase`, with no message, peer or values.
    pub fn new(phase: Phase) -> Self {
        Self {
            phase,
            message: String::new(),
            peer: None,
            values: BTreeMap::new(),
        }
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn peer(mut self, peer: impl Into<String>) -> Self {
        self.peer = Some(peer.into());
        self
    }

    /// With the value `key`, as [`State::value`].
    pub fn value(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    fn json(&self) -> Value {
        let mut instance = Map::new();
        instance.insert("message".into(), Value::from(self.message.as_str()));
        instance.insert(
            "peer".into(),
            self.peer.as_deref().map_or(Value::Null, Value::from),
        );
        instance.insert("phase".into(), Value::from(self.phase.word()));
        instance.insert("values".into(), object(&self.values));
        Value::Object(instance)
    }
}

fn object(values: &BTreeMap<String, Value>) -> Value {
    Value::Object(values.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}
