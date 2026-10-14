//! The `config` row (`specs/model/row-records.md` R9). How values are written
//! and preserved is `specs/storage/config-table` (U23).

use crate::time::TimestampSecs;
use crate::usn::Usn;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigEntry {
    /// `KEY`.
    pub key: String,
    pub usn: Usn,
    pub mtime: TimestampSecs,
    /// `val`: the value as JSON bytes, kept exactly.
    pub value: Vec<u8>,
}
