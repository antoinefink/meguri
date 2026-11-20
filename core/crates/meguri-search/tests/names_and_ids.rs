//! Product SQL for search/names-and-ids N2-N7 on a copied schema-18
//! collection with fresh rows; the oracle scenarios (`search/tags`, `decks`,
//! `notetypes`, `ids`, `dupe`) pin the same rules against Anki.
use std::{fs, path::Path};

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
};
use meguri_search::{
    BuildError, ContextReads, ContextValue, ErrorKind, FoldMode, Param, Plan, SearchContext, SearchFailure,
    SearchMode, parse,
};
use meguri_storage::{Initializers, OpenOptions, OpenedCollection, open_collection, raw::SqlValue};

fn context() -> SearchContext {
    SearchContext {
        now: 2_000_000_000,
        today: 12,
        next_day_at: 2_000_000_100,
        learn_ahead_secs: 1200,
        current_deck: DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

/// The fixture with its notes, cards, decks, presets and note types removed.
fn collection() -> (tempfile::TempDir, OpenedCollection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    let clock = FixedClock::new(1_894_701_600_000, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    let mut initializers = Initializers::new();
    initializers.register(meguri_search::sqlfns::register);
    let db = open_collection(&path, &OpenOptions::new(&clock, &random).initializers(initializers)).unwrap();
    for table in ["cards", "notes", "decks", "deck_config", "notetypes", "templates", "fields"] {
        db.writer().execute_raw(&format!("DELETE FROM {table}"), &[]).unwrap();
    }
    (dir, db)
}

fn exec(db: &OpenedCollection, sql: &str, values: Vec<SqlValue>) {
    db.writer().execute_raw(sql, &values).unwrap();
}

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_owned())
}

fn note(db: &OpenedCollection, id: i64, mid: i64, tags: &str, flds: &str, csum: i64) {
    exec(
        db,
        "INSERT INTO notes (id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) VALUES (?1,?1,?2,0,0,?3,?4,'',?5,0,'')",
        vec![
            SqlValue::Integer(id),
            SqlValue::Integer(mid),
            text(tags),
            text(flds),
            SqlValue::Integer(csum),
        ],
    );
}

/// A card of note `nid` in deck `did` (home deck `odid` when it is not 0).
fn card(db: &OpenedCollection, id: i64, nid: i64, did: i64, odid: i64, ord: i64) {
    exec(
        db,
        "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data)
         VALUES (?1,?2,?3,?4,0,0,0,0,0,0,0,0,0,0,0,?5,0,'')",
        [id, nid, did, ord, odid].map(SqlValue::Integer).to_vec(),
    );
}

/// A normal deck using preset `preset` (`decks/deck-presets`: the kind's
/// field 1 holds the normal deck, whose field 1 is the preset id).
fn deck(db: &OpenedCollection, id: i64, human: &str, preset: u8) {
    exec(
        db,
        "INSERT INTO decks (id,name,mtime_secs,usn,common,kind) VALUES (?1,?2,0,0,X'',?3)",
        vec![
            SqlValue::Integer(id),
            text(&human.replace("::", "\u{1f}")),
            SqlValue::Blob(vec![0x0a, 0x02, 0x08, preset]),
        ],
    );
}

fn filtered_deck(db: &OpenedCollection, id: i64, human: &str) {
    exec(
        db,
        "INSERT INTO decks (id,name,mtime_secs,usn,common,kind) VALUES (?1,?2,0,0,X'',X'1200')",
        vec![SqlValue::Integer(id), text(human)],
    );
}

fn preset(db: &OpenedCollection, id: i64, name: &str) {
    exec(
        db,
        "INSERT INTO deck_config (id,name,mtime_secs,usn,config) VALUES (?1,?2,0,0,X'')",
        vec![SqlValue::Integer(id), text(name)],
    );
}

fn notetype(db: &OpenedCollection, id: i64, name: &str, templates: &[&str]) {
    exec(
        db,
        "INSERT INTO notetypes (id,name,mtime_secs,usn,config) VALUES (?1,?2,0,0,X'')",
        vec![SqlValue::Integer(id), text(name)],
    );
    for (ord, template) in templates.iter().enumerate() {
        exec(
            db,
            "INSERT INTO templates (ntid,ord,name,mtime_secs,usn,config) VALUES (?1,?2,?3,0,0,X'')",
            vec![SqlValue::Integer(id), SqlValue::Integer(ord as i64), text(template)],
        );
    }
}

fn plan(text: &str, mode: SearchMode) -> Plan {
    Plan::build(&parse(text).unwrap(), mode, &context()).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn ids(db: &OpenedCollection, text: &str, mode: SearchMode) -> Vec<i64> {
    let plan = plan(text, mode);
    let params = plan
        .params()
        .iter()
        .map(|p| match p {
            Param::Integer(v) => SqlValue::Integer(*v),
            Param::Real(v) => SqlValue::Real(*v),
            Param::Text(v) => SqlValue::Text(v.clone()),
        })
        .collect::<Vec<_>>();
    let mut ids: Vec<i64> = db
        .writer()
        .execute_raw(plan.sql(), &params)
        .unwrap_or_else(|e| panic!("{text}: {e:?} in {}", plan.sql()))
        .rows
        .into_iter()
        .map(|row| match row[0] {
            SqlValue::Integer(id) => id,
            ref other => panic!("noninteger id: {other:?}"),
        })
        .collect();
    ids.sort_unstable();
    ids
}

fn cards(db: &OpenedCollection, text: &str) -> Vec<i64> {
    ids(db, text, SearchMode::Cards)
}

fn notes(db: &OpenedCollection, text: &str) -> Vec<i64> {
    ids(db, text, SearchMode::Notes)
}

fn build_error(text: &str) -> ErrorKind {
    match Plan::build(&parse(text).unwrap(), SearchMode::Cards, &context()) {
        Err(BuildError::Search(error)) => error.kind().clone(),
        other => panic!("{text}: {other:?}"),
    }
}

#[test]
fn tag_values_match_whole_levels_from_the_top_with_space_free_wildcards() {
    let (_dir, db) = collection();
    // N2.2's table: one note per column.
    for (id, tags) in [
        (1, " animal::mammal "),
        (2, " animals "),
        (3, " Animal "),
        (4, ""),
        (5, " x ani "),
        (6, " x::ani "),
        (7, " r\u{a0}s "),
        (8, " c++ a*b a.b "),
    ] {
        note(&db, id, 1, tags, "f", 0);
    }
    for (query, expected) in [
        ("tag:animal", vec![1, 3]),
        ("tag:animal*", vec![1, 2, 3]),
        ("tag:ani*", vec![1, 2, 3, 5]),
        ("tag:*ani", vec![5, 6]),
        ("tag:x*i", vec![6]),
        ("tag:none", vec![4]),
        ("tag:None", vec![]),
        ("tag:*", vec![1, 2, 3, 4, 5, 6, 7, 8]),
        ("tag:_*", vec![1, 2, 3, 5, 6, 8]),
        ("tag:mammal", vec![]),
        ("tag:", vec![]),
        ("\"tag:a b\"", vec![]),
        ("tag:r", vec![]),
        ("tag:r*", vec![]),
        ("tag:r_s", vec![]),
        ("tag:re:^r", vec![7]),
        ("tag:c++", vec![8]),
        ("tag:a.b", vec![8]),
        ("tag:a\\*b", vec![8]),
        ("tag:a_b", vec![8]),
        ("tag:ANIMAL", vec![1, 3]),
    ] {
        assert_eq!(notes(&db, query), expected, "{query}");
    }
}

#[test]
fn tag_expressions_test_each_tag_and_nc_compares_combining_free_text() {
    let (_dir, db) = collection();
    for (id, tags) in [
        (1, " animal::mammal "),
        (2, " animals "),
        (3, " Animal "),
        (4, " x::ani "),
        (5, ""),
        (6, " Übung "),
        (7, " ｆｕｌｌ "),
        (8, " STRASSE "),
        (9, " a.b a*b axb "),
    ] {
        note(&db, id, 1, tags, "f", 0);
    }
    for (query, expected) in [
        ("tag:re:^ani", vec![1, 2, 3]),
        ("tag:re:a.b", vec![9]),
        ("\"tag:re:(?-i)^Animal$\"", vec![3]),
        ("tag:re:none", vec![]),
        ("tag:re:", vec![1, 2, 3, 4, 5, 6, 7, 8, 9]),
        ("tag:re:^$", vec![1, 2, 3, 4, 5, 6, 7, 8, 9]),
        ("tag:nc:ubung", vec![6]),
        ("tag:nc:UBUNG", vec![6]),
        ("tag:nc:full", vec![7]),
        ("tag:full", vec![]),
        ("tag:ＦＵＬＬ", vec![7]),
        ("tag:nc:none", vec![5]),
        ("tag:ÜBUNG", vec![6]),
        // N2.5: simple case folding, so ß never equals ss.
        ("tag:straße", vec![]),
        ("tag:strasse", vec![8]),
        // A decomposed value is composed first.
        ("tag:u\u{308}bung", vec![6]),
    ] {
        assert_eq!(notes(&db, query), expected, "{query}");
    }
}

#[test]
fn deck_names_match_the_tree_from_the_top_through_did_and_odid() {
    let (_dir, db) = collection();
    deck(&db, 10, "French", 1);
    deck(&db, 11, "French::Verbs", 1);
    deck(&db, 12, "Languages::French", 1);
    deck(&db, 13, "Frame", 1);
    deck(&db, 14, "a::b", 1);
    deck(&db, 15, "axb", 1);
    deck(&db, 16, "x:y", 1);
    deck(&db, 17, "blank", 1);
    deck(&db, 18, "über", 1);
    deck(&db, 19, "STRASSE", 1);
    deck(&db, 20, "a", 1);
    filtered_deck(&db, 30, "Cram");
    note(&db, 1, 1, "", "f", 0);
    // Card id = 100 + deck id; card 130 sits in Cram with home deck French.
    for did in [10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20] {
        card(&db, 100 + did, 1, did, 0, 0);
    }
    card(&db, 130, 1, 30, 10, 0);
    // A card whose deck row is missing.
    card(&db, 199, 1, 99, 0, 0);
    for (query, expected) in [
        ("deck:french", vec![110, 111, 130]),
        ("deck:*french", vec![110, 111, 112, 130]),
        ("deck:*::french", vec![112]),
        ("deck:french -deck:french::*", vec![110, 130]),
        ("deck:a_b", vec![114, 115]),
        ("deck:fr*", vec![110, 111, 113, 130]),
        ("\"deck:a :: b\"", vec![114]),
        ("deck:a:", vec![114, 120]),
        ("deck:x:y", vec![116]),
        ("deck:x\\:y", vec![116]),
        ("deck:", vec![117]),
        ("deck:ÜBER", vec![118]),
        ("deck:straße", vec![]),
        ("deck:filtered", vec![130]),
        ("deck:Filtered", vec![]),
        ("deck:cram", vec![130]),
        ("deck:**", vec![110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 130]),
        (
            "deck:*",
            vec![110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 130, 199],
        ),
        ("did:10", vec![110, 130]),
        ("did:30", vec![130]),
        ("did:0010,14", vec![110, 114, 130]),
        ("did:99999999999999999999", vec![]),
    ] {
        assert_eq!(cards(&db, query), expected, "{query}");
    }
}

#[test]
fn current_deck_reads_the_context_id_and_falls_back_to_the_name_default() {
    let (_dir, db) = collection();
    deck(&db, 1, "Default", 1);
    deck(&db, 2, "Default::Sub", 1);
    deck(&db, 3, "Other", 1);
    deck(&db, 4, "a.b", 1);
    deck(&db, 5, "axb", 1);
    filtered_deck(&db, 6, "Cram");
    note(&db, 1, 1, "", "f", 0);
    for did in 1..=5 {
        card(&db, 100 + did, 1, did, 0, 0);
    }
    card(&db, 106, 1, 6, 1, 0);
    let run = |current: i64| {
        let mut ctx = context();
        ctx.current_deck = DeckId(current);
        let plan = Plan::build(&parse("deck:current").unwrap(), SearchMode::Cards, &ctx).unwrap();
        assert_eq!(plan.reads(), ContextReads::NONE.with(ContextValue::CurrentDeck));
        let params: Vec<SqlValue> = plan
            .params()
            .iter()
            .map(|p| match p {
                Param::Text(v) => SqlValue::Text(v.clone()),
                other => panic!("{other:?}"),
            })
            .collect();
        let mut ids: Vec<i64> = db
            .writer()
            .execute_raw(plan.sql(), &params)
            .unwrap()
            .rows
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                _ => unreachable!(),
            })
            .collect();
        ids.sort_unstable();
        ids
    };
    assert_eq!(run(1), [101, 102, 106]);
    assert_eq!(run(3), [103]);
    // An escaped name: `.` is no wildcard.
    assert_eq!(run(4), [104]);
    // The current filtered deck finds its own cards.
    assert_eq!(run(6), [106]);
    // A missing current deck is the name `Default`, not deck id 1: rename
    // the default deck and give another deck that name.
    exec(&db, "UPDATE decks SET name = 'Renamed' WHERE id = 1", vec![]);
    exec(&db, "UPDATE decks SET name = 'default' WHERE id = 3", vec![]);
    assert_eq!(run(77), [102, 103]);
}

#[test]
fn presets_match_by_unicase_name_through_the_decks_that_use_them() {
    let (_dir, db) = collection();
    preset(&db, 1, "Default");
    preset(&db, 2, "Fast");
    preset(&db, 3, "STRASSE");
    preset(&db, 4, "fast");
    preset(&db, 5, "a\\b");
    preset(&db, 6, "Fas*");
    deck(&db, 10, "A", 1);
    deck(&db, 11, "B", 2);
    deck(&db, 12, "C", 3);
    deck(&db, 13, "D", 4);
    deck(&db, 14, "E", 5);
    deck(&db, 15, "F", 9);
    deck(&db, 16, "G", 6);
    filtered_deck(&db, 30, "Cram");
    note(&db, 1, 1, "", "f", 0);
    for did in 10..=16 {
        card(&db, 100 + did, 1, did, 0, 0);
    }
    card(&db, 130, 1, 30, 11, 0);
    for (query, expected) in [
        ("preset:fast", vec![111, 130]),
        ("preset:FAST", vec![111, 130]),
        ("preset:straße", vec![112]),
        ("preset:Default", vec![110]),
        ("preset:a\\b", vec![114]),
        ("preset:Fas*", vec![116]),
        ("preset:Nope", vec![]),
    ] {
        assert_eq!(cards(&db, query), expected, "{query}");
    }
}

#[test]
fn note_types_and_templates_match_by_name_or_ordinal() {
    let (_dir, db) = collection();
    notetype(&db, 1, "Vocab", &["Recall", "Recognize"]);
    notetype(&db, 2, "Vocab Extra", &["Recall"]);
    notetype(&db, 3, "STRASSE", &["Card 1"]);
    notetype(&db, 4, "x_y", &["Cloze"]);
    notetype(&db, 5, "xzy", &["a*b"]);
    for (nid, mid) in [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
        note(&db, nid, mid, "", "f", 0);
    }
    for (cid, nid, ord) in [(10, 1, 0), (11, 1, 1), (20, 2, 0), (30, 3, 0), (40, 4, 0), (41, 4, 1), (50, 5, 0)] {
        card(&db, cid, nid, 1, 0, ord);
    }
    for (query, expected) in [
        ("note:vocab*", vec![1, 2]),
        ("note:VOCAB", vec![1]),
        ("note:straße", vec![3]),
        ("note:straß*", vec![]),
        ("note:x\\_y", vec![4]),
        ("note:x_y", vec![4, 5]),
        ("note:*", vec![1, 2, 3, 4, 5]),
        ("note:", vec![]),
        ("mid:3", vec![3]),
    ] {
        assert_eq!(notes(&db, query), expected, "{query}");
    }
    for (query, expected) in [
        ("card:1", vec![10, 20, 30, 40, 50]),
        ("card:0", vec![10, 20, 30, 40, 50]),
        ("card:01", vec![10, 20, 30, 40, 50]),
        ("card:2", vec![11, 41]),
        ("card:recall", vec![10, 20]),
        ("card:rec*", vec![10, 11, 20]),
        // A cloze note type's one template is at ordinal 0 only.
        ("card:cloze", vec![40]),
        ("card:a\\*b", vec![50]),
        ("card:65536", vec![]),
    ] {
        assert_eq!(cards(&db, query), expected, "{query}");
    }
    assert_eq!(notes(&db, "card:2"), [1, 4]);
    assert_eq!(notes(&db, "-card:1"), [1, 4]);
}

#[test]
fn id_lists_match_cards_and_notes_in_either_mode() {
    let (_dir, db) = collection();
    for nid in [1, 2, 3] {
        note(&db, nid, 1, "", "f", 0);
    }
    for (cid, nid) in [(10, 1), (11, 1), (20, 2), (40, 4)] {
        card(&db, cid, nid, 1, 0, 0);
    }
    assert_eq!(cards(&db, "nid:1,4"), [10, 11, 40]);
    assert_eq!(notes(&db, "nid:1,3,3"), [1, 3]);
    assert_eq!(cards(&db, "cid:11,40,40"), [11, 40]);
    assert_eq!(notes(&db, "cid:11,40"), [1]);
    assert_eq!(cards(&db, "nid:1 -cid:10"), [11]);
    // Mixed with a note term, cards without a note drop out (Q5, Q6).
    assert_eq!(cards(&db, "nid:1,4 mid:1"), [10, 11]);
    assert!(!plan("nid:1", SearchMode::Cards).sql().contains("notes"));
    assert!(!plan("nid:1", SearchMode::Notes).sql().contains("cards"));
}

#[test]
fn duplicates_compare_the_stripped_first_field_through_its_checksum() {
    let (_dir, db) = collection();
    let csum = |first: &str| i64::from(meguri_text::checksum::note_checksum(first));
    for (id, mid, first) in [
        (1, 1, "Cat"),
        (2, 1, "<b>Cat</b>"),
        (3, 1, "cat"),
        (4, 1, "Cat "),
        (5, 1, "Cat[sound:x.mp3]"),
        (6, 1, "<img src=\"x.jpg\">Cat"),
        (7, 1, "Tom & Jerry"),
        (8, 1, "Tom &amp; Jerry"),
        (9, 2, "Cat"),
        (10, 1, "&lt;b&gt;Cat"),
        (11, 1, "<b></b>"),
    ] {
        note(&db, id, mid, "", &format!("{first}\u{1f}back"), csum(first));
    }
    // A note whose stored checksum does not fit its field is not found.
    note(&db, 12, 1, "", "Cat\u{1f}x", 1);
    note(&db, 13, 1, "", "other\u{1f}Cat", csum("other"));
    for (query, expected) in [
        ("dupe:1,Cat", vec![1, 2]),
        ("dupe:2,Cat", vec![9]),
        ("dupe:1,Cat[sound:x.mp3]", vec![5]),
        ("\"dupe:1,<img src=\\\"x.jpg\\\">Cat\"", vec![6]),
        ("\"dupe:1,Tom & Jerry\"", vec![7, 8]),
        ("dupe:1,&lt;b&gt;Cat", vec![10]),
        ("dupe:1,<b>Cat", vec![1, 2]),
        ("dupe:1,", vec![11]),
    ] {
        assert_eq!(notes(&db, query), expected, "{query}");
    }
    let plan = plan("dupe:1,Cat", SearchMode::Notes);
    assert_eq!(plan.reads(), ContextReads::NONE.with(ContextValue::NormalizeNoteText));
    assert!(!plan.is_slow());
}

#[test]
fn only_tag_terms_are_slow_and_names_read_no_context() {
    for (query, slow) in [
        ("tag:x", true),
        ("tag:re:x", true),
        ("tag:nc:x", true),
        ("tag:none", false),
        ("deck:x*", false),
        ("preset:x", false),
        ("note:x*", false),
        ("card:x", false),
        ("nid:1", false),
        ("mid:1", false),
    ] {
        let plan = plan(query, SearchMode::Cards);
        assert_eq!(plan.is_slow(), slow, "{query}");
        assert_eq!(plan.reads(), ContextReads::NONE, "{query}");
    }
}

#[test]
fn glob_expressions_over_the_size_limit_fail_at_plan_time_after_statement_checks() {
    let many = "_".repeat(30_000);
    for key in ["deck", "note", "card", "tag", "tag:nc"] {
        let query = format!("{key}:{many}");
        assert_eq!(
            build_error(&query),
            ErrorKind::SearchFailed(SearchFailure::ExpressionTooLarge { pattern: many.clone() }),
            "{key}"
        );
    }
    assert_eq!(
        build_error("tag:re:["),
        ErrorKind::InvalidRegex {
            pattern: "[".to_owned(),
            reason: "unclosed character class".to_owned()
        }
    );
    // E4 step 3: every statement check before any pattern check.
    assert_eq!(
        build_error(&format!("-tag:re:[ deck:{many} -and")),
        ErrorKind::SearchFailed(SearchFailure::NegatedOperator)
    );
    assert_eq!(
        build_error("tag:re:[ nid:١"),
        ErrorKind::SearchFailed(SearchFailure::IdList {
            context: "nid:".to_owned()
        })
    );
    // The pattern checks run in reading order.
    assert!(matches!(
        build_error(&format!("\"tag:re:(\" deck:{many}")),
        ErrorKind::InvalidRegex { .. }
    ));
}
