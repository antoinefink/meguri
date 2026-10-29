//! Canonical K5.2 MGBlob metadata and K20 content validation.
use crate::cloud::entity_codec::{
    CodecError,
    wire::{Reader, Writer},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub sha256: [u8; 32],
    pub sha1: [u8; 20],
    pub size: u64,
}
impl Blob {
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        if !(1..=super::MAX_ASSET_BYTES).contains(&self.size) {
            return Err(CodecError::Oversize);
        }
        let mut w = Writer::emitting();
        w.scalar(1, 1, false)?;
        w.bytes(2, &self.sha256, false)?;
        w.bytes(3, &self.sha1, false)?;
        w.scalar(4, self.size, false)?;
        Ok(w.finish())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > 96 {
            return Err(CodecError::Oversize);
        }
        let mut r = Reader::new(bytes);
        let mut value = Self::default();
        let mut last = 0;
        while let Some((tag, field)) = r.next()? {
            if tag <= last {
                return Err(CodecError::NonCanonical);
            }
            last = tag;
            match tag {
                1 if field.integer()? == 1 => {}
                2 => value.sha256 = field.bytes()?.try_into().map_err(|_| CodecError::InvalidLength)?,
                3 => value.sha1 = field.bytes()?.try_into().map_err(|_| CodecError::InvalidLength)?,
                4 => value.size = field.integer()?,
                _ => return Err(CodecError::NonCanonical),
            }
        }
        if value.encode()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(value)
    }
    pub fn name(&self, key: &[u8; 32]) -> String {
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts a 32-byte key");
        mac.update(&self.sha256);
        let bytes = mac.finalize().into_bytes();
        format!(
            "b{}",
            data_encoding::BASE32_NOPAD.encode(&bytes[..16]).to_ascii_lowercase()
        )
    }
    pub fn media(&self, name: &str) -> super::c::MediaName {
        super::c::MediaName {
            name: name.as_bytes().to_vec(),
            sha256: self.sha256,
            sha1: self.sha1,
            size: self.size,
        }
    }
}
impl From<&super::c::MediaName> for Blob {
    fn from(m: &super::c::MediaName) -> Self {
        Self {
            sha256: m.sha256,
            sha1: m.sha1,
            size: m.size,
        }
    }
}
