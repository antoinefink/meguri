//! Protobuf record blobs (`specs/model/protobuf-records.md`).
//!
//! The messages of P6 are written by hand from the spec, with a small wire
//! codec of Meguri's own: decoding follows P2 and refuses with the reasons of
//! P9, encoding follows P3, and every message level keeps the fields it does
//! not declare (P4). [`ProtoBlob`] keeps the bytes a record was read with and
//! writes them back unless the content changed (P5).

pub mod deck;
pub mod deck_config;
pub mod notetype;
mod unknown;

use std::borrow::Cow;
use std::ops::{Deref, DerefMut};

pub use unknown::{UnknownField, UnknownFields};

use crate::error::{BlobColumn, BlobError, ModelError};

type Result<T> = std::result::Result<T, BlobError>;

/// P2 wire types; 6 and 7 are invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WireType {
    Varint,
    Fixed64,
    Len,
    StartGroup,
    EndGroup,
    Fixed32,
}

impl WireType {
    fn bits(self) -> u64 {
        match self {
            WireType::Varint => 0,
            WireType::Fixed64 => 1,
            WireType::Len => 2,
            WireType::StartGroup => 3,
            WireType::EndGroup => 4,
            WireType::Fixed32 => 5,
        }
    }
}

/// Reads one message's bytes: the whole blob, or the payload of a nested
/// message.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    /// A nested payload: a group still open at its end is a bad group; at the
    /// end of the whole blob it is a truncation (P9).
    nested: bool,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8], nested: bool) -> Reader<'a> {
        Reader { buf, pos: 0, nested }
    }

    fn done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let rest = &self.buf[self.pos..];
        if rest.len() < n {
            return Err(BlobError::Truncated);
        }
        self.pos += n;
        Ok(&rest[..n])
    }

    /// At most 10 bytes; bits beyond 64 are dropped.
    fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for i in 0..10 {
            let byte = *self.buf.get(self.pos).ok_or(BlobError::Truncated)?;
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << (7 * i);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(BlobError::BadVarint)
    }

    fn key(&mut self) -> Result<(u32, WireType)> {
        let key = self.varint()?;
        let key = u32::try_from(key).map_err(|_| BlobError::BadVarint)?;
        let wire = match key & 7 {
            0 => WireType::Varint,
            1 => WireType::Fixed64,
            2 => WireType::Len,
            3 => WireType::StartGroup,
            4 => WireType::EndGroup,
            5 => WireType::Fixed32,
            _ => return Err(BlobError::BadWireType),
        };
        let field = key >> 3;
        if field == 0 {
            return Err(BlobError::BadFieldNumber);
        }
        Ok((field, wire))
    }

    fn payload(&mut self) -> Result<&'a [u8]> {
        let len = self.varint()?;
        let len = usize::try_from(len).map_err(|_| BlobError::Truncated)?;
        self.take(len)
    }

    /// Skips the value of an undeclared field.
    fn skip(&mut self, field: u32, wire: WireType) -> Result<()> {
        match wire {
            WireType::Varint => self.varint().map(drop),
            WireType::Fixed64 => self.take(8).map(drop),
            WireType::Len => self.payload().map(drop),
            WireType::Fixed32 => self.take(4).map(drop),
            WireType::StartGroup => self.skip_group(field),
            WireType::EndGroup => Err(BlobError::BadGroup),
        }
    }

    /// Through the group-end key matching `field`, nested groups included,
    /// without recursion.
    fn skip_group(&mut self, field: u32) -> Result<()> {
        let mut open = vec![field];
        while let Some(&current) = open.last() {
            if self.done() {
                return Err(if self.nested {
                    BlobError::BadGroup
                } else {
                    BlobError::Truncated
                });
            }
            let (field, wire) = self.key()?;
            match wire {
                WireType::EndGroup if field == current => {
                    open.pop();
                }
                WireType::EndGroup => return Err(BlobError::BadGroup),
                WireType::StartGroup => open.push(field),
                other => self.skip(field, other)?,
            }
        }
        Ok(())
    }

    fn expect(wire: WireType, accepted: WireType) -> Result<()> {
        if wire == accepted {
            Ok(())
        } else {
            Err(BlobError::BadWireType)
        }
    }

    pub(crate) fn bool(&mut self, wire: WireType) -> Result<bool> {
        Self::expect(wire, WireType::Varint)?;
        Ok(self.varint()? != 0)
    }

    /// `int32` and enums: the low 32 bits as a signed integer.
    pub(crate) fn int32(&mut self, wire: WireType) -> Result<i32> {
        Self::expect(wire, WireType::Varint)?;
        Ok(self.varint()? as u32 as i32)
    }

    pub(crate) fn uint32(&mut self, wire: WireType) -> Result<u32> {
        Self::expect(wire, WireType::Varint)?;
        Ok(self.varint()? as u32)
    }

    pub(crate) fn int64(&mut self, wire: WireType) -> Result<i64> {
        Self::expect(wire, WireType::Varint)?;
        Ok(self.varint()? as i64)
    }

    fn fixed32(&mut self) -> Result<f32> {
        let bytes = self.take(4)?;
        Ok(f32::from_bits(u32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])))
    }

    /// Bit for bit, NaN payloads included.
    pub(crate) fn float(&mut self, wire: WireType) -> Result<f32> {
        Self::expect(wire, WireType::Fixed32)?;
        self.fixed32()
    }

    pub(crate) fn string(&mut self, wire: WireType) -> Result<String> {
        Self::expect(wire, WireType::Len)?;
        let bytes = self.payload()?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| BlobError::BadUtf8)
    }

    pub(crate) fn bytes(&mut self, wire: WireType) -> Result<Vec<u8>> {
        Self::expect(wire, WireType::Len)?;
        Ok(self.payload()?.to_vec())
    }

    /// A repeated `float`: packed (consecutive 4-byte values) or one value.
    pub(crate) fn floats(&mut self, wire: WireType, out: &mut Vec<f32>) -> Result<()> {
        match wire {
            WireType::Fixed32 => out.push(self.fixed32()?),
            WireType::Len => {
                let mut packed = Reader::new(self.payload()?, true);
                while !packed.done() {
                    out.push(packed.fixed32()?);
                }
            }
            _ => return Err(BlobError::BadWireType),
        }
        Ok(())
    }

    /// A repeated `uint32`: packed (consecutive varints) or one value.
    pub(crate) fn uint32s(&mut self, wire: WireType, out: &mut Vec<u32>) -> Result<()> {
        match wire {
            WireType::Varint => out.push(self.varint()? as u32),
            WireType::Len => {
                let mut packed = Reader::new(self.payload()?, true);
                while !packed.done() {
                    out.push(packed.varint()? as u32);
                }
            }
            _ => return Err(BlobError::BadWireType),
        }
        Ok(())
    }

    /// A nested message, merged into `message` (P2).
    pub(crate) fn message<M: Decode>(&mut self, wire: WireType, message: &mut M) -> Result<()> {
        Self::expect(wire, WireType::Len)?;
        let mut nested = Reader::new(self.payload()?, true);
        merge(message, &mut nested)
    }
}

/// A message that decodes from the wire.
pub(crate) trait Decode: Default {
    /// Reads one occurrence of a declared field; `Ok(false)` (nothing read)
    /// when the message does not declare `field`.
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool>;

    fn unknown_mut(&mut self) -> &mut UnknownFields;
}

/// A message that encodes to the wire.
pub(crate) trait Encode {
    /// Writes the declared fields in ascending field-number order.
    fn encode_fields(&self, w: &mut Writer<'_>);

    fn unknown(&self) -> &UnknownFields;
}

/// Reads fields until the reader's end, merging them into `message`.
pub(crate) fn merge<M: Decode>(message: &mut M, r: &mut Reader<'_>) -> Result<()> {
    while !r.done() {
        let start = r.pos;
        let (field, wire) = r.key()?;
        if !message.merge_field(field, wire, r)? {
            r.skip(field, wire)?;
            let bytes = &r.buf[start..r.pos];
            message.unknown_mut().push(field, bytes);
        }
    }
    Ok(())
}

pub(crate) fn decode_message<M: Decode>(bytes: &[u8]) -> Result<M> {
    let mut message = M::default();
    merge(&mut message, &mut Reader::new(bytes, false))?;
    Ok(message)
}

/// How a [`Writer`] lays out a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// P3 and P4: what Meguri stores.
    Canonical,
    /// Every implicit field written even at its default, floats bit for bit,
    /// unknown fields last in read order: equal bytes mean equal content (P5).
    Exact,
    /// As `Exact`, with what `Canonical` normalizes made equal: -0.0 in
    /// implicit floats reads as 0.0, and unknown fields sorted by number.
    Normalized,
}

pub(crate) struct Writer<'u> {
    out: Vec<u8>,
    mode: Mode,
    unknown: Vec<&'u UnknownField>,
    next: usize,
}

impl<'u> Writer<'u> {
    fn new(unknown: &'u UnknownFields, mode: Mode) -> Writer<'u> {
        let unknown = match mode {
            Mode::Canonical | Mode::Normalized => unknown.by_number(),
            Mode::Exact => unknown.iter().collect(),
        };
        Writer {
            out: Vec::new(),
            mode,
            unknown,
            next: 0,
        }
    }

    fn all_fields(&self) -> bool {
        self.mode != Mode::Canonical
    }

    fn varint(&mut self, value: u64) {
        put_varint(&mut self.out, value);
    }

    /// Writes the key, after the unknown fields numbered below `field` (P4).
    fn key(&mut self, field: u32, wire: WireType) {
        if self.mode == Mode::Canonical {
            while let Some(unknown) = self.unknown.get(self.next) {
                if unknown.number() >= field {
                    break;
                }
                self.out.extend_from_slice(unknown.bytes());
                self.next += 1;
            }
        }
        self.varint((u64::from(field) << 3) | wire.bits());
    }

    fn len_delimited(&mut self, field: u32, payload: &[u8]) {
        self.key(field, WireType::Len);
        self.varint(payload.len() as u64);
        self.out.extend_from_slice(payload);
    }

    pub(crate) fn bool(&mut self, field: u32, value: bool) {
        if value || self.all_fields() {
            self.key(field, WireType::Varint);
            self.varint(u64::from(value));
        }
    }

    /// Negative values are sign-extended to 64 bits (10 bytes).
    pub(crate) fn int32(&mut self, field: u32, value: i32) {
        if value != 0 || self.all_fields() {
            self.key(field, WireType::Varint);
            self.varint(i64::from(value) as u64);
        }
    }

    pub(crate) fn uint32(&mut self, field: u32, value: u32) {
        if value != 0 || self.all_fields() {
            self.key(field, WireType::Varint);
            self.varint(u64::from(value));
        }
    }

    pub(crate) fn int64(&mut self, field: u32, value: i64) {
        if value != 0 || self.all_fields() {
            self.key(field, WireType::Varint);
            self.varint(value as u64);
        }
    }

    /// An implicit float is left out when it equals 0.0, so -0.0 is too.
    pub(crate) fn float(&mut self, field: u32, value: f32) {
        if value != 0.0 || self.all_fields() {
            let value = if self.mode == Mode::Normalized && value == 0.0 {
                0.0
            } else {
                value
            };
            self.key(field, WireType::Fixed32);
            self.out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }

    pub(crate) fn string(&mut self, field: u32, value: &str) {
        self.bytes(field, value.as_bytes());
    }

    pub(crate) fn bytes(&mut self, field: u32, value: &[u8]) {
        if !value.is_empty() || self.all_fields() {
            self.len_delimited(field, value);
        }
    }

    pub(crate) fn opt_int64(&mut self, field: u32, value: Option<i64>) {
        if let Some(value) = value {
            self.key(field, WireType::Varint);
            self.varint(value as u64);
        }
    }

    pub(crate) fn opt_uint32(&mut self, field: u32, value: Option<u32>) {
        if let Some(value) = value {
            self.key(field, WireType::Varint);
            self.varint(u64::from(value));
        }
    }

    /// Explicit presence: written whenever set, -0.0 included.
    pub(crate) fn opt_float(&mut self, field: u32, value: Option<f32>) {
        if let Some(value) = value {
            self.key(field, WireType::Fixed32);
            self.out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }

    /// A non-empty list is written once, packed; an empty one is left out.
    pub(crate) fn floats(&mut self, field: u32, values: &[f32]) {
        if !values.is_empty() {
            let payload: Vec<u8> = values.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect();
            self.len_delimited(field, &payload);
        }
    }

    pub(crate) fn uint32s(&mut self, field: u32, values: &[u32]) {
        if !values.is_empty() {
            let mut payload = Vec::new();
            for value in values {
                put_varint(&mut payload, u64::from(*value));
            }
            self.len_delimited(field, &payload);
        }
    }

    /// A present message field, written even when all its fields are defaults.
    pub(crate) fn message<M: Encode>(&mut self, field: u32, message: &M) {
        let payload = encode_with(message, self.mode);
        self.len_delimited(field, &payload);
    }

    fn finish(mut self) -> Vec<u8> {
        for unknown in &self.unknown[self.next..] {
            self.out.extend_from_slice(unknown.bytes());
        }
        self.out
    }
}

/// Fewest bytes, 7 bits each, low groups first.
fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

pub(crate) fn encode_with<M: Encode>(message: &M, mode: Mode) -> Vec<u8> {
    let mut writer = Writer::new(message.unknown(), mode);
    message.encode_fields(&mut writer);
    writer.finish()
}

/// The message of one blob column (P1).
pub trait BlobMessage: Sized + Clone {
    const COLUMN: BlobColumn;

    /// P2: the whole blob, or the reason it is invalid (P9).
    fn decode(bytes: &[u8]) -> std::result::Result<Self, ModelError>;

    /// P3 and P4: the bytes Anki would give this content, unknown fields
    /// placed by number.
    fn encode(&self) -> Vec<u8>;

    /// P5: known values (floats bit for bit) and unknown fields (in read
    /// order) all equal.
    fn same_content(&self, other: &Self) -> bool;

    /// Equal once encoded: `same_content` except that -0.0 in an implicit
    /// float equals 0.0 and unknown fields compare sorted by number (P3, P4).
    fn equivalent(&self, other: &Self) -> bool;
}

/// Implements [`BlobMessage`] for a message type that decodes and encodes
/// itself.
macro_rules! blob_message {
    ($message:ty, $column:expr) => {
        impl $crate::proto::BlobMessage for $message {
            const COLUMN: $crate::error::BlobColumn = $column;

            fn decode(bytes: &[u8]) -> ::std::result::Result<Self, $crate::error::ModelError> {
                $crate::proto::decode_message(bytes).map_err(|reason| $crate::error::ModelError::Blob {
                    column: $column,
                    reason,
                })
            }

            fn encode(&self) -> Vec<u8> {
                $crate::proto::encode_with(self, $crate::proto::Mode::Canonical)
            }

            fn same_content(&self, other: &Self) -> bool {
                $crate::proto::encode_with(self, $crate::proto::Mode::Exact)
                    == $crate::proto::encode_with(other, $crate::proto::Mode::Exact)
            }

            fn equivalent(&self, other: &Self) -> bool {
                $crate::proto::encode_with(self, $crate::proto::Mode::Normalized)
                    == $crate::proto::encode_with(other, $crate::proto::Mode::Normalized)
            }
        }
    };
}
pub(crate) use blob_message;

/// A blob column's message with the bytes it was read with (P5).
#[derive(Clone, Debug)]
pub struct ProtoBlob<M> {
    message: M,
    original: Option<Box<[u8]>>,
}

impl<M: BlobMessage> ProtoBlob<M> {
    /// A record Meguri creates: it has no original bytes.
    pub fn new(message: M) -> ProtoBlob<M> {
        ProtoBlob {
            message,
            original: None,
        }
    }

    pub fn decode(bytes: &[u8]) -> std::result::Result<ProtoBlob<M>, ModelError> {
        Ok(ProtoBlob {
            message: M::decode(bytes)?,
            original: Some(bytes.into()),
        })
    }

    /// The bytes the blob was read with, if it was read.
    pub fn original(&self) -> Option<&[u8]> {
        self.original.as_deref()
    }

    pub fn into_message(self) -> M {
        self.message
    }

    /// The bytes to store: the original bytes when the content is unchanged,
    /// even if they are not canonical; otherwise the canonical encoding.
    pub fn to_bytes(&self) -> Cow<'_, [u8]> {
        if let Some(original) = &self.original
            && let Ok(read) = M::decode(original)
            && read.same_content(&self.message)
        {
            return Cow::Borrowed(original);
        }
        Cow::Owned(self.message.encode())
    }
}

impl<M> Deref for ProtoBlob<M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.message
    }
}

impl<M> DerefMut for ProtoBlob<M> {
    fn deref_mut(&mut self) -> &mut M {
        &mut self.message
    }
}
