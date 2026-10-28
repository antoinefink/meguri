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
pub fn epoch(epoch: [u8; 16], key: [u8; 32], crt: i64, at: i64, counts: &[(Kind, u64)]) -> Result<Vec<u8>, CodecError> {
    let mut w = Writer::emitting();
    w.scalar(1, 1, false)?;
    w.bytes(2, &epoch, false)?;
    w.bytes(3, &key, false)?;
    w.scalar(4, crt as u64, false)?;
    w.scalar(5, 18, false)?;
    w.scalar(6, 2, false)?;
    w.scalar(7, 1, false)?;
    w.scalar(8, at as u64, false)?;
    let mut previous = 0;
    for (kind, count) in counts {
        if *kind as u8 <= previous {
            return Err(CodecError::NonCanonical);
        }
        previous = *kind as u8;
        let mut c = Writer::emitting();
        c.scalar(1, *kind as u64, false)?;
        c.scalar(2, *count, false)?;
        w.bytes(9, &c.finish(), true)?;
    }
    Ok(w.finish())
}
pub fn media_root(key: [u8; 32], at: i64) -> Result<Vec<u8>, CodecError> {
    let mut w = Writer::emitting();
    w.scalar(1, 1, false)?;
    w.bytes(2, &key, false)?;
    w.scalar(3, at as u64, false)?;
    Ok(w.finish())
}
