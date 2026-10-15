//! `tags` rows (`model/row-records` R8). The key compares with `unicase`:
//! a tag that differs from a stored one only in letter case is that row.

use meguri_model::tag::Tag;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

impl Record for Tag {
    const TABLE: Table = Table::Tags;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::text(self.name.clone()),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Integer(i64::from(self.collapsed)),
            self.config.clone().map_or(Cell::Null, Cell::Blob),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<Tag, RowError> {
        let row = RowCells {
            table: Table::Tags,
            cells,
        };
        let collapsed = match row.raw_int(2)? {
            0 => false,
            1 => true,
            other => return row.fail(2, format!("{other} is not 0 or 1")),
        };
        let config = match &cells[3] {
            Cell::Null => None,
            Cell::Blob(bytes) => Some(bytes.clone()),
            _ => return row.fail(3, "neither a blob nor NULL"),
        };
        Ok(Tag {
            name: row.text(0)?,
            usn: Usn(row.int(1)?),
            collapsed,
            config,
        })
    }
}
