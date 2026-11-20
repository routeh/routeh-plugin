//! A setting's declaration: its type with the attributes of that type,
//! its role and the two constraints between fields.
use crate::{kind, minimal, refusal};

/// The refusal of a kind with one field `f` declared as `spec`.
fn field(spec: &str) -> String {
    refusal(&kind(&format!("f = {spec}"), &["f"]))
}

#[test]
fn a_field_has_a_type_of_ours_and_a_label() {
    assert_eq!(
        field("{ type = \"secret\", label = \"F\" }"),
        "tunnel.settings.f.type: \"secret\" is not one of text, bytes, template, bool, u8, u16, u32, range, duration, port, host_port, ipv4_prefix, ipv6_prefix, address, choice, uplink, network, tunnel, client, list, group"
    );
    assert_eq!(
        field("{ type = \"bool\" }"),
        "tunnel.settings.f.label: required"
    );
    assert_eq!(
        field("{ label = \"F\" }"),
        "tunnel.settings.f.type: required"
    );
    assert_eq!(field("\"bool\""), "tunnel.settings.f: must be a table");
    assert_eq!(
        refusal(&kind("F = { type = \"bool\", label = \"F\" }", &["F"])),
        "tunnel.settings.F: a key is 1 to 32 lowercase letters, digits or underscores, starting with a letter"
    );
}

#[test]
fn attributes_belong_to_their_type() {
    for (spec, reason) in [
        (
            "{ type = \"bool\", max = 3, label = \"F\" }",
            "tunnel.settings.f.max: unknown key",
        ),
        (
            "{ type = \"bytes\", label = \"F\" }",
            "tunnel.settings.f.len: required",
        ),
        (
            "{ type = \"bytes\", len = 0, label = \"F\" }",
            "tunnel.settings.f.len: 1 to 1024",
        ),
        (
            "{ type = \"u8\", max = 256, label = \"F\" }",
            "tunnel.settings.f.max: 0 to 255",
        ),
        (
            "{ type = \"u16\", min = 9, max = 3, label = \"F\" }",
            "tunnel.settings.f: min ≤ max",
        ),
        (
            "{ type = \"range\", label = \"F\" }",
            "tunnel.settings.f.of: required",
        ),
        (
            "{ type = \"range\", of = \"u8\", label = \"F\" }",
            "tunnel.settings.f.of: \"u8\" is not one of u16, u32",
        ),
        (
            "{ type = \"range\", of = \"u16\", max = 70000, label = \"F\" }",
            "tunnel.settings.f.max: 0 to 65535",
        ),
        (
            "{ type = \"duration\", min = \"5\", label = \"F\" }",
            "tunnel.settings.f.min: a duration is a whole number and s, m or h (\"25s\")",
        ),
        (
            "{ type = \"duration\", min = \"1m\", max = \"30s\", label = \"F\" }",
            "tunnel.settings.f: min ≤ max",
        ),
        (
            "{ type = \"text\", max_len = 5000, label = \"F\" }",
            "tunnel.settings.f.max_len: 1 to 4096",
        ),
        (
            "{ type = \"template\", max_len = 70000, label = \"F\" }",
            "tunnel.settings.f.max_len: 1 to 65536",
        ),
        (
            "{ type = \"text\", pattern = \"(\", label = \"F\" }",
            "tunnel.settings.f.pattern: not a regular expression",
        ),
        (
            "{ type = \"text\", pattern = \"a{1000}{1000}\", label = \"F\" }",
            "tunnel.settings.f.pattern: not a regular expression",
        ),
        (
            "{ type = \"choice\", label = \"F\" }",
            "tunnel.settings.f.values: required",
        ),
        (
            "{ type = \"choice\", values = [], label = \"F\" }",
            "tunnel.settings.f.values: 1 to 64 values",
        ),
        (
            "{ type = \"choice\", values = [\"a\", \"a\"], label = \"F\" }",
            "tunnel.settings.f.values: \"a\" is named twice",
        ),
        (
            "{ type = \"choice\", values = [\"A b\"], label = \"F\" }",
            "tunnel.settings.f.values: \"A b\" is not 1 to 64 letters, digits, '.', '_' or '-'",
        ),
        (
            "{ type = \"bool\", secret = true, label = \"F\" }",
            "tunnel.settings.f.secret: only text, bytes and template are secret",
        ),
        (
            "{ type = \"bool\", required = 1, label = \"F\" }",
            "tunnel.settings.f.required: must be a boolean",
        ),
    ] {
        assert_eq!(field(spec), reason, "{spec}");
    }
    // A pattern's size is bounded however short its text: the `regex`
    // crate refuses what compiles past the limit.
    assert!(
        field("{ type = \"text\", pattern = '\\w{200}', label = \"F\" }")
            .contains("not a regular expression")
    );
}

#[test]
fn a_list_has_items_of_one_type_and_a_group_rows_of_fields() {
    for (spec, reason) in [
        (
            "{ type = \"list\", label = \"F\" }",
            "tunnel.settings.f.item: required",
        ),
        (
            "{ type = \"list\", item = { type = \"list\", item = { type = \"bool\" } }, label = \"F\" }",
            "tunnel.settings.f.item.type: a list holds text, bytes, u8, u16, u32, range, duration, port, host_port, ipv4_prefix, ipv6_prefix, address, choice, uplink, network, tunnel or client",
        ),
        (
            "{ type = \"list\", item = { type = \"u8\", label = \"I\" }, label = \"F\" }",
            "tunnel.settings.f.item.label: unknown key",
        ),
        (
            "{ type = \"list\", item = { type = \"u8\" }, max_items = 0, label = \"F\" }",
            "tunnel.settings.f.max_items: 1 to 256",
        ),
        (
            "{ type = \"group\", label = \"F\" }",
            "tunnel.settings.f.fields: required",
        ),
        (
            "{ type = \"group\", fields = { a = { type = \"bool\", label = \"A\" } }, label = \"F\" }",
            "tunnel.settings.f.columns: required",
        ),
        (
            "{ type = \"group\", fields = { a = { type = \"bool\", label = \"A\" } }, columns = [\"b\"], label = \"F\" }",
            "tunnel.settings.f.columns: \"b\" is not a field of the group",
        ),
        (
            "{ type = \"group\", fields = { a = { type = \"bool\", label = \"A\" }, b = { type = \"bool\", label = \"B\" } }, columns = [\"a\"], label = \"F\" }",
            "tunnel.settings.f.columns: b is in no column",
        ),
        (
            "{ type = \"group\", fields = { a = { type = \"group\", fields = {}, columns = [], label = \"A\" } }, columns = [\"a\"], label = \"F\" }",
            "tunnel.settings.f.fields.a.type: a group holds no group",
        ),
        (
            "{ type = \"group\", fields = { a = { type = \"address\", role = \"endpoint\", label = \"A\" } }, columns = [\"a\"], label = \"F\" }",
            "tunnel.settings.f.fields.a.role: a role is given only to a field of the tunnel's own",
        ),
    ] {
        assert_eq!(field(spec), reason, "{spec}");
    }
}

#[test]
fn roles_fit_their_types_and_come_once() {
    for (spec, reason) in [
        (
            "{ type = \"address\", role = \"address\", label = \"F\" }",
            "tunnel.settings.f.role: an address is an ipv4_prefix or ipv6_prefix",
        ),
        (
            "{ type = \"u16\", role = \"listen\", protocol = \"udp\", label = \"F\" }",
            "tunnel.settings.f.role: listen is a port",
        ),
        (
            "{ type = \"port\", role = \"listen\", label = \"F\" }",
            "tunnel.settings.f.protocol: required",
        ),
        (
            "{ type = \"port\", role = \"listen\", protocol = \"sctp\", label = \"F\" }",
            "tunnel.settings.f.protocol: \"sctp\" is not one of udp, tcp",
        ),
        (
            "{ type = \"port\", protocol = \"udp\", label = \"F\" }",
            "tunnel.settings.f.protocol: only with role listen",
        ),
        (
            "{ type = \"text\", role = \"endpoint\", label = \"F\" }",
            "tunnel.settings.f.role: an endpoint is a host_port",
        ),
        (
            "{ type = \"text\", role = \"gateway\", label = \"F\" }",
            "tunnel.settings.f.role: \"gateway\" is not one of address, listen, endpoint",
        ),
    ] {
        assert_eq!(field(spec), reason, "{spec}");
    }
    let two = "a = { type = \"port\", role = \"listen\", protocol = \"udp\", label = \"A\" }\nb = { type = \"port\", role = \"listen\", protocol = \"tcp\", label = \"B\" }";
    assert_eq!(
        refusal(&kind(two, &["a", "b"])),
        "tunnel.settings.b.role: a has role listen already"
    );
    let both = "a = { type = \"ipv4_prefix\", role = \"address\", required = true, label = \"A\" }\nb = { type = \"ipv6_prefix\", role = \"address\", label = \"B\" }";
    assert!(
        routeh_plugin_protocol::Manifest::parse(&kind(both, &["a", "b"])).is_ok(),
        "one address of each family"
    );
    let twice = both.replace("ipv6_prefix", "ipv4_prefix");
    assert_eq!(
        refusal(&kind(&twice, &["a", "b"])),
        "tunnel.settings.b.role: a has role address already"
    );
    assert_eq!(
        refusal(&minimal(
            "[settings]\na = { type = \"host_port\", role = \"endpoint\", label = \"A\" }\n[[form]]\ntitle = \"T\"\nfields = [\"a\"]\n"
        )),
        "settings.a.role: a role is given only to a field of the tunnel's own"
    );
}

#[test]
fn constraints_name_fields_there_are() {
    let two = "a = { type = \"bool\", label = \"A\" }\nb = { type = \"bool\", label = \"B\" }";
    let with = |constraints: &str| {
        refusal(&format!(
            "{}\n[tunnel.constraints]\n{constraints}\n",
            kind(two, &["a", "b"])
        ))
    };
    assert_eq!(
        with("exactly_one = [[\"a\"]]"),
        "tunnel.constraints.exactly_one: a group of 2 fields at least"
    );
    assert_eq!(
        with("exactly_one = [[\"a\", \"c\"]]"),
        "tunnel.constraints.exactly_one: c is not a setting"
    );
    assert_eq!(
        with("exactly_one = [[\"a\", \"a\"]]"),
        "tunnel.constraints.exactly_one: a is named twice"
    );
    assert_eq!(
        with("requires = { a = \"c\" }"),
        "tunnel.constraints.requires.a: c is not a setting"
    );
    assert_eq!(
        with("requires = { c = \"a\" }"),
        "tunnel.constraints.requires.c: c is not a setting"
    );
    assert_eq!(
        with("requires = { a = \"a\" }"),
        "tunnel.constraints.requires.a: a requires itself"
    );
    assert_eq!(
        with("le = [[\"a\", \"b\"]]"),
        "tunnel.constraints.le: unknown key"
    );
    let required = two.replace("label = \"A\"", "required = true, label = \"A\"");
    assert_eq!(
        refusal(&format!(
            "{}\n[tunnel.constraints]\nexactly_one = [[\"a\", \"b\"]]\n",
            kind(&required, &["a", "b"])
        )),
        "tunnel.constraints.exactly_one: a is required"
    );
}

#[test]
fn every_part_has_its_shape() {
    let two = "a = { type = \"bool\", label = \"A\" }\nb = { type = \"bool\", label = \"B\" }";
    let with = |constraints: &str| {
        refusal(&format!(
            "{}\n[tunnel.constraints]\n{constraints}\n",
            kind(two, &["a", "b"])
        ))
    };
    const GROUPS: &str = "tunnel.constraints.exactly_one: must be an array of arrays of field keys";
    assert_eq!(with("exactly_one = [\"a\", \"b\"]"), GROUPS);
    assert_eq!(with("exactly_one = [[\"a\", 1]]"), GROUPS);
    assert_eq!(with("exactly_one = \"a\""), GROUPS);
    assert_eq!(
        with("requires = { a = 1 }"),
        "tunnel.constraints.requires.a: must be a field key"
    );
    assert_eq!(
        field("{ type = \"list\", item = { type = \"template\" }, label = \"F\" }"),
        "tunnel.settings.f.item.type: a list holds text, bytes, u8, u16, u32, range, duration, port, host_port, ipv4_prefix, ipv6_prefix, address, choice, uplink, network, tunnel or client"
    );
    assert_eq!(
        field(
            "{ type = \"group\", fields = { a = { type = \"bool\", label = \"A\" } }, columns = [\"a\", \"a\"], label = \"F\" }"
        ),
        "tunnel.settings.f.columns: a is named twice"
    );
    assert_eq!(
        field("{ type = \"group\", fields = 1, columns = [], label = \"F\" }"),
        "tunnel.settings.f.fields: must be a table"
    );
    assert_eq!(
        field("{ type = \"u16\", min = \"1\", label = \"F\" }"),
        "tunnel.settings.f.min: must be an integer"
    );
    assert_eq!(
        field("{ type = \"choice\", values = [1], label = \"F\" }"),
        "tunnel.settings.f.values: must be an array of strings"
    );
    assert_eq!(
        field("{ type = \"choice\", values = \"a\", label = \"F\" }"),
        "tunnel.settings.f.values: must be an array of strings"
    );
    assert_eq!(
        refusal(&minimal("\n[[actions]]\n")).as_str(),
        "actions.key: required"
    );
    assert_eq!(
        refusal(&minimal("actions = 1\n").replace(
            "[service]\ncommand = \"bin/demo\"\nactions = 1",
            "actions = 1\n[service]\ncommand = \"bin/demo\""
        )),
        "actions: must be an array of tables"
    );
    assert_eq!(
        refusal("id = \"x\"\nname = \"X\"\nversion = \"1\"\napi = 1\n"),
        "service: required"
    );
    assert_eq!(
        refusal("id = \"x\"\nname = \"X\"\nversion = \"1\"\napi = 1\nservice = 1\n"),
        "service: must be a table"
    );
    let ends = "a = { type = \"host_port\", role = \"endpoint\", label = \"A\" }\nb = { type = \"host_port\", role = \"endpoint\", label = \"B\" }";
    assert_eq!(
        refusal(&kind(ends, &["a", "b"])),
        "tunnel.settings.b.role: a has role endpoint already"
    );
}

#[test]
fn a_pattern_matches_the_whole_value_whatever_it_says() {
    // Pasted between anchors as text, an unbalanced alternation would
    // close the group and leave one side unanchored.
    for pattern in ["x)|(.*", "[a-z]+)|(?:"] {
        assert_eq!(
            field(&format!(
                "{{ type = \"text\", pattern = '{pattern}', label = \"F\" }}"
            )),
            "tunnel.settings.f.pattern: not a regular expression",
            "{pattern}"
        );
    }
}

#[test]
fn a_kind_has_a_required_address() {
    let manifest = |settings: &str, form: &str| {
        crate::minimal(&format!(
            "\n[tunnel]\nlink_kind = \"demo\"\nmtu = {{ default = 1400, min = 1280, max = 1420 }}\n[[tunnel.form]]\ntitle = \"All\"\nfields = [{form}]\n[tunnel.settings]\n{settings}\n"
        ))
    };
    assert_eq!(
        refusal(&manifest("a = { type = \"bool\", label = \"A\" }", "\"a\"")),
        "tunnel.settings: no field of role address"
    );
    assert_eq!(
        refusal(&manifest(
            "a = { type = \"ipv4_prefix\", role = \"address\", label = \"A\" }",
            "\"a\""
        )),
        "tunnel.settings: no field of role address is required"
    );
    let both = "a = { type = \"ipv4_prefix\", role = \"address\", required = true, label = \"A\" }\nb = { type = \"ipv6_prefix\", role = \"address\", label = \"B\" }";
    assert!(
        routeh_plugin_protocol::Manifest::parse(&manifest(both, "\"a\", \"b\"")).is_ok(),
        "one family may be optional"
    );
}

#[test]
fn a_kind_with_ipv6_can_carry_it() {
    // IPv6 needs a link MTU of 1280 at least (RFC 8200 5).
    let v6 = "a = { type = \"ipv6_prefix\", role = \"address\", required = true, label = \"A\" }";
    let with = |mtu: &str| {
        refusal(&kind(v6, &["a"]).replace("{ default = 1400, min = 1280, max = 1420 }", mtu))
    };
    const RULE: &str =
        "tunnel.mtu: a kind with an IPv6 address has a default and max of 1280 at least";
    assert_eq!(with("{ default = 1000, min = 576, max = 1100 }"), RULE);
    assert_eq!(with("{ default = 1200, min = 576, max = 1400 }"), RULE);
    assert!(
        routeh_plugin_protocol::Manifest::parse(
            &kind(v6, &["a"]).replace("min = 1280", "min = 576")
        )
        .is_ok()
    );
}

#[test]
fn a_manifest_is_bounded() {
    let many = |n: usize| {
        (0..n)
            .map(|i| format!("f{i} = {{ type = \"bool\", label = \"F\" }}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let names = |n: usize| (0..n).map(|i| format!("f{i}")).collect::<Vec<_>>();
    let form = |n: usize| names(n);
    let refs = |n: usize| form(n);
    let f = refs(256);
    assert!(
        routeh_plugin_protocol::Manifest::parse(&kind(
            &many(255),
            &f[..255].iter().map(String::as_str).collect::<Vec<_>>()
        ))
        .is_ok()
    );
    let f = refs(256);
    assert_eq!(
        refusal(&kind(
            &many(256),
            &f.iter().map(String::as_str).collect::<Vec<_>>()
        )),
        "tunnel.settings: at most 256 fields"
    );
    let columns = (0..33)
        .map(|i| format!("\"c{i}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let fields = (0..33)
        .map(|i| format!("c{i} = {{ type = \"bool\", label = \"C\" }}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        field(&format!(
            "{{ type = \"group\", fields = {{ {fields} }}, columns = [{columns}], label = \"F\" }}"
        )),
        "tunnel.settings.f.fields: at most 32 fields"
    );
    let actions = (0..33)
        .map(|i| format!("[[actions]]\nkey = \"a{i}\"\nlabel = \"A\"\n"))
        .collect::<String>();
    assert_eq!(
        refusal(&minimal(&format!("\n{actions}"))),
        "actions: at most 32 actions"
    );
    let status = (0..65)
        .map(|i| format!("[[status]]\nkey = \"s{i}\"\nlabel = \"S\"\ntype = \"count\"\n"))
        .collect::<String>();
    assert_eq!(
        refusal(&minimal(&format!("\n{status}"))),
        "status: at most 64 values"
    );
}

#[test]
fn groups_and_sections_are_not_empty() {
    assert_eq!(
        field("{ type = \"group\", fields = {}, columns = [], label = \"F\" }"),
        "tunnel.settings.f.fields: at least one"
    );
    assert_eq!(
        refusal(
            &kind("a = { type = \"bool\", label = \"A\" }", &["a"]).replace(
                "[tunnel.settings]",
                "[[tunnel.form]]\ntitle = \"Empty\"\nfields = []\n\n[tunnel.settings]"
            )
        ),
        "tunnel.form.fields: at least one"
    );
}
