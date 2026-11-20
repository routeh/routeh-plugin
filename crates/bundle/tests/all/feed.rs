//! A feed's signed index and the choice of a build in it.
use routeh_plugin_bundle::TrustedKeys;
use routeh_plugin_bundle::feed::{Build, Index, Resolved, Router};

const KERNEL: &str = "6.12.112~67e205f26d0458c4d12f1fd315216398-r1";

fn router() -> Router {
    Router {
        arch: "aarch64_generic".into(),
        kernel: KERNEL.into(),
        release: "25.12.6".into(),
        board: "armsr/armv8".into(),
    }
}

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn index_text(serial: u64) -> String {
    format!(
        r#"{{"feed":1,"serial":{serial},"expires":2000000000,"plugins":{{
  "stand_in":[
    {{"version":"1.0.0","builds":[{{"arch":"aarch64_generic","kernel":"{KERNEL}","blob":"{a}","size":10}}]}},
    {{"version":"1.2.0","builds":[{{"arch":"aarch64_generic","kernel":"6.12.113~ffff-r1","blob":"{b}","size":10}},
                                   {{"arch":"x86_64","kernel":"{KERNEL}","blob":"{c}","size":10}}]}},
    {{"version":"1.1.0","builds":[{{"arch":"aarch64_generic","kernel":"{KERNEL}","blob":"{d}","size":12}}]}}
  ],
  "tool":[{{"version":"2.0","builds":[{{"arch":"aarch64_generic","blob":"{e}","size":3}}]}},
          {{"version":"3.0","builds":[{{"arch":"mipsel_24kc","blob":"{f}","size":3}}]}}]
}}}}"#,
        a = digest('a'),
        b = digest('b'),
        c = digest('c'),
        d = digest('d'),
        e = digest('e'),
        f = digest('f'),
    )
}

fn trusted() -> TrustedKeys {
    TrustedKeys::default()
        .with_files([("feed.pub", crate::key(9).public().to_line("feed"))])
        .unwrap()
}

fn signed(text: &str) -> String {
    let key = crate::key(9);
    key.sign(text.as_bytes()).to_line(&key.public())
}

#[test]
fn chooses_the_newest_build_for_the_router() {
    let text = index_text(5);
    let index = Index::open(text.as_bytes(), &signed(&text), &trusted(), None).unwrap();
    assert_eq!(index.serial, 5);
    assert!(!index.expired(1_900_000_000));
    assert!(index.expired(2_000_000_001));
    let Resolved { version, build } = index.resolve("stand_in", &router()).unwrap();
    assert_eq!(version, "1.1.0");
    assert_eq!(
        build,
        &Build {
            arch: "aarch64_generic".into(),
            kernel: Some(KERNEL.into()),
            blob: digest('d'),
            size: 12,
        }
    );
    assert_eq!(build.digest(), "d".repeat(64));
    assert_eq!(build.path(), format!("blobs/{}", "d".repeat(64)));
    // Without modules a build fits every kernel.
    assert_eq!(index.resolve("tool", &router()).unwrap().version, "2.0");
}

#[test]
fn says_why_there_is_no_build() {
    let text = index_text(5);
    let index = Index::open(text.as_bytes(), &signed(&text), &trusted(), None).unwrap();
    let other = Router {
        kernel: "6.6.93~a1b2c3d4-r1".into(),
        board: "mediatek/filogic".into(),
        ..router()
    };
    assert_eq!(
        index.resolve("stand_in", &other).unwrap_err().to_string(),
        "no build of stand_in for kernel 6.6.93~a1b2c3d4-r1 of 25.12.6 mediatek/filogic"
    );
    let mips = Router {
        arch: "riscv64".into(),
        ..router()
    };
    assert_eq!(
        index.resolve("tool", &mips).unwrap_err().to_string(),
        "no build of tool for riscv64"
    );
    assert_eq!(
        index.resolve("awg", &router()).unwrap_err().to_string(),
        "awg is not in the feed"
    );
}

#[test]
fn checks_a_blob_against_its_build() {
    let bytes = b"bundle bytes".to_vec();
    let build = Build::of("a", None, &bytes);
    assert_eq!(
        build.blob,
        format!(
            "sha256:{}",
            routeh_plugin_protocol::sha256::hex(&routeh_plugin_protocol::sha256::sha256(&bytes))
        )
    );
    assert!(build.check(&bytes).is_ok());
    assert_eq!(
        build.check(b"other bytes!").unwrap_err(),
        "the blob is not the digest the index says"
    );
    assert_eq!(
        build.check(b"short").unwrap_err(),
        "the blob is 5 bytes, not 12"
    );
}

#[test]
fn refuses_an_index_that_is_not_signed_old_or_malformed() {
    let text = index_text(5);
    assert_eq!(
        Index::open(
            text.as_bytes(),
            &signed(&text),
            &TrustedKeys::default(),
            None
        )
        .unwrap_err()
        .to_string(),
        "index.json.sig: signed by a key the router does not trust"
    );
    assert_eq!(
        Index::open(text.as_bytes(), &signed("other"), &trusted(), None)
            .unwrap_err()
            .to_string(),
        "index.json.sig: the signature does not match index.json"
    );
    assert_eq!(
        Index::open(text.as_bytes(), "garbage", &trusted(), None)
            .unwrap_err()
            .to_string(),
        "index.json.sig: a signature is a line `ed25519 <base64 key> <base64 signature>`"
    );
    assert_eq!(
        Index::open(text.as_bytes(), &signed(&text), &trusted(), Some(6))
            .unwrap_err()
            .to_string(),
        "index.json: serial 5 is older than 6, which this router has seen"
    );
    assert!(Index::open(text.as_bytes(), &signed(&text), &trusted(), Some(5)).is_ok());
    for (bad, said) in [
        ("{}", "index.json: missing field `feed`"),
        (
            r#"{"feed":2,"serial":1,"expires":1,"plugins":{}}"#,
            "index.json: feed format 2 is not 1",
        ),
        (
            r#"{"feed":1,"serial":1,"expires":1,"plugins":{"x":[{"version":"1","builds":[{"arch":"a","blob":"sha256:zz","size":1}]}]}}"#,
            "index.json: x 1: a blob is `sha256:` and 64 hexadecimal digits",
        ),
        (
            r#"{"feed":1,"serial":1,"expires":1,"plugins":{"Bad":[]}}"#,
            "index.json: Bad is not a plugin id",
        ),
        (
            r#"{"feed":1,"serial":1,"expires":1,"plugins":{"x":[{"version":"1","builds":[]},{"version":"1","builds":[]}]}}"#,
            "index.json: x 1: the version twice",
        ),
        (
            &format!(
                r#"{{"feed":1,"serial":1,"expires":1,"plugins":{{"x":[{{"version":"1","builds":[{{"arch":"a","blob":"{a}","size":1}},{{"arch":"a","blob":"{b}","size":1}}]}}]}}}}"#,
                a = digest('a'),
                b = digest('b')
            ),
            "index.json: x 1: two builds for a",
        ),
    ] {
        let error = Index::open(bad.as_bytes(), &signed(bad), &trusted(), None)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(said), "{error}");
    }
}

#[test]
fn a_feed_writes_what_the_router_reads() {
    let mut index = Index::new(7, 2_000_000_000);
    let build = |arch: &str, kernel: Option<&str>, bytes: &[u8]| Build::of(arch, kernel, bytes);
    assert_eq!(index.add("tool", "1.9", build("x86_64", None, b"a")), None);
    assert_eq!(index.add("tool", "1.10", build("x86_64", None, b"b")), None);
    assert_eq!(
        index.add("tool", "1.10", build("aarch64_generic", None, b"c")),
        None
    );
    // The same version, architecture and kernel again: replaced.
    let replaced = index.add("tool", "1.9", build("x86_64", None, b"d"));
    assert_eq!(replaced, Some(build("x86_64", None, b"a")));
    let versions: Vec<&str> = index.plugins["tool"]
        .iter()
        .map(|v| v.version.as_str())
        .collect();
    assert_eq!(versions, ["1.10", "1.9"], "newest first");
    let arches: Vec<&str> = index.plugins["tool"][0]
        .builds
        .iter()
        .map(|b| b.arch.as_str())
        .collect();
    assert_eq!(arches, ["aarch64_generic", "x86_64"]);

    let text = index.to_json();
    assert!(text.ends_with(b"}\n"));
    assert!(
        !String::from_utf8_lossy(&text).contains("kernel"),
        "no kernel: not written"
    );
    let read = Index::open(
        &text,
        &crate::key(9).signature_line(&text),
        &trusted(),
        Some(7),
    )
    .unwrap();
    assert_eq!(read, index);
    assert_eq!(Index::parse(&text).unwrap(), index);
}
