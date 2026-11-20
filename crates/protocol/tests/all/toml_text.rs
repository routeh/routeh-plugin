//! A string as TOML writes it: one basic string that reads back as it was.
use routeh_plugin_protocol::toml_string;

#[test]
fn a_string_is_a_basic_string_that_reads_back_as_it_was() {
    assert_eq!(toml_string("plain"), "\"plain\"");
    assert_eq!(toml_string("a \"b\" \\ c"), "\"a \\\"b\\\" \\\\ c\"");
    assert_eq!(toml_string("one\ntwo\tthree"), "\"one\\ntwo\\tthree\"");
    assert_eq!(
        toml_string("bell\u{7}del\u{7f}cr\r"),
        "\"bell\\u0007del\\u007Fcr\\r\""
    );
    for text in ["", "ünïcode ☃", "\u{0}\u{1f}\u{85}", "a\r\nb", "\"\\\""] {
        let line = format!("value = {}", toml_string(text));
        let table: toml::Table = line.parse().unwrap();
        assert_eq!(table["value"].as_str(), Some(text), "{line}");
    }
}
