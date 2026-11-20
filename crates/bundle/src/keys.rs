//! ed25519 (RFC 8032) keys and signatures, each a line of text:
//!
//! - a public key: `ed25519 <base64 of its 32 bytes> [comment]`, a
//!   router's `*.pub` files;
//! - a secret key, the author's alone: `ed25519-secret <base64 of the
//!   32-byte seed>`;
//! - a signature: `ed25519 <base64 of the public key> <base64 of the 64
//!   bytes>`, so that it names the key it is by.
//!
//! The arithmetic is ring's (0.17.14), the router's pin.
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use routeh_plugin_protocol::base64;

const PUBLIC_RULE: &str = "a key is a line `ed25519 <base64 of 32 bytes> [comment]`";
const SECRET_RULE: &str = "a secret key is a line `ed25519-secret <base64 of 32 bytes>`";
const SIGNATURE_RULE: &str = "a signature is a line `ed25519 <base64 key> <base64 signature>`";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KeyError(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub fn parse(line: &str) -> Result<Self, KeyError> {
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("ed25519"), Some(key)) => decode_32(key)
                .map(Self)
                .ok_or_else(|| KeyError(PUBLIC_RULE.into())),
            _ => Err(KeyError(PUBLIC_RULE.into())),
        }
    }
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
    /// The line of a key file, with a comment if there is one.
    pub fn to_line(&self, comment: &str) -> String {
        let line = format!(
            "ed25519 {} {}",
            base64::encode(&self.0, true),
            comment.trim()
        );
        format!("{}\n", line.trim_end())
    }
    /// The signature is by this key and of `message`.
    pub fn verifies(&self, message: &[u8], signature: &Signature) -> bool {
        UnparsedPublicKey::new(&ED25519, &self.0)
            .verify(message, &signature.0)
            .is_ok()
    }
}

fn decode_32(text: &str) -> Option<[u8; 32]> {
    base64::decode(text)?.try_into().ok()
}

/// An author's key. Its `Debug` shows only the public half; its seed is
/// wiped when it is dropped.
pub struct SecretKey {
    seed: [u8; 32],
    pair: Ed25519KeyPair,
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.seed.fill(0);
        std::hint::black_box(&self.seed);
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SecretKey")
            .field(&self.public())
            .finish()
    }
}

impl SecretKey {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let pair = Ed25519KeyPair::from_seed_unchecked(&seed).expect("any 32 bytes are a seed");
        Self { seed, pair }
    }
    /// A new key of 32 bytes from the operating system.
    pub fn generate() -> Result<Self, KeyError> {
        use ring::rand::SecureRandom;
        let mut seed = [0u8; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut seed)
            .map_err(|_| KeyError("no randomness from the operating system".into()))?;
        Ok(Self::from_seed(seed))
    }
    pub fn parse(line: &str) -> Result<Self, KeyError> {
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("ed25519-secret"), Some(seed)) => decode_32(seed)
                .map(Self::from_seed)
                .ok_or_else(|| KeyError(SECRET_RULE.into())),
            _ => Err(KeyError(SECRET_RULE.into())),
        }
    }
    pub fn to_line(&self) -> String {
        format!("ed25519-secret {}\n", base64::encode(&self.seed, true))
    }
    pub fn public(&self) -> PublicKey {
        PublicKey(
            self.pair
                .public_key()
                .as_ref()
                .try_into()
                .expect("an ed25519 public key is 32 bytes"),
        )
    }
    /// The signature line of `message`: `SHA256SUMS.sig`, `index.json.sig`.
    pub fn signature_line(&self, message: &[u8]) -> String {
        self.sign(message).to_line(&self.public())
    }
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(
            self.pair
                .sign(message)
                .as_ref()
                .try_into()
                .expect("an ed25519 signature is 64 bytes"),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    /// The key a signature line names, and the signature.
    pub fn parse(line: &str) -> Result<(PublicKey, Self), KeyError> {
        let mut words = line.split_whitespace();
        let refused = || KeyError(SIGNATURE_RULE.into());
        let (Some("ed25519"), Some(key), Some(signature), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return Err(refused());
        };
        let key = decode_32(key).ok_or_else(refused)?;
        let signature: [u8; 64] = base64::decode(signature)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(refused)?;
        Ok((PublicKey(key), Self(signature)))
    }
    pub fn bytes(&self) -> &[u8; 64] {
        &self.0
    }
    pub fn to_line(&self, by: &PublicKey) -> String {
        format!(
            "ed25519 {} {}\n",
            base64::encode(&by.0, true),
            base64::encode(&self.0, true)
        )
    }
}

/// The key a signature line names, when its signature of `message`
/// holds: who signed it, whether or not anyone trusts that key.
pub fn signer_of(line: &str, message: &[u8]) -> Result<PublicKey, KeyError> {
    let (by, signature) = Signature::parse(line)?;
    if by.verifies(message, &signature) {
        Ok(by)
    } else {
        Err(KeyError("the signature does not hold".into()))
    }
}

/// The keys a router takes a bundle or an index signed by. Which keys
/// those are, and where it finds them, is the router's.
#[derive(Clone, Debug, Default)]
pub struct TrustedKeys(Vec<PublicKey>);

impl TrustedKeys {
    pub fn new(keys: impl IntoIterator<Item = PublicKey>) -> Self {
        Self(keys.into_iter().collect())
    }
    /// With the keys of these files, by file name: a file named `*.pub`
    /// is one key; any other file is not read. A key file that is no key
    /// is refused, naming it.
    pub fn with_files(
        mut self,
        files: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
    ) -> Result<Self, KeyError> {
        for (name, text) in files {
            let name = name.as_ref();
            if !name.ends_with(".pub") {
                continue;
            }
            let key = PublicKey::parse(text.as_ref())
                .map_err(|error| KeyError(format!("{name}: {error}")))?;
            self.0.push(key);
        }
        Ok(self)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn trusts(&self, key: &PublicKey) -> bool {
        self.0.contains(key)
    }
}
