//! What a kernel module says of itself in its `.modinfo` section.
use routeh_plugin_bundle::modinfo::{ModInfo, read};

#[test]
fn reads_openwrts_dummy_ko() {
    let info = read(&crate::dummy_ko()).unwrap();
    assert_eq!(
        info,
        ModInfo {
            name: "dummy".into(),
            vermagic: "6.12.112 SMP mod_unload aarch64".into(),
            srcversion: None,
            depends: vec![],
        }
    );
}

#[test]
fn reads_every_class_and_byte_order() {
    let modinfo = b"name=m\0depends=a,b\0srcversion=ABC123\0vermagic=6.6 SMP\0";
    for (class, big) in [(1, false), (1, true), (2, true), (2, false)] {
        let info = read(&crate::elf(class, big, &[(".modinfo", modinfo)])).unwrap();
        assert_eq!(info.name, "m");
        assert_eq!(info.depends, ["a", "b"]);
        assert_eq!(info.srcversion.as_deref(), Some("ABC123"));
        assert_eq!(info.vermagic, "6.6 SMP");
    }
}

#[test]
fn refuses_what_is_no_module() {
    for (bytes, said) in [
        (b"not an elf".to_vec(), "not an ELF file"),
        (
            crate::elf(2, false, &[(".text", b"x")]),
            "no .modinfo section",
        ),
        (
            crate::elf(2, false, &[(".modinfo", b"name=m\0")]),
            ".modinfo has no vermagic",
        ),
        (
            crate::elf(2, false, &[(".modinfo", b"vermagic=6\0")]),
            ".modinfo has no name",
        ),
        (
            crate::elf(2, false, &[(".modinfo", b"\xff=\xff\0")]),
            ".modinfo is not text",
        ),
        (
            {
                let mut cut = crate::dummy_ko();
                cut.truncate(70);
                cut
            },
            "a section lies outside the file",
        ),
        (
            {
                let mut odd = crate::dummy_ko();
                odd[4] = 3;
                odd
            },
            "not an ELF file",
        ),
    ] {
        assert_eq!(read(&bytes).unwrap_err().to_string(), said);
    }
}
