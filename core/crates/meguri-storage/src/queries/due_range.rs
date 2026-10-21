//! W2: due alone restricts cards. Deck decoding decides whether each row
//! belongs to a current normal deck; no card type or queue is inspected.
use crate::StorageError;
use meguri_model::ids::{CardId, DeckId, NoteId};
#[derive(Clone, Copy, Debug)]
pub struct DueRow {
    pub card: CardId,
    pub note: NoteId,
    pub deck: DeckId,
    pub day: u32,
}
pub(crate) fn read(conn: &rusqlite::Connection, today: u32) -> Result<Vec<DueRow>, StorageError> {
    let start = i64::from(today);
    Ok(conn
        .prepare_cached("SELECT id,nid,did,due FROM cards WHERE due>=?1 AND due<?2 ORDER BY id")?
        .query_map((start, start + 99), |r| {
            Ok(DueRow {
                card: CardId(r.get(0)?),
                note: NoteId(r.get(1)?),
                deck: DeckId(r.get(2)?),
                day: (r.get::<_, i64>(3)? - start) as u32,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
