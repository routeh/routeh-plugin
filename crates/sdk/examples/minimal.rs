//! The least a plugin's service is: every instance it is told of is up at
//! once. Run as procd runs a service: `minimal <socket>`.
use routeh_plugin_sdk::{Desired, InstanceState, Log, Phase, Plugin, State};
use std::process::ExitCode;

struct Minimal;

impl Plugin for Minimal {
    const ID: &'static str = "minimal";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    type Settings = serde_json::Value;
    type Instance = serde_json::Value;

    fn converge(&mut self, desired: &Desired, log: &Log) -> State {
        desired
            .instances
            .iter()
            .fold(State::settled(), |state, (key, instance)| {
                log.instance(key).info(format!("{} is up", instance.device));
                state.instance(key, InstanceState::new(Phase::Up))
            })
    }
}

fn main() -> ExitCode {
    routeh_plugin_sdk::run(Minimal)
}
