//! K6.1 values. Bytes retain the stored TEXT/BLOB representation without repair.
use super::wire::{FieldValue, Message, Reader, Value, Writer};
use super::{CodecError, Kind};

type Result<T> = std::result::Result<T, CodecError>;

/// SQLite storage class and raw value. REAL equality compares all IEEE-754 bits.
#[derive(Clone)]
pub enum Stored {
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
// Persistence and IPC carry REAL as bits, avoiding JSON float conversion.
#[derive(serde::Serialize, serde::Deserialize)]
enum StoredEncoding {
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
impl serde::Serialize for Stored {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Integer(n) => StoredEncoding::Integer(*n),
            Self::Real(n) => StoredEncoding::Real(n.to_bits()),
            Self::Text(b) => StoredEncoding::Text(b.clone()),
            Self::Blob(b) => StoredEncoding::Blob(b.clone()),
        };
        serde::Serialize::serialize(&wire, serializer)
    }
}
impl<'de> serde::Deserialize<'de> for Stored {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Ok(
            match <StoredEncoding as serde::Deserialize>::deserialize(deserializer)? {
                StoredEncoding::Integer(n) => Self::Integer(n),
                StoredEncoding::Real(n) => Self::Real(f64::from_bits(n)),
                StoredEncoding::Text(b) => Self::Text(b),
                StoredEncoding::Blob(b) => Self::Blob(b),
            },
        )
    }
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
        #[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
        #[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub enum Payload { $($name($name),)* }
        impl std::fmt::Debug for Payload {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self { $(Self::$name(_) => f.write_str(concat!("Payload::", stringify!($name))),)* }
            }
        }
        impl Payload {
            pub fn kind(&self) -> Kind { match self { $(Self::$name(_) => Kind::$name,)* } }
            pub fn encode(&self) -> Result<Vec<u8>> { match self { $(Self::$name(value) => value.encode(false),)* } }
            pub fn encoded_len(&self) -> Result<usize> { match self { $(Self::$name(value) => value.encoded_len(false),)* } }
            pub fn decode(kind: Kind, bytes: &[u8]) -> Result<Self> {
                match kind { $(Kind::$name => Ok(Self::$name($name::decode(bytes)?)),)* }
            }
            pub fn row_digest(&self) -> Result<[u8; 32]> {
                match self { $(Self::$name(value) => value.digest(true),)* }
            }
            pub fn payload_digest(&self) -> Result<[u8; 32]> { match self { $(Self::$name(value) => value.digest(false),)* } }
        }
    };
}
payloads!(Note, Card, Revlog, Deck, Preset, Notetype, Tag, Config, MediaName);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_count_and_streamed_hashes_match_bytes_for_every_kind() {
        let values = vec![
            Payload::Note(Note {
                id: -7,
                guid: vec![0, 255],
                flds: vec![31; 257],
                sfld: Stored::Real(-0.0),
                nt_struct_gen: 19,
                ..Default::default()
            }),
            Payload::Card(Card {
                id: 128,
                due: Stored::Text(vec![0, 255]),
                odue: Stored::Blob(vec![1; 128]),
                data: Stored::Integer(-1),
                last_review_id: -9,
                nt_struct_gen: 21,
                ..Default::default()
            }),
            Payload::Revlog(Revlog {
                id: -1,
                ivl: Stored::Real(f64::from_bits(0x7ff8_0000_0000_0001)),
                last_ivl: Stored::Text(vec![]),
                time: Stored::Blob(vec![]),
                ..Default::default()
            }),
            Payload::Deck(Deck {
                id: 128,
                name: b"parent\x1fchild".to_vec(),
                parent_id: 127,
                common: vec![0; 129],
                kind: vec![255],
                ..Default::default()
            }),
            Payload::Preset(Preset {
                id: -2,
                config: vec![3; 257],
                ..Default::default()
            }),
            Payload::Notetype(Notetype {
                id: 3,
                struct_gen: 18,
                fields: vec![
                    Field {
                        ord: 2,
                        name: vec![255],
                        ..Default::default()
                    },
                    Field::default(),
                ],
                templates: vec![Template {
                    ord: 1,
                    config: vec![128; 129],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            Payload::Tag(Tag {
                tag: b"tag".to_vec(),
                collapsed: -1,
                config: Some(vec![]),
            }),
            Payload::Config(Config {
                key: b"key".to_vec(),
                mtime_secs: -1,
                val: vec![0; 129],
            }),
            Payload::MediaName(MediaName {
                name: b"media".to_vec(),
                sha256: [127; 32],
                sha1: [255; 20],
                size: 128,
            }),
        ];
        for payload in values {
            let bytes = payload.encode().unwrap();
            assert_eq!(payload.encoded_len().unwrap(), bytes.len());
            assert_eq!(payload.payload_digest().unwrap(), super::super::sha256(&bytes));
            let row = match &payload {
                Payload::Note(v) => v.encode(true),
                Payload::Card(v) => v.encode(true),
                Payload::Revlog(v) => v.encode(true),
                Payload::Deck(v) => v.encode(true),
                Payload::Preset(v) => v.encode(true),
                Payload::Notetype(v) => v.encode(true),
                Payload::Tag(v) => v.encode(true),
                Payload::Config(v) => v.encode(true),
                Payload::MediaName(v) => v.encode(true),
            }
            .unwrap();
            assert_eq!(payload.row_digest().unwrap(), super::super::sha256(&row));
            let decoded = Payload::decode(payload.kind(), &bytes).unwrap();
            assert_eq!(decoded.encode().unwrap(), bytes);
            assert_eq!(decoded.payload_digest(), payload.payload_digest());
            assert_eq!(decoded.row_digest(), payload.row_digest());
        }
    }

    #[test]
    fn every_kind_keeps_the_same_oversize_error_for_count_encode_and_hash() {
        for kind in [
            Kind::Note,
            Kind::Card,
            Kind::Revlog,
            Kind::Deck,
            Kind::Preset,
            Kind::Notetype,
            Kind::Tag,
            Kind::Config,
            Kind::MediaName,
        ] {
            let bytes = vec![0; usize::try_from(super::super::MAX_PAYLOAD_BYTES).unwrap() + 1];
            let payload = match kind {
                Kind::Note => Payload::Note(Note {
                    flds: bytes,
                    ..Default::default()
                }),
                Kind::Card => Payload::Card(Card {
                    data: Stored::Blob(bytes),
                    ..Default::default()
                }),
                Kind::Revlog => Payload::Revlog(Revlog {
                    ivl: Stored::Text(bytes),
                    ..Default::default()
                }),
                Kind::Deck => Payload::Deck(Deck {
                    common: bytes,
                    ..Default::default()
                }),
                Kind::Preset => Payload::Preset(Preset {
                    config: bytes,
                    ..Default::default()
                }),
                Kind::Notetype => Payload::Notetype(Notetype {
                    config: bytes,
                    ..Default::default()
                }),
                Kind::Tag => Payload::Tag(Tag {
                    config: Some(bytes),
                    ..Default::default()
                }),
                Kind::Config => Payload::Config(Config {
                    val: bytes,
                    ..Default::default()
                }),
                Kind::MediaName => Payload::MediaName(MediaName {
                    name: bytes,
                    ..Default::default()
                }),
            };
            assert_eq!(payload.encode(), Err(CodecError::Oversize));
            assert_eq!(payload.encoded_len(), Err(CodecError::Oversize));
            assert_eq!(payload.row_digest(), Err(CodecError::Oversize));
            assert_eq!(payload.payload_digest(), Err(CodecError::Oversize));
        }
    }

    #[test]
    fn count_and_hash_reject_the_same_invalid_notetype_as_encode() {
        for templates in [false, true] {
            let mut nt = Notetype::default();
            if templates {
                nt.templates = vec![Template::default(), Template::default()];
            } else {
                nt.fields = vec![Field::default(), Field::default()];
            }
            let payload = Payload::Notetype(nt);
            assert_eq!(payload.encode(), Err(CodecError::NonCanonical));
            assert_eq!(payload.encoded_len(), Err(CodecError::NonCanonical));
            assert_eq!(payload.row_digest(), Err(CodecError::NonCanonical));
            assert_eq!(payload.payload_digest(), Err(CodecError::NonCanonical));
        }
    }

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
