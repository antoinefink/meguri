//! The `col` row (`specs/model/row-records.md` R2, R1456).

use crate::time::{TimestampMillis, TimestampSecs};
use crate::usn::Usn;

/// The one `col` row (`id` = 1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Col {
    /// `crt`: collection creation in epoch seconds, the start of day 0.
    pub created: TimestampSecs,
    /// `mod`: last change to the collection, epoch milliseconds.
    pub modified: TimestampMillis,
    /// `scm`: last schema change, epoch milliseconds; later than `ls` means the
    /// next sync must be one-way.
    pub schema_modified: TimestampMillis,
    /// `ver`: the schema version, 18.
    pub version: i64,
    /// `dty`: unused, 0.
    pub dirty: i64,
    /// `usn`: on a client, the server's update sequence number as of the last
    /// sync.
    pub usn: Usn,
    /// `ls`: last sync, epoch milliseconds.
    pub last_sync: TimestampMillis,
    /// The five legacy JSON columns: empty strings at schema 18, JSON at
    /// schema 11 (`model/schema11-json` S12).
    pub legacy: LegacyColumns,
}

/// `conf`, `models`, `decks`, `dconf` and `tags`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyColumns {
    pub conf: String,
    pub models: String,
    pub decks: String,
    pub dconf: String,
    pub tags: String,
}

impl Col {
    /// The schema version Meguri writes.
    pub const SCHEMA_VERSION: i64 = 18;
}
