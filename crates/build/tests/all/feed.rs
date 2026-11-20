use std::path::Path;

use routeh_plugin_build::feed::*;
use routeh_plugin_build::routeh_plugin_bundle::SecretKey as Secret;
use routeh_plugin_build::routeh_plugin_bundle::feed::{
    Build, INDEX, INDEX_SIG, Index, Version, newer,
};

use crate::{scratch, unsigned};

const K1: &str = "6.12.112~aa-r1";
const K2: &str = "6.12.113~bb-r1";

fn propose(incoming: &Path, id: &str, name: &str, bytes: &[u8]) {
    let dir = incoming.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

fn index(dir: &Path) -> Index {
    Index::parse(&std::fs::read(dir.join(INDEX)).unwrap()).unwrap()
}

fn blobs(dir: &Path) -> usize {
    std::fs::read_dir(dir.join(BLOBS)).unwrap().count()
}

#[test]
fn a_run_signs_proposals_and_rebuilds_the_index() {
    let (dir, inc) = (scratch("feed-run"), scratch("feed-run-in"));
    let key = Secret::generate().unwrap();
    propose(
        &inc,
        "awg",
        "a.tar",
        &unsigned("awg", "1.0-r1", "aarch64_generic", Some(K1)),
    );
    propose(
        &inc,
        "awg",
        "b.tar",
        &unsigned("awg", "1.0-r1", "x86_64", Some(K1)),
    );
    propose(
        &inc,
        "demo",
        "c.tar",
        &unsigned("demo", "2", "x86_64", None),
    );
    std::fs::write(inc.join("not-a-plugin"), b"").unwrap();
    std::fs::write(inc.join("demo/readme"), b"").unwrap();
    let policy = Policy::default();

    let r = run(&dir, &key, Some(&inc), &policy, false, 90, 1000).unwrap();
    assert_eq!(r.added.len(), 3, "{:?}", r.added);
    assert!(r.refused.is_empty());
    assert!(r.written);
    assert_eq!(r.serial, 1);
    let i = index(&dir);
    assert_eq!(i.expires, 1000 + 90 * 86400);
    assert_eq!(i.plugins["awg"][0].builds.len(), 2);
    assert_eq!(i.plugins["demo"][0].builds[0].kernel, None);
    assert_eq!(read_index(&dir, &key).unwrap(), Some(i.clone()));

    // The same proposals again: nothing new, the index stays.
    let r = run(&dir, &key, Some(&inc), &policy, false, 90, 2000).unwrap();
    assert!(r.added.is_empty() && !r.written);
    assert_eq!(r.serial, 1);
    assert_eq!(index(&dir), i);

    // Signed again: a new serial and expires, the same plugins.
    let r = run(&dir, &key, None, &policy, true, 30, 3000).unwrap();
    assert!(r.written);
    let again = index(&dir);
    assert_eq!((again.serial, again.expires), (2, 3000 + 30 * 86400));
    assert_eq!(again.plugins, i.plugins);

    // Another key does not write over this feed.
    let other = Secret::generate().unwrap();
    assert!(
        run(&dir, &other, None, &policy, true, 30, 3000)
            .unwrap_err()
            .contains("another key")
    );
}

#[test]
fn a_bundle_of_another_plugin_is_refused_and_the_rest_goes_on() {
    let (dir, inc) = (scratch("feed-refuse"), scratch("feed-refuse-in"));
    let key = Secret::generate().unwrap();
    propose(&inc, "awg", "x.tar", &unsigned("demo", "1", "x86_64", None));
    propose(&inc, "awg", "y.tar", b"not a bundle");
    propose(
        &inc,
        "demo",
        "z.tar",
        &unsigned("demo", "1", "x86_64", None),
    );
    let r = run(&dir, &key, Some(&inc), &Policy::default(), false, 90, 0).unwrap();
    assert_eq!(r.refused.len(), 2, "{:?}", r.refused);
    assert!(r.refused.iter().any(|e| e.contains("proposed as awg's")));
    assert_eq!(r.added.len(), 1);
    assert!(!index(&dir).plugins.contains_key("awg"));
}

#[test]
fn a_version_built_again_replaces_its_blob() {
    let (dir, inc) = (scratch("feed-rebuilt"), scratch("feed-rebuilt-in"));
    let key = Secret::generate().unwrap();
    let first = unsigned("demo", "1", "x86_64", Some(K1));
    propose(&inc, "demo", "a.tar", &first);
    run(&dir, &key, Some(&inc), &Policy::default(), false, 90, 0).unwrap();
    // Other bytes of the same build: another commit of the same version.
    let mut files = crate::files("demo", "1", "x86_64", Some(K1));
    files.insert("bin/demod".into(), b"\x7fELF rebuilt".to_vec());
    std::fs::remove_dir_all(&inc).unwrap();
    propose(&inc, "demo", "a.tar", &crate::pack(&files));
    let r = run(&dir, &key, Some(&inc), &Policy::default(), false, 90, 0).unwrap();
    assert!(r.added[0].contains("replacing"), "{:?}", r.added);
    assert_eq!(blobs(&dir), 1);
    assert_eq!(index(&dir).serial, 2);
}

#[test]
fn the_policy_yanks_and_keeps() {
    let (dir, inc) = (scratch("feed-policy"), scratch("feed-policy-in"));
    let key = Secret::generate().unwrap();
    for v in ["1.9", "1.10", "1.11"] {
        propose(
            &inc,
            "awg",
            &format!("{v}-k1.tar"),
            &unsigned("awg", v, "x86_64", Some(K1)),
        );
    }
    propose(
        &inc,
        "awg",
        "1.9-k2.tar",
        &unsigned("awg", "1.9", "x86_64", Some(K2)),
    );
    let none = run(&dir, &key, Some(&inc), &Policy::default(), false, 90, 0).unwrap();
    assert!(none.written);
    let versions: Vec<String> = index(&dir).plugins["awg"]
        .iter()
        .map(|v| v.version.clone())
        .collect();
    assert_eq!(
        versions,
        ["1.11", "1.10", "1.9"],
        "newest first, as the router orders"
    );

    let policy = Policy::parse("[awg]\nkeep = 1\nyank = [\"1.11\"]\n").unwrap();
    let r = run(&dir, &key, None, &policy, false, 90, 0).unwrap();
    assert!(r.written);
    let i = index(&dir);
    let awg = &i.plugins["awg"];
    // 1.11 yanked; of the rest the newest per kernel: 1.10 on K1, 1.9 on K2.
    assert_eq!(awg.len(), 2);
    assert_eq!(awg[0].version, "1.10");
    assert_eq!(awg[1].version, "1.9");
    assert_eq!(awg[1].builds.len(), 1);
    assert_eq!(awg[1].builds[0].kernel.as_deref(), Some(K2));
    assert_eq!(blobs(&dir), 4, "blobs stay");

    let all = Policy::parse("[awg]\nyank = [\"1.9\", \"1.10\", \"1.11\"]\n").unwrap();
    run(&dir, &key, None, &all, false, 90, 0).unwrap();
    assert!(index(&dir).plugins.is_empty());
}

#[test]
fn feed_toml() {
    assert!(
        Policy::parse("[awg]\nkeep = 0\n")
            .unwrap_err()
            .contains("keep = 0")
    );
    assert!(Policy::parse("[awg]\nhold = true\n").is_err());
    let dir = scratch("feed-toml");
    assert!(
        Policy::read(&dir.join("feed.toml"))
            .unwrap()
            .plugins
            .is_empty()
    );
    std::fs::write(dir.join("feed.toml"), "[awg]\nkeep = 3\n").unwrap();
    assert_eq!(
        Policy::read(&dir.join("feed.toml")).unwrap().plugins["awg"].keep,
        Some(3)
    );
    assert!(Policy::read(&dir).is_err(), "a directory");
}

#[test]
fn blobs_are_checked() {
    let dir = scratch("feed-blobs");
    let key = Secret::generate().unwrap();
    std::fs::create_dir_all(dir.join(BLOBS)).unwrap();
    std::fs::write(dir.join(BLOBS).join(".keep"), b"").unwrap();
    std::fs::write(dir.join(BLOBS).join("abc"), b"x").unwrap();
    assert!(
        run(&dir, &key, None, &Policy::default(), false, 90, 0)
            .unwrap_err()
            .contains("not the SHA-256")
    );
    std::fs::remove_file(dir.join(BLOBS).join("abc")).unwrap();
    let b = unsigned("demo", "1", "x86_64", None);
    let sha = routeh_plugin_build::bundle::sha256_hex(&b);
    std::fs::write(dir.join(BLOBS).join(&sha), &b).unwrap();
    assert!(
        run(&dir, &key, None, &Policy::default(), false, 90, 0)
            .unwrap_err()
            .contains("not signed by the feed's key")
    );
}

#[test]
fn an_index_is_read_with_care() {
    assert!(Index::parse(b"{}").is_err());
    assert!(
        Index::parse(br#"{"feed":2,"serial":1,"expires":1,"plugins":{}}"#)
            .unwrap_err()
            .to_string()
            .contains("format 2")
    );
    let dir = scratch("feed-index");
    let key = Secret::generate().unwrap();
    assert_eq!(read_index(&dir, &key).unwrap(), None);
    std::fs::write(dir.join(INDEX), b"{}").unwrap();
    assert!(read_index(&dir, &key).is_err(), "no signature");
    std::fs::write(dir.join(INDEX_SIG), b"nonsense").unwrap();
    assert!(read_index(&dir, &key).is_err());
}

#[test]
fn versions_order_as_the_router() {
    use std::cmp::Ordering::*;
    assert_eq!(newer("3.1.20261113.2-r10", "3.1.20261113.2-r9"), Greater);
    assert_eq!(newer("3.1.20261114.1-r1", "3.1.20261113.2-r9"), Greater);
    assert_eq!(newer("1.0", "1.0"), Equal);
    assert_eq!(newer("1.0", "1.0.1"), Less);
    assert_eq!(newer("1.0a", "1.0b"), Less);
}

#[test]
fn missing_builds() {
    let b = |arch: &str, kernel: Option<&str>| Build {
        arch: arch.into(),
        kernel: kernel.map(str::to_owned),
        blob: "sha256:x".into(),
        size: 1,
    };
    let i = Index {
        feed: 1,
        serial: 1,
        expires: 1,
        plugins: [
            (
                "awg".to_owned(),
                vec![Version {
                    version: "1".into(),
                    builds: vec![b("a", Some(K1))],
                }],
            ),
            (
                "demo".to_owned(),
                vec![Version {
                    version: "1".into(),
                    builds: vec![b("a", None)],
                }],
            ),
        ]
        .into(),
    };
    let wanted = vec![
        ("a".to_owned(), Some(K1.to_owned())),
        ("a".to_owned(), Some(K2.to_owned())),
        ("b".to_owned(), Some(K1.to_owned())),
    ];
    assert_eq!(
        missing(Some(&i), "awg", "1", &wanted),
        vec![&wanted[1], &wanted[2]]
    );
    assert_eq!(missing(Some(&i), "awg", "2", &wanted).len(), 3);
    assert_eq!(
        missing(Some(&i), "demo", "1", &wanted),
        vec![&wanted[2]],
        "no kernel: every kernel"
    );
    assert_eq!(missing(None, "awg", "1", &wanted).len(), 3);
}

#[test]
fn a_plugin_publishes_its_own_feed() {
    let (dir, src) = (scratch("own"), scratch("own-src"));
    let key = Secret::generate().unwrap();
    let put = |name: &str, bytes: Vec<u8>| {
        let p = src.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    };
    let a = put("a.tar", unsigned("mine", "1.0", "x86_64", Some(K1)));
    let b = put(
        "b.tar",
        unsigned("mine", "1.0", "aarch64_generic", Some(K1)),
    );
    let r = publish(&dir, &key, &[a.clone(), b], 90, 100).unwrap();
    assert_eq!((r.added.len(), r.serial), (2, 1));
    assert_eq!(blobs(&dir), 2);
    assert_eq!(index(&dir).plugins["mine"][0].builds.len(), 2);

    // The next run starts from the index alone, as downloaded: no blobs.
    std::fs::remove_dir_all(dir.join(BLOBS)).unwrap();
    let newer = put("c.tar", unsigned("mine", "1.1", "x86_64", Some(K1)));
    let r = publish(&dir, &key, &[newer], 90, 200).unwrap();
    assert_eq!(r.serial, 2);
    assert_eq!(blobs(&dir), 1, "only what is to be uploaded");
    let i = index(&dir);
    let versions: Vec<&str> = i.plugins["mine"]
        .iter()
        .map(|v| v.version.as_str())
        .collect();
    assert_eq!(versions, ["1.1", "1.0"]);

    // The same version built again replaces its build.
    let mut files = crate::files("mine", "1.0", "x86_64", Some(K1));
    files.insert("bin/mined".into(), b"\x7fELF rebuilt".to_vec());
    let again = put("d.tar", crate::pack(&files));
    publish(&dir, &key, &[again], 90, 300).unwrap();
    let old = &index(&dir).plugins["mine"][1];
    assert_eq!(old.builds.len(), 2);
    assert_ne!(old.builds, i.plugins["mine"][1].builds);

    // No bundles: signed again.
    let r = publish(&dir, &key, &[], 30, 400).unwrap();
    assert!(r.added.is_empty());
    assert_eq!(index(&dir).expires, 400 + 30 * 86400);

    // Another key, or what is not a bundle: refused, nothing written.
    let other = Secret::generate().unwrap();
    assert!(
        publish(&dir, &other, &[], 30, 0)
            .unwrap_err()
            .contains("another key")
    );
    let bad = put("bad.tar", b"no".to_vec());
    assert!(publish(&dir, &key, &[bad], 30, 0).is_err());
    assert!(publish(&dir, &key, &[src.join("none.tar")], 30, 0).is_err());
    assert_eq!(index(&dir).serial, 4);
}
