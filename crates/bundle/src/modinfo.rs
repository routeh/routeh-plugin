//! What a kernel module says of itself: its `.modinfo` section, a run of
//! `key=value` strings each ended by NUL (`include/linux/moduleparam.h`,
//! `MODULE_INFO`), read out of the module's ELF (32 or 64 bits, either
//! byte order) the way `modinfo` does, without loading it.
//!
//! OpenWrt 25.12.6's modules (kernel 6.12.112) carry `name`, `vermagic`
//! and `depends`, and no `srcversion`: its kernel is built without
//! `CONFIG_MODULE_SRCVERSION_ALL`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModInfo {
    pub name: String,
    /// The kernel's version and the flags it was built with.
    pub vermagic: String,
    /// The hash of its sources; only some modules have one.
    pub srcversion: Option<String>,
    /// The modules it needs loaded first, by name.
    pub depends: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ModInfoError(pub &'static str);

/// The module information of an ELF file.
pub fn read(elf: &[u8]) -> Result<ModInfo, ModInfoError> {
    let section = section(elf, ".modinfo")?.ok_or(ModInfoError("no .modinfo section"))?;
    let text = std::str::from_utf8(section).map_err(|_| ModInfoError(".modinfo is not text"))?;
    let mut name = None;
    let mut vermagic = None;
    let mut srcversion = None;
    let mut depends = Vec::new();
    for entry in text.split('\0') {
        let Some((key, value)) = entry.split_once('=') else {
            continue;
        };
        match key {
            "name" => name = Some(value.to_owned()),
            "vermagic" => vermagic = Some(value.to_owned()),
            "srcversion" => srcversion = Some(value.to_owned()),
            "depends" => {
                depends = value
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            _ => {}
        }
    }
    Ok(ModInfo {
        name: name.ok_or(ModInfoError(".modinfo has no name"))?,
        vermagic: vermagic.ok_or(ModInfoError(".modinfo has no vermagic"))?,
        srcversion,
        depends,
    })
}

/// The bytes of the section of this name.
fn section<'a>(elf: &'a [u8], wanted: &str) -> Result<Option<&'a [u8]>, ModInfoError> {
    let outside = ModInfoError("a section lies outside the file");
    if elf.len() < 16 || &elf[..4] != b"\x7fELF" {
        return Err(ModInfoError("not an ELF file"));
    }
    let wide = match elf[4] {
        1 => false,
        2 => true,
        _ => return Err(ModInfoError("not an ELF file")),
    };
    let big = match elf[5] {
        1 => false,
        2 => true,
        _ => return Err(ModInfoError("not an ELF file")),
    };
    let take = |at: usize, width: usize| -> Option<u64> {
        let bytes = elf.get(at..at.checked_add(width)?)?;
        let mut word = [0u8; 8];
        if big {
            word[8 - width..].copy_from_slice(bytes);
            Some(u64::from_be_bytes(word))
        } else {
            word[..width].copy_from_slice(bytes);
            Some(u64::from_le_bytes(word))
        }
    };
    let to = |value: u64| usize::try_from(value).ok();
    let (shoff, table_at, word) = if wide {
        (0x28, 0x3a, 8)
    } else {
        (0x20, 0x2e, 4)
    };
    let headers = take(shoff, word).and_then(to).ok_or(outside.clone())?;
    let entry = take(table_at, 2).and_then(to).ok_or(outside.clone())?;
    let count = take(table_at + 2, 2).and_then(to).ok_or(outside.clone())?;
    let names = take(table_at + 4, 2).and_then(to).ok_or(outside.clone())?;
    // A section header: name (4), type (4), flags, address, offset, size.
    let (offset_at, size_at) = if wide { (24, 32) } else { (16, 20) };
    let bytes_of = |index: usize| -> Option<&'a [u8]> {
        let header = headers.checked_add(index.checked_mul(entry)?)?;
        let offset = to(take(header + offset_at, word)?)?;
        let size = to(take(header + size_at, word)?)?;
        elf.get(offset..offset.checked_add(size)?)
    };
    let name_table = bytes_of(names).ok_or(outside.clone())?;
    for index in 0..count {
        let header = headers
            .checked_add(index.checked_mul(entry).ok_or(outside.clone())?)
            .ok_or(outside.clone())?;
        let name_at = take(header, 4).and_then(to).ok_or(outside.clone())?;
        let name = name_table.get(name_at..).ok_or(outside.clone())?;
        let name = &name[..name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name.len())];
        if name == wanted.as_bytes() {
            return bytes_of(index).map(Some).ok_or(outside);
        }
    }
    Ok(None)
}
