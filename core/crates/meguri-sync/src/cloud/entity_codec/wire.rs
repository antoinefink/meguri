//! Private wire primitives for K5.3; deliberately unrelated to tolerant model protobufs.
use super::{CodecError, MAX_PAYLOAD_BYTES};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, CodecError>;

pub(crate) struct Writer {
    bytes: Option<Vec<u8>>,
    hash: Option<Sha256>,
    pub len: usize,
}
impl Writer {
    pub fn counting() -> Self {
        Self {
            bytes: None,
            hash: None,
            len: 0,
        }
    }
    pub fn emitting() -> Self {
        Self::with_capacity(0)
    }
    pub fn with_capacity(len: usize) -> Self {
        Self {
            bytes: Some(Vec::with_capacity(len)),
            hash: None,
            len: 0,
        }
    }
    pub fn hashing() -> Self {
        Self {
            bytes: None,
            hash: Some(Sha256::new()),
            len: 0,
        }
    }
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.len = self.len.checked_add(bytes.len()).ok_or(CodecError::Oversize)?;
        if let Some(out) = &mut self.bytes {
            out.extend_from_slice(bytes);
        }
        if let Some(hash) = &mut self.hash {
            hash.update(bytes);
        }
        Ok(())
    }
    pub fn finish_hash(self) -> [u8; 32] {
        self.hash.expect("hash writer").finalize().into()
    }
    pub fn finish(self) -> Vec<u8> {
        self.bytes.unwrap_or_default()
    }
    pub fn varint(&mut self, mut value: u64) -> Result<()> {
        while value >= 128 {
            self.append(&[(value as u8 & 127) | 128])?;
            value >>= 7;
        }
        self.append(&[value as u8])
    }
    pub fn scalar(&mut self, tag: u32, value: u64, explicit: bool) -> Result<()> {
        if explicit || value != 0 {
            self.varint(u64::from(tag) << 3)?;
            self.varint(value)?;
        }
        Ok(())
    }
    pub fn bytes(&mut self, tag: u32, bytes: &[u8], explicit: bool) -> Result<()> {
        if explicit || !bytes.is_empty() {
            self.header(tag, bytes.len())?;
            self.append(bytes)?;
        }
        Ok(())
    }
    pub fn header(&mut self, tag: u32, len: usize) -> Result<()> {
        self.varint((u64::from(tag) << 3) | 2)?;
        self.varint(u64::try_from(len).map_err(|_| CodecError::Oversize)?)
    }
    pub fn double(&mut self, tag: u32, value: f64) -> Result<()> {
        self.varint((u64::from(tag) << 3) | 1)?;
        self.append(&value.to_bits().to_le_bytes())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32,
}
impl<'a> Value<'a> {
    pub fn integer(self) -> Result<u64> {
        if let Self::Varint(value) = self {
            Ok(value)
        } else {
            Err(CodecError::NonCanonical)
        }
    }
    pub fn bytes(self) -> Result<&'a [u8]> {
        if let Self::Bytes(value) = self {
            Ok(value)
        } else {
            Err(CodecError::NonCanonical)
        }
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(len).ok_or(CodecError::NonCanonical)?;
        let bytes = self.bytes.get(self.pos..end).ok_or(CodecError::NonCanonical)?;
        self.pos = end;
        Ok(bytes)
    }
    fn varint(&mut self) -> Result<u64> {
        let mut value = 0;
        for shift in (0..70).step_by(7) {
            let byte = self.take(1)?[0];
            if shift == 63 && byte > 1 {
                return Err(CodecError::NonCanonical);
            }
            value |= u64::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(CodecError::NonCanonical)
    }
    pub fn next(&mut self) -> Result<Option<(u32, Value<'a>)>> {
        if self.pos == self.bytes.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let tag = u32::try_from(key >> 3).map_err(|_| CodecError::NonCanonical)?;
        if tag == 0 || tag > 0x1fff_ffff {
            return Err(CodecError::NonCanonical);
        }
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => Value::Fixed64(u64::from_le_bytes(
                self.take(8)?.try_into().map_err(|_| CodecError::NonCanonical)?,
            )),
            2 => {
                let len = usize::try_from(self.varint()?).map_err(|_| CodecError::NonCanonical)?;
                Value::Bytes(self.take(len)?)
            }
            5 => {
                self.take(4)?;
                Value::Fixed32
            }
            _ => return Err(CodecError::NonCanonical),
        };
        Ok(Some((tag, value)))
    }
}

pub(crate) trait Message: Sized {
    fn write(&self, out: &mut Writer, row: bool) -> Result<()>;
    fn read(bytes: &[u8]) -> Result<Self>;
    fn encoded_len(&self, row: bool) -> Result<usize> {
        let mut out = Writer::counting();
        self.write(&mut out, row)?;
        if out.len as u64 > MAX_PAYLOAD_BYTES {
            return Err(CodecError::Oversize);
        }
        Ok(out.len)
    }
    fn encode(&self, row: bool) -> Result<Vec<u8>> {
        let len = self.encoded_len(row)?;
        let mut out = Writer::with_capacity(len);
        self.write(&mut out, row)?;
        Ok(out.finish())
    }
    fn digest(&self, row: bool) -> Result<[u8; 32]> {
        self.encoded_len(row)?;
        let mut out = Writer::hashing();
        self.write(&mut out, row)?;
        Ok(out.finish_hash())
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > MAX_PAYLOAD_BYTES {
            return Err(CodecError::Oversize);
        }
        let value = Self::read(bytes)?;
        if value.encode(false)? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(value)
    }
}

pub(crate) trait FieldValue: Default {
    const REQUIRED: bool = false;
    const REPEATED: bool = false;
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()>;
    fn read_field(&mut self, value: Value<'_>) -> Result<()>;
}
impl FieldValue for i64 {
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
        out.scalar(tag, *self as u64, false)
    }
    fn read_field(&mut self, value: Value<'_>) -> Result<()> {
        *self = value.integer()? as i64;
        Ok(())
    }
}
impl FieldValue for u64 {
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
        out.scalar(tag, *self, false)
    }
    fn read_field(&mut self, value: Value<'_>) -> Result<()> {
        *self = value.integer()?;
        Ok(())
    }
}
impl FieldValue for Vec<u8> {
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
        out.bytes(tag, self, false)
    }
    fn read_field(&mut self, value: Value<'_>) -> Result<()> {
        *self = value.bytes()?.to_vec();
        Ok(())
    }
}
impl FieldValue for Option<Vec<u8>> {
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
        if let Some(value) = self {
            out.bytes(tag, value, true)?;
        }
        Ok(())
    }
    fn read_field(&mut self, value: Value<'_>) -> Result<()> {
        *self = Some(value.bytes()?.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OverCap;
    impl Message for OverCap {
        fn write(&self, out: &mut Writer, _row: bool) -> Result<()> {
            let chunk = [0; 1024];
            for _ in 0..=MAX_PAYLOAD_BYTES / 1024 {
                out.bytes(1, &chunk, true)?;
            }
            Ok(())
        }
        fn read(_bytes: &[u8]) -> Result<Self> {
            Err(CodecError::NonCanonical)
        }
    }

    #[test]
    fn counting_hashing_and_encoding_keep_the_same_cap() {
        for row in [false, true] {
            assert_eq!(OverCap.encoded_len(row), Err(CodecError::Oversize));
            assert_eq!(OverCap.encode(row), Err(CodecError::Oversize));
            assert_eq!(OverCap.digest(row), Err(CodecError::Oversize));
        }
    }
}
