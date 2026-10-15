//! `notes` rows (`model/row-records` R3).

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

impl Record for Note {
    const TABLE: Table = Table::Notes;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(self.id.0),
            Cell::text(self.guid.clone()),
            Cell::Integer(self.notetype_id.0),
            Cell::Integer(self.mtime.0),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::text(self.tags.clone()),
            Cell::text(self.fields.clone()),
            // The column's integer affinity stores a sort field that reads
            // as a number as an INTEGER (`storage/schema-18` S2).
            Cell::text(self.sort_field.clone()),
            Cell::Integer(i64::from(self.checksum)),
            Cell::Integer(self.flags),
            Cell::text(self.data.clone()),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<Note, RowError> {
        let row = RowCells {
            table: Table::Notes,
            cells,
        };
        Ok(Note {
            id: NoteId(row.int(0)?),
            guid: row.text(1)?,
            notetype_id: NotetypeId(row.int(2)?),
            mtime: EpochSecs(row.int(3)?),
            usn: Usn(row.int(4)?),
            tags: row.text(5)?,
            fields: row.text(6)?,
            sort_field: match &cells[7] {
                Cell::Integer(v) => v.to_string(),
                Cell::Real(v) => v.to_string(),
                _ => row.text(7)?,
            },
            checksum: Note::checksum_from_stored(row.raw_int(8)?),
            flags: row.int(9)?,
            data: row.text(10)?,
        })
    }
}
