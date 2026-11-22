# Writing a plugin

A plugin's service in Rust, on `routeh-plugin-sdk`. The SDK speaks the
channel ([PROTOCOL.md](PROTOCOL.md)): the socket, `hello`, `desired` read
into the plugin's own types, `state` sent when it changed, actions,
SIGTERM, the system log. The plugin writes only what it does.

```toml
[dependencies]
routeh-plugin-sdk = { git = "https://github.com/routeh/routeh-plugin" }
serde = { version = "1", features = ["derive"] }
```

## The service

```rust
use routeh_plugin_sdk::{Desired, InstanceState, Log, Phase, Plugin, State};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
struct Tunnel {
    listen_port: Option<u16>,
}

struct Wg;

impl Plugin for Wg {
    const ID: &'static str = "awg";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    type Settings = serde_json::Value;
    type Instance = Tunnel;

    fn converge(&mut self, desired: &Desired<Self::Settings, Tunnel>, log: &Log) -> State {
        let mut state = State::settled();
        for (key, tunnel) in &desired.instances {
            // Make `tunnel.device` as `tunnel.settings` say …
            log.instance(key).info(format!("{} is up", tunnel.device));
            state = state.instance(key, InstanceState::new(Phase::Up));
        }
        state
    }

    // A kernel with no events is read again every few seconds.
    fn tick(&self) -> Option<Duration> {
        Some(Duration::from_secs(5))
    }
}

fn main() -> std::process::ExitCode {
    routeh_plugin_sdk::run(Wg) // `<program> <socket>`, as procd starts it
}
```

- `converge` makes the system what `desired` says and returns everything
  the plugin is now; the SDK sends it when it changed.
- `tick` converges again with the last `desired`, for a plugin whose
  kernel sends no events. `shutdown` runs once when it stops.
- `examples/minimal.rs` is the least a plugin is.

## A page

The router draws a plugin's page from its manifest (`[page]`, a group of
`[settings]` with a `key`, `[[status]] of` the group, `[[actions]]`:
[PROTOCOL.md](PROTOCOL.md), The manifest of a page). The plugin reports
each row in `state` and answers its actions:

```rust
use routeh_plugin_sdk::{Action, Desired, Log, Phase, Plugin, RowState, State};

impl Plugin for Ddns {
    // … ID, VERSION, Settings (with `records`), Instance

    fn converge(&mut self, desired: &Desired<Settings>, _: &Log) -> State {
        desired.settings.records.iter().fold(State::settled(), |state, record| {
            state.row("records", &record.host,
                RowState::new(Phase::Up).value("address", "203.0.113.7"))
        })
    }

    fn action(&mut self, action: &Action, desired: Option<&Desired<Settings>>, _: &Log)
        -> Result<String, String> {
        let host = action.row.as_deref().unwrap_or_default();
        Ok(format!("Updated {host}"))
    }
}
```

After an action the SDK converges again and sends `state` if it
changed. `examples/records.rs` is the whole of it.

## Being alive

Every plugin built with the SDK says in its `hello` that it answers
pings, and answers them, with no code of its own. The answer comes from
the thread that reads the connection, so a `converge` that takes a minute
does not make the plugin look dead; a process that is stopped or gone
answers nothing, and the router then stops sending traffic through its
tunnels until it answers again. A `converge` that hangs for ever is not
seen this way: it is the plugin's own to bound. `Router::ping` of the
harness asks and waits for the answer.

## Tests without a router

```rust
use routeh_plugin_sdk::protocol::{Manifest, Settings, channel};
use routeh_plugin_sdk::testing::Harness;

#[test]
fn converges() {
    let manifest = Manifest::parse(include_str!("../plugin/manifest.toml")).unwrap();
    let harness = Harness::start(Wg).unwrap();
    let mut router = harness.connect(&manifest).unwrap();
    let desired = channel::desired(&Settings::default(), &[]);
    router.send(&desired).unwrap();
    assert!(router.settled(&desired.digest).unwrap().settled);
}
```

The harness reads what the plugin sends as the router does, against the
plugin's manifest: a value the manifest does not declare is in the
report's `dropped`. `router.action("update_now", Some("home"))` sends an
action and returns its `done`.
