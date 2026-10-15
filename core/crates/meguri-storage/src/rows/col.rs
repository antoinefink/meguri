//! The `col` row (`model/row-records` R2, `storage/ids-usn-mtime` S6). Its
//! `mod` moves when an operation commits (S5), never row by row, and its
//! `usn` only at the end of a sync.

use meguri_model::col::{Col, LegacyColumns};
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, RowKey, Table};

/// The key of the single row.
pub const COL_KEY: RowKey = RowKey::Id(1);

impl Record for Col {
    const TABLE: Table = Table::Col;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(1),
            Cell::Integer(self.created.0),
            Cell::Integer(self.modified.0),
            Cell::Integer(self.schema_modified.0),
            Cell::Integer(self.version),
            Cell::Integer(self.dirty),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Integer(self.last_sync.0),
            Cell::text(self.legacy.conf.clone()),
            Cell::text(self.legacy.models.clone()),
            Cell::text(self.legacy.decks.clone()),
            Cell::text(self.legacy.dconf.clone()),
            Cell::text(self.legacy.tags.clone()),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<Col, RowError> {
        let row = RowCells {
            table: Table::Col,
            cells,
        };
        Ok(Col {
            version: row.int(4)?,
            created: EpochSecs(row.int(1)?),
            modified: EpochMillis(row.int(2)?),
            schema_modified: EpochMillis(row.int(3)?),
            last_sync: EpochMillis(row.int(7)?),
            usn: Usn(row.int(6)?),
            dirty: row.int(5)?,
            legacy: LegacyColumns {
                conf: row.text(8)?,
                models: row.text(9)?,
                decks: row.text(10)?,
                dconf: row.text(11)?,
                tags: row.text(12)?,
            },
        })
    }
}
