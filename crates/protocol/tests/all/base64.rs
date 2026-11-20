//! Standard base64 (RFC 4648 4), shared with the SSH keys of Access.
use routeh_plugin_protocol::base64::{decode, encode};

#[test]
fn round_trips_with_and_without_padding() {
    for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
        let padded = encode(bytes, true);
        assert_eq!(decode(&padded).unwrap(), bytes);
        assert_eq!(decode(&encode(bytes, false)).unwrap(), bytes);
    }
    assert_eq!(encode(b"foobar", true), "Zm9vYmFy");
    assert_eq!(encode(b"fo", true), "Zm8=");
    assert_eq!(encode(b"fo", false), "Zm8");
}

#[test]
fn refuses_what_is_not_base64() {
    for text in ["Z", "Zm9v!", "Zm 9v", "Zm9v-_"] {
        assert_eq!(decode(text), None, "{text:?}");
    }
}
