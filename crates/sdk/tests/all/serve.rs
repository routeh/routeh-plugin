//! A plugin served on its socket, spoken to as the router speaks.
use crate::support::{Recorder, desired, eventually, instance, manifest};
use routeh_plugin_sdk::protocol::channel::{Message, Phase, Reading};
use routeh_plugin_sdk::testing::{Harness, Router, TempDir};
use routeh_plugin_sdk::{Log, Plugin, Stop, serve};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::time::Duration;

#[test]
fn says_hello_then_what_it_is_before_any_desired() {
    let harness = Harness::start(Recorder::default()).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    let hello = router.hello().unwrap();
    assert_eq!(
        (
            hello.protocol,
            hello.plugin.as_str(),
            hello.version.as_str()
        ),
        (1, "demo", "1.0")
    );
    let state = router.state().unwrap();
    assert_eq!(state.digest, "");
    assert!(!state.settled);
    // The socket is in place, only its owner's, and nothing is left under
    // the name it was bound with.
    let metadata = std::fs::metadata(harness.socket()).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let staging = format!("{}.new", harness.socket().display());
    assert!(!std::path::Path::new(&staging).exists());
}

#[test]
fn converges_to_desired_and_says_so() {
    let plugin = Recorder::default();
    let seen = plugin.seen.clone();
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router.hello().unwrap();
    router.state().unwrap();
    let line = desired(
        "log_level = \"info\"",
        &[instance("vpn", "listen_port = 1")],
    );
    router.send(&line).unwrap();
    let state = router.settled(&line.digest).unwrap();
    assert_eq!(state.values, [("converged".to_owned(), Reading::Count(1))]);
    assert_eq!(state.instances["vpn"].phase, Phase::Up);
    assert!(state.dropped.is_empty() && state.wrong.is_empty());
    assert_eq!(seen.lock().unwrap().digests, [line.digest]);
    assert!(harness.log().contains("converged 1"));
}

#[test]
fn each_desired_is_converged_before_the_next_line_is_read() {
    let plugin = Recorder::default();
    let seen = plugin.seen.clone();
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    let lines: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|key| desired("", &[instance(key, "")]))
        .collect();
    let all: String = lines.iter().map(|line| line.text.as_str()).collect();
    router.send_text(&all).unwrap();
    router.settled(&lines[2].digest).unwrap();
    let digests: Vec<String> = lines.iter().map(|line| line.digest.clone()).collect();
    assert_eq!(seen.lock().unwrap().digests, digests);
}

#[test]
fn state_goes_to_every_connection_and_only_when_it_changed() {
    let plugin = Recorder {
        steady: true,
        ..Recorder::default()
    };
    let harness = Harness::start(plugin).unwrap();
    let mut first = harness.connect(&manifest()).unwrap();
    let mut second = harness.connect(&manifest()).unwrap();
    for router in [&mut first, &mut second] {
        router.hello().unwrap();
        router.state().unwrap();
    }
    let one = desired("", &[instance("vpn", "")]);
    first.send(&one).unwrap();
    assert_eq!(first.state().unwrap().digest, one.digest);
    assert_eq!(second.state().unwrap().digest, one.digest);
    // The same again changes nothing, so nothing is sent; the next state
    // either connection reads is that of another desired.
    first.send(&one).unwrap();
    let two = desired("log_level = \"info\"", &[instance("vpn", "")]);
    second.send(&two).unwrap();
    assert_eq!(first.state().unwrap().digest, two.digest);
    assert_eq!(second.state().unwrap().digest, two.digest);
    // A connection made later is told the last state at once.
    let mut third = harness.connect(&manifest()).unwrap();
    third.hello().unwrap();
    assert_eq!(third.state().unwrap().digest, two.digest);
}

#[test]
fn a_line_it_cannot_read_is_logged_and_the_next_one_is_read() {
    let harness = Harness::start(Recorder::default()).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router
        .send_text("not json\n\n{\"hello\":{}}\n{\"action\":{}}\n")
        .unwrap();
    let line = desired("", &[]);
    router.send(&line).unwrap();
    router.settled(&line.digest).unwrap();
    assert!(
        harness
            .log()
            .contains("a line from the router was not read: not a JSON object")
    );
    assert!(
        harness
            .log()
            .contains("a line from the router was not read: not desired: hello")
    );
    assert!(
        harness
            .log()
            .contains("a line from the router was not read: action: missing field `id`")
    );
}

#[derive(Default)]
struct Typed;
#[derive(serde::Deserialize)]
struct NeedsPort {
    #[allow(dead_code)]
    listen_port: u16,
}
impl Plugin for Typed {
    const ID: &'static str = "demo";
    const VERSION: &'static str = "1.0";
    type Settings = serde_json::Value;
    type Instance = NeedsPort;
    fn converge(
        &mut self,
        _: &routeh_plugin_sdk::Desired<Self::Settings, Self::Instance>,
        _: &Log,
    ) -> routeh_plugin_sdk::State {
        routeh_plugin_sdk::State::settled()
    }
}

#[test]
fn a_desired_its_types_do_not_take_is_not_converged() {
    let harness = Harness::start(Typed).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router
        .send(&desired("", &[instance("vpn", "note = \"x\"")]))
        .unwrap();
    let good = desired("", &[instance("vpn", "listen_port = 2")]);
    router.send(&good).unwrap();
    router.settled(&good.digest).unwrap();
    assert!(harness.log().contains("missing field `listen_port`"));
}

#[test]
fn desired_has_no_line_limit() {
    let harness = Harness::start(Recorder::default()).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    let note = "n".repeat(200 * 1024);
    let line = desired("", &[instance("vpn", &format!("note = \"{note}\""))]);
    assert!(line.text.len() > 200 * 1024);
    router.send(&line).unwrap();
    router.settled(&line.digest).unwrap();
}

#[test]
fn a_state_over_the_line_limit_is_not_sent() {
    let plugin = Recorder {
        padding: 70 * 1024,
        ..Recorder::default()
    };
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router.hello().unwrap();
    router.state().unwrap();
    // No instance, no padding: sent.
    let small = desired("", &[]);
    router.send(&small).unwrap();
    router.settled(&small.digest).unwrap();
    // An instance with 70 KiB of message: refused, logged, the last one kept.
    let large = desired("", &[instance("vpn", "")]);
    router.send(&large).unwrap();
    let mut late = harness.connect(&manifest()).unwrap();
    late.hello().unwrap();
    assert!(eventually(|| harness
        .log()
        .contains("over the 65536 bytes a line may be; not sent")));
    // Told after the plugin refused the large one: what it last sent.
    let mut later = harness.connect(&manifest()).unwrap();
    later.hello().unwrap();
    assert_eq!(later.state().unwrap().digest, small.digest);
    assert_eq!(late.state().unwrap().digest, small.digest);
}

#[test]
fn converges_again_on_its_tick_once_it_was_told() {
    let plugin = Recorder {
        tick: Some(Duration::from_millis(20)),
        ..Recorder::default()
    };
    let seen = plugin.seen.clone();
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router.hello().unwrap();
    router.state().unwrap();
    // Nothing to converge to before the first desired.
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(seen.lock().unwrap().converged, 0);
    let line = desired("", &[]);
    router.send(&line).unwrap();
    router.settled(&line.digest).unwrap();
    // Each tick converges again; its count changes the state.
    let again = router.state().unwrap();
    assert_eq!(again.digest, line.digest);
    assert!(matches!(again.values[0].1, Reading::Count(n) if n >= 2));
}

#[test]
fn stops_when_told_and_takes_its_socket_away() {
    let plugin = Recorder::default();
    let seen = plugin.seen.clone();
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    router.hello().unwrap();
    let socket = harness.socket().to_owned();
    let log = harness.log();
    harness.stop().unwrap();
    assert!(seen.lock().unwrap().shut_down);
    assert!(!socket.exists());
    assert!(log.contains("shut down"));
    assert!(log.contains("stopped"));
    // The router's connection ends.
    router.state().unwrap();
    assert!(router.message().is_err());
}

#[test]
fn replaces_what_an_earlier_run_left_under_its_names() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("demo.sock");
    std::fs::write(dir.path().join("demo.sock.new"), "").unwrap();
    std::fs::write(&socket, "").unwrap();
    let stop = Stop::new();
    let thread = {
        let (socket, stop) = (socket.clone(), stop.clone());
        std::thread::spawn(move || serve(&socket, Recorder::default(), &Log::memory("demo"), &stop))
    };
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    assert!(matches!(router.message().unwrap(), Message::Hello(_)));
    stop.stop();
    thread.join().unwrap().unwrap();
}

#[test]
fn refuses_to_serve_where_it_cannot() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("no/such/dir/demo.sock");
    let error = serve(
        &socket,
        Recorder::default(),
        &Log::memory("demo"),
        &Stop::new(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

struct BadId;
impl Plugin for BadId {
    const ID: &'static str = "Not-An-Id";
    const VERSION: &'static str = "1";
    type Settings = serde_json::Value;
    type Instance = serde_json::Value;
    fn converge(&mut self, _: &routeh_plugin_sdk::Desired, _: &Log) -> routeh_plugin_sdk::State {
        routeh_plugin_sdk::State::default()
    }
}

#[test]
fn refuses_an_id_the_router_would_not_take() {
    let dir = TempDir::new().unwrap();
    let error = serve(
        &dir.path().join("x.sock"),
        BadId,
        &Log::memory("x"),
        &Stop::new(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("a plugin's id is"), "{error}");
}

#[test]
fn reports_the_rows_of_its_groups() {
    let harness = Harness::start(Recorder::default()).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    let line = desired(
        "records = [{ host = \"home\" }, { host = \"office\" }]",
        &[],
    );
    router.send(&line).unwrap();
    let state = router.settled(&line.digest).unwrap();
    let keys: Vec<&String> = state.rows["records"].keys().collect();
    assert_eq!(keys, ["home", "office"]);
    assert_eq!(state.rows["records"]["home"].phase, Phase::Up);
}

#[test]
fn answers_an_action_to_whoever_asked_and_then_says_what_changed() {
    let plugin = Recorder::default();
    let seen = plugin.seen.clone();
    let harness = Harness::start(plugin).unwrap();
    let mut asking = harness.connect(&manifest()).unwrap();
    let mut other = harness.connect(&manifest()).unwrap();
    let line = desired("records = [{ host = \"home\" }]", &[]);
    asking.send(&line).unwrap();
    asking.settled(&line.digest).unwrap();
    other.settled(&line.digest).unwrap();

    let done = asking.action("update_now", Some("home")).unwrap();
    assert!(done.ok);
    assert_eq!(done.message, "did update_now");
    let failed = asking.action("fail", None).unwrap();
    assert!(!failed.ok);
    assert_eq!(failed.message, "it failed");
    assert_ne!(done.id, failed.id);
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.actions.len(), 2);
        assert_eq!(seen.actions[0].0.row.as_deref(), Some("home"));
        assert_eq!(
            seen.actions[0].1, line.digest,
            "asked under the last desired"
        );
    }
    // Converged again after each action: the count changed, and every
    // connection is told; only the one that asked was answered.
    let after = other.state().unwrap();
    assert_eq!(after.values, [("converged".to_owned(), Reading::Count(2))]);
    assert_eq!(asking.state().unwrap().values, after.values);
    assert!(harness.log().contains("asked update_now"));
}

#[test]
fn an_action_before_any_desired_is_refused() {
    let plugin = Recorder::default();
    let seen = plugin.seen.clone();
    let harness = Harness::start(plugin).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    let done = router.action("restart", None).unwrap();
    assert!(!done.ok);
    assert_eq!(done.message, "not told what to be yet");
    assert!(seen.lock().unwrap().actions.is_empty());
}

#[test]
fn a_plugin_without_actions_answers_that_it_has_none() {
    let harness = Harness::start(Typed).unwrap();
    let mut router = harness.connect(&manifest()).unwrap();
    let good = desired("", &[instance("vpn", "listen_port = 2")]);
    router.send(&good).unwrap();
    router.settled(&good.digest).unwrap();
    let done = router.action("restart", None).unwrap();
    assert!(!done.ok);
    assert_eq!(done.message, "restart is not supported");
    // An action the line cannot be read as is logged like any other.
    router.send_text("{\"action\":{\"key\":1}}\n").unwrap();
    router.send(&good).unwrap();
    assert!(eventually(|| harness
        .log()
        .contains("a line from the router was not read: action: ")));
}
