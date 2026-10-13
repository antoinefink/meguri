//! Calibration fixture for the similarity scanner: storage code over the card
//! and note tables, written for Meguri from the file-format facts alone. The
//! negative calibration expects it to stay below every threshold once the
//! interop vocabulary is masked.

use rusqlite::{params, Connection, OptionalExtension, Row};

/// One row of the card table, in column order.
#[derive(Debug, Clone, PartialEq)]
pub struct CardRecord {
    pub card_id: i64,
    pub note_id: i64,
    pub deck_id: i64,
    pub template_index: u16,
    pub modified_secs: i64,
    pub update_seq: i32,
    pub kind: u8,
    pub queue_kind: i8,
    pub due_value: i64,
    pub interval_days: u32,
    pub ease_permille: u16,
    pub review_count: u32,
    pub lapse_count: u32,
    pub remaining_steps: u32,
    pub original_due: i64,
    pub original_deck: i64,
    pub flag_bits: u8,
    pub extra_json: String,
}

const CARD_SELECT: &str = "select id, nid, did, ord, mod, usn, type, queue, due, ivl, \
    factor, reps, lapses, left, odue, odid, flags, data from cards";

fn decode_card(row: &Row<'_>) -> rusqlite::Result<CardRecord> {
    Ok(CardRecord {
        card_id: row.get(0)?,
        note_id: row.get(1)?,
        deck_id: row.get(2)?,
        template_index: row.get(3)?,
        modified_secs: row.get(4)?,
        update_seq: row.get(5)?,
        kind: row.get(6)?,
        queue_kind: row.get(7)?,
        due_value: row.get(8)?,
        interval_days: row.get(9)?,
        ease_permille: row.get(10)?,
        review_count: row.get(11)?,
        lapse_count: row.get(12)?,
        remaining_steps: row.get(13)?,
        original_due: row.get(14)?,
        original_deck: row.get(15)?,
        flag_bits: row.get(16)?,
        extra_json: row.get(17)?,
    })
}

/// Reads a card by id; `None` when the card does not exist.
pub fn card_by_id(db: &Connection, card_id: i64) -> rusqlite::Result<Option<CardRecord>> {
    let sql = format!("{CARD_SELECT} where id = ?1");
    db.query_row(&sql, params![card_id], decode_card).optional()
}

/// Every card generated from one note, by template position.
pub fn cards_for_note(db: &Connection, note_id: i64) -> rusqlite::Result<Vec<CardRecord>> {
    let sql = format!("{CARD_SELECT} where nid = ?1 order by ord");
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![note_id], decode_card)?;
    rows.collect()
}

/// Ids of review cards due on or before `day` in the given decks, oldest first.
pub fn due_review_ids(db: &Connection, decks: &[i64], day: i64, cap: usize) -> rusqlite::Result<Vec<i64>> {
    if decks.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; decks.len()].join(",");
    let sql = format!(
        "select id from cards where did in ({marks}) and queue = 2 and due <= {day} \
         order by due, id limit {cap}"
    );
    let mut stmt = db.prepare(&sql)?;
    let ids = stmt.query_map(rusqlite::params_from_iter(decks.iter()), |r| r.get(0))?;
    ids.collect()
}

/// Marks a card as changed locally so the next sync sends it.
pub fn touch_card(db: &Connection, card_id: i64, now_secs: i64) -> rusqlite::Result<usize> {
    db.execute(
        "update cards set mod = ?1, usn = -1 where id = ?2",
        params![now_secs, card_id],
    )
}

/// One row of the note table.
pub struct NoteRecord<'a> {
    pub note_id: i64,
    pub global_id: &'a str,
    pub notetype_id: i64,
    pub modified_secs: i64,
    pub update_seq: i32,
    pub tag_text: &'a str,
    pub field_text: &'a str,
    pub sort_text: &'a str,
    pub first_field_sum: u32,
}

pub fn insert_note(db: &Connection, n: &NoteRecord<'_>) -> rusqlite::Result<()> {
    db.execute(
        "insert into notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data) \
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, '')",
        params![
            n.note_id,
            n.global_id,
            n.notetype_id,
            n.modified_secs,
            n.update_seq,
            n.tag_text,
            n.field_text,
            n.sort_text,
            n.first_field_sum
        ],
    )?;
    Ok(())
}

/// Notes of one note type whose first-field checksum equals `sum`: the
/// candidates for a duplicate check, compared field by field by the caller.
pub fn duplicate_candidates(db: &Connection, notetype_id: i64, sum: u32) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = db.prepare_cached("select id, flds from notes where mid = ?1 and csum = ?2")?;
    let rows = stmt.query_map(params![notetype_id, sum], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}
