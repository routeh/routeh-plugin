//! The router in memory: what it refuses of a plugin.
use crate::support::manifest;
use routeh_plugin_sdk::protocol::channel::LINE_LIMIT;
use routeh_plugin_sdk::testing::{Router, TempDir};
use std::io::Write;
use std::os::unix::net::UnixListener;

/// A plugin that sends `text` on each connection, written by hand.
fn sending(text: String) -> (TempDir, std::path::PathBuf, std::thread::JoinHandle<()>) {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("demo.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.write_all(text.as_bytes());
    });
    (dir, socket, thread)
}

#[test]
fn a_line_over_the_limit_ends_the_connection() {
    let line = format!("{{\"hello\":\"{}\"}}\n", "x".repeat(LINE_LIMIT));
    let (_dir, socket, thread) = sending(line);
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    let error = router.message().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    thread.join().unwrap();
}

#[test]
fn what_the_router_refuses_is_an_error() {
    let hello = "{\"hello\":{\"protocol\":2,\"plugin\":\"demo\",\"version\":\"1\"}}\n";
    let state = "{\"state\":{\"digest\":null,\"settled\":false}}\n";
    let (_dir, socket, thread) = sending(format!("{hello}{state}{state}"));
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    let error = router.hello().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("protocol 2 is not 1"), "{error}");
    // A state where a hello is expected, and the reverse.
    assert!(router.hello().is_err());
    assert_eq!(router.state().unwrap().digest, "");
    // The end of the stream.
    assert_eq!(
        router.state().unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    thread.join().unwrap();
}

#[test]
fn a_hello_where_a_state_is_expected_is_an_error() {
    let hello = "{\"hello\":{\"protocol\":1,\"plugin\":\"demo\",\"version\":\"1\"}}\n";
    let (_dir, socket, thread) = sending(hello.to_owned());
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    assert!(router.state().is_err());
    thread.join().unwrap();
}

#[test]
fn connecting_where_nothing_listens_gives_up() {
    let dir = TempDir::new().unwrap();
    let error = Router::connect_within(
        &dir.path().join("none.sock"),
        &manifest(),
        std::time::Duration::from_millis(50),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn a_temporary_directory_is_removed_with_what_is_in_it() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_owned();
    std::fs::create_dir(path.join("inner")).unwrap();
    std::fs::write(path.join("inner/file"), "x").unwrap();
    assert_ne!(TempDir::new().unwrap().path(), path);
    drop(dir);
    assert!(!path.exists());
}

#[test]
fn an_action_the_manifest_does_not_declare_is_not_sent() {
    let (_dir, socket, thread) = sending(String::new());
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    let error = router.action("reboot", None).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "demo has no action reboot");
    thread.join().unwrap();
}
