//! SHA-256: the digests of a bundle's files, of a feed's blobs and of
//! what a plugin is told.
use routeh_plugin_protocol::sha256::{hex, sha256};

#[test]
fn is_fips_180_4s() {
    // FIPS 180-4's examples (NIST CSRC, SHA256.pdf): one block and two.
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn hex_is_lowercase_and_reads_back() {
    assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    assert_eq!(
        routeh_plugin_protocol::sha256::unhex("00abff"),
        Some(vec![0x00, 0xab, 0xff])
    );
    for refused in ["0", "0g", "AB", "zz"] {
        assert_eq!(
            routeh_plugin_protocol::sha256::unhex(refused),
            None,
            "{refused}"
        );
    }
}
