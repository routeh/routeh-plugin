//! What the build adds to the bundle. The format itself (the archive,
//! `SHA256SUMS`, its signature, packing and checking) is
//! `routeh-plugin-bundle`, the router's own; here are `build.json`, the
//! names of bundle files, versions pinned to a module's and ELF machines.

use routeh_plugin_bundle::{Bundle, PublicKey, SecretKey};
use routeh_plugin_protocol::{base64, sha256};

pub use routeh_plugin_bundle::MANIFEST;

/// What a bundle was built from and for.
pub const BUILD: &str = "build.json";

/// SHA-256 in lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha256::hex(&sha256::sha256(bytes))
}

/// A public key as its lines write it: base64 of its 32 bytes.
pub fn key_text(key: &PublicKey) -> String {
    base64::encode(key.bytes(), true)
}

/// A secret key's file.
pub fn read_secret(path: &str) -> Result<SecretKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    SecretKey::parse(&text).map_err(|e| format!("{path}: {e}"))
}

/// A public key's file.
pub fn read_public(path: &str) -> Result<PublicKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    PublicKey::parse(&text).map_err(|e| format!("{path}: {e}"))
}

/// What a bundle is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub plugin: String,
    pub version: String,
    /// `OPENWRT_ARCH`, from build.json.
    pub arch: String,
    /// `[modules] kernel`; `None` for a plugin without modules.
    pub kernel: Option<String>,
    /// The key of its signature, which holds; `None` when unsigned.
    pub signer: Option<PublicKey>,
}

impl std::fmt::Display for Info {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} for {}", self.plugin, self.version, self.arch)?;
        if let Some(k) = &self.kernel {
            write!(f, " kernel {k}")?;
        }
        match &self.signer {
            Some(k) => write!(f, ", signed by {}", key_text(k)),
            None => write!(f, ", unsigned"),
        }
    }
}

/// A bundle as the router checks it but for trust (`Bundle::read`), and
/// that its build.json agrees with its manifest.
pub fn check(bytes: &[u8]) -> Result<Info, String> {
    let bundle = Bundle::read(bytes).map_err(|e| e.to_string())?;
    let manifest = bundle.manifest();
    let build: serde_json::Value = serde_json::from_slice(
        bundle
            .files()
            .get(BUILD)
            .ok_or("build.json: not in the bundle")?,
    )
    .map_err(|e| format!("build.json: {e}"))?;
    if build["plugin"].as_str() != Some(manifest.id.as_str())
        || build["version"].as_str() != Some(manifest.version.as_str())
        || build["kernel_package"].as_str() != manifest.kernel.as_deref()
    {
        return Err(
            "build.json and manifest.toml disagree on the plugin, its version or the kernel".into(),
        );
    }
    let arch = build["arch"]
        .as_str()
        .filter(|a| !a.is_empty())
        .ok_or("build.json: no arch")?;
    Ok(Info {
        plugin: manifest.id.as_str().to_owned(),
        version: manifest.version.clone(),
        arch: arch.to_owned(),
        kernel: manifest.kernel.clone(),
        signer: bundle.signer().copied(),
    })
}

/// The manifest with `kernel` set in its `[modules]`: the version of the
/// `kernel` package the bundle's modules are built for. Every other line
/// stays as written.
pub fn with_kernel(text: &str, kernel: &str) -> Result<String, String> {
    let value: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let modules = value
        .get("modules")
        .ok_or("the manifest has no [modules]")?;
    if modules.get("kernel").is_some() {
        return Err("the manifest sets [modules] kernel: the build does".into());
    }
    let mut out = String::with_capacity(text.len() + 80);
    let mut done = false;
    for line in text.lines() {
        out.push_str(line);
        out.push('\n');
        if !done && line.trim() == "[modules]" {
            out.push_str(&format!("kernel = \"{kernel}\"\n"));
            done = true;
        }
    }
    if !done {
        return Err("the manifest's [modules] is not a line `[modules]` of its own".into());
    }
    Ok(out)
}

/// `6.12.112~<hash>-r1` as OpenWrt names its kmods directory:
/// `6.12.112-1-<hash>`.
pub fn kernel_dir(kernel: &str) -> String {
    match kernel
        .split_once('~')
        .and_then(|(v, rest)| rest.rsplit_once("-r").map(|(h, r)| (v, h, r)))
    {
        Some((v, h, r)) => format!("{v}-{r}-{h}"),
        None => kernel.replace('~', "-"),
    }
}

/// A bundle's file name: the same plugin, version, architecture and kernel
/// are always this name.
pub fn file_name(plugin: &str, version: &str, arch: &str, kernel: Option<&str>) -> String {
    match kernel {
        Some(k) => format!("{plugin}-{version}-{arch}-{}.tar", kernel_dir(k)),
        None => format!("{plugin}-{version}-{arch}.tar"),
    }
}

/// A version of numbers only, each without leading zeros: `3.1.20261113-02`
/// is `3.1.20261113.2`.
pub fn numeric_version(version: &str) -> Option<String> {
    let parts: Option<Vec<String>> = version
        .split(['.', '-'])
        .map(|p| p.parse::<u64>().ok().map(|n| n.to_string()))
        .collect();
    parts.filter(|p| !p.is_empty()).map(|p| p.join("."))
}

/// That a plugin's version is its module's: `<module version>-r<n>`
/// (`numeric_version`), `n` its own revision.
pub fn check_pinned(version: &str, module: &str, module_version: &str) -> Result<(), String> {
    let (base, rev) = version
        .rsplit_once("-r")
        .filter(|(_, r)| !r.is_empty() && r.bytes().all(|c| c.is_ascii_digit()))
        .ok_or(format!(
            "version {version:?} is not <{module}'s version>-r<n>"
        ))?;
    let want = numeric_version(module_version).ok_or(format!(
        "{module}'s version {module_version:?} is unreadable"
    ))?;
    if base != want {
        return Err(format!(
            "version {version} is not of the module it carries: {module} {module_version} is {want}-r{rev}"
        ));
    }
    Ok(())
}

/// ELF `e_machine` of an OpenWrt architecture (`aarch64_generic`).
pub fn machine_of(arch: &str) -> Option<u16> {
    Some(match arch.split('_').next()? {
        "aarch64" => 183,
        "arm" => 40,
        // OpenWrt's `x86_64`, split at its `_`.
        "x86" => 62,
        "i386" => 3,
        "mips" | "mipsel" | "mips64" | "mips64el" => 8,
        _ => return None,
    })
}

/// ELF `e_machine` of a binary.
pub fn elf_machine(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" {
        return None;
    }
    let m = [bytes[18], bytes[19]];
    Some(if bytes[5] == 1 {
        u16::from_le_bytes(m)
    } else {
        u16::from_be_bytes(m)
    })
}
