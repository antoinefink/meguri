//! The crate's one error type.

use std::fmt;

/// A blob column of a schema-18 collection (`specs/model/protobuf-records.md`
/// P1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlobColumn {
    NotetypeConfig,
    FieldConfig,
    TemplateConfig,
    DeckCommon,
    DeckKind,
    DeckConfig,
}

impl BlobColumn {
    pub fn table(self) -> &'static str {
        match self {
            BlobColumn::NotetypeConfig => "notetypes",
            BlobColumn::FieldConfig => "fields",
            BlobColumn::TemplateConfig => "templates",
            BlobColumn::DeckCommon | BlobColumn::DeckKind => "decks",
            BlobColumn::DeckConfig => "deck_config",
        }
    }

    pub fn column(self) -> &'static str {
        match self {
            BlobColumn::DeckCommon => "common",
            BlobColumn::DeckKind => "kind",
            _ => "config",
        }
    }
}

/// Why a blob does not decode (`specs/model/protobuf-records.md` P9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlobError {
    /// The blob ends inside a key, varint, fixed value, length-delimited
    /// payload or group.
    Truncated,
    /// A varint longer than 10 bytes or with a tenth byte above `01`, or a key
    /// above 32 bits.
    BadVarint,
    /// Field number 0.
    BadFieldNumber,
    /// Wire type 6 or 7, or a declared field with a wire type it does not
    /// accept.
    BadWireType,
    /// A group-end key with no open group, or whose field number differs from
    /// the innermost open group's.
    BadGroup,
    /// A field at nesting depth 100 or more (P2).
    TooDeep,
    /// A `string` field that is not UTF-8.
    BadUtf8,
    /// A deck kind blob with neither member.
    NoDeckKind,
}

/// A schema-11 JSON record or `col` column (`specs/model/schema11-json.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Schema11Item {
    Notetype,
    Deck,
    DeckConfig,
    /// One of the `col` JSON columns (S12): `conf`, `models`, `decks`, `dconf`
    /// or `tags`.
    Column(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    /// A protobuf blob broke the wire rules (P9).
    Blob { column: BlobColumn, reason: BlobError },
    /// A schema-11 object or column refused (S1). `key` names what failed,
    /// as a path such as `flds[0].sticky`.
    InvalidJson { item: Schema11Item, key: String },
}

impl ModelError {
    pub(crate) fn invalid_json(item: Schema11Item, key: impl Into<String>) -> ModelError {
        ModelError::InvalidJson { item, key: key.into() }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::Blob { column, reason } => {
                write!(f, "{}.{}: invalid blob ({reason:?})", column.table(), column.column())
            }
            ModelError::InvalidJson { item, key } => {
                write!(f, "schema-11 {item:?} refused at {key:?}")
            }
        }
    }
}

impl std::error::Error for ModelError {}
