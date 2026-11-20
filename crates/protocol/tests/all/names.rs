//! The names the manifest and the channel use: a plugin's id, a key of a
//! setting or status value, and a display name. Copied with their rules
//! from the router's `crates/names`, which has the same tests.
use routeh_plugin_protocol::names::{DISPLAY_NAME_LIMIT, DisplayName, FieldKey, PluginId};

#[test]
fn a_plugin_id_is_one_to_sixteen_plain_characters() {
    // It is a table of the document, a directory, a socket's name and a
    // program of the system log: lowercase letters, digits and `_` only.
    const RULE: &str =
        "a plugin's id is 1 to 16 lowercase letters, digits or underscores, starting with a letter";
    for text in ["awg", "adguard_home", "a", &"a".repeat(16)] {
        assert_eq!(PluginId::new(text).unwrap().as_str(), text);
    }
    for text in [
        &"a".repeat(17),
        "",
        "Awg",
        "2awg",
        "_awg",
        "a-wg",
        "a.wg",
        "a wg",
        "аwg",
    ] {
        assert_eq!(PluginId::new(text).unwrap_err().reason(), RULE, "{text:?}");
        assert_eq!(PluginId::new(text).unwrap_err().to_string(), RULE);
    }
}

#[test]
fn a_field_key_is_one_to_thirty_two_plain_characters() {
    // The key of a plugin's setting, status value or page.
    const RULE: &str =
        "a key is 1 to 32 lowercase letters, digits or underscores, starting with a letter";
    for text in ["jc", "listen_port", "h1", &"a".repeat(32)] {
        assert_eq!(FieldKey::new(text).unwrap().as_str(), text);
    }
    for text in [&"a".repeat(33), "", "Jc", "1h", "_a", "a-b", "a.b"] {
        assert_eq!(FieldKey::new(text).unwrap_err().reason(), RULE, "{text:?}");
    }
}

#[test]
fn names_display_compare_and_order_by_their_text() {
    let key = FieldKey::new("jc").unwrap();
    assert_eq!(key.to_string(), "jc");
    assert_eq!(key, "jc");
    assert_eq!(key, *"jc");
    assert_eq!(key, "jc".to_owned());
    let mut ids = [
        PluginId::new("awg2").unwrap(),
        PluginId::new("awg").unwrap(),
    ];
    ids.sort();
    assert_eq!(ids[0].as_str(), "awg");
    assert_eq!(ids[0].to_string(), "awg");
    let set: std::collections::BTreeSet<PluginId> = ids.into();
    assert!(set.contains("awg2"));
}

#[test]
fn a_display_name_is_trimmed_text_up_to_its_limit() {
    let name = DisplayName::new("  Living room TV ").unwrap();
    assert_eq!(name.as_str(), "Living room TV");
    assert_eq!(name.to_string(), "Living room TV");
    assert_eq!(DisplayName::new("Кухня").unwrap().as_str(), "Кухня");
    assert_eq!(DISPLAY_NAME_LIMIT, 64);
    // Characters, not bytes.
    assert!(DisplayName::new("я".repeat(64)).is_ok());
    assert!(DisplayName::new(format!(" {} ", "a".repeat(64))).is_ok());
    for text in [
        String::new(),
        "   ".to_owned(),
        "a".repeat(65),
        "a\nb".to_owned(),
        "a\u{0}".to_owned(),
    ] {
        assert!(DisplayName::new(text.clone()).is_err(), "{text:?}");
    }
}

/// Unicode's format characters (general category Cf) are invisible and
/// change how text around them reads: bidirectional controls can make a
/// name look like another, zero-width ones make two names that look alike
/// differ. A display name has none.
#[test]
fn a_display_name_has_no_format_characters() {
    for character in [
        '\u{ad}',
        '\u{600}',
        '\u{61c}',
        '\u{180e}',
        '\u{200b}',
        '\u{200d}',
        '\u{200e}',
        '\u{202e}',
        '\u{2060}',
        '\u{2066}',
        '\u{2069}',
        '\u{feff}',
        '\u{fff9}',
        '\u{110bd}',
        '\u{e0001}',
        '\u{e007f}',
    ] {
        let text = format!("Kitchen{character}TV");
        assert!(DisplayName::new(text.clone()).is_err(), "{character:?}");
        assert!(routeh_plugin_protocol::names::format_character(character));
    }
    // Letters and symbols around them are not format characters.
    for text in ["Café", "Кухня", "台所", "TV 📺", "a–b"] {
        assert!(DisplayName::new(text).is_ok(), "{text}");
    }
    assert!(!routeh_plugin_protocol::names::format_character('a'));
}
