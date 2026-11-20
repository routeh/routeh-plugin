//! A bundle: its files, the list of their digests and the signature of
//! that list, checked before anything of it is used.
use routeh_plugin_bundle::{Bundle, TrustedKeys, pack, sign, tar};

fn files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("manifest.toml".into(), crate::MANIFEST.as_bytes().to_vec()),
        ("bin/stand-in".into(), b"\x7fELF stand-in".to_vec()),
        ("modules/dummy.ko".into(), crate::dummy_ko()),
    ]
}

fn trusted() -> TrustedKeys {
    TrustedKeys::default()
        .with_files([("lab.pub", crate::key(1).public().to_line("lab"))])
        .unwrap()
}

fn packed_by(key: Option<&routeh_plugin_bundle::SecretKey>) -> Vec<u8> {
    pack(files().iter().map(|(p, b)| (p.as_str(), b.as_slice())), key).unwrap()
}

fn packed() -> Vec<u8> {
    packed_by(Some(&crate::key(1)))
}

#[test]
fn opens_a_bundle_signed_by_a_trusted_key() {
    let bytes = packed();
    assert_eq!(bytes, packed(), "packing is deterministic");
    let bundle = Bundle::open(&bytes, &trusted()).unwrap();
    assert_eq!(bundle.manifest().id.as_str(), "stand_in");
    assert_eq!(bundle.manifest().version, "1.0.0");
    assert_eq!(
        bundle.files().keys().collect::<Vec<_>>(),
        ["bin/stand-in", "manifest.toml", "modules/dummy.ko"]
    );
    assert_eq!(bundle.signer(), Some(&crate::key(1).public()));
    assert_eq!(
        bundle.module("dummy").unwrap(),
        crate::dummy_ko().as_slice()
    );
    assert!(bundle.module("other").is_none());
    assert!(bundle.executable("bin/stand-in"));
    assert!(!bundle.executable("manifest.toml"));
    let listed =
        String::from_utf8(tar::read(&bytes, &Default::default()).unwrap()["SHA256SUMS"].clone())
            .unwrap();
    assert_eq!(listed.lines().count(), 3);
    assert!(
        listed
            .lines()
            .all(|line| line.len() > 66 && &line[64..66] == "  ")
    );
}

#[test]
fn refuses_a_bundle_nobody_trusted_signed() {
    let bytes = packed_by(Some(&crate::key(2)));
    assert_eq!(
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
        "SHA256SUMS.sig: signed by a key the router does not trust"
    );
    assert_eq!(
        Bundle::open(&bytes, &TrustedKeys::default())
            .unwrap_err()
            .to_string(),
        "SHA256SUMS.sig: signed by a key the router does not trust"
    );
}

/// The files of the packed bundle with `change` done to them, written
/// again without signing anew.
fn tampered(change: impl FnOnce(&mut std::collections::BTreeMap<String, Vec<u8>>)) -> Vec<u8> {
    let mut read = tar::read(&packed(), &Default::default()).unwrap();
    change(&mut read);
    tar::write(read.iter().map(|(p, b)| (p.as_str(), b.as_slice()))).unwrap()
}

#[test]
fn refuses_a_file_that_is_not_what_the_list_says() {
    let bytes = tampered(|files| {
        files.insert("bin/stand-in".into(), b"another program".to_vec());
    });
    assert_eq!(
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
        "bin/stand-in: not the digest SHA256SUMS lists"
    );
    let bytes = tampered(|files| {
        files.insert("bin/extra".into(), b"x".to_vec());
    });
    assert_eq!(
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
        "bin/extra: not in SHA256SUMS"
    );
    let bytes = tampered(|files| {
        files.remove("bin/stand-in");
    });
    assert_eq!(
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
        "bin/stand-in: listed in SHA256SUMS but not in the bundle"
    );
    let bytes = tampered(|files| {
        let list = files.get_mut("SHA256SUMS").unwrap();
        list.extend_from_slice(b"\n");
    });
    assert_eq!(
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
        "SHA256SUMS.sig: the signature does not match SHA256SUMS"
    );
    let bytes = tampered(|files| {
        files.remove("SHA256SUMS.sig");
    });
    assert_eq!(
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
        "SHA256SUMS.sig: not in the bundle"
    );
}

#[test]
fn refuses_a_malformed_list_even_when_signed() {
    for (list, said) in [
        (
            "zz  a\n",
            "SHA256SUMS: line 1 is not `<sha256 in hex>  <path>`",
        ),
        (
            &format!("{}  b\n{}  a\n", "0".repeat(64), "0".repeat(64)) as &str,
            "SHA256SUMS: line 2 is not in the order of paths",
        ),
    ] {
        let mut files = tar::read(&packed(), &Default::default()).unwrap();
        files.insert("SHA256SUMS".into(), list.as_bytes().to_vec());
        let key = crate::key(1);
        files.insert(
            "SHA256SUMS.sig".into(),
            key.sign(list.as_bytes())
                .to_line(&key.public())
                .into_bytes(),
        );
        let bytes = tar::write(files.iter().map(|(p, b)| (p.as_str(), b.as_slice()))).unwrap();
        assert_eq!(
            Bundle::open(&bytes, &trusted()).unwrap_err().to_string(),
            said
        );
    }
}

#[test]
fn refuses_a_bundle_whose_manifest_names_what_it_lacks() {
    let refuse = |files: Vec<(&str, Vec<u8>)>| {
        let bytes = pack(
            files.iter().map(|(p, b)| (*p, b.as_slice())),
            Some(&crate::key(1)),
        )
        .unwrap();
        Bundle::open(&bytes, &trusted()).unwrap_err().to_string()
    };
    assert_eq!(
        refuse(vec![("bin/stand-in", b"x".to_vec())]),
        "manifest.toml: not in the bundle"
    );
    assert_eq!(
        refuse(vec![("manifest.toml", b"id = 1".to_vec())]),
        "manifest.toml: id: must be a string"
    );
    assert_eq!(
        refuse(vec![
            ("manifest.toml", crate::MANIFEST.as_bytes().to_vec()),
            ("modules/dummy.ko", crate::dummy_ko()),
        ]),
        "bin/stand-in: the service's command is not in the bundle"
    );
    assert_eq!(
        refuse(vec![
            ("manifest.toml", crate::MANIFEST.as_bytes().to_vec()),
            ("bin/stand-in", b"x".to_vec()),
        ]),
        "modules/dummy.ko: a module the manifest loads is not in the bundle"
    );
    assert_eq!(
        refuse(vec![("manifest.toml", vec![0xff])]),
        "manifest.toml: not UTF-8"
    );
    assert_eq!(
        pack([("SHA256SUMS", &b""[..])].into_iter(), Some(&crate::key(1)))
            .unwrap_err()
            .to_string(),
        "SHA256SUMS: a name the bundle keeps for itself"
    );
    assert_eq!(
        pack([("a", &b"1"[..]), ("a", &b"2"[..])], None)
            .unwrap_err()
            .to_string(),
        "a: twice in the bundle"
    );
    assert!(pack([("../a", &b"1"[..])], None).is_err());
}

#[test]
fn a_build_reads_a_bundle_unsigned_and_signs_it() {
    let unsigned = packed_by(None);
    assert_eq!(unsigned, packed_by(None), "packing is deterministic");
    let bundle = Bundle::read(&unsigned).unwrap();
    assert_eq!(bundle.signer(), None);
    assert_eq!(bundle.manifest().id.as_str(), "stand_in");
    assert_eq!(
        Bundle::open(&unsigned, &trusted()).unwrap_err().to_string(),
        "SHA256SUMS.sig: not in the bundle"
    );
    // Signed: the same bytes as packed with the key.
    let signed = sign(&unsigned, &crate::key(1)).unwrap();
    assert_eq!(signed, packed());
    assert!(Bundle::open(&signed, &trusted()).is_ok());
    // Signed again by another key: its signature replaces the first.
    let other = sign(&signed, &crate::key(2)).unwrap();
    assert_eq!(other, packed_by(Some(&crate::key(2))));
    // `read` takes any key whose signature holds, and no broken one.
    assert_eq!(
        Bundle::read(&other).unwrap().signer(),
        Some(&crate::key(2).public())
    );
    let broken = tampered(|files| {
        files.insert("bin/stand-in".into(), b"another".to_vec());
    });
    assert!(Bundle::read(&broken).is_err());
    assert!(sign(&broken, &crate::key(1)).is_err());
    let mut lost = tar::read(&unsigned, &Default::default()).unwrap();
    lost.insert("SHA256SUMS.sig".into(), b"ed25519 AAAA BBBB\n".to_vec());
    let lost = tar::write(lost.iter().map(|(p, b)| (p.as_str(), b.as_slice()))).unwrap();
    assert!(
        Bundle::read(&lost)
            .unwrap_err()
            .to_string()
            .starts_with("SHA256SUMS.sig")
    );
    let mut no_list = tar::read(&unsigned, &Default::default()).unwrap();
    no_list.remove("SHA256SUMS");
    let no_list = tar::write(no_list.iter().map(|(p, b)| (p.as_str(), b.as_slice()))).unwrap();
    assert_eq!(
        Bundle::read(&no_list).unwrap_err().to_string(),
        "SHA256SUMS: not in the bundle"
    );
    assert_eq!(Bundle::read(b"not a tar").unwrap_err().at, "the bundle");
}
