//! `decks` rows (`model/row-records` R7): names in native form, the two
//! blobs written back as read when their content is unchanged (P5).

use meguri_model::deck::Deck;
use meguri_model::ids::DeckId;
use meguri_model::proto::ProtoBlob;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

impl Record for Deck {
    const TABLE: Table = Table::Decks;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(self.id.0),
            Cell::text(self.name.clone()),
            Cell::Integer(self.mtime.0),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Blob(self.common.to_bytes().into_owned()),
            Cell::Blob(self.kind.to_bytes().into_owned()),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<Deck, RowError> {
        let row = RowCells {
            table: Table::Decks,
            cells,
        };
        Ok(Deck {
            id: DeckId(row.int(0)?),
            name: row.text(1)?,
            mtime: EpochSecs(row.int(2)?),
            usn: Usn(row.int(3)?),
            common: row.wrap(4, ProtoBlob::decode(row.blob(4)?))?,
            kind: row.wrap(5, ProtoBlob::decode(row.blob(5)?))?,
        })
    }
}
