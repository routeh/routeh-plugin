//! A plugin's `manifest.toml`, read as `install` checks it
//! (docs/PLUGINS.md, The bundle and the manifest; docs/TUNNELS.md, The
//! manifest).
use crate::names::{DisplayName, FieldKey, PluginId};
use crate::read::{Section, distinct};
use crate::schema::FieldType;
use crate::schema::{Schema, display, field_key, token};
use crate::value::{self, DURATION_RULE};
use crate::{API, ManifestError, TUNNEL_KEYS};
use std::time::Duration;

/// Everything a manifest declares.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub id: PluginId,
    pub name: DisplayName,
    pub version: String,
    /// The version of the manifest's schema it is written for ([`API`]).
    pub api: u64,
    /// Firmware packages, by name.
    pub requires: Vec<String>,
    /// Kernel modules to load, in order.
    pub modules: Vec<String>,
    /// The version of the `kernel` package (as apk reports it, the hash
    /// of the kernel's configuration after `~`) the modules are built for;
    /// `None` without modules.
    pub kernel: Option<String>,
    pub service: Service,
    /// The schema of `[plugins.<id>]`, in the order of `[[form]]`.
    pub settings: Schema,
    pub form: Vec<FormSection>,
    /// What its `state` says of the plugin.
    pub status: Vec<StatusValue>,
    pub tunnel: Option<TunnelKind>,
    /// Its page, drawn by the router.
    pub page: Option<Page>,
    /// What a person may ask it to do now, from its page.
    pub actions: Vec<DeclaredAction>,
}

/// The plugin's one process.
#[derive(Clone, Debug)]
pub struct Service {
    /// A path inside the bundle.
    pub command: String,
    pub capabilities: Vec<String>,
    /// How long an apply waits for it to settle.
    pub settle_within: Duration,
}

/// A kind of tunnel (`[tunnel]`).
#[derive(Clone, Debug)]
pub struct TunnelKind {
    /// `IFLA_INFO_KIND` of the device the plugin makes.
    pub link_kind: String,
    pub mtu: Mtu,
    pub status: Vec<StatusValue>,
    pub form: Vec<FormSection>,
    /// The schema of `[tunnels.<key>.<id>]`, in the form's order.
    pub settings: Schema,
}

/// The MTU of a tunnel's device: netifd sets it from the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mtu {
    pub default: u16,
    pub min: u16,
    pub max: u16,
}

/// One section of a form: a title and the fields under it, in order.
#[derive(Clone, Debug)]
pub struct FormSection {
    pub title: String,
    pub fields: Vec<FieldKey>,
    pub folded: bool,
}

/// A value a plugin's `state` may carry, with its label and type.
#[derive(Clone, Debug)]
pub struct StatusValue {
    pub key: FieldKey,
    pub label: String,
    pub kind: StatusType,
    /// A value of each row of this group of the plugin's settings, not of
    /// the plugin.
    pub of: Option<FieldKey>,
}

/// An action of the manifest (`[[actions]]`): a button of the plugin's
/// page that asks its service to do something now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredAction {
    pub key: FieldKey,
    pub label: String,
    /// A button of each row of this group, not of the page.
    pub of: Option<FieldKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusType {
    Text,
    Count,
    /// A number of bytes.
    Bytes,
    Duration,
    /// A moment, in seconds since 1970 UTC.
    Time,
    Address,
}

/// The plugin's page (`[page]`), drawn by the router from the manifest:
/// an item of the navigation's group of plugins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub title: String,
    /// One of the router's icons.
    pub icon: Option<String>,
}

const KERNEL_RULE: &str =
    "the version of the kernel package: 1 to 128 letters, digits, '.', '+', '~', '_' or '-'";
/// A version of the `kernel` package as apk writes it.
fn kernel_like(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+~_-".contains(&byte))
}
const VERSION_RULE: &str =
    "1 to 32 letters, digits, '.', '+', '~' or '-', starting with a letter or digit";
const COMMAND_RULE: &str =
    "a path inside the bundle: segments of letters, digits, '.', '_' or '-', not '.' or '..'";
const LINK_KIND_RULE: &str =
    "1 to 16 lowercase letters, digits, '_' or '-', starting with a letter";
const ICON_RULE: &str = "1 to 32 lowercase letters, digits or '-'";
const SETTLE_DEFAULT: u32 = 10;
/// The least MTU of a link that carries IPv6 (RFC 8200 5).
const IPV6_MTU: u16 = 1280;
/// The most sections of a form, status values and actions.
const SECTIONS: usize = 32;
const STATUS: usize = 64;
const ACTIONS: usize = 32;
const SETTLE_MOST: u32 = 60;

/// The capabilities of Linux (`capability.h`, 6.12).
const CAPABILITIES: [&str; 41] = [
    "CAP_CHOWN",
    "CAP_DAC_OVERRIDE",
    "CAP_DAC_READ_SEARCH",
    "CAP_FOWNER",
    "CAP_FSETID",
    "CAP_KILL",
    "CAP_SETGID",
    "CAP_SETUID",
    "CAP_SETPCAP",
    "CAP_LINUX_IMMUTABLE",
    "CAP_NET_BIND_SERVICE",
    "CAP_NET_BROADCAST",
    "CAP_NET_ADMIN",
    "CAP_NET_RAW",
    "CAP_IPC_LOCK",
    "CAP_IPC_OWNER",
    "CAP_SYS_MODULE",
    "CAP_SYS_RAWIO",
    "CAP_SYS_CHROOT",
    "CAP_SYS_PTRACE",
    "CAP_SYS_PACCT",
    "CAP_SYS_ADMIN",
    "CAP_SYS_BOOT",
    "CAP_SYS_NICE",
    "CAP_SYS_RESOURCE",
    "CAP_SYS_TIME",
    "CAP_SYS_TTY_CONFIG",
    "CAP_MKNOD",
    "CAP_LEASE",
    "CAP_AUDIT_WRITE",
    "CAP_AUDIT_CONTROL",
    "CAP_SETFCAP",
    "CAP_MAC_OVERRIDE",
    "CAP_MAC_ADMIN",
    "CAP_SYSLOG",
    "CAP_WAKE_ALARM",
    "CAP_BLOCK_SUSPEND",
    "CAP_AUDIT_READ",
    "CAP_PERFMON",
    "CAP_BPF",
    "CAP_CHECKPOINT_RESTORE",
];

impl Manifest {
    /// Reads and checks a manifest's text.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let table: toml::Table = text
            .parse()
            .map_err(|error: toml::de::Error| syntax(text, &error))?;
        let mut top = Section::new("", &table);

        let id = top.required("id", Section::string)?;
        let id = PluginId::new(id).or_else(|error| top.refuse("id", error.reason()))?;
        if TUNNEL_KEYS.contains(&id.as_str()) {
            return top.refuse("id", format!("\"{id}\" is a key every tunnel has"));
        }
        let name = top.required("name", Section::string)?;
        let name = DisplayName::new(name).or_else(|error| top.refuse("name", error.reason()))?;
        let version = top.required("version", Section::string)?;
        if !version_like(version) {
            return top.refuse("version", VERSION_RULE);
        }
        let api = top.required("api", Section::integer)?;
        if api != API as i64 {
            return top.refuse("api", format!("this router speaks api {API}"));
        }
        let requires = top.strings("requires")?.unwrap_or_default();
        named(
            &top,
            "requires",
            &requires,
            package_like,
            "is not a package name",
        )?;
        let (modules, kernel) = match top.table("modules")? {
            None => (Vec::new(), None),
            Some(mut modules) => {
                let load = modules.required("load", Section::strings)?;
                named(&modules, "load", &load, module_like, "is not a module name")?;
                let kernel = modules.required("kernel", Section::string)?;
                if !kernel_like(kernel) {
                    return modules.refuse("kernel", KERNEL_RULE);
                }
                modules.finish()?;
                (load, Some(kernel.to_owned()))
            }
        };
        let Some(service) = top.table("service")? else {
            return top.refuse("service", "required");
        };
        let service = read_service(service)?;

        let mut settings = Schema::read(top.table("settings")?, false)?;
        let form = read_form(counted(&mut top, "form", SECTIONS, "sections")?)?;
        settings.order_by(
            "form",
            &form.iter().map(|s| s.fields.clone()).collect::<Vec<_>>(),
        )?;
        let status = read_status(
            counted(&mut top, "status", STATUS, "values")?,
            Some(&settings),
        )?;
        let tunnel = match top.table("tunnel")? {
            None => None,
            Some(tunnel) => Some(read_tunnel(tunnel)?),
        };
        let page = match top.table("page")? {
            None => None,
            Some(page) => Some(read_page(page)?),
        };
        let actions = read_actions(counted(&mut top, "actions", ACTIONS, "actions")?, &settings)?;
        top.finish()?;

        Ok(Self {
            id,
            name,
            version: version.to_owned(),
            api: API,
            requires: requires.into_iter().map(str::to_owned).collect(),
            modules: modules.into_iter().map(str::to_owned).collect(),
            kernel,
            service,
            settings,
            form,
            status,
            tunnel,
            page,
            actions,
        })
    }
}

/// Each of `items` of its form and once.
fn named(
    section: &Section<'_>,
    key: &str,
    items: &[&str],
    like: fn(&str) -> bool,
    not: &str,
) -> Result<(), ManifestError> {
    if let Some(bad) = items.iter().find(|item| !like(item)) {
        return section.refuse(key, format!("\"{bad}\" {not}"));
    }
    distinct(items.iter().copied())
        .or_else(|twice| section.refuse(key, format!("\"{twice}\" is named twice")))
}

fn read_service(mut service: Section<'_>) -> Result<Service, ManifestError> {
    let command = service.required("command", Section::string)?;
    if !command_like(command) {
        return service.refuse("command", COMMAND_RULE);
    }
    let capabilities = service.strings("capabilities")?.unwrap_or_default();
    named(
        &service,
        "capabilities",
        &capabilities,
        |name| CAPABILITIES.contains(&name),
        "is not a capability of Linux",
    )?;
    let settle_within = match service.string("settle_within")? {
        None => SETTLE_DEFAULT,
        Some(text) => match value::duration(text) {
            None => return service.refuse("settle_within", DURATION_RULE),
            Some(seconds) if (1..=SETTLE_MOST).contains(&seconds) => seconds,
            Some(_) => return service.refuse("settle_within", "1s to 1m"),
        },
    };
    service.finish()?;
    Ok(Service {
        command: command.to_owned(),
        capabilities: capabilities.into_iter().map(str::to_owned).collect(),
        settle_within: Duration::from_secs(settle_within.into()),
    })
}

fn read_tunnel(mut tunnel: Section<'_>) -> Result<TunnelKind, ManifestError> {
    let link_kind = tunnel.required("link_kind", Section::string)?;
    if !link_kind_like(link_kind) {
        return tunnel.refuse("link_kind", LINK_KIND_RULE);
    }
    let Some(mut mtu) = tunnel.table("mtu")? else {
        return tunnel.refuse("mtu", "required");
    };
    let default = mtu.required("default", Section::integer)?;
    let min = mtu.required("min", Section::integer)?;
    let max = mtu.required("max", Section::integer)?;
    if [default, min, max]
        .iter()
        .any(|n| !(68..=65535).contains(n))
    {
        return mtu.refuse_here("68 to 65535");
    }
    if !(min <= default && default <= max) {
        return mtu.refuse_here("min ≤ default ≤ max");
    }
    mtu.finish()?;
    let mtu = Mtu {
        default: default as u16,
        min: min as u16,
        max: max as u16,
    };
    let status = read_status(counted(&mut tunnel, "status", STATUS, "values")?, None)?;
    let mut settings = Schema::read(tunnel.table("settings")?, true)?;
    if settings.address(false).is_some() && (mtu.default < IPV6_MTU || mtu.max < IPV6_MTU) {
        return tunnel.refuse(
            "mtu",
            "a kind with an IPv6 address has a default and max of 1280 at least",
        );
    }
    let form = read_form(counted(&mut tunnel, "form", SECTIONS, "sections")?)?;
    settings.order_by(
        "tunnel.form",
        &form.iter().map(|s| s.fields.clone()).collect::<Vec<_>>(),
    )?;
    if let Some(constraints) = tunnel.table("constraints")? {
        settings.constrain(constraints)?;
    }
    tunnel.finish()?;
    Ok(TunnelKind {
        link_kind: link_kind.to_owned(),
        mtu,
        status,
        form,
        settings,
    })
}

fn read_form(sections: Vec<Section<'_>>) -> Result<Vec<FormSection>, ManifestError> {
    sections
        .into_iter()
        .map(|mut section| {
            let title = section.required("title", Section::string)?;
            let title = display(&section, "title", title)?;
            let fields = section
                .required("fields", Section::strings)?
                .into_iter()
                .map(|key| field_key(&section, "fields", key))
                .collect::<Result<Vec<_>, _>>()?;
            if fields.is_empty() {
                return section.refuse("fields", "at least one");
            }
            let folded = section.boolean("folded")?.unwrap_or(false);
            section.finish()?;
            Ok(FormSection {
                title,
                fields,
                folded,
            })
        })
        .collect()
}

/// The status values of the plugin (`settings`: their `of` names one of
/// its keyed groups) or of a tunnel (`None`: no `of`).
fn read_status(
    values: Vec<Section<'_>>,
    settings: Option<&Schema>,
) -> Result<Vec<StatusValue>, ManifestError> {
    let mut status: Vec<StatusValue> = Vec::new();
    for mut value in values {
        let of = match settings {
            Some(settings) => keyed_group(&mut value, settings)?,
            None => None,
        };
        let key = value.required("key", Section::string)?;
        let key = field_key(&value, "key", key)?;
        if status
            .iter()
            .any(|known| known.key == key && known.of == of)
        {
            return value.refuse("key", format!("{key} is declared twice"));
        }
        let label = value.required("label", Section::string)?;
        let label = display(&value, "label", label)?;
        let kind = match value.required("type", Section::string)? {
            "text" => StatusType::Text,
            "count" => StatusType::Count,
            "bytes" => StatusType::Bytes,
            "duration" => StatusType::Duration,
            "time" => StatusType::Time,
            "address" => StatusType::Address,
            other => {
                return value.refuse(
                    "type",
                    format!(
                        "\"{other}\" is not one of text, count, bytes, duration, time, address"
                    ),
                );
            }
        };
        value.finish()?;
        status.push(StatusValue {
            key,
            label,
            kind,
            of,
        });
    }
    Ok(status)
}

/// `of` of a status value or an action: a group of the settings that has
/// a key, by which `state` and actions name its rows.
fn keyed_group(
    section: &mut Section<'_>,
    settings: &Schema,
) -> Result<Option<FieldKey>, ManifestError> {
    let Some(of) = section.string("of")? else {
        return Ok(None);
    };
    match settings.field(of).map(|field| &field.kind) {
        Some(FieldType::Group { key: Some(_), .. }) => Ok(Some(field_key(section, "of", of)?)),
        Some(FieldType::Group { key: None, .. }) => {
            section.refuse("of", format!("{of} has no key"))
        }
        _ => section.refuse("of", format!("{of} is not a group of the settings")),
    }
}

fn read_actions(
    sections: Vec<Section<'_>>,
    settings: &Schema,
) -> Result<Vec<DeclaredAction>, ManifestError> {
    let mut actions: Vec<DeclaredAction> = Vec::new();
    for mut action in sections {
        let of = keyed_group(&mut action, settings)?;
        let key = action.required("key", Section::string)?;
        let key = field_key(&action, "key", key)?;
        if actions.iter().any(|known| known.key == key) {
            return action.refuse("key", format!("{key} is declared twice"));
        }
        let label = action.required("label", Section::string)?;
        let label = display(&action, "label", label)?;
        action.finish()?;
        actions.push(DeclaredAction { key, label, of });
    }
    Ok(actions)
}

fn read_page(mut page: Section<'_>) -> Result<Page, ManifestError> {
    let title = page.required("title", Section::string)?;
    let title = display(&page, "title", title)?;
    let icon = page.string("icon")?;
    if icon.is_some_and(|icon| !icon_like(icon)) {
        return page.refuse("icon", ICON_RULE);
    }
    page.finish()?;
    Ok(Page {
        title,
        icon: icon.map(str::to_owned),
    })
}

fn version_like(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text.starts_with(|c: char| c.is_ascii_alphanumeric())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'~' | b'-'))
}

fn package_like(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'+' | b'-')
        })
}

fn module_like(text: &str) -> bool {
    (1..=55).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn command_like(text: &str) -> bool {
    text.len() <= 255
        && text
            .split('/')
            .all(|segment| token(segment, 255) && !matches!(segment, "." | ".."))
}

fn link_kind_like(text: &str) -> bool {
    (1..=16).contains(&text.len())
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn icon_like(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The tables of `key`, at most `most` of them.
fn counted<'a>(
    section: &mut Section<'a>,
    key: &'a str,
    most: usize,
    what: &str,
) -> Result<Vec<Section<'a>>, ManifestError> {
    let tables = section.tables(key)?;
    if tables.len() > most {
        return section.refuse(key, format!("at most {most} {what}"));
    }
    Ok(tables)
}

/// A manifest that is no TOML, refused with the line and column where
/// the parser stopped.
fn syntax(text: &str, error: &toml::de::Error) -> ManifestError {
    let reason = match error.span() {
        Some(span) => {
            let before = text.get(..span.start).unwrap_or(text);
            let line = before.matches('\n').count() + 1;
            let column = before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1;
            format!("line {line}, column {column}: {}", error.message())
        }
        None => error.message().to_owned(),
    };
    ManifestError::new("manifest", reason)
}
