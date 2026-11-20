//! A plugin's settings: each field's type with that type's attributes, its
//! role, the form's order and the two constraints between fields; and the
//! check of a section of the document against them.
use crate::names::FieldKey;
use crate::read::{Section, distinct};
use crate::value::{self, DURATION_RULE, HOST_PORT_RULE, RANGE_RULE, Settings, Value};
use crate::{ManifestError, base64};
use regex::{Regex, RegexBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

/// What one field of the settings is.
#[derive(Clone, Debug)]
pub struct Field {
    pub label: String,
    pub required: bool,
    pub role: Option<Role>,
    /// A password field with reveal; the value is never listed or logged.
    pub secret: bool,
    pub kind: FieldType,
}

/// The closed set of types (docs/TUNNELS.md, Types).
#[derive(Clone, Debug)]
pub enum FieldType {
    Text {
        max_len: usize,
        pattern: Option<Regex>,
    },
    /// Text of several lines whose form is the service's.
    Template {
        max_len: usize,
        pattern: Option<Regex>,
    },
    Bytes {
        len: usize,
    },
    Bool,
    Integer {
        width: Width,
        min: u32,
        max: u32,
    },
    Range {
        of: Width,
        min: u32,
        max: u32,
    },
    /// Bounds in seconds.
    Duration {
        min: u32,
        max: u32,
    },
    Port,
    HostPort,
    Ipv4Prefix,
    Ipv6Prefix,
    Address,
    Choice {
        values: Vec<String>,
    },
    /// The key of a section of the router's document: which sections
    /// there are is the router's to check.
    Reference(Reference),
    List {
        item: Box<FieldType>,
        max_items: usize,
    },
    /// Rows of fields; its schema's order is the columns'.
    Group {
        fields: Schema,
        max_rows: usize,
        /// The field that names a row, unique among the rows ignoring
        /// ASCII case: how `state` and actions name a row.
        key: Option<FieldKey>,
    },
}

/// What kind of section of the router's document a reference names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reference {
    Uplink,
    Network,
    Tunnel,
    Client,
}

impl Reference {
    /// Its type's name in a manifest.
    pub fn word(self) -> &'static str {
        match self {
            Reference::Uplink => "uplink",
            Reference::Network => "network",
            Reference::Tunnel => "tunnel",
            Reference::Client => "client",
        }
    }
}

/// The width of an integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    U8,
    U16,
    U32,
}

impl Width {
    fn max(self) -> u32 {
        match self {
            Width::U8 => u8::MAX.into(),
            Width::U16 => u16::MAX.into(),
            Width::U32 => u32::MAX,
        }
    }
}

/// What a field means to the router (docs/TUNNELS.md, Roles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The interface's address, set by netifd.
    Address,
    /// A port opened in the firewall.
    Listen(Protocol),
    /// Where the tunnel goes, shown.
    Endpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Udp,
    Tcp,
}

/// The fields of one section in the order a form shows them, and the
/// constraints between them.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    order: Vec<FieldKey>,
    fields: BTreeMap<FieldKey, Field>,
    exactly_one: Vec<Vec<FieldKey>>,
    requires: Vec<(FieldKey, FieldKey)>,
}

/// A value of the document that its schema refuses, at its path within
/// the section (`jc`, `peers.0.endpoint`).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{at}: {reason}")]
pub struct SettingError {
    pub at: String,
    pub reason: String,
}

const TYPES: &str = "text, bytes, template, bool, u8, u16, u32, range, duration, port, host_port, ipv4_prefix, ipv6_prefix, address, choice, uplink, network, tunnel, client, list, group";
const LIST_ITEMS: &str = "a list holds text, bytes, u8, u16, u32, range, duration, port, host_port, ipv4_prefix, ipv6_prefix, address, choice, uplink, network, tunnel or client";
/// The syntax of a reference: every key of a section the router's
/// document has (an uplink's, a network's, a tunnel's, a client's) is of
/// this form; which keys there are is the router's to check.
const REFERENCE_RULE: &str =
    "a key of the router's document: 1 to 32 lowercase letters, digits or underscores";
const ROLES_OWN: &str = "a role is given only to a field of the tunnel's own";
/// The largest compiled pattern: `regex`'s own bound, so that a short
/// pattern cannot grow without end.
const PATTERN_SIZE: usize = 64 * 1024;
const CHOICE_RULE: &str = "1 to 64 letters, digits, '.', '_' or '-'";
/// The most fields of a section, and of a group.
pub(crate) const FIELDS: usize = 256;
const GROUP_FIELDS: usize = 32;

/// Where a field is declared: whether it may have a role and what it may
/// hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// `[tunnel.settings]`.
    Tunnel,
    /// `[settings]` of the plugin.
    Plain,
    /// A field of a group.
    Cell,
    /// The item of a list: a type and its attributes alone.
    Item,
}

impl Schema {
    /// The fields' keys in the form's order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(FieldKey::as_str)
    }

    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.get(key)
    }

    /// The field of role `address` of IPv4 (`v4`) or IPv6.
    pub fn address(&self, v4: bool) -> Option<&str> {
        self.with_role(|field| {
            field.role == Some(Role::Address) && matches!(field.kind, FieldType::Ipv4Prefix) == v4
        })
    }

    /// The field of role `listen` and its protocol.
    pub fn listen(&self) -> Option<(&str, Protocol)> {
        self.order
            .iter()
            .find_map(|key| match self.fields[key].role {
                Some(Role::Listen(protocol)) => Some((key.as_str(), protocol)),
                _ => None,
            })
    }

    /// The field of role `endpoint`.
    pub fn endpoint(&self) -> Option<&str> {
        self.with_role(|field| field.role == Some(Role::Endpoint))
    }

    fn with_role(&self, wanted: impl Fn(&Field) -> bool) -> Option<&str> {
        self.order
            .iter()
            .find(|key| wanted(&self.fields[*key]))
            .map(FieldKey::as_str)
    }

    /// The keys of the rows of the group `group` in `settings`, in their
    /// order: the values of its key field. Empty when it is no group with
    /// a key, or has no rows.
    pub fn row_keys(&self, settings: &Settings, group: &str) -> Vec<String> {
        let Some(Field {
            kind: FieldType::Group { key: Some(key), .. },
            ..
        }) = self.fields.get(group)
        else {
            return Vec::new();
        };
        let Some(Value::Group(rows)) = settings.get(group) else {
            return Vec::new();
        };
        rows.iter()
            .filter_map(|row| row.get(key.as_str()).and_then(row_key))
            .collect()
    }

    /// The settings as the document's text: one line for each field that
    /// is set, in the form's order, each value in its canonical form.
    pub fn write(&self, settings: &Settings) -> String {
        self.order
            .iter()
            .filter_map(|key| {
                let value = settings.get(key.as_str())?;
                Some(format!(
                    "{key} = {}\n",
                    literal(&self.fields[key].kind, value)
                ))
            })
            .collect()
    }

    /// Reads `[tunnel.settings]` (`tunnel`) or `[settings]`; the order is
    /// set by the form.
    pub(crate) fn read(section: Option<Section<'_>>, tunnel: bool) -> Result<Self, ManifestError> {
        let place = if tunnel { Place::Tunnel } else { Place::Plain };
        let Some(mut section) = section else {
            return Ok(Self::default());
        };
        let entries = section.entries();
        if entries.len() > FIELDS {
            return section.refuse_here(format!("at most {FIELDS} fields"));
        }
        let mut fields = BTreeMap::new();
        for (key, spec) in entries {
            let key = field_key(&section, key, key)?;
            let field = read_field(&section, key.as_str(), spec, place)?;
            fields.insert(key, field);
        }
        let schema = Self {
            order: fields.keys().cloned().collect(),
            fields,
            ..Self::default()
        };
        schema.one_of_each_role(&section)?;
        if tunnel {
            let addresses: Vec<&Field> = schema
                .fields
                .values()
                .filter(|field| field.role == Some(Role::Address))
                .collect();
            if addresses.is_empty() {
                return section.refuse_here("no field of role address");
            }
            if !addresses.iter().any(|field| field.required) {
                return section.refuse_here("no field of role address is required");
            }
        }
        Ok(schema)
    }

    fn one_of_each_role(&self, section: &Section<'_>) -> Result<(), ManifestError> {
        let mut seen: Vec<(&str, &FieldKey)> = Vec::new();
        for (key, field) in &self.fields {
            let Some(role) = field.role else { continue };
            let slot = match (role, &field.kind) {
                (Role::Address, FieldType::Ipv4Prefix) => "address4",
                (Role::Address, _) => "address6",
                (Role::Listen(_), _) => "listen",
                (Role::Endpoint, _) => "endpoint",
            };
            if let Some((_, first)) = seen.iter().find(|(taken, _)| *taken == slot) {
                let name = match role {
                    Role::Address => "address",
                    Role::Listen(_) => "listen",
                    Role::Endpoint => "endpoint",
                };
                return section.refuse(
                    &format!("{key}.role"),
                    format!("{first} has role {name} already"),
                );
            }
            seen.push((slot, key));
        }
        Ok(())
    }

    /// Puts the fields in the order of the form's sections, each field in
    /// exactly one; `at` is the form's path.
    pub(crate) fn order_by(
        &mut self,
        at: &str,
        sections: &[Vec<FieldKey>],
    ) -> Result<(), ManifestError> {
        let mut order: Vec<FieldKey> = Vec::new();
        let mut placed = BTreeSet::new();
        for key in sections.iter().flatten() {
            if !self.fields.contains_key(key) {
                return Err(ManifestError::new(at, format!("{key} is not a setting")));
            }
            if !placed.insert(key) {
                return Err(ManifestError::new(at, format!("{key} is in two sections")));
            }
            order.push(key.clone());
        }
        if let Some(missing) = self.fields.keys().find(|key| !placed.contains(key)) {
            return Err(ManifestError::new(
                at,
                format!("{missing} is in no section"),
            ));
        }
        self.order = order;
        Ok(())
    }

    /// Reads `[tunnel.constraints]`.
    pub(crate) fn constrain(&mut self, mut section: Section<'_>) -> Result<(), ManifestError> {
        const GROUPS: &str = "must be an array of arrays of field keys";
        let groups: Vec<Vec<&str>> = match section.get("exactly_one") {
            None => Vec::new(),
            Some(toml::Value::Array(groups)) => match groups
                .iter()
                .map(|group| group.as_array()?.iter().map(toml::Value::as_str).collect())
                .collect::<Option<_>>()
            {
                Some(groups) => groups,
                None => return section.refuse("exactly_one", GROUPS),
            },
            Some(_) => return section.refuse("exactly_one", GROUPS),
        };
        for keys in &groups {
            if keys.len() < 2 {
                return section.refuse("exactly_one", "a group of 2 fields at least");
            }
            let mut fields = Vec::new();
            for key in keys {
                let (key, field) = self.known(&section, "exactly_one", key)?;
                if field.required {
                    return section.refuse("exactly_one", format!("{key} is required"));
                }
                fields.push(key);
            }
            if let Err(twice) = distinct(keys.iter().copied()) {
                return section.refuse("exactly_one", format!("{twice} is named twice"));
            }
            self.exactly_one.push(fields);
        }
        if let Some(mut requires) = section.table("requires")? {
            for (key, other) in requires.entries() {
                let at = format!("requires.{key}");
                let toml::Value::String(other) = other else {
                    return section.refuse(&at, "must be a field key");
                };
                let (key, _) = self.known(&section, &at, key)?;
                let (other, _) = self.known(&section, &at, other)?;
                if key == other {
                    return section.refuse(&at, format!("{key} requires itself"));
                }
                self.requires.push((key, other));
            }
        }
        section.finish()
    }

    fn known(
        &self,
        section: &Section<'_>,
        at: &str,
        key: &str,
    ) -> Result<(FieldKey, &Field), ManifestError> {
        match self.fields.get_key_value(key) {
            Some((key, field)) => Ok((key.clone(), field)),
            None => section.refuse(at, format!("{key} is not a setting")),
        }
    }

    /// Checks a section of the document: every key is a field, every
    /// required field is there, every value is of its type and within its
    /// bounds, and the constraints hold.
    pub fn check(&self, table: &toml::Table) -> Result<Settings, SettingError> {
        self.check_at("", table)
    }

    fn check_at(&self, at: &str, table: &toml::Table) -> Result<Settings, SettingError> {
        let path = |key: &str| {
            if at.is_empty() {
                key.to_owned()
            } else {
                format!("{at}.{key}")
            }
        };
        if let Some(unknown) = table
            .keys()
            .find(|key| !self.fields.contains_key(key.as_str()))
        {
            return Err(SettingError {
                at: path(unknown),
                reason: "unknown setting".into(),
            });
        }
        let mut settings = BTreeMap::new();
        for key in &self.order {
            let field = &self.fields[key];
            match table.get(key.as_str()) {
                None if field.required => {
                    return Err(SettingError {
                        at: path(key.as_str()),
                        reason: "required".into(),
                    });
                }
                None => {}
                Some(raw) => {
                    let value = check_value(&path(key.as_str()), &field.kind, raw)?;
                    settings.insert(key.clone(), value);
                }
            }
        }
        let label = |key: &FieldKey| self.fields[key].label.as_str();
        for group in &self.exactly_one {
            let set: Vec<&FieldKey> = group
                .iter()
                .filter(|key| settings.contains_key(key.as_str()))
                .collect();
            let names = spoken(group.iter().map(label));
            match set.as_slice() {
                [] => {
                    return Err(SettingError {
                        at: path(group[0].as_str()),
                        reason: format!("one of {names}"),
                    });
                }
                [_] => {}
                [_, second, ..] => {
                    return Err(SettingError {
                        at: path(second.as_str()),
                        reason: format!("only one of {names}"),
                    });
                }
            }
        }
        for (key, other) in &self.requires {
            if settings.contains_key(key.as_str()) && !settings.contains_key(other.as_str()) {
                return Err(SettingError {
                    at: path(key.as_str()),
                    reason: format!("{} needs {}", label(key), label(other)),
                });
            }
        }
        Ok(Settings(settings))
    }
}

/// `A`, `A and B`, `A, B and C`.
fn spoken<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.collect();
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

fn read_field(
    parent: &Section<'_>,
    key: &str,
    spec: &toml::Value,
    place: Place,
) -> Result<Field, ManifestError> {
    let toml::Value::Table(table) = spec else {
        return parent.refuse(key, "must be a table");
    };
    let mut section = Section::new(parent.path(key), table);
    let kind = read_type(&mut section, place)?;
    let field = if place == Place::Item {
        Field {
            label: String::new(),
            required: false,
            role: None,
            secret: false,
            kind,
        }
    } else {
        let secret = section.boolean("secret")?.unwrap_or(false);
        if secret
            && !matches!(
                kind,
                FieldType::Text { .. } | FieldType::Bytes { .. } | FieldType::Template { .. }
            )
        {
            return section.refuse("secret", "only text, bytes and template are secret");
        }
        let required = section.boolean("required")?.unwrap_or(false);
        let role = read_role(&mut section, &kind, place)?;
        let label = section.required("label", Section::string)?;
        let label = display(&section, "label", label)?;
        Field {
            label,
            required,
            role,
            secret,
            kind,
        }
    };
    section.finish()?;
    Ok(field)
}

fn read_role(
    section: &mut Section<'_>,
    kind: &FieldType,
    place: Place,
) -> Result<Option<Role>, ManifestError> {
    let role = section.string("role")?;
    if role.is_some() && place != Place::Tunnel {
        return section.refuse("role", ROLES_OWN);
    }
    let role = match role {
        None => None,
        Some("address") => match kind {
            FieldType::Ipv4Prefix | FieldType::Ipv6Prefix => Some(Role::Address),
            _ => return section.refuse("role", "an address is an ipv4_prefix or ipv6_prefix"),
        },
        Some("listen") => match kind {
            FieldType::Port => Some(Role::Listen(Protocol::Udp)),
            _ => return section.refuse("role", "listen is a port"),
        },
        Some("endpoint") => match kind {
            FieldType::HostPort => Some(Role::Endpoint),
            _ => return section.refuse("role", "an endpoint is a host_port"),
        },
        Some(other) => {
            return section.refuse(
                "role",
                format!("\"{other}\" is not one of address, listen, endpoint"),
            );
        }
    };
    let protocol = section.string("protocol")?;
    match (role, protocol) {
        (Some(Role::Listen(_)), None) => section.refuse("protocol", "required"),
        (Some(Role::Listen(_)), Some("udp")) => Ok(Some(Role::Listen(Protocol::Udp))),
        (Some(Role::Listen(_)), Some("tcp")) => Ok(Some(Role::Listen(Protocol::Tcp))),
        (Some(Role::Listen(_)), Some(other)) => {
            section.refuse("protocol", format!("\"{other}\" is not one of udp, tcp"))
        }
        (_, Some(_)) => section.refuse("protocol", "only with role listen"),
        (role, None) => Ok(role),
    }
}

fn read_type(section: &mut Section<'_>, place: Place) -> Result<FieldType, ManifestError> {
    let name = section.required("type", Section::string)?;
    let kind = match name {
        "text" => FieldType::Text {
            max_len: max_len(section, 255, 4096)?,
            pattern: pattern(section)?,
        },
        "template" => FieldType::Template {
            max_len: max_len(section, 4096, 65536)?,
            pattern: pattern(section)?,
        },
        "bytes" => FieldType::Bytes {
            len: section.required("len", |s, k| s.bounded(k, 1, 1024))? as usize,
        },
        "bool" => FieldType::Bool,
        "u8" | "u16" | "u32" => {
            let width = width(name);
            let (min, max) = bounds(section, width)?;
            FieldType::Integer { width, min, max }
        }
        "range" => {
            let of = match section.required("of", Section::string)? {
                "u16" => Width::U16,
                "u32" => Width::U32,
                other => {
                    return section.refuse("of", format!("\"{other}\" is not one of u16, u32"));
                }
            };
            let (min, max) = bounds(section, of)?;
            FieldType::Range { of, min, max }
        }
        "duration" => {
            let mut limit = |key| match section.string(key)? {
                None => Ok(None),
                Some(text) => value::duration(text)
                    .map(Some)
                    .map_or_else(|| section.refuse(key, DURATION_RULE), Ok),
            };
            let min = limit("min")?.unwrap_or(0);
            let max = limit("max")?.unwrap_or(u32::MAX);
            if min > max {
                return section.refuse_here("min ≤ max");
            }
            FieldType::Duration { min, max }
        }
        "port" => FieldType::Port,
        "host_port" => FieldType::HostPort,
        "ipv4_prefix" => FieldType::Ipv4Prefix,
        "ipv6_prefix" => FieldType::Ipv6Prefix,
        "address" => FieldType::Address,
        "choice" => {
            let values = section.required("values", Section::strings)?;
            if !(1..=64).contains(&values.len()) {
                return section.refuse("values", "1 to 64 values");
            }
            if let Some(bad) = values.iter().find(|value| !token(value, 64)) {
                return section.refuse("values", format!("\"{bad}\" is not {CHOICE_RULE}"));
            }
            if let Err(twice) = distinct(values.iter().copied()) {
                return section.refuse("values", format!("\"{twice}\" is named twice"));
            }
            FieldType::Choice {
                values: values.into_iter().map(str::to_owned).collect(),
            }
        }
        "list" if place != Place::Item => {
            let Some(item) = section.get("item") else {
                return section.refuse("item", "required");
            };
            let item = read_field(section, "item", item, Place::Item)?.kind;
            if matches!(item, FieldType::Bool | FieldType::Template { .. }) {
                return section.refuse("item.type", LIST_ITEMS);
            }
            let max_items = section.bounded("max_items", 1, 256)?.unwrap_or(64) as usize;
            FieldType::List {
                item: Box::new(item),
                max_items,
            }
        }
        "uplink" => FieldType::Reference(Reference::Uplink),
        "network" => FieldType::Reference(Reference::Network),
        "tunnel" => FieldType::Reference(Reference::Tunnel),
        "client" => FieldType::Reference(Reference::Client),
        "group" if matches!(place, Place::Tunnel | Place::Plain) => {
            let Some(mut fields) = section.table("fields")? else {
                return section.refuse("fields", "required");
            };
            let entries = fields.entries();
            if entries.is_empty() {
                return section.refuse("fields", "at least one");
            }
            if entries.len() > GROUP_FIELDS {
                return section.refuse("fields", format!("at most {GROUP_FIELDS} fields"));
            }
            let mut schema = Schema::default();
            for (key, spec) in entries {
                let key = field_key(&fields, key, key)?;
                schema.fields.insert(
                    key.clone(),
                    read_field(&fields, key.as_str(), spec, Place::Cell)?,
                );
            }
            let columns = section.required("columns", Section::strings)?;
            let mut order = Vec::new();
            for column in &columns {
                let Some((key, _)) = schema.fields.get_key_value(*column) else {
                    return section.refuse(
                        "columns",
                        format!("\"{column}\" is not a field of the group"),
                    );
                };
                if order.contains(key) {
                    return section.refuse("columns", format!("{column} is named twice"));
                }
                order.push(key.clone());
            }
            if let Some(missing) = schema.fields.keys().find(|key| !order.contains(key)) {
                return section.refuse("columns", format!("{missing} is in no column"));
            }
            schema.order = order;
            let max_rows = section.bounded("max_rows", 1, 256)?.unwrap_or(16) as usize;
            let key = match section.string("key")? {
                None => None,
                Some(key) => Some(group_key(section, &schema, key)?),
            };
            FieldType::Group {
                fields: schema,
                max_rows,
                key,
            }
        }
        "list" => return section.refuse("type", LIST_ITEMS),
        "group" if place == Place::Item => return section.refuse("type", LIST_ITEMS),
        "group" => return section.refuse("type", "a group holds no group"),
        other => return section.refuse("type", format!("\"{other}\" is not one of {TYPES}")),
    };
    Ok(kind)
}

fn width(name: &str) -> Width {
    match name {
        "u8" => Width::U8,
        "u16" => Width::U16,
        _ => Width::U32,
    }
}

fn bounds(section: &mut Section<'_>, width: Width) -> Result<(u32, u32), ManifestError> {
    let top = i64::from(width.max());
    let min = section.bounded("min", 0, top)?.unwrap_or(0) as u32;
    let max = section.bounded("max", 0, top)?.unwrap_or(top) as u32;
    if min > max {
        return section.refuse_here("min ≤ max");
    }
    Ok((min, max))
}

fn max_len(section: &mut Section<'_>, default: i64, top: i64) -> Result<usize, ManifestError> {
    Ok(section.bounded("max_len", 1, top)?.unwrap_or(default) as usize)
}

fn pattern(section: &mut Section<'_>) -> Result<Option<Regex>, ManifestError> {
    let Some(text) = section.string("pattern")? else {
        return Ok(None);
    };
    // The pattern is compiled alone first: one that is not a whole
    // expression (`x)|(.*`) would otherwise close the group around it and
    // leave a side of its alternation unanchored.
    let build = |text: &str| RegexBuilder::new(text).size_limit(PATTERN_SIZE).build();
    match build(text).and_then(|_| build(&format!("^(?:{text})$"))) {
        Ok(pattern) => Ok(Some(pattern)),
        Err(_) => section.refuse("pattern", "not a regular expression"),
    }
}

/// The key of a group: one of its fields, required, of text or a
/// reference.
fn group_key(section: &Section<'_>, schema: &Schema, key: &str) -> Result<FieldKey, ManifestError> {
    let Some((key, field)) = schema.fields.get_key_value(key) else {
        return section.refuse("key", format!("\"{key}\" is not a field of the group"));
    };
    if !matches!(field.kind, FieldType::Text { .. } | FieldType::Reference(_)) {
        return section.refuse("key", "a group's key is text or a reference");
    }
    if !field.required {
        return section.refuse("key", format!("{key} is not required"));
    }
    Ok(key.clone())
}

/// The text a row is named by: the value of its group's key field.
fn row_key(value: &Value) -> Option<String> {
    match value {
        Value::Text(text) | Value::Reference(_, text) => Some(text.clone()),
        _ => None,
    }
}

/// A key of the manifest as a field's key, refused at `at`.
pub(crate) fn field_key(
    section: &Section<'_>,
    at: &str,
    key: &str,
) -> Result<FieldKey, ManifestError> {
    FieldKey::new(key).or_else(|error| section.refuse(at, error.reason()))
}

/// Text a person reads: a display name's rule.
pub(crate) fn display(
    section: &Section<'_>,
    key: &str,
    text: &str,
) -> Result<String, ManifestError> {
    match crate::names::DisplayName::new(text) {
        Ok(name) => Ok(name.as_str().to_owned()),
        Err(error) => section.refuse(key, error.reason()),
    }
}

/// 1 to `limit` letters, digits, `.`, `_` or `-`.
pub(crate) fn token(text: &str, limit: usize) -> bool {
    (1..=limit).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn check_value(at: &str, kind: &FieldType, raw: &toml::Value) -> Result<Value, SettingError> {
    let refuse = |reason: &str| {
        Err(SettingError {
            at: at.to_owned(),
            reason: reason.to_owned(),
        })
    };
    let string = || {
        raw.as_str().ok_or_else(|| SettingError {
            at: at.to_owned(),
            reason: "must be a string".into(),
        })
    };
    let integer = || {
        raw.as_integer().ok_or_else(|| SettingError {
            at: at.to_owned(),
            reason: "must be an integer".into(),
        })
    };
    match kind {
        FieldType::Text { max_len, pattern } | FieldType::Template { max_len, pattern } => {
            let text = string()?;
            let template = matches!(kind, FieldType::Template { .. });
            let hidden = |c: char| c.is_control() || crate::names::format_character(c);
            if template && text.chars().any(|c| hidden(c) && c != '\t' && c != '\n') {
                return refuse("no control or invisible format characters but tab and newline");
            }
            if !template && text.chars().any(hidden) {
                return refuse("no control or invisible format characters");
            }
            if text.chars().count() > *max_len {
                return refuse(&format!("at most {max_len} characters"));
            }
            if pattern
                .as_ref()
                .is_some_and(|pattern| !pattern.is_match(text))
            {
                return refuse("not of the form the plugin asks for");
            }
            Ok(Value::Text(text.to_owned()))
        }
        FieldType::Bytes { len } => match base64::decode(string()?) {
            Some(bytes) if bytes.len() == *len => Ok(Value::Bytes(bytes)),
            _ => refuse(&format!("base64 of {len} bytes")),
        },
        FieldType::Bool => raw.as_bool().map(Value::Bool).ok_or_else(|| SettingError {
            at: at.to_owned(),
            reason: "must be a boolean".into(),
        }),
        FieldType::Integer { min, max, .. } => match u32::try_from(integer()?) {
            Ok(number) if (*min..=*max).contains(&number) => Ok(Value::Integer(number)),
            _ => refuse(&format!("{min} to {max}")),
        },
        FieldType::Range { min, max, .. } => {
            let Some((lo, hi)) = value::range(string()?) else {
                return refuse(RANGE_RULE);
            };
            let within = |n: u64| (u64::from(*min)..=u64::from(*max)).contains(&n);
            if !within(lo) || !within(hi) {
                return refuse(&format!("{min} to {max}"));
            }
            if lo > hi {
                return refuse("the first number is above the second");
            }
            Ok(Value::Range {
                lo: lo as u32,
                hi: hi as u32,
            })
        }
        FieldType::Duration { min, max } => match value::duration(string()?) {
            None => refuse(DURATION_RULE),
            Some(seconds) if (*min..=*max).contains(&seconds) => Ok(Value::Duration(seconds)),
            Some(_) => refuse(&format!(
                "{} to {}",
                value::duration_text(*min),
                value::duration_text(*max)
            )),
        },
        FieldType::Port => match u16::try_from(integer()?) {
            Ok(port) if port > 0 => Ok(Value::Port(port)),
            _ => refuse("1 to 65535"),
        },
        FieldType::HostPort => match value::host_port(string()?) {
            Some((host, port)) => Ok(Value::HostPort { host, port }),
            None => refuse(HOST_PORT_RULE),
        },
        FieldType::Ipv4Prefix | FieldType::Ipv6Prefix => {
            let v4 = matches!(kind, FieldType::Ipv4Prefix);
            match value::prefix(string()?, v4) {
                Some((address, length)) => Ok(Value::Prefix(address, length)),
                None if v4 => refuse("an IPv4 address with its prefix length (\"10.8.0.2/32\")"),
                None => refuse("an IPv6 address with its prefix length (\"fd00::2/64\")"),
            }
        }
        FieldType::Address => match string()?.parse::<IpAddr>() {
            Ok(address) => Ok(Value::Address(address)),
            Err(_) => refuse("an IPv4 or IPv6 address"),
        },
        FieldType::Choice { values } => {
            let text = string()?;
            if values.iter().any(|value| value == text) {
                Ok(Value::Choice(text.to_owned()))
            } else {
                refuse(&format!("one of {}", values.join(", ")))
            }
        }
        FieldType::Reference(kind) => {
            let text = string()?;
            let valid = (1..=32).contains(&text.len())
                && text
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if valid {
                Ok(Value::Reference(*kind, text.to_owned()))
            } else {
                refuse(REFERENCE_RULE)
            }
        }
        FieldType::List { item, max_items } => {
            let Some(items) = raw.as_array() else {
                return refuse("must be an array");
            };
            if items.len() > *max_items {
                return refuse(&format!("at most {max_items}"));
            }
            items
                .iter()
                .enumerate()
                .map(|(index, raw)| check_value(&format!("{at}.{index}"), item, raw))
                .collect::<Result<_, _>>()
                .map(Value::List)
        }
        FieldType::Group {
            fields,
            max_rows,
            key,
        } => {
            let Some(rows) = raw.as_array() else {
                return refuse("must be an array");
            };
            if rows.len() > *max_rows {
                return refuse(&format!("at most {max_rows}"));
            }
            let rows: Vec<Settings> = rows
                .iter()
                .enumerate()
                .map(|(index, row)| match row {
                    toml::Value::Table(row) => fields.check_at(&format!("{at}.{index}"), row),
                    _ => Err(SettingError {
                        at: format!("{at}.{index}"),
                        reason: "must be a table".into(),
                    }),
                })
                .collect::<Result<_, _>>()?;
            if let Some(key) = key {
                let mut seen = BTreeSet::new();
                for (index, row) in rows.iter().enumerate() {
                    let Some(name) = row.get(key.as_str()).and_then(row_key) else {
                        continue;
                    };
                    if !seen.insert(name.to_ascii_lowercase()) {
                        return Err(SettingError {
                            at: format!("{at}.{index}.{key}"),
                            reason: format!("{name} is named twice"),
                        });
                    }
                }
            }
            Ok(Value::Group(rows))
        }
    }
}

/// A value as a TOML literal: strings quoted with `\\`, `"`, newline and
/// tab escaped (no other control character passes a check), a group's
/// rows as inline tables in the order of its columns.
fn literal(kind: &FieldType, value: &Value) -> String {
    match (kind, value) {
        (FieldType::List { item, .. }, Value::List(items)) => {
            let items: Vec<String> = items.iter().map(|value| literal(item, value)).collect();
            format!("[{}]", items.join(", "))
        }
        (FieldType::Group { fields, .. }, Value::Group(rows)) => {
            let rows: Vec<String> = rows
                .iter()
                .map(|row| {
                    let cells: Vec<String> = fields
                        .order
                        .iter()
                        .filter_map(|key| {
                            let value = row.get(key.as_str())?;
                            Some(format!(
                                "{key} = {}",
                                literal(&fields.fields[key].kind, value)
                            ))
                        })
                        .collect();
                    format!("{{ {} }}", cells.join(", "))
                })
                .collect();
            format!("[{}]", rows.join(", "))
        }
        (_, Value::Integer(number)) => number.to_string(),
        (_, Value::Port(port)) => port.to_string(),
        (_, Value::Bool(flag)) => flag.to_string(),
        (_, value) => crate::toml_string(value.to_toml().as_str().unwrap_or_default()),
    }
}
