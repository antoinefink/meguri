//! The `col` row (`specs/model/row-records.md` R2, R1456).

use crate::{
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};

/// The text columns `conf`, `models`, `decks`, `dconf` and `tags`: each the
/// empty string at schema 18 and a JSON document at schema 11
/// (`model/schema11-json` S12).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyColumns {
    pub conf: String,
    pub models: String,
    pub decks: String,
    pub dconf: String,
    pub tags: String,
}

impl Col {
    /// The `ver` Meguri writes.
    pub const SCHEMA_VERSION: i64 = 18;
}

/// The single row of `col`; its `id` is always 1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Col {
    /// `ver`: the schema version.
    pub version: i64,
    /// `crt`, in epoch seconds: when the collection was created. Day numbers
    /// count from the day holding this instant.
    pub created: EpochSecs,
    /// `mod`, in epoch milliseconds: the last change to the collection.
    pub modified: EpochMillis,
    /// `scm`, in epoch milliseconds: the last schema change. A value after
    /// `last_sync` makes the next sync one-way.
    pub schema_modified: EpochMillis,
    /// `ls`, in epoch milliseconds: the last sync.
    pub last_sync: EpochMillis,
    /// `usn`: on a client, the server's update sequence number at the last
    /// sync.
    pub usn: Usn,
    /// `dty`: unused; 0.
    pub dirty: i64,
    pub legacy: LegacyColumns,
}
