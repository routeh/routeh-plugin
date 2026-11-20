//! A kernel module's ELF as far as its module information goes: what the
//! tests of `crates/bundle` load.

/// The `.modinfo` section of OpenWrt 25.12.6's `dummy.ko` for armsr/armv8,
/// kernel 6.12.112 (kmod-dummy-6.12.112-r1.apk, SHA-256
/// ef733bcc61130e4aecee6746315ca938e235eb0ca958329212e83238caa56ac4 from
/// downloads.openwrt.org, kmods/6.12.112-1-67e205f26d0458c4d12f1fd315216398),
/// read out of its ELF as it is.
pub const DUMMY_MODINFO: &[u8] = b"alias=rtnl-link-dummy\x00license=GPL\x00parmtype=numdummies:int\x00depends=\x00intree=Y\x00name=dummy\x00vermagic=6.12.112 SMP mod_unload aarch64\x00";

/// A relocatable ELF of `class` (1: 32 bits, 2: 64) and byte order
/// (`big`) with these sections, as a kernel module is laid out: a null
/// section, the named ones, then the table of their names.
pub fn elf(class: u8, big: bool, sections: &[(&str, &[u8])]) -> Vec<u8> {
    let wide = class == 2;
    let (header, entry) = if wide { (64, 64) } else { (52, 40) };
    let mut names = vec![0u8];
    let mut name_at = Vec::new();
    for (name, _) in sections {
        name_at.push(names.len() as u32);
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }
    let shstrtab_name = names.len() as u32;
    names.extend_from_slice(b".shstrtab\0");
    let mut body = Vec::new();
    let mut placed = Vec::new();
    for (_, bytes) in sections {
        placed.push((header + body.len(), bytes.len()));
        body.extend_from_slice(bytes);
    }
    let names_at = header + body.len();
    body.extend_from_slice(&names);
    let table_at = header + body.len();
    let count = sections.len() + 2;
    let u16b = |v: u16| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let u32b = |v: u32| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let u64b = |v: u64| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let mut out = vec![0x7f, b'E', b'L', b'F', class, if big { 2 } else { 1 }, 1, 0];
    out.resize(16, 0);
    out.extend_from_slice(&u16b(1)); // ET_REL
    out.extend_from_slice(&u16b(183)); // EM_AARCH64
    out.extend_from_slice(&u32b(1));
    let word = |out: &mut Vec<u8>, v: u64| {
        if wide {
            out.extend_from_slice(&u64b(v))
        } else {
            out.extend_from_slice(&u32b(v as u32))
        }
    };
    word(&mut out, 0); // entry
    word(&mut out, 0); // phoff
    word(&mut out, table_at as u64); // shoff
    out.extend_from_slice(&u32b(0)); // flags
    out.extend_from_slice(&u16b(header as u16));
    out.extend_from_slice(&u16b(0));
    out.extend_from_slice(&u16b(0));
    out.extend_from_slice(&u16b(entry as u16));
    out.extend_from_slice(&u16b(count as u16));
    out.extend_from_slice(&u16b((count - 1) as u16));
    assert_eq!(out.len(), header);
    out.extend_from_slice(&body);
    let section = |out: &mut Vec<u8>, name: u32, offset: usize, size: usize| {
        out.extend_from_slice(&u32b(name));
        out.extend_from_slice(&u32b(if size == 0 && offset == 0 { 0 } else { 1 }));
        word(out, 0); // flags
        word(out, 0); // addr
        word(out, offset as u64);
        word(out, size as u64);
        out.extend_from_slice(&u32b(0));
        out.extend_from_slice(&u32b(0));
        word(out, 1);
        word(out, 0);
    };
    section(&mut out, 0, 0, 0);
    for (at, (offset, size)) in name_at.iter().zip(&placed) {
        section(&mut out, *at, *offset, *size);
    }
    section(&mut out, shstrtab_name, names_at, names.len());
    out
}

/// `dummy.ko` as far as its module information goes.
pub fn dummy_ko() -> Vec<u8> {
    elf(
        2,
        false,
        &[(".text", b"\0\0\0\0"), (".modinfo", DUMMY_MODINFO)],
    )
}
