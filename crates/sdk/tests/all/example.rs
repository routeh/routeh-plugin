//! The examples run as the router runs a plugin's service: one
//! argument, the socket; SIGTERM ends it.
use crate::support::{desired, instance};
use routeh_plugin_sdk::protocol::Manifest;
use routeh_plugin_sdk::protocol::channel::Phase;
use routeh_plugin_sdk::testing::{Router, TempDir};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

/// `cargo test` builds the examples beside the test binaries' directory.
fn example() -> PathBuf {
    built("minimal")
}

fn built(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let path = exe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join(name);
    assert!(
        path.exists(),
        "{} is not built: run `cargo test -p routeh-plugin-sdk` or `make test`, which build the examples",
        path.display()
    );
    path
}

/// The example's manifest: what the router would install with it.
fn manifest() -> Manifest {
    Manifest::parse(
        "id = \"minimal\"\nname = \"Minimal\"\nversion = \"0.1.0\"\napi = 1\n\n[service]\ncommand = \"bin/minimal\"\n",
    )
    .unwrap()
}

/// A child killed and reaped when dropped, so a failed assertion leaves
/// no process behind.
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Running {
    fn exited_within(&mut self, within: Duration) -> ExitStatus {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the example did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn the_example_serves_until_it_is_terminated() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("minimal.sock");
    let mut child = Running(Command::new(example()).arg(&socket).spawn().unwrap());
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    assert_eq!(router.hello().unwrap().plugin, "minimal");
    router.state().unwrap();
    let line = desired("", &[instance("vpn", "")]);
    router.send(&line).unwrap();
    assert_eq!(
        router.settled(&line.digest).unwrap().instances["vpn"].phase,
        Phase::Up
    );
    // SAFETY: a signal to the child this test started.
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) }, 0);
    assert!(child.exited_within(Duration::from_secs(5)).success());
    assert!(!socket.exists());
}

/// procd runs a service in ujail, its parent: a jail that is killed
/// leaves the service to nobody, and procd starts another one. The
/// service ends with its parent instead of running on beside the new one.
#[cfg(target_os = "linux")]
#[test]
fn the_example_ends_when_its_parent_does() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("minimal.sock");
    let mut parent = Running(
        Command::new("sh")
            .arg("-c")
            .arg("\"$0\" \"$1\" & echo $!; wait")
            .arg(example())
            .arg(&socket)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut said = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(parent.0.stdout.take().unwrap()),
        &mut said,
    )
    .unwrap();
    let service: i32 = said.trim().parse().unwrap();
    let mut router = Router::connect(&socket, &manifest()).unwrap();
    assert_eq!(router.hello().unwrap().plugin, "minimal");
    parent.0.kill().unwrap();
    parent.0.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    // SAFETY: signal 0 only asks whether the process is there.
    while unsafe { libc::kill(service, 0) } == 0 {
        if Instant::now() >= deadline {
            // SAFETY: the process this test made; it is not left behind.
            unsafe { libc::kill(service, libc::SIGKILL) };
            panic!("the service outlived its parent");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_example_wants_exactly_its_socket() {
    for args in [&[][..], &["a", "b"][..]] {
        let output = Command::new(example()).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage: minimal <socket>"));
    }
}

#[test]
fn the_example_says_why_it_cannot_serve() {
    let dir = TempDir::new().unwrap();
    let output = Command::new(example())
        .arg(dir.path().join("no/such.sock"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_records_example_reports_its_rows_and_does_its_action() {
    let manifest = Manifest::parse(include_str!("../../examples/records.toml")).unwrap();
    assert_eq!(manifest.page.as_ref().unwrap().title, "Dynamic DNS");
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("records.sock");
    let _child = Running(Command::new(built("records")).arg(&socket).spawn().unwrap());
    let mut router = Router::connect(&socket, &manifest).unwrap();
    let settings = manifest
        .settings
        .check(
            &"token = \"t\"\nrecords = [{ host = \"home\", domain = \"example.org\", uplink = \"wan\" }]"
                .parse()
                .unwrap(),
        )
        .unwrap();
    let line = routeh_plugin_sdk::protocol::channel::desired(&settings, &[]);
    router.send(&line).unwrap();
    let state = router.settled(&line.digest).unwrap();
    let home = &state.rows["records"]["home"];
    assert_eq!(home.phase, Phase::Up);
    assert_eq!(home.values[0].0, "updates");
    let done = router.action("update_now", Some("home")).unwrap();
    assert!(done.ok, "{}", done.message);
    assert_eq!(done.message, "Updated home.example.org");
    let after = router.state().unwrap();
    assert_eq!(
        after.rows["records"]["home"].values,
        [(
            "updates".to_owned(),
            routeh_plugin_sdk::protocol::channel::Reading::Count(1)
        )]
    );
    let missing = router.action("update_now", Some("nas")).unwrap();
    assert!(!missing.ok);
    assert_eq!(missing.message, "no record nas");
}
