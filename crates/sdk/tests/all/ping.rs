//! A plugin answers the router's pings by itself, also while it converges.
use crate::support::{Recorder, desired, manifest};
use routeh_plugin_sdk::protocol::channel::{self, Message};
use routeh_plugin_sdk::testing::Harness;
use routeh_plugin_sdk::{Desired, Log, Plugin, State};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[test]
fn a_plugin_built_with_the_sdk_says_it_answers_pings_and_does() {
    let harness = Harness::start(Recorder::default()).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    assert!(router.hello().unwrap().pings);
    router.state().unwrap();
    router.ping().unwrap();
    router.ping().unwrap();
}

/// Converges only when it is let go.
struct Slow {
    entered: mpsc::Sender<()>,
    release: Arc<Mutex<mpsc::Receiver<()>>>,
}
impl Plugin for Slow {
    const ID: &'static str = "demo";
    const VERSION: &'static str = "1.0";
    type Settings = serde_json::Value;
    type Instance = serde_json::Value;
    fn converge(&mut self, _: &Desired<Self::Settings, Self::Instance>, _: &Log) -> State {
        let _ = self.entered.send(());
        let _ = self.release.lock().unwrap().recv();
        State::settled()
    }
}

#[test]
fn a_long_converge_does_not_keep_a_ping_from_being_answered() {
    let (entered, converging) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let plugin = Slow {
        entered,
        release: Arc::new(Mutex::new(released)),
    };
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router.hello().unwrap();
    router.state().unwrap();
    let line = desired("log_level = \"info\"", &[]);
    router.send(&line).unwrap();
    converging.recv().unwrap();
    // It is still in `converge`, and the answer comes all the same.
    router.ping().unwrap();
    release.send(()).unwrap();
    assert!(router.settled(&line.digest).unwrap().settled);
}

#[test]
fn lines_of_a_ping_and_of_state_are_never_mixed() {
    let harness = Harness::start(Recorder::default()).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router.hello().unwrap();
    router.state().unwrap();
    let line = desired("log_level = \"info\"", &[]);
    router.send_text(channel::ping_line()).unwrap();
    router.send(&line).unwrap();
    router.send_text(channel::ping_line()).unwrap();
    let (mut pongs, mut settled) = (0, false);
    while pongs < 2 || !settled {
        match router.message().unwrap() {
            Message::Pong => pongs += 1,
            Message::State(report) => settled |= report.settled,
            other => panic!("{other:?}"),
        }
    }
}
