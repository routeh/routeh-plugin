//! What a plugin's page is made of (the router's docs/PLUGINS.md, The
//! plugin's page): a group in the plugin's own settings, keyed by a field;
//! references to the router's sections; status values of each row; and
//! actions.
use crate::{minimal, refusal};
use routeh_plugin_protocol::{FieldType, Manifest, Reference, Referenced, Value};

/// A plugin with a group `records` keyed by `host` (`group` is the rest
/// of its declaration), every field in one form section, and `more`.
fn records(group: &str, more: &str) -> String {
    minimal(&format!(
        "\n[settings.records]\ntype = \"group\"\nlabel = \"Records\"\ncolumns = [\"host\", \"uplink\"]\n{group}\n\
         [settings.records.fields]\nhost = {{ type = \"text\", required = true, label = \"Host\" }}\n\
         uplink = {{ type = \"uplink\", required = true, label = \"Uplink\" }}\n\
         \n[[form]]\ntitle = \"Records\"\nfields = [\"records\"]\n{more}"
    ))
}

const KEYED: &str = "key = \"host\"";

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

#[test]
fn a_plugin_has_a_group_of_its_own_keyed_by_a_field() {
    let manifest = Manifest::parse(&records(KEYED, "")).unwrap();
    let field = manifest.settings.field("records").unwrap();
    let FieldType::Group { key, max_rows, .. } = &field.kind else {
        panic!("not a group");
    };
    assert_eq!(key.as_ref().unwrap().as_str(), "host");
    assert_eq!(*max_rows, 16);
    // A key is optional while nothing is of the group.
    assert!(Manifest::parse(&records("", "")).is_ok());
}

#[test]
fn a_groups_key_is_a_required_field_of_text_or_a_reference() {
    assert_eq!(
        refusal(&records("key = \"name\"", "")),
        "settings.records.key: \"name\" is not a field of the group"
    );
    assert_eq!(
        refusal(&records(KEYED, "").replace(
            "host = { type = \"text\", required = true",
            "host = { type = \"text\", required = false"
        )),
        "settings.records.key: host is not required"
    );
    assert_eq!(
        refusal(&records(KEYED, "").replace("host = { type = \"text\"", "host = { type = \"u16\"")),
        "settings.records.key: a group's key is text or a reference"
    );
    // A reference is a key too.
    assert!(Manifest::parse(&records("key = \"uplink\"", "")).is_ok());
}

#[test]
fn a_group_holds_no_group_in_the_plugins_settings_either() {
    let nested = minimal(
        "\n[settings.g]\ntype = \"group\"\nlabel = \"G\"\ncolumns = [\"a\"]\n\
         [settings.g.fields]\na = { type = \"group\", fields = { b = { type = \"bool\", label = \"B\" } }, columns = [\"b\"], label = \"A\" }\n\
         \n[[form]]\ntitle = \"G\"\nfields = [\"g\"]\n",
    );
    assert_eq!(
        refusal(&nested),
        "settings.g.fields.a.type: a group holds no group"
    );
    let listed = minimal(
        "\n[settings]\nl = { type = \"list\", item = { type = \"group\" }, label = \"L\" }\n\n[[form]]\ntitle = \"L\"\nfields = [\"l\"]\n",
    );
    assert!(refusal(&listed).starts_with("settings.l.item.type: a list holds "));
}

#[test]
fn rows_are_named_once_by_their_key_ignoring_case() {
    let manifest = Manifest::parse(&records(KEYED, "")).unwrap();
    let check = |rows: &str| {
        manifest
            .settings
            .check(&table(&format!("records = [{rows}]")))
            .map_err(|refusal| refusal.to_string())
    };
    assert!(
        check("{ host = \"home\", uplink = \"wan\" }, { host = \"office\", uplink = \"wan\" }")
            .is_ok()
    );
    assert_eq!(
        check("{ host = \"home\", uplink = \"wan\" }, { host = \"HOME\", uplink = \"lte\" }")
            .unwrap_err(),
        "records.1.host: HOME is named twice"
    );
    // Without a key, rows may repeat.
    let unkeyed = Manifest::parse(&records("", "")).unwrap();
    assert!(
        unkeyed
            .settings
            .check(&table(
                "records = [{ host = \"a\", uplink = \"wan\" }, { host = \"a\", uplink = \"wan\" }]"
            ))
            .is_ok()
    );
}

/// A plugin whose settings have one field `f` declared as `spec`.
fn one(spec: &str) -> Manifest {
    Manifest::parse(&minimal(&format!(
        "\n[settings]\nf = {spec}\n\n[[form]]\ntitle = \"F\"\nfields = [\"f\"]\n"
    )))
    .unwrap()
}

#[test]
fn a_reference_is_the_key_of_a_section_of_the_router() {
    for (word, kind) in [
        ("uplink", Reference::Uplink),
        ("network", Reference::Network),
        ("tunnel", Reference::Tunnel),
        ("client", Reference::Client),
    ] {
        let manifest = one(&format!("{{ type = \"{word}\", label = \"F\" }}"));
        assert!(
            matches!(manifest.settings.field("f").unwrap().kind, FieldType::Reference(k) if k == kind)
        );
        assert_eq!(kind.word(), word);
        let settings = manifest.settings.check(&table("f = \"wan_2\"")).unwrap();
        assert_eq!(
            settings.get("f"),
            Some(&Value::Reference(kind, "wan_2".into()))
        );
        // Written as the key, in the document and in `desired`.
        assert_eq!(manifest.settings.write(&settings), "f = \"wan_2\"\n");
        assert_eq!(settings.to_toml(), table("f = \"wan_2\""));
    }
    let manifest = one("{ type = \"client\", label = \"F\" }");
    // A client's key may start with a digit.
    assert!(manifest.settings.check(&table("f = \"2a\"")).is_ok());
    for bad in [
        "\"\"",
        "\"Wan\"",
        "\"wan-2\"",
        &format!("\"{}\"", "a".repeat(33)),
    ] {
        assert_eq!(
            manifest
                .settings
                .check(&table(&format!("f = {bad}")))
                .unwrap_err()
                .to_string(),
            "f: a key of the router's document: 1 to 32 lowercase letters, digits or underscores",
            "{bad}"
        );
    }
    assert_eq!(
        manifest
            .settings
            .check(&table("f = 1"))
            .unwrap_err()
            .to_string(),
        "f: must be a string"
    );
    assert_eq!(
        refusal(&minimal(
            "\n[settings]\nf = { type = \"uplink\", secret = true, label = \"F\" }\n\n[[form]]\ntitle = \"F\"\nfields = [\"f\"]\n"
        )),
        "settings.f.secret: only text, bytes and template are secret"
    );
}

#[test]
fn every_reference_is_listed_with_its_path() {
    let manifest = Manifest::parse(&minimal(
        "\n[settings]\nvia = { type = \"list\", item = { type = \"network\" }, label = \"Via\" }\n\
         exit = { type = \"tunnel\", label = \"Exit\" }\n\
         rows = { type = \"group\", fields = { c = { type = \"client\", label = \"C\" } }, columns = [\"c\"], label = \"Rows\" }\n\
         \n[[form]]\ntitle = \"All\"\nfields = [\"via\", \"exit\", \"rows\"]\n",
    ))
    .unwrap();
    let settings = manifest
        .settings
        .check(&table(
            "via = [\"lan\", \"iot\"]\nexit = \"vpn\"\nrows = [{ c = \"tv\" }, {}]",
        ))
        .unwrap();
    let referenced = |at: &str, kind, key: &str| Referenced {
        at: at.into(),
        kind,
        key: key.into(),
    };
    assert_eq!(
        settings.references(),
        [
            referenced("exit", Reference::Tunnel, "vpn"),
            referenced("rows.0.c", Reference::Client, "tv"),
            referenced("via.0", Reference::Network, "lan"),
            referenced("via.1", Reference::Network, "iot"),
        ]
    );
}

#[test]
fn a_status_value_or_an_action_may_be_of_each_row_of_a_keyed_group() {
    let more = "\n[[status]]\nof = \"records\"\nkey = \"address\"\nlabel = \"Address\"\ntype = \"address\"\n\
                \n[[status]]\nkey = \"address\"\nlabel = \"Address\"\ntype = \"address\"\n\
                \n[[actions]]\nkey = \"update_now\"\nlabel = \"Update now\"\nof = \"records\"\n\
                \n[[actions]]\nkey = \"restart\"\nlabel = \"Restart\"\n";
    let manifest = Manifest::parse(&records(KEYED, more)).unwrap();
    assert_eq!(manifest.status[0].of.as_ref().unwrap().as_str(), "records");
    assert_eq!(manifest.status[1].of, None, "the same key of the plugin");
    assert_eq!(manifest.actions[0].key.as_str(), "update_now");
    assert_eq!(manifest.actions[0].label, "Update now");
    assert_eq!(manifest.actions[0].of.as_ref().unwrap().as_str(), "records");
    assert_eq!(manifest.actions[1].of, None);

    let status_of = |of: &str| {
        format!("\n[[status]]\nof = \"{of}\"\nkey = \"a\"\nlabel = \"A\"\ntype = \"text\"\n")
    };
    assert_eq!(
        refusal(&records(KEYED, &status_of("hosts"))),
        "status.of: hosts is not a group of the settings"
    );
    assert_eq!(
        refusal(&records("", &status_of("records"))),
        "status.of: records has no key"
    );
    assert_eq!(
        refusal(&records(
            KEYED,
            &format!("{}{}", status_of("records"), status_of("records"))
        )),
        "status.key: a is declared twice"
    );
    let action = |lines: &str| format!("\n[[actions]]\n{lines}\n");
    assert_eq!(
        refusal(&records(
            KEYED,
            &action("key = \"go\"\nlabel = \"Go\"\nof = \"host\"")
        )),
        "actions.of: host is not a group of the settings"
    );
    assert_eq!(
        refusal(&records(
            "",
            &action("key = \"go\"\nlabel = \"Go\"\nof = \"records\"")
        )),
        "actions.of: records has no key"
    );
    assert_eq!(
        refusal(&records(
            KEYED,
            &action("key = \"go\"\nlabel = \"Go\"").repeat(2)
        )),
        "actions.key: go is declared twice"
    );
    assert_eq!(
        refusal(&records(KEYED, &action("key = \"Go\"\nlabel = \"Go\""))),
        "actions.key: a key is 1 to 32 lowercase letters, digits or underscores, starting with a letter"
    );
    assert_eq!(
        refusal(&records(KEYED, &action("key = \"go\"\nlabel = \"\""))),
        "actions.label: a display name is 1 to 64 characters, not only whitespace, without control or invisible format characters"
    );
    assert_eq!(
        refusal(&records(KEYED, &action("key = \"go\""))),
        "actions.label: required"
    );
    // A tunnel's status is of the tunnel: it has no `of`.
    assert_eq!(
        refusal(&crate::kind("", &[]).replace(
            "[[tunnel.form]]",
            "[[tunnel.status]]\nof = \"records\"\nkey = \"a\"\nlabel = \"A\"\ntype = \"text\"\n\n[[tunnel.form]]"
        )),
        "tunnel.status.of: unknown key"
    );
}

#[test]
fn the_keys_of_a_groups_rows_are_its_key_fields_values() {
    let manifest = Manifest::parse(&records(KEYED, "")).unwrap();
    let settings = manifest
        .settings
        .check(&table(
            "records = [{ host = \"home\", uplink = \"wan\" }, { host = \"office\", uplink = \"wan\" }]",
        ))
        .unwrap();
    assert_eq!(
        manifest.settings.row_keys(&settings, "records"),
        ["home", "office"]
    );
    assert!(manifest.settings.row_keys(&settings, "none").is_empty());
    let unkeyed = Manifest::parse(&records("", "")).unwrap();
    assert!(unkeyed.settings.row_keys(&settings, "records").is_empty());
}
