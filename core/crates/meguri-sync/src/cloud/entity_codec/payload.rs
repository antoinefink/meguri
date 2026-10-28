//! K6.1 values. Bytes retain the stored TEXT/BLOB representation without repair.
use super::wire::{FieldValue, Message, Reader, Value, Writer};
use super::{CodecError, Kind, sha256};

type Result<T> = std::result::Result<T, CodecError>;

/// SQLite storage class and raw value. REAL equality compares all IEEE-754 bits.
#[derive(Clone)]
pub enum Stored {
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
impl Default for Stored {
    fn default() -> Self {
        Self::Integer(0)
    }
}
impl PartialEq for Stored {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Real(a), Self::Real(b)) => a.to_bits() == b.to_bits(),
            (Self::Text(a), Self::Text(b)) | (Self::Blob(a), Self::Blob(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for Stored {}
impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Integer(_) => "Stored::Integer",
            Self::Real(_) => "Stored::Real",
            Self::Text(_) => "Stored::Text",
            Self::Blob(_) => "Stored::Blob",
        })
    }
}
impl Stored {
    fn write_member(&self, out: &mut Writer) -> Result<()> {
        match self {
            Self::Integer(value) => out.scalar(1, *value as u64, true),
            Self::Real(value) => out.double(2, *value),
            Self::Text(value) => out.bytes(3, value, true),
            Self::Blob(value) => out.bytes(4, value, true),
        }
    }
}
impl FieldValue for Stored {
    const REQUIRED: bool = true;
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
        let mut count = Writer::counting();
        self.write_member(&mut count)?;
        out.header(tag, count.len)?;
        self.write_member(out)
    }
    fn read_field(&mut self, value: Value<'_>) -> Result<()> {
        let mut input = Reader::new(value.bytes()?);
        *self = match input.next()?.ok_or(CodecError::NonCanonical)? {
            (1, value) => Self::Integer(value.integer()? as i64),
            (2, Value::Fixed64(value)) => Self::Real(f64::from_bits(value)),
            (3, value) => Self::Text(value.bytes()?.to_vec()),
            (4, value) => Self::Blob(value.bytes()?.to_vec()),
            _ => return Err(CodecError::NonCanonical),
        };
        if input.next()?.is_some() {
            return Err(CodecError::NonCanonical);
        }
        Ok(())
    }
}
impl<const N: usize> FieldValue for [u8; N]
where
    [u8; N]: Default,
{
    fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
        out.bytes(tag, self, false)
    }
    fn read_field(&mut self, value: Value<'_>) -> Result<()> {
        *self = value.bytes()?.try_into().map_err(|_| CodecError::InvalidLength)?;
        Ok(())
    }
    // These fixed bytes are nonempty even when every byte is zero.
    const REQUIRED: bool = true;
}

// A deliberately local schema table: the last boolean marks only K6.1 Meguri fields.
macro_rules! message {
    ($name:ident { $($tag:literal => $field:ident : $ty:ty, $meguri:literal;)* }) => {
        #[derive(Clone, Default, PartialEq, Eq)]
        pub struct $name { $(pub $field: $ty,)* }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(stringify!($name)) }
        }
        impl Message for $name {
            fn write(&self, out: &mut Writer, row: bool) -> Result<()> {
                $(if !(row && $meguri) { self.$field.write_field($tag, out)?; })*
                Ok(())
            }
            fn read(bytes: &[u8]) -> Result<Self> {
                let mut result = Self::default();
                let mut seen = 0_u32;
                let mut input = Reader::new(bytes);
                let mut previous = 0;
                while let Some((tag, value)) = input.next()? {
                    if tag < previous { return Err(CodecError::NonCanonical); }
                    previous = tag;
                    match tag {
                        $($tag => {
                            if seen & (1 << $tag) != 0 && !<$ty as FieldValue>::REPEATED { return Err(CodecError::NonCanonical); }
                            seen |= 1 << $tag;
                            result.$field.read_field(value)?;
                        },)*
                        _ => return Err(CodecError::NonCanonical),
                    }
                }
                $(if <$ty as FieldValue>::REQUIRED && seen & (1 << $tag) == 0 { return Err(CodecError::NonCanonical); })*
                Ok(result)
            }
        }
    };
}

message!(Note {
    1 => id: i64, false;
    2 => guid: Vec<u8>, false;
    3 => mid: i64, false;
    4 => modified: i64, false;
    5 => tags: Vec<u8>, false;
    6 => flds: Vec<u8>, false;
    7 => sfld: Stored, false;
    8 => csum: i64, false;
    9 => flags: i64, false;
    10 => data: Vec<u8>, false;
    11 => nt_struct_gen: u64, true;
});
message!(Card {
    1 => id: i64, false;
    2 => nid: i64, false;
    3 => did: i64, false;
    4 => ord: i64, false;
    5 => modified: i64, false;
    6 => card_type: i64, false;
    7 => queue: i64, false;
    8 => due: Stored, false;
    9 => ivl: i64, false;
    10 => factor: i64, false;
    11 => reps: i64, false;
    12 => lapses: i64, false;
    13 => left: i64, false;
    14 => odue: Stored, false;
    15 => odid: i64, false;
    16 => flags: i64, false;
    17 => data: Stored, false;
    18 => last_review_id: i64, true;
    19 => nt_struct_gen: u64, true;
});
message!(Revlog {
    1 => id: i64, false;
    2 => cid: i64, false;
    3 => ease: i64, false;
    4 => ivl: Stored, false;
    5 => last_ivl: Stored, false;
    6 => factor: i64, false;
    7 => time: Stored, false;
    8 => review_type: i64, false;
});
message!(Deck {
    1 => id: i64, false;
    2 => name: Vec<u8>, false;
    3 => mtime_secs: i64, false;
    4 => common: Vec<u8>, false;
    5 => kind: Vec<u8>, false;
    6 => parent_id: i64, true;
});
message!(Preset {
    1 => id: i64, false;
    2 => name: Vec<u8>, false;
    3 => mtime_secs: i64, false;
    4 => config: Vec<u8>, false;
});
message!(Notetype {
    1 => id: i64, false;
    2 => name: Vec<u8>, false;
    3 => mtime_secs: i64, false;
    4 => config: Vec<u8>, false;
    5 => fields: Vec<Field>, false;
    6 => templates: Vec<Template>, false;
    7 => struct_gen: u64, true;
});
message!(Field {
    1 => ord: i64, false;
    2 => name: Vec<u8>, false;
    3 => config: Vec<u8>, false;
});
message!(Template {
    1 => ord: i64, false;
    2 => name: Vec<u8>, false;
    3 => mtime_secs: i64, false;
    4 => config: Vec<u8>, false;
});
message!(Tag {
    1 => tag: Vec<u8>, false;
    2 => collapsed: i64, false;
    3 => config: Option<Vec<u8>>, false;
});
message!(Config {
    1 => key: Vec<u8>, false;
    2 => mtime_secs: i64, false;
    3 => val: Vec<u8>, false;
});
message!(MediaName {
    1 => name: Vec<u8>, false;
    2 => sha256: [u8; 32], false;
    3 => sha1: [u8; 20], false;
    4 => size: u64, false;
});

macro_rules! ordered_rows {
    ($name:ident) => {
        impl FieldValue for Vec<$name> {
            const REPEATED: bool = true;
            fn write_field(&self, tag: u32, out: &mut Writer) -> Result<()> {
                let mut ordered: Vec<_> = self.iter().collect();
                ordered.sort_by_key(|row| row.ord);
                if ordered.windows(2).any(|rows| rows[0].ord == rows[1].ord) {
                    return Err(CodecError::NonCanonical);
                }
                for row in ordered {
                    out.header(tag, row.encoded_len(false)?)?;
                    row.write(out, false)?;
                }
                Ok(())
            }
            fn read_field(&mut self, value: Value<'_>) -> Result<()> {
                // One row per actual length-delimited field; no untrusted count allocation.
                let row = $name::read(value.bytes()?)?;
                if self.last().is_some_and(|previous| previous.ord >= row.ord) {
                    return Err(CodecError::NonCanonical);
                }
                self.push(row);
                Ok(())
            }
        }
    };
}
ordered_rows!(Field);
ordered_rows!(Template);

macro_rules! payloads {
    ($($name:ident),*) => {
        /// A supplied row image and supplied Meguri fields, without row-reading policy.
        #[derive(Clone, PartialEq, Eq)]
        pub enum Payload { $($name($name),)* }
        impl std::fmt::Debug for Payload {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self { $(Self::$name(_) => f.write_str(concat!("Payload::", stringify!($name))),)* }
            }
        }
        impl Payload {
            pub fn kind(&self) -> Kind { match self { $(Self::$name(_) => Kind::$name,)* } }
            pub fn encode(&self) -> Result<Vec<u8>> { match self { $(Self::$name(value) => value.encode(false),)* } }
            pub fn decode(kind: Kind, bytes: &[u8]) -> Result<Self> {
                match kind { $(Kind::$name => Ok(Self::$name($name::decode(bytes)?)),)* }
            }
            pub fn row_digest(&self) -> Result<[u8; 32]> {
                let bytes = match self { $(Self::$name(value) => value.encode(true)?,)* };
                Ok(sha256(&bytes))
            }
            pub fn payload_digest(&self) -> Result<[u8; 32]> { Ok(sha256(&self.encode()?)) }
        }
    };
}
payloads!(Note, Card, Revlog, Deck, Preset, Notetype, Tag, Config, MediaName);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_read_rejects_nonincreasing_ord_before_append() {
        for (first, next) in [
            (&[][..], &[][..]),
            (&[0x08, 0x01][..], &[0x08, 0x01][..]),
            (&[0x08, 0x01][..], &[][..]),
        ] {
            let mut rows = Vec::<Field>::new();
            rows.read_field(Value::Bytes(first)).unwrap();
            assert_eq!(rows.len(), 1);
            let original = rows[0].clone();
            assert_eq!(rows.read_field(Value::Bytes(next)), Err(CodecError::NonCanonical));
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0], original);
        }
    }

    #[test]
    fn template_read_rejects_nonincreasing_ord_before_append() {
        for (first, next) in [
            (&[][..], &[][..]),
            (&[0x08, 0x01][..], &[0x08, 0x01][..]),
            (&[0x08, 0x01][..], &[][..]),
        ] {
            let mut rows = Vec::<Template>::new();
            rows.read_field(Value::Bytes(first)).unwrap();
            assert_eq!(rows.len(), 1);
            let original = rows[0].clone();
            assert_eq!(rows.read_field(Value::Bytes(next)), Err(CodecError::NonCanonical));
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0], original);
        }
    }

    #[test]
    fn field_read_appends_increasing_ord() {
        let mut rows = Vec::<Field>::new();
        rows.read_field(Value::Bytes(&[])).unwrap();
        rows.read_field(Value::Bytes(&[0x08, 0x01])).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ord, 0);
        assert_eq!(rows[1].ord, 1);
    }

    #[test]
    fn template_read_appends_increasing_ord() {
        let mut rows = Vec::<Template>::new();
        rows.read_field(Value::Bytes(&[])).unwrap();
        rows.read_field(Value::Bytes(&[0x08, 0x01])).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ord, 0);
        assert_eq!(rows[1].ord, 1);
    }
}
