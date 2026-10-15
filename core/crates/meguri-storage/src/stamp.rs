//! Stamping modes (`specs/storage/ids-usn-mtime.md` S2, S3, S5): what a real
//! change does to a row's `usn` and modification time, and whether the
//! operation moves `col.mod` when it commits.

use meguri_model::time::EpochMillis;
use meguri_model::usn::Usn;

use crate::rows::{Cell, Table};

/// How a write scope stamps the rows it changes. Every mode skips a write
/// that changes nothing (S4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StampMode {
    /// A local change (S2): `usn` -1 and the modification time now, in
    /// seconds; `col.mod` is set to now once when the scope commits with a
    /// change (S5). User, local API, optimizer and day-rollover writes.
    Local,
    /// `usn` and modification times as the caller gives them, and `col.mod`
    /// left alone: rows and graves a sync brings, marking sent rows as
    /// synced, undo and redo restores (which set `col.mod` themselves) and
    /// the one-way upload transform.
    CallerSupplied,
    /// `usn` -1 with the modification time the row arrives with, and
    /// `col.mod` set to now at commit: `.apkg` updates of existing notes
    /// (R1602).
    KeepMtime,
    /// Rows written as given, `col.mod` left alone: staging copies (U33,
    /// U122, U123), the unbury at day rollover (U40) and scenario `db_exec`.
    Raw,
}

impl StampMode {
    /// Whether a change in this mode moves `col.mod` at commit (S3, S5).
    pub fn moves_col_mod(self) -> bool {
        matches!(self, StampMode::Local | StampMode::KeepMtime)
    }

    /// Stamps a row about to be written in this mode, in place.
    pub(crate) fn stamp(self, table: Table, cells: &mut [Cell], now: EpochMillis) {
        let stamps = table.stamps();
        match self {
            StampMode::Local => {
                if let Some(usn) = stamps.usn {
                    cells[usn] = Cell::Integer(i64::from(Usn::PENDING.0));
                }
                if let Some(mtime) = stamps.mtime {
                    cells[mtime] = Cell::Integer(now.whole_secs().0);
                }
            }
            StampMode::KeepMtime => {
                if let Some(usn) = stamps.usn {
                    cells[usn] = Cell::Integer(i64::from(Usn::PENDING.0));
                }
            }
            StampMode::CallerSupplied | StampMode::Raw => {}
        }
    }
}
