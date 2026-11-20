//! A string as TOML writes it, the one writer of a basic string that the
//! canonical texts of the document and of a plugin's settings use.

/// `text` as a TOML basic string (TOML 1.0, String): a quote and a
/// backslash escaped, a newline, a tab and a carriage return by their
/// short escapes, and any
/// other control character, which a basic string may not hold as it is,
/// as `\uXXXX`. It reads back as `text`.
pub fn toml_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            '\r' => quoted.push_str("\\r"),
            control if control.is_control() => {
                quoted.push_str(&format!("\\u{:04X}", u32::from(control)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}
