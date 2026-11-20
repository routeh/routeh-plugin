//! Rows in `state`, actions and their answers, as the router reads and
//! writes them.
use routeh_plugin_protocol::channel::{
    self, Action, ChannelError, Done, MESSAGE_LIMIT, Message, Phase, Reading,
};
use routeh_plugin_protocol::{Manifest, Settings};

/// A plugin with a group `records` keyed by `host`, a value and an action
/// of each row, an action of its own, and a group `notes` nothing is of.
const MANIFEST: &str = r#"
id = "ddns"
name = "Dynamic DNS"
version = "1.0"
api = 1

[service]
command = "bin/ddns"

[settings.records]
type = "group"
label = "Records"
key = "host"
columns = ["host"]
fields = { host = { type = "text", required = true, label = "Host" } }

[settings.notes]
type = "group"
label = "Notes"
key = "text"
columns = ["text"]
fields = { text = { type = "text", required = true, label = "Text" } }

[[form]]
title = "All"
fields = ["records", "notes"]

[[status]]
of = "records"
key = "address"
label = "Address"
type = "address"

[[status]]
key = "runs"
label = "Runs"
type = "count"

[[actions]]
key = "update_now"
label = "Update now"
of = "records"

[[actions]]
key = "restart"
label = "Restart"
"#;

fn manifest() -> Manifest {
    Manifest::parse(MANIFEST).unwrap()
}

fn settings(text: &str) -> Settings {
    manifest().settings.check(&text.parse().unwrap()).unwrap()
}

fn state(line: &str) -> channel::Report {
    match channel::read(line.as_bytes(), &manifest()).unwrap() {
        Message::State(report) => report,
        other => panic!("{other:?}"),
    }
}

#[test]
fn rows_are_read_for_groups_something_is_of() {
    let report = state(
        r#"{"state":{"digest":"d","settled":true,"values":{"runs":2},"rows":{
            "records":{"home":{"phase":"up","message":"ok","values":{"address":"203.0.113.7","extra":1}},
                       "office":{"phase":"error","values":{"address":7}}},
            "notes":{"a":{"phase":"up"}}}}}"#,
    );
    assert_eq!(report.values, [("runs".to_owned(), Reading::Count(2))]);
    let records = &report.rows["records"];
    assert_eq!(records["home"].phase, Phase::Up);
    assert_eq!(records["home"].message, "ok");
    assert_eq!(
        records["home"].values,
        [(
            "address".to_owned(),
            Reading::Address("203.0.113.7".parse().unwrap())
        )]
    );
    assert_eq!(records["office"].phase, Phase::Error);
    assert_eq!(records["office"].message, "");
    assert!(records["office"].values.is_empty());
    // A group nothing is of, and a value not of the group, are dropped;
    // a value of another type is missing.
    assert!(!report.rows.contains_key("notes"));
    assert_eq!(report.dropped, ["rows.notes", "rows.records.home.extra"]);
    assert_eq!(report.wrong, ["rows.records.office.address"]);
    // A row in error is no instance in error.
    assert!(!report.in_error());
}

#[test]
fn a_row_is_refused_as_an_instance_is() {
    let refused = |line: &str| channel::read(line.as_bytes(), &manifest()).unwrap_err();
    assert_eq!(
        refused(
            r#"{"state":{"digest":null,"settled":false,"rows":{"records":{"home":{"phase":"gone"}}}}}"#
        ),
        ChannelError::Refused("state: row records.home: unknown phase gone".into())
    );
    assert!(matches!(
        refused(r#"{"state":{"digest":null,"settled":false,"rows":{"records":{"home":1}}}}"#),
        ChannelError::Refused(reason) if reason.starts_with("state: row records.home: ")
    ));
    assert!(matches!(
        refused(r#"{"state":{"digest":null,"settled":false,"rows":{"records":1}}}"#),
        ChannelError::Refused(reason) if reason.starts_with("state: ")
    ));
}

#[test]
fn a_bare_reading_drops_every_row() {
    let line =
        r#"{"state":{"digest":"d","settled":true,"rows":{"records":{"home":{"phase":"up"}}}}}"#;
    let Message::State(report) = channel::read_bare(line.as_bytes(), "ddns").unwrap() else {
        panic!("not a state");
    };
    assert!(report.rows.is_empty());
    assert_eq!(report.dropped, ["rows.records"]);
}

#[test]
fn rows_the_document_does_not_have_are_dropped_by_the_router() {
    let mut report = state(
        r#"{"state":{"digest":"d","settled":true,"rows":{"records":{"home":{"phase":"up"},"old":{"phase":"down"}}}}}"#,
    );
    report.retain_rows(
        &manifest().settings,
        &settings("records = [{ host = \"home\" }]"),
    );
    let keys: Vec<&String> = report.rows["records"].keys().collect();
    assert_eq!(keys, ["home"]);
    assert_eq!(report.dropped, ["rows.records.old"]);
}

#[test]
fn an_action_is_written_for_what_the_manifest_declares() {
    let manifest = manifest();
    assert_eq!(
        channel::action_line(&manifest, 7, "update_now", Some("home")).unwrap(),
        "{\"action\":{\"id\":7,\"key\":\"update_now\",\"row\":\"home\"}}\n"
    );
    assert_eq!(
        channel::action_line(&manifest, 8, "restart", None).unwrap(),
        "{\"action\":{\"id\":8,\"key\":\"restart\"}}\n"
    );
    let refused = |key: &str, row: Option<&str>| {
        channel::action_line(&manifest, 1, key, row)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(refused("reboot", None), "ddns has no action reboot");
    assert_eq!(
        refused("update_now", None),
        "update_now is done to a row of records"
    );
    assert_eq!(
        refused("restart", Some("home")),
        "restart is done to no row"
    );
}

#[test]
fn a_plugin_reads_an_action() {
    assert_eq!(
        channel::read_action(br#"{"action":{"id":7,"key":"update_now","row":"home"}}"#).unwrap(),
        Action {
            id: 7,
            key: "update_now".into(),
            row: Some("home".into())
        }
    );
    assert_eq!(
        channel::read_action(br#"{"action":{"id":8,"key":"restart"}}"#)
            .unwrap()
            .row,
        None
    );
    assert!(channel::read_action(br#"{"desired":{}}"#).is_err());
    assert!(channel::read_action(br#"{"action":{"key":"x"}}"#).is_err());
    assert!(channel::read_action(b"x").is_err());
}

#[test]
fn an_answer_is_done_with_its_id_and_a_bounded_message() {
    let line = channel::done_line(7, true, "Updated to 203.0.113.7");
    assert_eq!(
        line,
        "{\"done\":{\"id\":7,\"message\":\"Updated to 203.0.113.7\",\"ok\":true}}\n"
    );
    assert_eq!(
        channel::read(line.trim_end().as_bytes(), &manifest()).unwrap(),
        Message::Done(Done {
            id: 7,
            ok: true,
            message: "Updated to 203.0.113.7".into()
        })
    );
    // A longer message is cut to the limit, at a whole character.
    let long = channel::done_line(1, false, &"я".repeat(MESSAGE_LIMIT + 5));
    let Message::Done(done) = channel::read(long.trim_end().as_bytes(), &manifest()).unwrap()
    else {
        panic!("not done");
    };
    assert_eq!(done.message.chars().count(), MESSAGE_LIMIT);
    assert!(!done.ok);
    // The router refuses a longer one it did not write.
    let over = format!(
        "{{\"done\":{{\"id\":1,\"ok\":true,\"message\":\"{}\"}}}}",
        "a".repeat(MESSAGE_LIMIT + 1)
    );
    assert_eq!(
        channel::read(over.as_bytes(), &manifest()).unwrap_err(),
        ChannelError::Refused(format!(
            "done: a message is at most {MESSAGE_LIMIT} characters"
        ))
    );
    assert!(matches!(
        channel::read(br#"{"done":{"ok":true}}"#, &manifest()).unwrap_err(),
        ChannelError::Refused(reason) if reason.starts_with("done: ")
    ));
}
