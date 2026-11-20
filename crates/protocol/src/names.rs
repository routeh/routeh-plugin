//! The names the manifest and the channel use: a plugin's id, the key of
//! a setting or status value, and a display name. Copied with their rules
//! from the router's `crates/names` (only what this crate needs), so that
//! the contract builds on its own.
use std::{borrow::Borrow, fmt};

/// Why a text is no name of that type. The reason never repeats the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameError {
    reason: &'static str,
}
impl NameError {
    /// The rule the text broke, in words.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}
impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason)
    }
}
impl std::error::Error for NameError {}

fn refused<T>(reason: &'static str) -> Result<T, NameError> {
    Err(NameError { reason })
}

/// The most characters a display name has.
pub const DISPLAY_NAME_LIMIT: usize = 64;

const PLUGIN_RULE: &str =
    "a plugin's id is 1 to 16 lowercase letters, digits or underscores, starting with a letter";
const FIELD_RULE: &str =
    "a key is 1 to 32 lowercase letters, digits or underscores, starting with a letter";
const DISPLAY_RULE: &str = "a display name is 1 to 64 characters, not only whitespace, without control or invisible format characters";

/// Unicode's format characters, general category Cf (Unicode 16.0.0
/// `DerivedGeneralCategory.txt`): invisible, and some of them reorder or
/// join the text around them (bidirectional controls, zero-width joiners).
const FORMAT: [(char, char); 21] = [
    ('\u{ad}', '\u{ad}'),
    ('\u{600}', '\u{605}'),
    ('\u{61c}', '\u{61c}'),
    ('\u{6dd}', '\u{6dd}'),
    ('\u{70f}', '\u{70f}'),
    ('\u{890}', '\u{891}'),
    ('\u{8e2}', '\u{8e2}'),
    ('\u{180e}', '\u{180e}'),
    ('\u{200b}', '\u{200f}'),
    ('\u{202a}', '\u{202e}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206f}'),
    ('\u{feff}', '\u{feff}'),
    ('\u{fff9}', '\u{fffb}'),
    ('\u{110bd}', '\u{110bd}'),
    ('\u{110cd}', '\u{110cd}'),
    ('\u{13430}', '\u{1343f}'),
    ('\u{1bca0}', '\u{1bca3}'),
    ('\u{1d173}', '\u{1d17a}'),
    ('\u{e0001}', '\u{e0001}'),
    ('\u{e0020}', '\u{e007f}'),
];
/// Whether `character` is an invisible format character (Unicode's
/// `Cf`): text a person reads refuses them, since they change how other
/// text looks without being seen.
pub fn format_character(character: char) -> bool {
    FORMAT
        .iter()
        .any(|(first, last)| (*first..=*last).contains(&character))
}

/// Lowercase letters, digits or underscores, starting with a letter, at
/// most `limit` bytes.
fn plain(text: &str, limit: usize) -> bool {
    text.len() <= limit
        && text.starts_with(|first: char| first.is_ascii_lowercase())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// What every name has: its text out, shown, compared with text and looked
/// up by text in ordered sets and maps. Nothing goes in but `new`.
macro_rules! name {
    ($name:ident) => {
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.0 == *other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
    };
}

/// A plugin's id (`awg`): the table `[plugins.<id>]`, a tunnel's kind
/// `[tunnels.<key>.<id>]`, the plugin's directory and socket and its
/// program in the system log. 1 to 16 lowercase letters, digits or
/// underscores, starting with a letter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);
impl PluginId {
    pub fn new(text: impl Into<String>) -> Result<Self, NameError> {
        let text = text.into();
        if text.is_empty() || !plain(&text, 16) {
            return refused(PLUGIN_RULE);
        }
        Ok(Self(text))
    }
}
name!(PluginId);

/// The key of a plugin's setting (`listen_port`), status value or page:
/// a key of the document's table of a tunnel's kind, of the plugin's
/// `desired` and of a page's address. 1 to 32 lowercase letters, digits
/// or underscores, starting with a letter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldKey(String);
impl FieldKey {
    pub fn new(text: impl Into<String>) -> Result<Self, NameError> {
        let text = text.into();
        if text.is_empty() || !plain(&text, 32) {
            return refused(FIELD_RULE);
        }
        Ok(Self(text))
    }
}
name!(FieldKey);

/// Text for a person: trimmed, 1 to `DISPLAY_NAME_LIMIT` characters, one
/// line, without Unicode's invisible format characters (category Cf).
/// Nothing is made from it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayName(String);
impl DisplayName {
    pub fn new(text: impl Into<String>) -> Result<Self, NameError> {
        let text = text.into();
        let trimmed = text.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > DISPLAY_NAME_LIMIT
            || trimmed
                .chars()
                .any(|c| c.is_control() || format_character(c))
        {
            return refused(DISPLAY_RULE);
        }
        Ok(Self(trimmed.to_owned()))
    }
}
name!(DisplayName);
