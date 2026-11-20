//! What the tests share: a manifest, `desired` lines made as the router
//! makes them, and a plugin that records what it is told.
use routeh_plugin_sdk::protocol::channel::{self, Instance};
use routeh_plugin_sdk::protocol::{Manifest, Settings};
use routeh_plugin_sdk::{Action, Desired, InstanceState, Log, Phase, Plugin, RowState, State};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The manifest of the plugin the tests serve.
pub const MANIFEST: &str = r#"
id = "demo"
name = "Demo"
version = "1.0"
api = 1

[service]
command = "bin/demo"

[settings]
log_level = { type = "choice", values = ["error", "info"], label = "Log level" }

[settings.records]
type = "group"
label = "Records"
key = "host"
columns = ["host"]
fields = { host = { type = "text", required = true, label = "Host" } }

[[form]]
title = "Logging"
fields = ["log_level", "records"]

[[status]]
of = "records"
key = "address"
label = "Address"
type = "address"

[[actions]]
key = "update_now"
label = "Update now"
of = "records"

[[actions]]
key = "restart"
label = "Restart"

[[actions]]
key = "fail"
label = "Fail"

[[status]]
key = "converged"
label = "Converged"
type = "count"

[tunnel]
link_kind = "dummy"
mtu = { default = 1400, min = 1280, max = 1420 }

[[tunnel.form]]
title = "All"
fields = ["address", "listen_port", "note"]

[[tunnel.status]]
key = "since"
label = "Since"
type = "time"

[tunnel.settings]
address = { type = "ipv4_prefix", role = "address", required = true, label = "Address" }
listen_port = { type = "port", role = "listen", protocol = "udp", label = "Listen port" }
note = { type = "text", label = "Note" }
"#;

pub fn manifest() -> Manifest {
    Manifest::parse(MANIFEST).unwrap()
}

/// The plugin's own settings, checked by its manifest as the router does.
pub fn settings(text: &str) -> Settings {
    manifest()
        .settings
        .check(&text.parse::<toml::Table>().unwrap())
        .unwrap()
}

/// An instance as `desired` tells it: a tunnel with these settings.
pub fn instance(key: &str, settings: &str) -> Instance {
    Instance {
        key: key.to_owned(),
        device: format!("tn_{key}"),
        mark: 257,
        settings: settings.parse().unwrap(),
    }
}

/// `desired` of the plugin with these settings and instances.
pub fn desired(settings_text: &str, instances: &[Instance]) -> channel::Desired {
    channel::desired(&settings(settings_text), instances)
}

/// What [`Recorder`] has seen.
#[derive(Default)]
pub struct Seen {
    pub digests: Vec<String>,
    /// Each action with the digest of the `desired` it was asked under.
    pub actions: Vec<(Action, String)>,
    pub converged: u64,
    pub shut_down: bool,
}

/// A plugin that records what it is told and reports each instance up,
/// with how often it converged as a value unless `steady`.
#[derive(Clone, Default)]
pub struct Recorder {
    pub seen: Arc<Mutex<Seen>>,
    pub tick: Option<Duration>,
    /// Report the same state however often it converges.
    pub steady: bool,
    /// A note this long in each instance's message: a state too long to
    /// send once it is over the channel's limit.
    pub padding: usize,
}

impl Plugin for Recorder {
    const ID: &'static str = "demo";
    const VERSION: &'static str = "1.0";
    type Settings = serde_json::Value;
    type Instance = serde_json::Value;

    fn converge(&mut self, desired: &Desired, log: &Log) -> State {
        let mut seen = self.seen.lock().unwrap();
        seen.digests.push(desired.digest.clone());
        seen.converged += 1;
        log.instance("all")
            .info(format!("converged {}", seen.converged));
        let mut state = State::settled();
        if !self.steady {
            state = state.value("converged", seen.converged);
        }
        for key in desired.instances.keys() {
            state = state.instance(
                key,
                InstanceState::new(Phase::Up).message("x".repeat(self.padding)),
            );
        }
        let records = desired.settings.get("records").and_then(|r| r.as_array());
        for record in records.into_iter().flatten() {
            let host = record["host"].as_str().unwrap_or_default();
            state = state.row(
                "records",
                host,
                RowState::new(Phase::Up).value("address", "192.0.2.1"),
            );
        }
        state
    }

    fn action(
        &mut self,
        action: &Action,
        desired: Option<&Desired>,
        log: &Log,
    ) -> Result<String, String> {
        let digest = desired.map(|d| d.digest.clone()).unwrap_or_default();
        self.seen
            .lock()
            .unwrap()
            .actions
            .push((action.clone(), digest));
        log.info(format!("asked {}", action.key));
        match action.key.as_str() {
            "fail" => Err("it failed".into()),
            key => Ok(format!("did {key}")),
        }
    }

    fn tick(&self) -> Option<Duration> {
        self.tick
    }

    fn shutdown(&mut self, log: &Log) {
        self.seen.lock().unwrap().shut_down = true;
        log.info("shut down");
    }
}

/// Whether `condition` holds within five seconds.
pub fn eventually(condition: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}
