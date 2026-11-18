//! Focused defaults D1/D2 and tag completion C1–C4 regressions.
//! The committed scenario/golden data stays inactive;
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

fn stored_tag(c: &Collection, name: &str) {
    raw(c, "INSERT INTO tags(tag, usn, collapsed, config) VALUES (?1, -1, 1, NULL)",
        &[SqlValue::Text(name.into())]);
}

fn complete(c: &Collection, input: &str, limit: u64) -> Vec<String> {
    editor::tag_complete(c, input, limit, &meguri_engine::CancelToken::default()).unwrap()
}

#[test]
fn tag_completion_cuts_whole_registry_rows_before_stable_prefix_partition_without_writes() {
    let h = common::tagging::fresh();
    // No notes use these rows; Bar is an implicit parent, never a suggestion.
    for name in ["Grammar::Japan", "Japan", "Japanese::Audio", "Bar::x", "Aardvark"] {
        stored_tag(&h.collection, name);
    }
    // Keep a real undo step and settle opening rollover before the read proof.
    let nt = notetype(&h, "Completion type");
    add(&h, input(nt, &["kept undo", "back"]));
    h.collection.run(
        OpSpec::named("completion_read_barrier", OpSource::Rollover).stamp(StampMode::Raw),
        |_| Ok(()),
    ).unwrap();
    let before = persisted(&h.collection);
    let seq = h.collection.last_seq();
    let undo = format!("{:?}", h.collection.peek_undo(UndoScope::Global).unwrap());
    assert_eq!(complete(&h.collection, "jap", 1), ["Grammar::Japan"]);
    assert_eq!(complete(&h.collection, "jap", 2), ["Japan", "Grammar::Japan"]);
    assert_eq!(complete(&h.collection, "jap", 3), ["Japan", "Japanese::Audio", "Grammar::Japan"]);
    // Japanese takes the inner 'a' in Japanese, before Audio can be tried.
    assert_eq!(complete(&h.collection, "a", 500), ["Aardvark", "Bar::x", "Grammar::Japan", "Japan", "Japanese::Audio"]);
    assert_eq!(complete(&h.collection, "bar", 500), ["Bar::x"]);
    assert_eq!(complete(&h.collection, "jap::aud", 500), ["Japanese::Audio"]);
    assert!(complete(&h.collection, "aud::jap", 500).is_empty());
    assert_eq!(persisted(&h.collection), before);
    assert_eq!(h.collection.last_seq(), seq);
    assert_eq!(format!("{:?}", h.collection.peek_undo(UndoScope::Global).unwrap()), undo);
}

#[test]
fn tag_completion_simple_folds_without_normalizing_or_rewriting_raw_components() {
    let h = common::tagging::fresh();
    let raw_name = " \u{1}q:::::r::o\u{301}::";
    for name in ["Straße", "Σοφία", "Ｇｒａｍｍａｒ", "Über", "K::ſ", "e\u{301}", "é", "İ", "ı", "Ǆ", "П", "は", raw_name, "a:::b", "German::Verb::modal"] {
        stored_tag(&h.collection, name);
    }
    for (query, expected) in [
        ("STRAẞE", Some("Straße")), ("strasse", None),
        ("σοφ", Some("Σοφία")), ("ςοφ", Some("Σοφία")),
        ("ｇｒａ", Some("Ｇｒａｍｍａｒ")), ("gra", None),
        ("über", Some("Über")), ("k::s", Some("K::ſ")),
        ("e\u{301}", Some("e\u{301}")), ("É", Some("é")),
        ("i", None), ("ǆ", Some("Ǆ")), ("п", Some("П")), ("ハ", None),
        (raw_name, Some(raw_name)), (" \u{1}q:::::r", Some(raw_name)),
        ("a:::b", Some("a:::b")), ("q::r::", Some(raw_name)),
        ("verb::mod", Some("German::Verb::modal")), ("mod::verb", None),
        ("ger::mod", Some("German::Verb::modal")),
    ] {
        let expected: Vec<&str> = expected.into_iter().collect();
        assert_eq!(complete(&h.collection, query, 500), expected, "{query:?}");
    }
    // Repeated empty filters consume separate raw components, including the
    // empty component between four colons and the trailing empty component.
    assert_eq!(complete(&h.collection, "::::::", 500), [raw_name]);
    assert!(complete(&h.collection, " \u{1}q:::::r::ó", 500).is_empty());
}

#[test]
fn tag_completion_streams_names_stops_before_unread_rows_and_checks_exact_cancellation() {
    use std::{cell::Cell, ops::ControlFlow};
    use meguri_engine::CancelToken;

    let h = common::tagging::fresh();
    for name in ["_x", "A", "b", "ä"] {
        stored_tag(&h.collection, name);
    }
    // The query reads only names, not unrelated columns in a Tag aggregate.
    raw(&h.collection, "UPDATE tags SET collapsed = 7, config = X'00' WHERE tag = 'A'", &[]);
    for limit in [1_u64 << 32, u64::MAX] {
        assert_eq!(complete(&h.collection, "", limit), ["_x", "A", "b", "ä"]);
    }
    // A BLOB key sorts after all TEXT keys. Decoding it would fail: a finite
    // limit must stop stepping the statement before reaching this sentinel.
    raw(&h.collection, "INSERT INTO tags(tag, usn, collapsed) VALUES (X'ff', 0, 1)", &[]);
    assert_eq!(complete(&h.collection, "a", 1), ["A"]);
    assert_eq!(complete(&h.collection, "", 4), ["_x", "A", "b", "ä"]);
    assert!(editor::tag_complete(&h.collection, "", 5, &CancelToken::default()).is_err());

    let cancel = CancelToken::new();
    {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.snapshot().unwrap();
        let visits = Cell::new(0);
        let result: Result<(), EngineError> = snapshot.visit_tag_names(|| cancel.check(), |_| {
            visits.set(visits.get() + 1);
            cancel.cancel();
            Ok(ControlFlow::Continue(()))
        });
        assert_eq!(result, Err(EngineError::Cancelled));
        assert_eq!(visits.get(), 1);
    }
    assert_eq!(editor::tag_complete(&h.collection, "", 500, &cancel), Err(EngineError::Cancelled));
    h.engine.close().unwrap();
    // Zero must not even acquire a reader/snapshot on this closed collection.
    assert!(complete(&h.collection, "", 0).is_empty());
    assert!(editor::tag_complete(&h.collection, "", 1, &CancelToken::default()).is_err());
}
