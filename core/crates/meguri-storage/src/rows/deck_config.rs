//! `deck_config` rows (`model/row-records` R7).

use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::DeckConfigId;
use meguri_model::proto::ProtoBlob;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

impl Record for DeckConfig {
    const TABLE: Table = Table::DeckConfig;

    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(self.id.0),
            Cell::text(self.name.clone()),
            Cell::Integer(self.mtime.0),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Blob(self.config.to_bytes().into_owned()),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<DeckConfig, RowError> {
        let row = RowCells {
            table: Table::DeckConfig,
            cells,
        };
        Ok(DeckConfig {
            id: DeckConfigId(row.int(0)?),
            name: row.text(1)?,
            mtime: EpochSecs(row.int(2)?),
            usn: Usn(row.int(3)?),
            config: row.wrap(4, ProtoBlob::decode(row.blob(4)?))?,
        })
    }
}
