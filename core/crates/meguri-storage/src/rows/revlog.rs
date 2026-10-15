//! `revlog` rows (`model/row-records` R5, with its read tolerances).

use meguri_model::ids::{CardId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

impl Record for RevlogEntry {
    const TABLE: Table = Table::Revlog;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(self.id.0),
            Cell::Integer(self.card_id.0),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Integer(i64::from(self.button)),
            Cell::Integer(i64::from(self.interval)),
            Cell::Integer(i64::from(self.last_interval)),
            Cell::Integer(i64::from(self.ease_factor)),
            Cell::Integer(i64::from(self.answer_millis)),
            Cell::Integer(self.kind.stored()),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<RevlogEntry, RowError> {
        let row = RowCells {
            table: Table::Revlog,
            cells,
        };
        Ok(RevlogEntry {
            id: RevlogId(row.int(0)?),
            card_id: CardId(row.int(1)?),
            usn: Usn(row.int(2)?),
            button: row.int(3)?,
            interval: truncated(&row, 4)?,
            last_interval: truncated(&row, 5)?,
            ease_factor: row.int(6)?,
            answer_millis: row.int_or(7, 0),
            kind: RevlogKind::from_stored(row.raw_int(8)?),
        })
    }
}

/// R5: `ivl` and `lastIvl` are read after conversion to an integer, so a
/// REAL is truncated toward zero.
fn truncated(row: &RowCells<'_>, index: usize) -> Result<i32, RowError> {
    match &row.cells[index] {
        Cell::Real(v) if v.is_finite() && v.trunc() >= f64::from(i32::MIN) && v.trunc() <= f64::from(i32::MAX) => {
            Ok(v.trunc() as i32)
        }
        _ => row.int(index),
    }
}
