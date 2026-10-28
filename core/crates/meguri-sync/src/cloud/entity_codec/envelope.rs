use super::wire::{Reader, Writer};
use super::{
    CodecError, INLINE_ENVELOPE_BYTES, Kind, MAX_PAYLOAD_BYTES, Payload, entity_name, sha256, validate_payload_size,
};

type Result<T> = std::result::Result<T, CodecError>;

/// Supplied metadata only; revisions and history are never inferred here.
#[derive(Clone, PartialEq, Eq)]
pub struct EntityFields {
    pub kind: Kind,
    pub key: Vec<u8>,
    pub origin: [u8; 16],
    pub erev: u64,
    pub generation: Option<[u8; 16]>,
    pub tombstone: bool,
    pub deleted_at_ms: i64,
    pub writer: [u8; 16],
    pub base_sha256: Option<[u8; 32]>,
}
impl std::fmt::Debug for EntityFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EntityFields")
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Entity {
    pub fields: EntityFields,
    pub payload: Option<Vec<u8>>,
    pub payload_sha256: [u8; 32],
    pub payload_size: u64,
}
impl std::fmt::Debug for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Entity")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeOutcome {
    Supported(Entity),
    UnsupportedFormat(u32),
}

impl Entity {
    fn write(&self, out: &mut Writer, payload: Option<&[u8]>) -> Result<()> {
        self.fields.kind.validate_key(&self.fields.key)?;
        validate_payload_size(self.payload_size)?;
        if let Some(bytes) = payload {
            validate_payload_size(bytes.len() as u64)?;
        }
        out.scalar(1, 1, false)?;
        out.scalar(2, self.fields.kind as u64, false)?;
        out.bytes(3, &self.fields.key, false)?;
        out.bytes(4, &self.fields.origin, false)?;
        out.scalar(5, self.fields.erev, false)?;
        if let Some(value) = &self.fields.generation {
            out.bytes(6, value, true)?;
        }
        out.scalar(7, u64::from(self.fields.tombstone), false)?;
        out.scalar(8, self.fields.deleted_at_ms as u64, false)?;
        if let Some(value) = payload {
            out.bytes(9, value, true)?;
        }
        out.bytes(10, &self.payload_sha256, false)?;
        out.scalar(11, self.payload_size, false)?;
        out.bytes(12, &self.fields.writer, false)?;
        if let Some(value) = &self.fields.base_sha256 {
            out.bytes(13, value, true)?;
        }
        Ok(())
    }
    /// Sender choice uses the entire encoded envelope, including optional metadata.
    /// When `payload` is absent in the result, the caller retains the asset bytes.
    pub fn build(fields: EntityFields, payload: &[u8]) -> Result<Self> {
        validate_payload_size(payload.len() as u64)?;
        let mut entity = Self {
            fields,
            payload: None,
            payload_sha256: sha256(payload),
            payload_size: payload.len() as u64,
        };
        let mut count = Writer::counting();
        entity.write(&mut count, Some(payload))?;
        if count.len <= INLINE_ENVELOPE_BYTES {
            entity.payload = Some(payload.to_vec());
        }
        Ok(entity)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Writer::emitting();
        self.write(&mut out, self.payload.as_deref())?;
        Ok(out.finish())
    }
    pub fn decode(bytes: &[u8]) -> Result<DecodeOutcome> {
        // Inspect only the discriminator before selecting a schema. In particular,
        // future fields and payloads are not subjected to format-1 canonical rules.
        let mut probe = Reader::new(bytes);
        let format = loop {
            let (tag, value) = probe.next()?.ok_or(CodecError::NonCanonical)?;
            if tag == 1 {
                break u32::try_from(value.integer()?).map_err(|_| CodecError::NonCanonical)?;
            }
        };
        if format > 1 {
            return Ok(DecodeOutcome::UnsupportedFormat(format));
        }
        if format != 1 {
            return Err(CodecError::NonCanonical);
        }
        // An envelope can add metadata to a maximum-sized inline payload. Bound
        // allocations by its actual supplied buffer; declared lengths never reserve.
        let mut input = Reader::new(bytes);
        let mut seen = 0_u16;
        let mut previous = 0;
        let mut kind = None;
        let mut key = Vec::new();
        let mut origin = None;
        let mut erev = 0;
        let mut generation = None;
        let mut tombstone = false;
        let mut deleted_at_ms = 0;
        let mut payload = None;
        let mut payload_sha256 = None;
        let mut payload_size = 0;
        let mut writer = None;
        let mut base_sha256 = None;
        while let Some((tag, value)) = input.next()? {
            if tag <= previous || tag > 13 {
                return Err(CodecError::NonCanonical);
            }
            previous = tag;
            seen |= 1 << tag;
            match tag {
                1 => {
                    if value.integer()? != 1 {
                        return Err(CodecError::NonCanonical);
                    }
                }
                2 => kind = Some(Kind::try_from(value.integer()?)?),
                3 => key = value.bytes()?.to_vec(),
                4 => origin = Some(fixed(value.bytes()?)?),
                5 => erev = value.integer()?,
                6 => generation = Some(fixed(value.bytes()?)?),
                7 => tombstone = value.integer()? != 0,
                8 => deleted_at_ms = value.integer()? as i64,
                9 => {
                    let bytes = value.bytes()?;
                    validate_payload_size(bytes.len() as u64)?;
                    payload = Some(bytes.to_vec());
                }
                10 => payload_sha256 = Some(fixed(value.bytes()?)?),
                11 => {
                    payload_size = value.integer()?;
                    validate_payload_size(payload_size)?;
                }
                12 => writer = Some(fixed(value.bytes()?)?),
                13 => base_sha256 = Some(fixed(value.bytes()?)?),
                _ => return Err(CodecError::NonCanonical),
            }
        }
        if seen & (1 << 1) == 0 {
            return Err(CodecError::NonCanonical);
        }
        let entity = Self {
            fields: EntityFields {
                kind: kind.ok_or(CodecError::NonCanonical)?,
                key,
                origin: origin.ok_or(CodecError::InvalidLength)?,
                erev,
                generation,
                tombstone,
                deleted_at_ms,
                writer: writer.ok_or(CodecError::InvalidLength)?,
                base_sha256,
            },
            payload,
            payload_sha256: payload_sha256.ok_or(CodecError::InvalidLength)?,
            payload_size,
        };
        if entity.encode()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(DecodeOutcome::Supported(entity))
    }
}
fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N]> {
    bytes.try_into().map_err(|_| CodecError::InvalidLength)
}

/// Classification of facts supplied by the asset reader; this code opens no files.
#[derive(Clone, Copy)]
pub enum AssetInput<'a> {
    Absent,
    Unreadable,
    Bytes(&'a [u8]),
}
impl std::fmt::Debug for AssetInput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Absent => "AssetInput::Absent",
            Self::Unreadable => "AssetInput::Unreadable",
            Self::Bytes(_) => "AssetInput::Bytes",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetProblem {
    Absent,
    Unreadable,
    Integrity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Encoding(CodecError),
    Name,
    InlineIntegrity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveError {
    Refused(Refusal),
    RetryableAsset(AssetProblem),
    UnsupportedFormat(u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub entity: Entity,
    pub payload: Option<Payload>,
}

/// Checks pure record facts only. Empty bytes remain representable without making
/// any claim about which tombstones should carry a previous payload.
pub fn receive(
    record_key: &[u8; 32],
    record_name: &str,
    envelope: &[u8],
    asset: AssetInput<'_>,
) -> std::result::Result<Received, ReceiveError> {
    let refuse = |error| ReceiveError::Refused(Refusal::Encoding(error));
    let entity = match Entity::decode(envelope).map_err(refuse)? {
        DecodeOutcome::UnsupportedFormat(format) => return Err(ReceiveError::UnsupportedFormat(format)),
        DecodeOutcome::Supported(entity) => entity,
    };
    if entity_name(record_key, entity.fields.kind, &entity.fields.key).map_err(refuse)? != record_name {
        return Err(ReceiveError::Refused(Refusal::Name));
    }
    let inline = entity.payload.is_some();
    let bytes = if let Some(bytes) = entity.payload.as_deref() {
        bytes
    } else {
        match asset {
            AssetInput::Absent => return Err(ReceiveError::RetryableAsset(AssetProblem::Absent)),
            AssetInput::Unreadable => return Err(ReceiveError::RetryableAsset(AssetProblem::Unreadable)),
            AssetInput::Bytes(bytes) => bytes,
        }
    };
    if bytes.len() as u64 > MAX_PAYLOAD_BYTES
        || bytes.len() as u64 != entity.payload_size
        || sha256(bytes) != entity.payload_sha256
    {
        return Err(if inline {
            ReceiveError::Refused(Refusal::InlineIntegrity)
        } else {
            ReceiveError::RetryableAsset(AssetProblem::Integrity)
        });
    }
    let payload = if bytes.is_empty() {
        None
    } else {
        Some(Payload::decode(entity.fields.kind, bytes).map_err(refuse)?)
    };
    Ok(Received { entity, payload })
}
