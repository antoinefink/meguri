//! The salt, the hash `H` and the random streams (`specs/fixtures/anonymizer.md`
//! A3). Every hash and every random draw is keyed by the salt, so the clone
//! depends only on the source and the salt, and one place's draws (a note,
//! a deck, a config key, a media file) never depend on another's.

use rand::rngs::StdRng;
use rand::{Rng as _, RngCore, SeedableRng};
use sha2::{Digest, Sha256};

use super::AnonymizeError;

/// The domain of media file name hashes (A3.1).
pub const MEDIA_DOMAIN: &str = "meguri-anonymize/media";
/// The domain of tag component hashes (A3.1).
pub const TAG_DOMAIN: &str = "meguri-anonymize/tag";
/// The domain of stream seeds (A3.2).
const STREAM_DOMAIN: &str = "meguri-anonymize/stream";

/// 32 secret bytes that key every hash and random draw.
#[derive(Clone, PartialEq, Eq)]
pub struct Salt([u8; 32]);

impl std::fmt::Debug for Salt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The salt is a secret: only its digest is ever shown.
        write!(f, "Salt(sha256 {})", self.sha256_hex())
    }
}

impl Salt {
    pub fn new(bytes: [u8; 32]) -> Salt {
        Salt(bytes)
    }

    /// A2.2: a salt file holds 64 lower-case hex digits, optionally followed
    /// by one LF. Anything else is A18 `salt`.
    pub fn parse(contents: &[u8]) -> Result<Salt, AnonymizeError> {
        let digits = contents.strip_suffix(b"\n").unwrap_or(contents);
        if digits.len() != 64 {
            return Err(AnonymizeError::Salt);
        }
        let mut bytes = [0u8; 32];
        for (i, pair) in digits.chunks(2).enumerate() {
            let hi = lower_hex_value(pair[0]).ok_or(AnonymizeError::Salt)?;
            let lo = lower_hex_value(pair[1]).ok_or(AnonymizeError::Salt)?;
            bytes[i] = hi << 4 | lo;
        }
        Ok(Salt(bytes))
    }

    /// The salt file's contents: 64 lower-case hex digits and an LF.
    pub fn to_file_contents(&self) -> String {
        format!("{}\n", hex(&self.0))
    }

    /// A16 `salt_sha256`: identifies the salt without revealing it.
    pub fn sha256_hex(&self) -> String {
        hex(&Sha256::digest(self.0))
    }

    /// A3.1: `H(domain, key)`, the first 8 bytes of SHA-256(salt ‖ domain ‖
    /// 0x00 ‖ key) as 16 lower-case hex digits.
    pub fn hash(&self, domain: &str, key: &str) -> String {
        hex(&self.digest(domain, key)[..8])
    }

    /// A3.2: the stream of the place `place` (`note:<id>`, `deck:<id>`,
    /// `config:<KEY>`, `media:<clone name>`).
    pub fn stream(&self, place: &str) -> Stream {
        Stream(StdRng::from_seed(self.digest(STREAM_DOMAIN, place)))
    }

    fn digest(&self, domain: &str, key: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.0);
        hasher.update(domain.as_bytes());
        hasher.update([0u8]);
        hasher.update(key.as_bytes());
        let mut out = [0u8; 32];
        out.copy_from_slice(&hasher.finalize());
        out
    }
}

fn lower_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One place's random stream: ChaCha12 (`rand` =0.9.4 `StdRng`).
#[derive(Debug)]
pub struct Stream(StdRng);

impl Stream {
    /// A choice among `n` candidates: one `random_range(0..n)` draw.
    pub fn choose(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0, "a choice needs a candidate");
        self.0.random_range(0..n)
    }

    /// Fills `bytes` with the stream's next bytes.
    pub fn fill(&mut self, bytes: &mut [u8]) {
        self.0.fill_bytes(bytes);
    }
}
