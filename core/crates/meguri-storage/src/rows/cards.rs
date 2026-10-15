//! `cards` rows (`model/row-records` R4, with its read tolerances).

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

impl Record for Card {
    const TABLE: Table = Table::Cards;

    /// `data` is built again from the card's values every time the row is
    /// written (`model/card-data-json` C4).
    fn to_cells(&self) -> RowImage {
        vec![
            Cell::Integer(self.id.0),
            Cell::Integer(self.note_id.0),
            Cell::Integer(self.deck_id.0),
            Cell::Integer(i64::from(self.ord)),
            Cell::Integer(self.mtime.0),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Integer(self.card_type.stored()),
            Cell::Integer(self.queue.stored()),
            Cell::Integer(i64::from(self.due)),
            Cell::Integer(i64::from(self.interval)),
            Cell::Integer(i64::from(self.ease_factor)),
            Cell::Integer(i64::from(self.reps)),
            Cell::Integer(i64::from(self.lapses)),
            Cell::Integer(i64::from(self.remaining_steps)),
            Cell::Integer(i64::from(self.original_due)),
            Cell::Integer(self.original_deck_id.0),
            Cell::Integer(i64::from(self.flags)),
            Cell::text(self.data.to_text()),
        ]
    }

    fn from_cells(cells: &[Cell]) -> Result<Card, RowError> {
        let row = RowCells {
            table: Table::Cards,
            cells,
        };
        let card_type = match CardType::from_stored(row.raw_int(6)?) {
            Some(t) => t,
            None => return row.fail(6, "not a card type"),
        };
        let queue = match CardQueue::from_stored(row.raw_int(7)?) {
            Some(q) => q,
            None => return row.fail(7, "not a card queue"),
        };
        Ok(Card {
            id: CardId(row.int(0)?),
            note_id: NoteId(row.int(1)?),
            deck_id: DeckId(row.int(2)?),
            ord: row.int(3)?,
            mtime: EpochSecs(row.int(4)?),
            usn: Usn(row.int(5)?),
            card_type,
            queue,
            due: row.int_or(8, 0),
            interval: row.int(9)?,
            ease_factor: row.int(10)?,
            reps: row.int(11)?,
            lapses: row.int(12)?,
            remaining_steps: row.int(13)?,
            original_due: row.int_or(14, 0),
            original_deck_id: DeckId(row.int(15)?),
            flags: row.int(16)?,
            data: CardData::parse(&row.text_or_empty(17)),
        })
    }
}
