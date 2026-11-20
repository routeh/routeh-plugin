//! The system log: the plugin's id as the program, an instance's key
//! after it.
use routeh_plugin_sdk::testing::TempDir;
use routeh_plugin_sdk::{Entry, Level, Log};
use std::os::unix::net::UnixDatagram;
use std::time::Duration;

fn received(socket: &UnixDatagram) -> String {
    let mut buffer = [0u8; 512];
    let length = socket.recv(&mut buffer).unwrap();
    String::from_utf8(buffer[..length].to_vec()).unwrap()
}

#[test]
fn writes_syslog_lines_of_the_daemon_facility() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("log");
    let socket = UnixDatagram::bind(&path).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let log = Log::to_socket(&path, "awg").unwrap();
    let pid = std::process::id();
    log.info("listening");
    assert_eq!(received(&socket), format!("<30>awg[{pid}]: listening"));
    log.instance("vpn").error("no peer");
    assert_eq!(received(&socket), format!("<27>awg/vpn[{pid}]: no peer"));
    log.warning("w");
    assert_eq!(received(&socket), format!("<28>awg[{pid}]: w"));
    log.notice("n");
    assert_eq!(received(&socket), format!("<29>awg[{pid}]: n"));
    log.debug("d");
    assert_eq!(received(&socket), format!("<31>awg[{pid}]: d"));
    // Not kept: it went to the system log.
    assert!(log.entries().is_empty());
}

#[test]
fn a_log_without_its_socket_writes_to_standard_error() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("none");
    assert!(Log::to_socket(&missing, "awg").is_err());
    let log = Log::open_at(&missing, "awg");
    log.info("to standard error");
    assert!(log.entries().is_empty());
    Log::stderr("awg").instance("vpn").error("also");
}

#[test]
fn a_log_in_memory_keeps_every_line_of_its_instances() {
    let log = Log::memory("awg");
    log.info("started");
    log.instance("vpn").error("no peer");
    assert_eq!(
        log.entries(),
        [
            Entry {
                level: Level::Info,
                program: "awg".into(),
                text: "started".into()
            },
            Entry {
                level: Level::Error,
                program: "awg/vpn".into(),
                text: "no peer".into()
            },
        ]
    );
    assert!(log.instance("x").entries().len() == 2);
    assert!(log.contains("no peer"));
    assert!(!log.contains("absent"));
}
