//! Write a routeh plugin's service as its `converge` and nothing else.
//!
//! The router tells a plugin's service what to be (`desired`, whole) over
//! one Unix socket, and the service says what it is (`state`, whole) when
//! that changes (`docs/PROTOCOL.md`). This crate is that channel written
//! once:
//!
//! - [`Plugin`]: the plugin's part. Its settings and instances are read
//!   from `desired` into its own types with serde; [`Plugin::converge`]
//!   makes the system what they say and returns the [`State`]; an
//!   optional [`Plugin::tick`] converges again on a period for a plugin
//!   that must read its kernel state (one with no events); an optional
//!   [`Plugin::action`] answers a button of its page (`[[actions]]`).
//! - [`serve`] and [`run`]: listen on the socket, say `hello`, converge to
//!   every `desired` before reading the next line, answer each `action`
//!   with `done`, send `state` to every connection when it changed, end on
//!   SIGTERM or SIGINT ([`Stop`]).
//! - [`Log`]: the system log, the plugin's id as the program and an
//!   instance's key after it (`awg/vpn`).
//! - [`testing`]: the router in memory, so a plugin is tested without one.
//!
//! The router's side of the contract is [`protocol`], re-exported.
//!
//! ```no_run
//! use routeh_plugin_sdk::{Desired, InstanceState, Log, Phase, Plugin, State};
//!
//! struct Minimal;
//!
//! impl Plugin for Minimal {
//!     const ID: &'static str = "minimal";
//!     const VERSION: &'static str = env!("CARGO_PKG_VERSION");
//!     type Settings = serde_json::Value;
//!     type Instance = serde_json::Value;
//!
//!     fn converge(&mut self, desired: &Desired, _: &Log) -> State {
//!         desired.instances.keys().fold(State::settled(), |state, key| {
//!             state.instance(key, InstanceState::new(Phase::Up))
//!         })
//!     }
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     routeh_plugin_sdk::run(Minimal)
//! }
//! ```
mod desired;
mod log;
mod serve;
mod state;
mod stop;
pub mod testing;

pub use desired::{Desired, Instance, ReadError};
pub use log::{Entry, Level, Log};
pub use routeh_plugin_protocol as protocol;
pub use routeh_plugin_protocol::channel::{Action, Phase};
pub use serve::{run, serve};
pub use state::{InstanceState, RowState, State};
pub use stop::Stop;

use serde::de::DeserializeOwned;
use std::time::Duration;

/// A plugin's service: what it is, what it is told and what it does.
pub trait Plugin {
    /// Its id, as its manifest says: the program in the system log and
    /// what `hello` names.
    const ID: &'static str;
    /// Its version, as its manifest says (`env!("CARGO_PKG_VERSION")`).
    const VERSION: &'static str;
    /// The plugin's own settings (`[plugins.<id>]`), read from `desired`;
    /// `serde_json::Value` to read them by hand.
    type Settings: DeserializeOwned;
    /// The settings of one instance (a tunnel of its kind).
    type Instance: DeserializeOwned;

    /// Make the system what `desired` says: make what is new, change what
    /// differs, remove what is no longer there. Called with every
    /// `desired` the router sends, and on every tick with the last one.
    /// Returns everything the plugin is now; the digest is the SDK's.
    fn converge(&mut self, desired: &Desired<Self::Settings, Self::Instance>, log: &Log) -> State;

    /// How often to converge again with the last `desired`, for a plugin
    /// whose kernel sends no events; `None` (the default) is never.
    fn tick(&self) -> Option<Duration> {
        None
    }

    /// Does the action a person asked for (`[[actions]]` of the manifest;
    /// `action.row` is the key of the row it is done to, for an action of
    /// a group), with the last `desired` (`None` is never given: an action
    /// before any `desired` is refused by the SDK). The answer is a message
    /// for the person: `Ok` when it was done, `Err` when not. An action
    /// changes no configuration. After it the SDK converges again with the
    /// last `desired` and sends `state` if it changed, so what the action
    /// did is reported by `converge`. By default no action is supported.
    fn action(
        &mut self,
        action: &Action,
        _desired: Option<&Desired<Self::Settings, Self::Instance>>,
        _log: &Log,
    ) -> Result<String, String> {
        Err(format!("{} is not supported", action.key))
    }

    /// Called once when the service stops, after the last `converge`.
    fn shutdown(&mut self, _log: &Log) {}
}
