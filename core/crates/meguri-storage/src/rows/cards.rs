//! `cards` rows (`model/row-records` R4, with its read tolerances).

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

/// Scheduling and temporary membership belong to an answer. Identity, template
/// ordinal and flags retain their stored cells, including concurrent edits.
pub const ANSWER_COLUMNS: [usize; 14] = [2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17];

struct CardScalars {
    id: CardId,
    note_id: NoteId,
    deck_id: DeckId,
    ord: u16,
    mtime: EpochSecs,
    usn: Usn,
    card_type: CardType,
    queue: CardQueue,
    due: i32,
    interval: u32,
    ease_factor: u16,
    reps: u32,
    lapses: u32,
    remaining_steps: u32,
    original_due: i32,
    original_deck_id: DeckId,
    flags: u8,
}

fn read_scalars(cells: &[Cell]) -> Result<CardScalars, RowError> {
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
    Ok(CardScalars {
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
    })
}

/// Validates a stored card with the full reader's scalar rules, without
/// decoding its infallibly parsed `data` column.
pub fn validated_note_id(cells: &[Cell]) -> Result<NoteId, RowError> {
    Ok(read_scalars(cells)?.note_id)
}

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
        let scalars = read_scalars(cells)?;
        let row = RowCells {
            table: Table::Cards,
            cells,
        };
        Ok(Card {
            id: scalars.id,
            note_id: scalars.note_id,
            deck_id: scalars.deck_id,
            ord: scalars.ord,
            mtime: scalars.mtime,
            usn: scalars.usn,
            card_type: scalars.card_type,
            queue: scalars.queue,
            due: scalars.due,
            interval: scalars.interval,
            ease_factor: scalars.ease_factor,
            reps: scalars.reps,
            lapses: scalars.lapses,
            remaining_steps: scalars.remaining_steps,
            original_due: scalars.original_due,
            original_deck_id: scalars.original_deck_id,
            flags: scalars.flags,
            data: CardData::parse(&row.text_or_empty(17)),
        })
    }
}
