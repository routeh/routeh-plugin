//! The bundle's container: a tar archive, of which regular files and
//! directories are taken and nothing else.
use routeh_plugin_bundle::tar::{self, Limits};

/// A ustar header of `name` of type `kind` and `size`, its checksum
/// computed, followed by `data` padded to a block.
fn entry(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut out = header.to_vec();
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out
}

fn end() -> Vec<u8> {
    vec![0; 1024]
}

fn archive(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = entries.concat();
    out.extend(end());
    out
}

fn pax(records: &[(&str, &str)]) -> Vec<u8> {
    let mut text = String::new();
    for (key, value) in records {
        let body = format!(" {key}={value}\n");
        let mut length = body.len() + 1;
        while format!("{length}{body}").len() != length {
            length += 1;
        }
        text.push_str(&format!("{length}{body}"));
    }
    text.into_bytes()
}

#[test]
fn reads_regular_files_and_directories() {
    let bytes = archive(&[
        entry("./bin/", b'5', b""),
        entry("./bin/stand-in", b'0', b"#!/bin/sh\n"),
        entry("manifest.toml", 0, b"id = 1\n"),
    ]);
    let files = tar::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        ["bin/stand-in", "manifest.toml"]
    );
    assert_eq!(files["bin/stand-in"], b"#!/bin/sh\n");
}

#[test]
fn reads_a_long_path_of_a_pax_record_and_a_prefix() {
    let long = format!("modules/{}.ko", "m".repeat(120));
    let mut prefixed = entry("dummy.ko", b'0', b"ko");
    prefixed[345..352].copy_from_slice(b"modules");
    let sum: u32 = {
        let mut header = prefixed[..512].to_vec();
        header[148..156].copy_from_slice(b"        ");
        header.iter().map(|b| u32::from(*b)).sum()
    };
    prefixed[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let bytes = archive(&[
        entry(
            "PaxHeaders/x",
            b'x',
            &pax(&[("path", &long), ("mtime", "1")]),
        ),
        entry("ignored-name", b'0', b"long"),
        entry("pax_global_header", b'g', &pax(&[("comment", "x")])),
        prefixed,
    ]);
    let files = tar::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(files[&long], b"long");
    assert_eq!(files["modules/dummy.ko"], b"ko");
}

#[test]
fn refuses_links_devices_and_unknown_kinds() {
    for (kind, said) in [
        (b'1', "a hard link"),
        (b'2', "a symbolic link"),
        (b'3', "a device"),
        (b'4', "a device"),
        (b'6', "a FIFO"),
        (b'7', "an entry of kind 7"),
    ] {
        let bytes = archive(&[entry("x", kind, b"")]);
        assert_eq!(
            tar::read(&bytes, &Limits::default())
                .unwrap_err()
                .to_string(),
            format!("x: {said} is not taken"),
        );
    }
}

#[test]
fn refuses_paths_that_leave_the_bundle_or_repeat() {
    for (name, said) in [
        ("/etc/passwd", "/etc/passwd: an absolute path"),
        ("bin/../../x", "bin/../../x: '..' is not taken"),
        ("a//b", "a//b: an empty segment"),
        ("bin/\u{1}x", "bin/\u{1}x: a control character"),
    ] {
        let bytes = archive(&[entry(name, b'0', b"")]);
        assert_eq!(
            tar::read(&bytes, &Limits::default())
                .unwrap_err()
                .to_string(),
            said
        );
    }
    let bytes = archive(&[entry("a", b'0', b"1"), entry("./a", b'0', b"2")]);
    assert_eq!(
        tar::read(&bytes, &Limits::default())
            .unwrap_err()
            .to_string(),
        "a: twice in the bundle"
    );
    let bytes = archive(&[entry("a", b'0', b"1"), entry("a/b", b'0', b"2")]);
    assert_eq!(
        tar::read(&bytes, &Limits::default())
            .unwrap_err()
            .to_string(),
        "a/b: under a file"
    );
}

#[test]
fn is_bounded() {
    let limits = Limits {
        files: 1,
        file_bytes: 4,
        total_bytes: 6,
    };
    let two = archive(&[entry("a", b'0', b"1"), entry("b", b'0', b"2")]);
    assert_eq!(
        tar::read(&two, &limits).unwrap_err().to_string(),
        "b: more than 1 files"
    );
    let big = archive(&[entry("a", b'0', b"12345")]);
    assert_eq!(
        tar::read(&big, &limits).unwrap_err().to_string(),
        "a: larger than 4 bytes"
    );
    let limits = Limits {
        files: 5,
        file_bytes: 4,
        total_bytes: 6,
    };
    let many = archive(&[entry("a", b'0', b"1234"), entry("b", b'0', b"123")]);
    assert_eq!(
        tar::read(&many, &limits).unwrap_err().to_string(),
        "b: the bundle is larger than 6 bytes"
    );
    let defaults = Limits::default();
    assert_eq!(defaults.files, 1024);
    assert_eq!(defaults.file_bytes, 32 << 20);
    assert_eq!(defaults.total_bytes, 64 << 20);
}

#[test]
fn refuses_what_is_no_archive() {
    let mut bad = entry("a", b'0', b"1");
    bad[148] = b'9';
    let mut odd = entry("a", b'0', b"1");
    odd[124..136].copy_from_slice(b"00000000x01\0");
    let sum: u32 = {
        let mut header = odd[..512].to_vec();
        header[148..156].copy_from_slice(b"        ");
        header.iter().map(|b| u32::from(*b)).sum()
    };
    odd[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    for bytes in [
        vec![1u8; 100],
        archive(&[bad]),
        archive(&[odd]),
        entry("a", b'0', b"12345")[..512 + 2].to_vec(),
    ] {
        let error = tar::read(&bytes, &Limits::default()).unwrap_err();
        assert!(
            error.to_string().starts_with("not a tar archive"),
            "{error}"
        );
    }
    // A pax record that is no record names nothing: the header's name
    // stands, and is checked as any.
    let files = tar::read(
        &archive(&[entry("x", b'x', b"5 path\n"), entry("a", b'0', b"")]),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(files.keys().collect::<Vec<_>>(), ["a"]);
    // An archive without its two zero blocks ends where its bytes end.
    let files = tar::read(&entry("a", b'0', b"1"), &Limits::default()).unwrap();
    assert_eq!(files["a"], b"1");
    let not_utf8 = archive(&[entry("a\u{80}", b'0', b"")]);
    let mut not_utf8 = not_utf8;
    not_utf8[1] = 0xff;
    let sum: u32 = {
        let mut header = not_utf8[..512].to_vec();
        header[148..156].copy_from_slice(b"        ");
        header.iter().map(|b| u32::from(*b)).sum()
    };
    not_utf8[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    assert!(
        tar::read(&not_utf8, &Limits::default())
            .unwrap_err()
            .to_string()
            .ends_with("the path is not UTF-8")
    );
}

#[test]
fn writes_what_it_reads_deterministically() {
    let files = [
        ("manifest.toml".to_owned(), b"id\n".to_vec()),
        (format!("bin/{}", "n".repeat(150)), b"#!".to_vec()),
        ("modules/dummy.ko".to_owned(), vec![0u8; 700]),
    ];
    let write = || tar::write(files.iter().map(|(p, b)| (p.as_str(), b.as_slice()))).unwrap();
    let written = write();
    assert_eq!(written, write());
    assert_eq!(written.len() % 512, 0);
    let read = tar::read(&written, &Limits::default()).unwrap();
    assert_eq!(read.len(), 3);
    for (path, bytes) in &files {
        assert_eq!(&read[path], bytes);
    }
}

#[test]
fn writes_only_what_it_would_read() {
    for bad in ["../x", "/x", "a//b", "./a", "a/", "a/../b", "bin/\u{1}"] {
        assert!(tar::write([(bad, &b"x"[..])]).is_err(), "{bad}");
    }
    // A long path of characters wider than a byte: written whole.
    let long = format!("modules/{}.ko", "ж".repeat(80));
    let written = tar::write([(long.as_str(), &b"k"[..])]).unwrap();
    assert_eq!(
        tar::read(&written, &Limits::default()).unwrap()[&long],
        b"k"
    );
    let modes = tar::write([("bin/x", &b"#!"[..]), ("y", &b""[..])]).unwrap();
    assert_eq!(&modes[100..107], b"0000755");
    assert_eq!(&modes[1024 + 100..1024 + 107], b"0000644");
}
