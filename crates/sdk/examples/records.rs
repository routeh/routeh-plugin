//! A plugin with a page: records keyed by their host, each with how often
//! it was updated, and an action that updates one now. Shaped like a
//! client of a dynamic DNS service, without the service: an update only
//! counts. Its manifest is `records.toml` beside it.
use routeh_plugin_sdk::{Action, Desired, Log, Phase, Plugin, RowState, State};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::ExitCode;

#[derive(Deserialize)]
struct Settings {
    #[serde(default)]
    records: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    host: String,
    domain: String,
    /// The key of the uplink whose address the record is to carry.
    #[allow(dead_code)]
    uplink: String,
}

#[derive(Default)]
struct Records {
    /// How often each host was updated.
    updates: BTreeMap<String, u64>,
}

impl Plugin for Records {
    const ID: &'static str = "records";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    type Settings = Settings;
    type Instance = serde_json::Value;

    fn converge(&mut self, desired: &Desired<Settings>, _: &Log) -> State {
        desired
            .settings
            .records
            .iter()
            .fold(State::settled(), |state, record| {
                let updates = self.updates.get(&record.host).copied().unwrap_or(0);
                state.row(
                    "records",
                    &record.host,
                    RowState::new(Phase::Up).value("updates", updates),
                )
            })
    }

    fn action(
        &mut self,
        action: &Action,
        desired: Option<&Desired<Settings>>,
        log: &Log,
    ) -> Result<String, String> {
        let host = action.row.as_deref().unwrap_or_default();
        let record = desired
            .and_then(|desired| desired.settings.records.iter().find(|r| r.host == host))
            .ok_or_else(|| format!("no record {host}"))?;
        *self.updates.entry(record.host.clone()).or_default() += 1;
        log.instance(host).info("updated");
        Ok(format!("Updated {}.{}", record.host, record.domain))
    }
}

fn main() -> ExitCode {
    routeh_plugin_sdk::run(Records::default())
}
