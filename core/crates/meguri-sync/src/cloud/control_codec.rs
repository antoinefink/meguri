//! Narrow K5.2 control records, using the entity codec's canonical wire primitives.
use super::entity_codec::{
    CodecError, Kind,
    wire::{Reader, Writer},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    pub epoch: [u8; 16],
    pub activated_at_ms: i64,
    pub previous: Option<[u8; 16]>,
    pub replaced_through_ms: Option<i64>,
}
impl Active {
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut w = Writer::emitting();
        w.scalar(1, 1, false)?;
        w.bytes(2, &self.epoch, false)?;
        w.scalar(3, self.activated_at_ms as u64, false)?;
        if let Some(p) = self.previous {
            w.bytes(4, &p, true)?
        }
        if let Some(t) = self.replaced_through_ms {
            w.scalar(5, t as u64, true)?
        }
        Ok(w.finish())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut r = Reader::new(bytes);
        let mut a = Self {
            epoch: [0; 16],
            activated_at_ms: 0,
            previous: None,
            replaced_through_ms: None,
        };
        let mut last = 0;
        let mut format = 0;
        let mut epoch = false;
        while let Some((t, v)) = r.next()? {
            if t <= last {
                return Err(CodecError::NonCanonical);
            }
            last = t;
            match t {
                1 => format = v.integer()?,
                2 => {
                    a.epoch = v.bytes()?.try_into().map_err(|_| CodecError::InvalidLength)?;
                    epoch = true
                }
                3 => a.activated_at_ms = v.integer()? as i64,
                4 => a.previous = Some(v.bytes()?.try_into().map_err(|_| CodecError::InvalidLength)?),
                5 => a.replaced_through_ms = Some(v.integer()? as i64),
                _ => return Err(CodecError::NonCanonical),
            }
        }
        if format != 1 || !epoch || a.encode()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(a)
    }
}
/// Maximum canonical V1 sizes: epoch 217 bytes (all nine u64::MAX counts and
/// ten-byte signed crt/time varints), media root 47 bytes. The 1024-byte V1 cap
/// leaves safe headroom; future formats are probed without this cap.
pub const MAX_CONTROL_BYTES: usize = 1024;

/// As with the entity codec, inspect the format before interpreting V1 fields.
/// Unsupported records need attention; their bodies are not decoded as V1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeOutcome<T> {
    Supported(T),
    UnsupportedFormat(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EpochState {
    Publishing = 1,
    Complete = 2,
}
impl TryFrom<u64> for EpochState {
    type Error = CodecError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Publishing),
            2 => Ok(Self::Complete),
            _ => Err(CodecError::NonCanonical),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EpochReason {
    Initial = 1,
    ReplaceCloud = 2,
    Restore = 3,
    CollectionPackageImport = 4,
    ManualAnkiImport = 5,
}
impl TryFrom<u64> for EpochReason {
    type Error = CodecError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Initial),
            2 => Ok(Self::ReplaceCloud),
            3 => Ok(Self::Restore),
            4 => Ok(Self::CollectionPackageImport),
            5 => Ok(Self::ManualAnkiImport),
            _ => Err(CodecError::NonCanonical),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindCount {
    pub kind: Kind,
    pub live: u64,
}
impl KindCount {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut w = Writer::emitting();
        w.scalar(1, self.kind as u64, false)?;
        w.scalar(2, self.live, false)?;
        Ok(w.finish())
    }
    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut r = Reader::new(bytes);
        let mut previous = 0;
        let mut kind = None;
        let mut live = 0;
        while let Some((tag, value)) = r.next()? {
            if tag <= previous {
                return Err(CodecError::NonCanonical);
            }
            previous = tag;
            match tag {
                1 => kind = Some(Kind::try_from(value.integer()?)?),
                2 => live = value.integer()?,
                _ => return Err(CodecError::NonCanonical),
            }
        }
        let count = Self {
            kind: kind.ok_or(CodecError::InvalidKind)?,
            live,
        };
        if count.encode()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(count)
    }
}

/// K5.2 MGEpoch facts only. Keys and identifiers are opaque in Debug output.
/// `ver` is the collection schema and must be 18 for format 1.
#[derive(Clone, PartialEq, Eq)]
pub struct Epoch {
    pub epoch_id: [u8; 16],
    pub record_key: [u8; 32],
    pub crt: i64,
    pub ver: u32,
    pub state: EpochState,
    pub reason: EpochReason,
    pub published_at_ms: i64,
    /// Strict kind-code order, at most one entry per kind. K18.3 does not
    /// require all nine entries. Publishing has no entries; complete may be empty.
    pub counts: Vec<KindCount>,
}
impl std::fmt::Debug for Epoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Epoch")
    }
}
impl Epoch {
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        if self.ver != 18 || (self.state == EpochState::Publishing && !self.counts.is_empty()) {
            return Err(CodecError::NonCanonical);
        }
        if self.counts.len() > 9 {
            return Err(CodecError::NonCanonical);
        }
        let mut w = Writer::emitting();
        w.scalar(1, 1, false)?;
        w.bytes(2, &self.epoch_id, false)?;
        w.bytes(3, &self.record_key, false)?;
        w.scalar(4, self.crt as u64, false)?;
        w.scalar(5, u64::from(self.ver), false)?;
        w.scalar(6, self.state as u64, false)?;
        w.scalar(7, self.reason as u64, false)?;
        w.scalar(8, self.published_at_ms as u64, false)?;
        let mut previous = 0;
        for count in &self.counts {
            if count.kind as u8 <= previous {
                return Err(CodecError::NonCanonical);
            }
            previous = count.kind as u8;
            w.bytes(9, &count.encode()?, true)?;
        }
        Ok(w.finish())
    }

    /// Probe only the declared schema for K20's newer-schema refusal. This
    /// never establishes authority or makes a future body a supported epoch.
    pub fn schema_version(bytes: &[u8]) -> Result<u32, CodecError> {
        let mut reader = Reader::new(bytes);
        let mut previous = 0;
        while let Some((tag, value)) = reader.next()? {
            if tag <= previous { return Err(CodecError::NonCanonical); }
            previous = tag;
            if tag == 5 { return u32::try_from(value.integer()?).map_err(|_| CodecError::NonCanonical); }
        }
        Err(CodecError::NonCanonical)
    }
    pub fn decode(bytes: &[u8]) -> Result<DecodeOutcome<Self>, CodecError> {
        let format = control_format(bytes)?;
        if format > 1 {
            return Ok(DecodeOutcome::UnsupportedFormat(format));
        }
        let mut r = Reader::new(bytes);
        let mut previous = 0;
        let mut epoch_id = None;
        let mut record_key = None;
        let mut crt = 0;
        let mut ver = 0;
        let mut state = None;
        let mut reason = None;
        let mut published_at_ms = 0;
        let mut counts: Vec<KindCount> = Vec::new();
        while let Some((tag, value)) = r.next()? {
            if tag < previous || (tag == previous && tag != 9) {
                return Err(CodecError::NonCanonical);
            }
            previous = tag;
            match tag {
                1 => {
                    if value.integer()? != 1 {
                        return Err(CodecError::NonCanonical);
                    }
                }
                2 => epoch_id = Some(fixed(value.bytes()?)?),
                3 => record_key = Some(fixed(value.bytes()?)?),
                4 => crt = value.integer()? as i64,
                5 => ver = u32::try_from(value.integer()?).map_err(|_| CodecError::NonCanonical)?,
                6 => state = Some(EpochState::try_from(value.integer()?)?),
                7 => reason = Some(EpochReason::try_from(value.integer()?)?),
                8 => published_at_ms = value.integer()? as i64,
                9 => {
                    if state != Some(EpochState::Complete) || counts.len() == 9 {
                        return Err(CodecError::NonCanonical);
                    }
                    let count = KindCount::decode(value.bytes()?)?;
                    if counts.last().is_some_and(|last| last.kind >= count.kind) {
                        return Err(CodecError::NonCanonical);
                    }
                    counts.push(count);
                }
                _ => return Err(CodecError::NonCanonical),
            }
        }
        let epoch = Self {
            epoch_id: epoch_id.ok_or(CodecError::InvalidLength)?,
            record_key: record_key.ok_or(CodecError::InvalidLength)?,
            crt,
            ver,
            state: state.ok_or(CodecError::NonCanonical)?,
            reason: reason.ok_or(CodecError::NonCanonical)?,
            published_at_ms,
            counts,
        };
        if epoch.encode()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(DecodeOutcome::Supported(epoch))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MediaRoot {
    pub media_key: [u8; 32],
    pub created_at_ms: i64,
}
impl std::fmt::Debug for MediaRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MediaRoot")
    }
}
impl MediaRoot {
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut w = Writer::emitting();
        w.scalar(1, 1, false)?;
        w.bytes(2, &self.media_key, false)?;
        w.scalar(3, self.created_at_ms as u64, false)?;
        Ok(w.finish())
    }
    pub fn decode(bytes: &[u8]) -> Result<DecodeOutcome<Self>, CodecError> {
        let format = control_format(bytes)?;
        if format > 1 {
            return Ok(DecodeOutcome::UnsupportedFormat(format));
        }
        let mut r = Reader::new(bytes);
        let mut previous = 0;
        let mut media_key = None;
        let mut created_at_ms = 0;
        while let Some((tag, value)) = r.next()? {
            if tag <= previous {
                return Err(CodecError::NonCanonical);
            }
            previous = tag;
            match tag {
                1 => {
                    if value.integer()? != 1 {
                        return Err(CodecError::NonCanonical);
                    }
                }
                2 => media_key = Some(fixed(value.bytes()?)?),
                3 => created_at_ms = value.integer()? as i64,
                _ => return Err(CodecError::NonCanonical),
            }
        }
        let root = Self {
            media_key: media_key.ok_or(CodecError::InvalidLength)?,
            created_at_ms,
        };
        if root.encode()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(DecodeOutcome::Supported(root))
    }
}

fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], CodecError> {
    bytes.try_into().map_err(|_| CodecError::InvalidLength)
}

fn control_format(bytes: &[u8]) -> Result<u32, CodecError> {
    // Follow entity_codec's discriminator-only probe. Future formats may change
    // all subsequent fields; no V1 interpretation or allocation happens for them.
    let mut probe = Reader::new(bytes);
    loop {
        let (tag, value) = probe.next()?.ok_or(CodecError::NonCanonical)?;
        if tag == 1 {
            let format = u32::try_from(value.integer()?).map_err(|_| CodecError::NonCanonical)?;
            return if format == 0 {
                Err(CodecError::NonCanonical)
            } else if format == 1 && bytes.len() > MAX_CONTROL_BYTES {
                Err(CodecError::Oversize)
            } else {
                Ok(format)
            };
        }
    }
}

/// Compatibility wrapper: an initial, complete epoch, with unchanged V1 bytes.
pub fn epoch(epoch: [u8; 16], key: [u8; 32], crt: i64, at: i64, counts: &[(Kind, u64)]) -> Result<Vec<u8>, CodecError> {
    if counts.len() > 9 {
        return Err(CodecError::NonCanonical);
    }
    Epoch {
        epoch_id: epoch,
        record_key: key,
        crt,
        ver: 18,
        state: EpochState::Complete,
        reason: EpochReason::Initial,
        published_at_ms: at,
        counts: counts.iter().map(|&(kind, live)| KindCount { kind, live }).collect(),
    }
    .encode()
}

pub fn media_root(key: [u8; 32], at: i64) -> Result<Vec<u8>, CodecError> {
    MediaRoot {
        media_key: key,
        created_at_ms: at,
    }
    .encode()
}
