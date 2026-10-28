//! Pure MGEntity codecs (K4, K5.1/K5.3/K5.4, K6.1, K7, K20).
//!
//! Callers supply canonical key bytes, revisions, identities and Meguri fields.
//! This module performs no row reads, policy decisions, asset I/O or state changes.
//! In particular, tag keys must already use the pinned tags-table fold and media
//! keys must already use the media case key. Raw payload TEXT is never folded.
mod envelope;
mod payload;
mod wire;

pub use envelope::{
    AssetInput, AssetProblem, DecodeOutcome, Entity, EntityFields, ReceiveError, Received, Refusal, receive,
};
pub use payload::{
    Card, Config, Deck, Field, MediaName, Note, Notetype, Payload, Preset, Revlog, Stored, Tag, Template,
};

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

pub const INLINE_ENVELOPE_BYTES: usize = 786_432;
pub const MAX_PAYLOAD_BYTES: u64 = 104_857_600;

/// Errors contain classifications only, never content, identifiers or names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    NonCanonical,
    InvalidLength,
    InvalidKey,
    InvalidKind,
    Oversize,
}
impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonCanonical => "non-canonical entity encoding",
            Self::InvalidLength => "invalid entity field length",
            Self::InvalidKey => "invalid entity key",
            Self::InvalidKind => "invalid entity kind",
            Self::Oversize => "entity payload exceeds size limit",
        })
    }
}
impl std::error::Error for CodecError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Note = 1,
    Card,
    Revlog,
    Deck,
    Preset,
    Notetype,
    Tag,
    Config,
    MediaName,
}
impl TryFrom<u64> for Kind {
    type Error = CodecError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Note,
            2 => Self::Card,
            3 => Self::Revlog,
            4 => Self::Deck,
            5 => Self::Preset,
            6 => Self::Notetype,
            7 => Self::Tag,
            8 => Self::Config,
            9 => Self::MediaName,
            _ => return Err(CodecError::InvalidKind),
        })
    }
}
impl Kind {
    fn validate_key(self, key: &[u8]) -> Result<(), CodecError> {
        if (self as u8) <= 6 {
            if key.len() != 8 {
                return Err(CodecError::InvalidKey);
            }
        } else if std::str::from_utf8(key).is_err() {
            return Err(CodecError::InvalidKey);
        }
        Ok(())
    }
}

/// K4's exact signed representation, including negative ids.
pub fn id_key(id: i64) -> [u8; 8] {
    id.to_be_bytes()
}

/// K4 naming from caller-supplied canonical key bytes. No key is logged.
pub fn entity_name(record_key: &[u8; 32], kind: Kind, canonical_key: &[u8]) -> Result<String, CodecError> {
    kind.validate_key(canonical_key)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(record_key).map_err(|_| CodecError::InvalidLength)?;
    mac.update(&[kind as u8]);
    mac.update(canonical_key);
    let digest = mac.finalize().into_bytes();
    let text = data_encoding::BASE32_NOPAD.encode(&digest[..16]).to_ascii_lowercase();
    Ok(format!("e{text}"))
}

pub fn validate_payload_size(size: u64) -> Result<(), CodecError> {
    if size > MAX_PAYLOAD_BYTES {
        Err(CodecError::Oversize)
    } else {
        Ok(())
    }
}
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
