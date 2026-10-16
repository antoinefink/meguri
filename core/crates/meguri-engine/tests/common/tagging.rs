//! Helpers of the tag, flag and mark tests (U30): a collection with no
//! notes, cards or tags, notes saved as a note save stores them
//! (`specs/tags/tags.md` T4) or with tags as another client stored them,
//! raw SQL and reads of notes, cards and the registry.

use std::time::Duration;

use meguri_engine::executor::catalog;
use meguri_engine::tags::note_tags_for_save;
use meguri_engine::{ClearCause, Collection, OpSpec};
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, NoteId};
use meguri_model::note::Note;
use meguri_model::tag::Tag;
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole};

use super::Harness;

/// The engine over the schema-18 fixture with its notes, cards, review log,
/// graves and tags removed and an empty undo history.
pub fn fresh() -> Harness {
    let h = Harness::open();
    for table in ["notes", "cards", "revlog", "graves", "tags"] {
        raw(&h.collection, &format!("DELETE FROM {table}"), &[]);
    }
    h
}

/// One statement on the writer, outside any op, then the undo history
/// emptied as an external change empties it.
pub fn raw(c: &Collection, sql: &str, params: &[SqlValue]) {
    let (sql, params) = (sql.to_owned(), params.to_vec());
    c.submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), move |writer| {
        Ok(writer.execute_raw(&sql, &params)?)
    })
    .wait()
    .unwrap();
    c.clear_undo(ClearCause::ExternalChange).unwrap();
}

/// A setup op: leaves the undo history alone.
pub fn setup_spec() -> OpSpec {
    OpSpec::named("test_setup", OpSource::UserNonUndoable).skip_undo()
}

/// Adds a note whose tags are what a note save stores for `items` (T4),
/// registering them; returns its id.
pub fn save_note(c: &Collection, items: &[&str]) -> NoteId {
    let items: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
    c.run(setup_spec(), move |ctx| {
        let mut note = super::note(&["front", "back"]);
        note.tags = note_tags_for_save(ctx.scope(), &items)?;
        Ok(ctx.scope().add_note(&note)?)
    })
    .unwrap()
    .value
}

/// Saves `items` as the tags of an existing note (T4).
pub fn resave_note(c: &Collection, nid: NoteId, items: &[&str]) {
    let items: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
    c.run(setup_spec(), move |ctx| {
        let mut note = ctx.scope().note(nid)?.expect("the note exists");
        note.tags = note_tags_for_save(ctx.scope(), &items)?;
        ctx.scope().put(&note)?;
        Ok(())
    })
    .unwrap();
}

/// Adds a note whose `tags` column is `stored` exactly, as another client
/// could have stored it; nothing is registered.
pub fn stored_note(c: &Collection, stored: &str) -> NoteId {
    let stored = stored.to_owned();
    c.run(setup_spec(), move |ctx| {
        let mut note = super::note(&["front", "back"]);
        note.tags = stored;
        Ok(ctx.scope().add_note(&note)?)
    })
    .unwrap()
    .value
}

/// Adds a note with one card whose `flags` column is `flags`.
pub fn card_with_flags(c: &Collection, flags: u8) -> CardId {
    c.run(setup_spec(), move |ctx| {
        let nid = ctx.scope().add_note(&super::note(&["front", "back"]))?;
        let mut card = super::card(nid, 0);
        card.flags = flags;
        Ok(ctx.scope().add_card(&card)?)
    })
    .unwrap()
    .value
}

pub fn note(c: &Collection, nid: NoteId) -> Note {
    c.reader(ReaderRole::Query)
        .unwrap()
        .note(nid)
        .unwrap()
        .expect("the note exists")
}

pub fn tags_of(c: &Collection, nid: NoteId) -> String {
    note(c, nid).tags
}

/// Every registry row, by name in byte order.
pub fn rows(c: &Collection) -> Vec<Tag> {
    let mut rows = c.reader(ReaderRole::Query).unwrap().tag_rows().unwrap();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// Every registered name, in byte order.
pub fn names(c: &Collection) -> Vec<String> {
    rows(c).into_iter().map(|row| row.name).collect()
}

/// The row equal to `name`, if any.
pub fn row(c: &Collection, name: &str) -> Option<Tag> {
    c.reader(ReaderRole::Query).unwrap().tag(name).unwrap()
}
