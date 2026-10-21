use crate::{Cell, RowKey, StorageError, Table, WriteScope, Written, rows};
use meguri_model::{
    card::CardQueue,
    ids::{CardId, NoteId},
};
#[derive(Clone, Copy, Debug)]
pub struct Sibling {
    pub id: CardId,
    pub queue: CardQueue,
}
pub(crate) fn read(conn: &rusqlite::Connection, note: NoteId, except: CardId) -> Result<Vec<Sibling>, StorageError> {
    let found = conn
        .prepare_cached("SELECT id,queue FROM cards WHERE nid=?1 AND id!=?2 AND queue IN (0,2,3) ORDER BY id")?
        .query_map((note.0, except.0), |r| Ok((CardId(r.get(0)?), r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found
        .into_iter()
        .filter_map(|(id, q)| CardQueue::from_stored(q).map(|queue| Sibling { id, queue }))
        .collect())
}
impl WriteScope<'_> {
    /// Preserve data and every unrelated imported column byte for byte.
    pub fn bury_answer_sibling(&mut self, id: CardId, now: i64) -> Result<Written, StorageError> {
        let Some(mut row) = rows::select_image(self.conn(), Table::Cards, &RowKey::Id(id.0))? else {
            return Ok(Written::Unchanged);
        };
        row[4] = Cell::Integer(now);
        row[5] = Cell::Integer(-1);
        row[7] = Cell::Integer(-2);
        self.put_cells(Table::Cards, row, false)
    }
}
