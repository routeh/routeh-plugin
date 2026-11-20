//! `state` as the plugin writes it, and as the router reads it.
use crate::support::manifest;
use routeh_plugin_sdk::protocol::channel::{self, Message, Reading};
use routeh_plugin_sdk::{InstanceState, Phase, RowState, State};

#[test]
fn the_state_before_any_desired_has_no_digest() {
    assert_eq!(
        State::default().line(None),
        "{\"state\":{\"digest\":null,\"instances\":{},\"rows\":{},\"settled\":false,\"values\":{}}}\n"
    );
    let Message::State(report) = channel::read(
        State::default().line(None).trim_end().as_bytes(),
        &manifest(),
    )
    .unwrap() else {
        panic!("not a state");
    };
    assert_eq!(report.digest, "");
    assert!(!report.settled);
}

#[test]
fn a_state_is_one_line_with_its_keys_in_order() {
    let state = State::settled().value("converged", 2).instance(
        "vpn",
        InstanceState::new(Phase::Up)
            .message("handshake")
            .peer("203.0.113.7:51820")
            .value("since", 1_795_012_345),
    );
    assert_eq!(
        state.line(Some("sha256:ab")),
        concat!(
            "{\"state\":{\"digest\":\"sha256:ab\",\"instances\":{\"vpn\":",
            "{\"message\":\"handshake\",\"peer\":\"203.0.113.7:51820\",\"phase\":\"up\",",
            "\"values\":{\"since\":1795012345}}},\"rows\":{},\"settled\":true,",
            "\"values\":{\"converged\":2}}}\n"
        )
    );
}

#[test]
fn the_router_reads_what_the_plugin_writes() {
    let state = State::settled()
        .value("converged", 3)
        .value("undeclared", "x")
        .instance(
            "vpn",
            InstanceState::new(Phase::Error)
                .message("no peer")
                .value("since", 5),
        )
        .instance("lab", InstanceState::new(Phase::Starting));
    let line = state.line(Some("sha256:cd"));
    let Message::State(report) = channel::read(line.trim_end().as_bytes(), &manifest()).unwrap()
    else {
        panic!("not a state");
    };
    assert_eq!(report.digest, "sha256:cd");
    assert!(report.settled);
    assert_eq!(report.values, [("converged".to_owned(), Reading::Count(3))]);
    // A key the manifest does not declare is the router's to drop.
    assert_eq!(report.dropped, ["undeclared"]);
    let vpn = &report.instances["vpn"];
    assert_eq!(vpn.phase, Phase::Error);
    assert_eq!(vpn.message, "no peer");
    assert_eq!(vpn.peer, None);
    assert_eq!(vpn.values, [("since".to_owned(), Reading::Time(5))]);
    assert_eq!(report.instances["lab"].phase, Phase::Starting);
    assert_eq!(report.first_error().unwrap().0, "vpn");
}

#[test]
fn every_phase_is_written_as_the_router_reads_it() {
    for phase in [
        Phase::Starting,
        Phase::Connecting,
        Phase::Up,
        Phase::Down,
        Phase::Error,
    ] {
        let line = State::settled()
            .instance("a", InstanceState::new(phase))
            .line(Some("d"));
        let Message::State(report) =
            channel::read_bare(line.trim_end().as_bytes(), "demo").unwrap()
        else {
            panic!("not a state");
        };
        assert_eq!(report.instances["a"].phase, phase);
    }
}

#[test]
fn rows_are_written_by_group_and_key_and_read_by_the_router() {
    let state = State::settled()
        .row(
            "records",
            "home",
            RowState::new(Phase::Up)
                .message("updated")
                .value("address", "203.0.113.7"),
        )
        .row("records", "office", RowState::new(Phase::Down));
    let line = state.line(Some("d"));
    assert!(
        line.contains(concat!(
            "\"rows\":{\"records\":{\"home\":{\"message\":\"updated\",\"phase\":\"up\",",
            "\"values\":{\"address\":\"203.0.113.7\"}},",
            "\"office\":{\"message\":\"\",\"phase\":\"down\",\"values\":{}}}}"
        )),
        "{line}"
    );
    let Message::State(report) = channel::read(line.trim_end().as_bytes(), &manifest()).unwrap()
    else {
        panic!("not a state");
    };
    let records = &report.rows["records"];
    assert_eq!(records["home"].phase, Phase::Up);
    assert_eq!(records["home"].message, "updated");
    assert_eq!(
        records["home"].values,
        [(
            "address".to_owned(),
            Reading::Address("203.0.113.7".parse().unwrap())
        )]
    );
    assert_eq!(records["office"].phase, Phase::Down);
}
