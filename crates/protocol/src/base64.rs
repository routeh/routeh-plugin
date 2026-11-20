//! Standard base64 (RFC 4648 4): the keys of a plugin's settings
//! (`bytes`) and the SSH keys of Access.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
/// Standard base64 (RFC 4648 4), padding optional; `None` for anything
/// else.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    if text.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut held) = (0u32, 0u32);
    for byte in text.bytes() {
        let value = ALPHABET.iter().position(|known| *known == byte)? as u32;
        bits = bits << 6 | value;
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes.push((bits >> held) as u8);
            bits &= (1 << held) - 1;
        }
    }
    Some(bytes)
}
/// Standard base64, with padding (RFC 4648 4) or without it, as
/// `ssh-keygen` writes a fingerprint.
pub fn encode(bytes: &[u8], padded: bool) -> String {
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let mut word = [0u8; 3];
        word[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from(word[0]) << 16 | u32::from(word[1]) << 8 | u32::from(word[2]);
        for at in 0..=chunk.len() {
            text.push(ALPHABET[(bits >> (18 - 6 * at) & 63) as usize] as char);
        }
        if padded {
            text.extend(std::iter::repeat_n('=', 3 - chunk.len()));
        }
    }
    text
}
