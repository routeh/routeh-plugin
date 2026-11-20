//! ed25519 keys and signatures as their files write them.
use routeh_plugin_bundle::{PublicKey, SecretKey, Signature, TrustedKeys, signer_of};

#[test]
fn a_secret_key_signs_what_its_public_key_verifies() {
    let key = crate::key(7);
    let public = key.public();
    let signature = key.sign(b"the list");
    assert!(public.verifies(b"the list", &signature));
    assert!(!public.verifies(b"another list", &signature));
    assert!(!crate::key(8).public().verifies(b"the list", &signature));
}

#[test]
fn rfc_8032_test_1() {
    // RFC 8032 7.1, TEST 1: the empty message.
    let seed = routeh_plugin_protocol::sha256::unhex(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    )
    .unwrap();
    let key = SecretKey::from_seed(seed.try_into().unwrap());
    assert_eq!(
        routeh_plugin_protocol::sha256::hex(key.public().bytes()),
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    assert_eq!(
        routeh_plugin_protocol::sha256::hex(key.sign(b"").bytes()),
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );
}

#[test]
fn keys_and_signatures_are_lines_of_text() {
    let key = crate::key(1);
    let public = key.public();
    let line = public.to_line("lab");
    assert!(line.starts_with("ed25519 "), "{line}");
    assert!(line.ends_with(" lab\n"), "{line}");
    assert_eq!(PublicKey::parse(&line).unwrap(), public);
    assert_eq!(
        PublicKey::parse(line.trim_end_matches(" lab\n")).unwrap(),
        public
    );
    assert_eq!(SecretKey::parse(&key.to_line()).unwrap().public(), public);
    let signature = key.sign(b"x");
    let said = signature.to_line(&public);
    let (by, read) = Signature::parse(&said).unwrap();
    assert_eq!(by, public);
    assert_eq!(read, signature);
    for refused in [
        "",
        "rsa AAAA",
        "ed25519 not-base64",
        "ed25519 AAAA",
        "ed25519-secret AAAA",
    ] {
        assert!(PublicKey::parse(refused).is_err(), "{refused}");
        assert!(SecretKey::parse(refused).is_err(), "{refused}");
        assert!(Signature::parse(refused).is_err(), "{refused}");
    }
    assert_eq!(
        PublicKey::parse("rsa AAAA").unwrap_err().to_string(),
        "a key is a line `ed25519 <base64 of 32 bytes> [comment]`"
    );
    let bare = public.to_line("");
    assert!(
        bare.starts_with("ed25519 ") && bare.ends_with("=\n"),
        "{bare:?}"
    );
    assert_eq!(bare.split_whitespace().count(), 2);
}

#[test]
fn a_signature_line_names_its_signer() {
    let key = crate::key(5);
    let line = key.signature_line(b"index");
    assert_eq!(line, key.sign(b"index").to_line(&key.public()));
    assert_eq!(signer_of(&line, b"index").unwrap(), key.public());
    assert_eq!(
        signer_of(&line, b"other").unwrap_err().to_string(),
        "the signature does not hold"
    );
    assert!(signer_of("garbage", b"index").is_err());
}

#[test]
fn a_new_key_is_random() {
    let one = SecretKey::generate().unwrap();
    let two = SecretKey::generate().unwrap();
    assert_ne!(one.public(), two.public());
}

#[test]
fn trusted_keys_are_given_and_read_from_files() {
    assert!(TrustedKeys::default().is_empty());
    let lab = crate::key(3).public();
    assert!(TrustedKeys::new([lab]).trusts(&lab));
    let keys = TrustedKeys::default()
        .with_files([
            ("lab.pub", lab.to_line("lab")),
            ("README", "not a key".to_owned()),
            ("broken.pub", "ed25519 AAAA\n".to_owned()),
        ])
        .unwrap_err();
    assert_eq!(
        keys.to_string(),
        "broken.pub: a key is a line `ed25519 <base64 of 32 bytes> [comment]`"
    );
    let keys = TrustedKeys::default()
        .with_files([
            ("lab.pub", lab.to_line("lab")),
            ("README", "not a key".to_owned()),
        ])
        .unwrap();
    assert!(!keys.is_empty());
    assert!(keys.trusts(&lab));
    assert!(!keys.trusts(&crate::key(4).public()));
}
