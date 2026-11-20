//! Every test of the crate in one binary: each file beside this one is a
//! module of it. One binary for a crate links once and starts once.
mod base64;
mod catalog;
mod channel;
mod manifest;
mod names;
mod page;
mod refusals;
mod rows;
mod schema;
mod sha256;
mod toml_text;
mod values;

/// The manifest of `docs/TUNNELS.md`, every field written out.
pub const AWG: &str = include_str!("../fixtures/awg.toml");

/// The least a manifest says: who it is and its one process.
pub const MINIMAL: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"1.0\"\napi = 1\n\n[service]\ncommand = \"bin/demo\"\n";

/// `MINIMAL` with `more` after it.
pub fn minimal(more: &str) -> String {
    format!("{MINIMAL}{more}")
}

/// A manifest whose tunnel kind has `settings` (lines of
/// `[tunnel.settings]`), every field in one form section named in `form`.
/// A kind always has a required address: unless `settings` gives one, the
/// field `addr` is that, first in the form.
pub fn kind(settings: &str, form: &[&str]) -> String {
    let (settings, form): (String, Vec<&str>) = if settings.contains("role = \"address\"") {
        (settings.to_owned(), form.to_vec())
    } else {
        (format!("{ADDR}\n{settings}"), [&["addr"], form].concat())
    };
    let fields = form
        .iter()
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(", ");
    minimal(&format!(
        "\n[tunnel]\nlink_kind = \"demo\"\nmtu = {{ default = 1400, min = 1280, max = 1420 }}\n\n[[tunnel.form]]\ntitle = \"All\"\nfields = [{fields}]\n\n[tunnel.settings]\n{settings}\n"
    ))
}

/// The address `kind` adds, and a value of it.
pub const ADDR: &str =
    "addr = { type = \"ipv4_prefix\", role = \"address\", required = true, label = \"Address\" }";
pub const ADDR_VALUE: &str = "addr = \"10.0.0.2/32\"";

/// The refusal of `text` as `at: reason`.
pub fn refusal(text: &str) -> String {
    routeh_plugin_protocol::Manifest::parse(text)
        .unwrap_err()
        .to_string()
}
