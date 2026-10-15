//! `graves` rows (`model/row-records` R6, `specs/storage/graves.md`).

use meguri_model::grave::{Grave, GraveKind};
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, RowKey, Table};

impl Record for Grave {
    const TABLE: Table = Table::Graves;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(self.oid),
            Cell::Integer(self.kind.stored()),
            Cell::Integer(i64::from(self.usn.0)),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<Grave, RowError> {
        let row = RowCells {
            table: Table::Graves,
            cells,
        };
        let kind = match GraveKind::from_stored(row.raw_int(1)?) {
            Some(kind) => kind,
            None => return row.fail(1, "not a grave type"),
        };
        Ok(Grave {
            oid: row.int(0)?,
            kind,
            usn: Usn(row.int(2)?),
        })
    }
}

/// The key of the grave of `oid` of `kind`.
pub fn key(oid: i64, kind: GraveKind) -> RowKey {
    RowKey::Pair(oid, kind.stored())
}
