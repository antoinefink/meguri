//! U36: real engine operations over copies of the preservation fixture.
//! U40 must keep these invariants when composing the scheduler's lifecycle ports.
mod common;

use common::{Harness, NOW_MS, open_config};
use meguri_engine::{Collection, Engine, EngineConfig, OpSource, Ports, deck_config, decks, flags, media::MediaService, notes, tags, undo::UndoScope};
use meguri_model::{clock::Clock, ids::NoteId};
use meguri_storage::{ReadRows, ReaderRole, raw::RawRows};
use meguri_testkit::dump::preservation::{assert_changes_allowed, pending, raw_dump};
use serde_json::Value;

fn fixture() -> Harness { Harness::open_fixture("../preservation/kitchen-sink.anki2", Ports::default()) }
fn dump(c: &Collection) -> Value {
    let reader = c.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    raw_dump(|sql| snapshot.execute_raw(sql, &[])).unwrap()
}
fn pending_counts(c: &Collection) -> [i64; 12] {
    let reader = c.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    pending(|sql| snapshot.execute_raw(sql, &[])).unwrap()
}
fn schema(c: &Collection) -> RawRows {
    c.reader(ReaderRole::Query).unwrap().execute_raw("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT IN ('sqlite_stat1', 'sqlite_stat4') ORDER BY type, name", &[]).unwrap()
}
fn relaunch(h: &mut Harness) {
    h.engine.close().unwrap();
    h.engine = Engine::new(EngineConfig::default()).unwrap();
    h.collection = h.engine.open_raw(&h.path, open_config(&h.clock)).unwrap();
}

#[test]
fn a_thousand_real_engine_ops_and_twenty_relaunches_preserve_schema_and_foreign_rows() {
    let mut h = fixture();
    let original_schema = schema(&h.collection);
    let original = dump(&h.collection);
    for _ in 0..10 { relaunch(&mut h); }
    let mid = common::notes::kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let options = tags::RunOptions::default();
    // Ten successful public engine calls per iteration. Answering is U43's later leg.
    for i in 0..100 {
        h.clock.set_now_ms(NOW_MS + (i + 1) * 60_000);
        let added = notes::add(&h.collection, common::notes::input(mid, &[&format!("own-{i}"), "before"]), OpSource::UserUndoable).unwrap().value;
        assert_eq!(added.cards.len(), 1);
        notes::update(&h.collection, notes::UpdateNote { id: added.id, fields: vec![format!("own-{i}"), "after".into()], tags: vec![] }, OpSource::UserUndoable).unwrap();
        tags::add_to_notes(&h.collection, &[added.id], "meguri-churn", &options).unwrap();
        flags::set_flag(&h.collection, &added.cards, 3, &options).unwrap();
        decks::add(&h.collection, format!("Churn {i}")).unwrap();
        let preset = deck_config::add(&h.collection, format!("Churn {i}")).unwrap().value;
        deck_config::rename(&h.collection, preset, format!("Churn renamed {i}")).unwrap();
        assert_eq!(notes::delete(&h.collection, vec![added.id], OpSource::UserUndoable).unwrap().unwrap().value, 1);
        h.collection.undo(UndoScope::Global).unwrap();
        h.collection.redo(UndoScope::Global).unwrap();
    }
    for _ in 0..10 { relaunch(&mut h); }
    assert_eq!(schema(&h.collection), original_schema);
    let after = dump(&h.collection);
    // User operations may add their own rows. Existing fixture records keep all bytes.
    for table in ["notes", "cards", "notetypes", "fields", "templates", "decks", "deck_config", "tags"] {
        for (key, row) in original[table].as_object().unwrap() { assert_eq!(&after[table][key], row, "{table}/{key}"); }
    }
}

#[test]
fn a_hundred_idle_relaunches_across_days_and_static_font_ensures_create_no_churn() {
    let mut h = fixture();
    let before = dump(&h.collection);
    let original_schema = schema(&h.collection);
    let name = "_meguri-preservation-font.woff2";
    let font = b"Meguri synthetic preservation font";
    let (counter, entries, modified) = {
        let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
        assert_eq!(media.store().ensure_static_media(name, font).unwrap(), name);
        let changes = media.store().pending().unwrap();
        assert_eq!(changes.len(), 1);
        assert!(media.store().acknowledge(name, changes[0].change_counter).unwrap());
        (media.store().change_counter().unwrap(), media.store().entries().unwrap(), std::fs::metadata(media.store().media_dir().join(name)).unwrap().modified().unwrap())
    };
    assert_eq!(pending_counts(&h.collection), [0; 12]);
    for i in 0..100 {
        h.clock.set_now_ms(NOW_MS + (i / 13) * 86_400_000 + i * 60_000);
        relaunch(&mut h);
        let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
        assert_eq!(media.store().ensure_static_media(name, font).unwrap(), name);
        assert!(media.store().pending().unwrap().is_empty());
        assert_eq!(media.store().change_counter().unwrap(), counter);
        assert_eq!(media.store().entries().unwrap(), entries);
        assert_eq!(std::fs::metadata(media.store().media_dir().join(name)).unwrap().modified().unwrap(), modified);
        assert_eq!(pending_counts(&h.collection), [0; 12]);
        assert_changes_allowed(&before, &dump(&h.collection), &[]).unwrap();
        assert_eq!(schema(&h.collection), original_schema);
    }
    assert!(h.clock.now_ms() > NOW_MS + 7 * 86_400_000);
}

#[test]
fn editing_one_fixture_note_changes_only_its_fields_and_stamps_and_no_op_edit_changes_nothing() {
    let h = fixture();
    let before = dump(&h.collection);
    let mut ids: Vec<i64> = before["notes"].as_object().unwrap().keys().map(|id| id.parse().unwrap()).collect();
    ids.sort();
    let id = NoteId(ids[1]); // N9: Vocab's second note.
    let note = h.collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap();
    let mut fields: Vec<_> = note.field_values().map(str::to_owned).collect();
    fields[1] = "to drink; to swallow".into();
    let edit = notes::UpdateNote { id, fields, tags: note.tags.split_whitespace().map(str::to_owned).collect() };
    h.clock.set_now_ms(NOW_MS + 60_000);
    assert!(notes::update(&h.collection, edit.clone(), OpSource::UserUndoable).unwrap().value.written);
    let after = dump(&h.collection);
    let allowed = ["/col/1/mod".into(), format!("/notes/{}/flds", id.0), format!("/notes/{}/mod", id.0), format!("/notes/{}/usn", id.0)];
    assert_changes_allowed(&before, &after, &allowed.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
    let counts = pending_counts(&h.collection);
    assert_eq!(counts[0], 1);
    assert_eq!(counts[10], 1);
    assert!(!notes::update(&h.collection, edit, OpSource::UserUndoable).unwrap().value.written);
    assert_changes_allowed(&after, &dump(&h.collection), &[]).unwrap();
}
