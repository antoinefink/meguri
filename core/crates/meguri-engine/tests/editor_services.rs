//! Focused D1/D2 regressions. The committed scenario/golden data stays inactive;
//! these tests exercise the production queries, including Meguri's presets.
mod common;

use common::{Harness, notes::{add, input}, tagging::raw};
use meguri_engine::{Collection, EngineError, OpSource, OpSpec, UndoScope, decks, editor, notetypes};
use meguri_model::{
    ids::{CardId, DeckId, NotetypeId},
    proto::{ProtoBlob, deck::{DeckKind, DeckKindMessage}},
};
use meguri_storage::{ReadRows, ReaderRole, StampMode, Table, raw::SqlValue};

fn notetype(h: &Harness, name: &str) -> NotetypeId {
    notetypes::add(&h.collection, notetypes::NewNotetype {
        name: name.into(),
        kind: notetypes::NotetypeKind::Standard,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![notetypes::NewTemplate {
            name: "Prompt".into(), front: "{{Front}}".into(), back: "{{Back}}".into(), ..Default::default()
        }],
        css: Some(String::new()),
    }).unwrap().value.id
}

fn deck(h: &Harness, name: &str) -> DeckId {
    decks::add(&h.collection, name.into()).unwrap().value.id
}

fn filtered(h: &Harness) -> DeckId {
    let id = deck(h, "Cram");
    h.collection.run(OpSpec::named("seed_filtered", OpSource::LocalApi), move |ctx| {
        let mut deck = ctx.scope().deck(id)?.unwrap();
        deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(Default::default())));
        ctx.scope().put(&deck)?;
        Ok(())
    }).unwrap();
    id
}

fn stored(c: &Collection, key: &str, value: SqlValue) {
    raw(c, "INSERT OR REPLACE INTO config (key, usn, mtime_secs, val) VALUES (?1, 0, 0, ?2)",
        &[SqlValue::Text(key.into()), value]);
}

fn put(c: &Collection, key: &str, json: impl ToString) {
    stored(c, key, SqlValue::Blob(json.to_string().into_bytes()));
}

fn expect(h: &Harness, review: Option<CardId>, preset: Option<DeckId>, deck_id: DeckId, notetype_id: NotetypeId) {
    assert_eq!(editor::add_defaults(&h.collection, review, preset).unwrap(), editor::AddDefaults { deck_id, notetype_id });
}

fn persisted(c: &Collection) -> Vec<Vec<Vec<SqlValue>>> {
    let reader = c.reader(ReaderRole::Query).unwrap();
    ["col", "config", "decks", "deck_config", "notetypes", "fields", "templates", "notes", "cards", "revlog", "graves", "tags"]
        .into_iter().map(|table| reader.execute_raw(&format!("SELECT * FROM {table} ORDER BY 1, 2"), &[]).unwrap().rows).collect()
}

#[test]
fn add_defaults_respect_real_add_memory_and_preset_precedence_without_writes() {
    let h = common::fresh();
    let alpha = notetype(&h, "alpha");
    let ink = notetype(&h, "Ink");
    let zone = notetype(&h, "Zone");
    let current = deck(&h, "Current");
    let last = deck(&h, "Last");
    let preset = deck(&h, "Preset");
    let cram = filtered(&h);
    let mut first = input(zone, &["first", "back"]);
    first.deck_id = current;
    first.update_add_defaults = true;
    add(&h, first);
    let mut second = input(ink, &["second", "back"]);
    second.deck_id = last;
    second.update_add_defaults = true;
    add(&h, second);
    put(&h.collection, "curDeck", current);
    put(&h.collection, &format!("_deck_{preset}_lastNotetype"), alpha);
    // Setup the mode first, then retain an undo step for the read-only proof.
    put(&h.collection, "addToCur", true);
    add(&h, input(ink, &["kept undo", "back"]));
    h.collection.run(
        OpSpec::named("editor_read_barrier", OpSource::Rollover).stamp(StampMode::Raw),
        |_| Ok(()),
    ).unwrap();
    let before = persisted(&h.collection);
    let seq = h.collection.last_seq();
    let undo = format!("{:?}", h.collection.peek_undo(UndoScope::Global).unwrap());
    expect(&h, None, None, current, zone);
    expect(&h, None, Some(preset), preset, alpha);
    expect(&h, None, Some(cram), current, zone);
    expect(&h, None, Some(DeckId(-77)), current, zone);
    assert_eq!(editor::notetype_last_deck(&h.collection, ink).unwrap(), Some(last));
    assert_eq!(persisted(&h.collection), before);
    assert_eq!(h.collection.last_seq(), seq);
    assert_eq!(format!("{:?}", h.collection.peek_undo(UndoScope::Global).unwrap()), undo);

    put(&h.collection, "addToCur", false);
    expect(&h, None, None, last, ink);
    // False ignores the preset deck's remembered note type and the type's last deck.
    expect(&h, None, Some(preset), preset, ink);
    expect(&h, None, Some(cram), last, ink);
    expect(&h, None, Some(DeckId(-77)), last, ink);
    put(&h.collection, &format!("_nt_{ink}_lastDeck"), cram);
    expect(&h, None, None, current, ink);
    assert_eq!(editor::notetype_last_deck(&h.collection, ink).unwrap(), None);
    put(&h.collection, &format!("_nt_{ink}_lastDeck"), -77);
    expect(&h, None, None, current, ink);
    put(&h.collection, "addToCur", true);
    put(&h.collection, &format!("_deck_{current}_lastNotetype"), -77);
    expect(&h, None, None, current, ink);
    raw(&h.collection, "DELETE FROM config WHERE key = ?1", &[SqlValue::Text(format!("_deck_{current}_lastNotetype"))]);
    expect(&h, None, None, current, ink);
}

#[test]
fn review_deck_uses_odid_else_did_then_existence_without_kind_filtering() {
    let h = common::fresh();
    let nt = notetype(&h, "Review type");
    let home = deck(&h, "Home");
    let cram = filtered(&h);
    let note = add(&h, input(nt, &["review", "back"]));
    let card = note.cards[0];
    put(&h.collection, "curModel", nt);
    put(&h.collection, "curDeck", cram);
    raw(&h.collection, "UPDATE cards SET did = ?1, odid = ?2 WHERE id = ?3",
        &[SqlValue::Integer(cram.0), SqlValue::Integer(home.0), SqlValue::Integer(card.0)]);
    expect(&h, Some(card), None, home, nt);
    raw(&h.collection, "UPDATE cards SET odid = 0 WHERE id = ?1", &[SqlValue::Integer(card.0)]);
    expect(&h, Some(card), None, cram, nt);
    raw(&h.collection, "UPDATE cards SET odid = -77 WHERE id = ?1", &[SqlValue::Integer(card.0)]);
    expect(&h, Some(card), None, DeckId(1), nt);
    raw(&h.collection, "UPDATE cards SET did = ?1, odid = 0 WHERE id = ?2",
        &[SqlValue::Integer(home.0), SqlValue::Integer(card.0)]);
    put(&h.collection, "curDeck", -77);
    expect(&h, Some(card), None, home, nt);
    expect(&h, Some(CardId(-77)), None, DeckId(1), nt);
    put(&h.collection, "curDeck", 1);
    expect(&h, Some(card), None, DeckId(1), nt);
    raw(&h.collection, "DELETE FROM config WHERE key = 'curDeck'", &[]);
    expect(&h, Some(card), None, DeckId(1), nt);
}

fn unreadable_ids(id: i64) -> Vec<SqlValue> {
    [format!("\"{id}\""), format!("{id}.0"), format!("{id}e0"), "true".into(), "null".into(),
        "-0".into(), "18446744073709551615".into(), "18446744073709551616".into(), "{".into()]
        .into_iter().map(|json| SqlValue::Blob(json.into_bytes()))
        .chain([SqlValue::Text(id.to_string())]).collect()
}

#[test]
fn strict_config_reads_fall_back_by_sql_unicase_and_do_not_coerce_id_values() {
    let h = common::fresh();
    let zulu = notetype(&h, "Zulu");
    let alpha = notetype(&h, "alpha");
    assert!(zulu < alpha); // id and binary-name order would both pick another type.
    let target = deck(&h, "Target");
    put(&h.collection, "curModel", zulu);
    put(&h.collection, "curDeck", 1);
    for invalid in unreadable_ids(zulu.0) {
        stored(&h.collection, "curModel", invalid.clone());
        expect(&h, None, None, DeckId(1), alpha);
        put(&h.collection, "curModel", alpha);
        stored(&h.collection, "_deck_1_lastNotetype", invalid);
        expect(&h, None, None, DeckId(1), alpha);
    }
    raw(&h.collection, "DELETE FROM config WHERE key IN ('curModel', '_deck_1_lastNotetype')", &[]);
    expect(&h, None, None, DeckId(1), alpha);
    put(&h.collection, "curModel", -77);
    expect(&h, None, None, DeckId(1), alpha);
    put(&h.collection, "curModel", zulu);
    expect(&h, None, None, DeckId(1), zulu);
    for invalid in unreadable_ids(target.0) {
        stored(&h.collection, "curDeck", invalid.clone());
        expect(&h, None, None, DeckId(1), zulu);
        stored(&h.collection, &format!("_nt_{zulu}_lastDeck"), invalid);
        assert_eq!(editor::notetype_last_deck(&h.collection, zulu).unwrap(), None);
        put(&h.collection, "addToCur", false);
        expect(&h, None, None, DeckId(1), zulu);
        put(&h.collection, "addToCur", true);
    }
    put(&h.collection, "curDeck", target);
    put(&h.collection, &format!("_nt_{zulu}_lastDeck"), 1);
    for invalid in ["0", "\"false\"", "null", "{"] {
        put(&h.collection, "addToCur", invalid);
        expect(&h, None, None, target, zulu); // unreadable boolean defaults true
    }
    stored(&h.collection, "addToCur", SqlValue::Text("false".into()));
    expect(&h, None, None, target, zulu);
}

#[test]
fn last_deck_ignores_preference_and_notetype_existence_but_requires_a_normal_deck() {
    let h = common::fresh();
    let target = deck(&h, "Target");
    let cram = filtered(&h);
    let leftover = NotetypeId(424242);
    for mode in [true, false] {
        put(&h.collection, "addToCur", mode);
        raw(&h.collection, "DELETE FROM config WHERE key = '_nt_424242_lastDeck'", &[]);
        assert_eq!(editor::notetype_last_deck(&h.collection, leftover).unwrap(), None);
        for (id, expected) in [(target, Some(target)), (cram, None), (DeckId(-77), None)] {
            put(&h.collection, "_nt_424242_lastDeck", id);
            assert_eq!(editor::notetype_last_deck(&h.collection, leftover).unwrap(), expected);
        }
    }
    put(&h.collection, "_nt_424242_lastDeck", target);
    raw(&h.collection, "DELETE FROM decks WHERE id = ?1", &[SqlValue::Integer(target.0)]);
    assert_eq!(editor::notetype_last_deck(&h.collection, leftover).unwrap(), None);
}

#[test]
fn missing_defaults_report_errors_in_mode_order_and_preset_can_only_fail_on_notetype() {
    let h = common::fresh();
    let nt = notetype(&h, "Only chosen type");
    let preset = deck(&h, "Preset");
    put(&h.collection, "curModel", nt);
    put(&h.collection, "curDeck", -77);
    raw(&h.collection, "DELETE FROM decks WHERE id = 1", &[]);
    put(&h.collection, &format!("_nt_{nt}_lastDeck"), preset);
    put(&h.collection, "addToCur", false);
    expect(&h, None, None, preset, nt); // No eager current-deck resolution.
    put(&h.collection, &format!("_nt_{nt}_lastDeck"), -77);
    assert!(matches!(editor::add_defaults(&h.collection, None, None), Err(EngineError::NotFound { table: Table::Decks, .. })));
    put(&h.collection, "addToCur", true);
    assert!(matches!(editor::add_defaults(&h.collection, None, None), Err(EngineError::NotFound { table: Table::Decks, .. })));
    expect(&h, None, Some(preset), preset, nt);
    for table in ["templates", "fields", "notetypes"] {
        raw(&h.collection, &format!("DELETE FROM {table}"), &[]);
    }
    assert!(matches!(editor::add_defaults(&h.collection, None, None), Err(EngineError::NotFound { table: Table::Decks, .. })));
    assert!(matches!(editor::add_defaults(&h.collection, None, Some(preset)), Err(EngineError::InvalidArgument { .. })));
    put(&h.collection, "addToCur", false);
    assert!(matches!(editor::add_defaults(&h.collection, None, None), Err(EngineError::InvalidArgument { .. })));
    assert!(matches!(editor::add_defaults(&h.collection, None, Some(preset)), Err(EngineError::InvalidArgument { .. })));
}
