//! `browser/sorting` S2 to S10 on product SQL, and the Meguri-only S11
//! properties: for every sortable column and mode, the reversed ascending
//! result equals the descending one (S8), the final key makes every order
//! total, a plan and an id list give the same order, and no sort creates a
//! schema object (S10). Fresh rows in a copied schema-18 collection, read
//! through a `query_only` reader with the product's SQL functions.
use std::{fs, path::Path};

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
};
use meguri_search::config::{Column, Refusal, SortDirection};
use meguri_search::{
    ContextValue, FoldMode, Param, Plan, SearchContext, SearchMode, Sort, SortColumn, SortError, SortedQuery, parse,
};
use meguri_storage::{Initializers, OpenOptions, OpenedCollection, ReaderRole, open_collection, raw::SqlValue};

const NOW: i64 = 1_894_701_600;

fn context() -> SearchContext {
    SearchContext {
        now: NOW,
        today: 0,
        next_day_at: NOW + 3600,
        learn_ahead_secs: 1200,
        current_deck: DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

struct Db {
    _dir: tempfile::TempDir,
    db: OpenedCollection,
}

fn values(params: &[Param]) -> Vec<SqlValue> {
    params
        .iter()
        .map(|p| match p {
            Param::Integer(v) => SqlValue::Integer(*v),
            Param::Real(v) => SqlValue::Real(*v),
            Param::Text(v) => SqlValue::Text(v.clone()),
        })
        .collect()
}

/// A card row: everything S4 to S7 read.
#[derive(Clone, Copy)]
struct C {
    id: i64,
    nid: i64,
    did: i64,
    ord: i64,
    ctype: i64,
    queue: i64,
    due: i64,
    ivl: i64,
    factor: i64,
    reps: i64,
    lapses: i64,
    modified: i64,
    odue: i64,
    odid: i64,
    data: &'static str,
}

const CARD: C = C {
    id: 0,
    nid: 0,
    did: 1,
    ord: 0,
    ctype: 0,
    queue: 0,
    due: 1,
    ivl: 0,
    factor: 0,
    reps: 0,
    lapses: 0,
    modified: 0,
    odue: 0,
    odid: 0,
    data: "",
};

impl Db {
    /// A copy of the schema-18 fixture with no cards, notes, decks, note
    /// types or templates.
    fn new() -> Db {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
            &path,
        )
        .unwrap();
        let clock = FixedClock::new(NOW * 1000, UtcOffset::UTC);
        let random = SeededRandom::new(1);
        let mut initializers = Initializers::new();
        initializers.register(meguri_search::sqlfns::register);
        let db = open_collection(&path, &OpenOptions::new(&clock, &random).initializers(initializers)).unwrap();
        let db = Db { _dir: dir, db };
        for table in ["cards", "notes", "decks", "notetypes", "templates", "fields"] {
            db.exec(&format!("DELETE FROM {table}"), &[]);
        }
        db
    }

    fn exec(&self, sql: &str, params: &[SqlValue]) {
        self.db.writer().execute_raw(sql, params).unwrap();
    }

    fn deck(&self, id: i64, name: &str) {
        self.exec(
            "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (?1, ?2, 0, 0, X'', X'')",
            &[SqlValue::Integer(id), SqlValue::Text(name.into())],
        );
    }

    fn notetype(&self, id: i64, name: &str, templates: &[&str]) {
        self.exec(
            "INSERT INTO notetypes (id, name, mtime_secs, usn, config) VALUES (?1, ?2, 0, 0, X'')",
            &[SqlValue::Integer(id), SqlValue::Text(name.into())],
        );
        for (ord, template) in templates.iter().enumerate() {
            self.exec(
                "INSERT INTO templates (ntid, ord, name, mtime_secs, usn, config) VALUES (?1, ?2, ?3, 0, 0, X'')",
                &[
                    SqlValue::Integer(id),
                    SqlValue::Integer(ord as i64),
                    SqlValue::Text((*template).into()),
                ],
            );
        }
    }

    fn note(&self, id: i64, mid: i64, sfld: SqlValue, tags: &str, modified: i64) {
        self.exec(
            "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, '', ?6, 0, 0, '')",
            &[
                SqlValue::Integer(id),
                SqlValue::Text(format!("g{id}")),
                SqlValue::Integer(mid),
                SqlValue::Integer(modified),
                SqlValue::Text(tags.into()),
                sfld,
            ],
        );
    }

    fn card(&self, c: C) {
        self.exec(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13, ?14, 0, ?15)",
            &[
                SqlValue::Integer(c.id),
                SqlValue::Integer(c.nid),
                SqlValue::Integer(c.did),
                SqlValue::Integer(c.ord),
                SqlValue::Integer(c.modified),
                SqlValue::Integer(c.ctype),
                SqlValue::Integer(c.queue),
                SqlValue::Integer(c.due),
                SqlValue::Integer(c.ivl),
                SqlValue::Integer(c.factor),
                SqlValue::Integer(c.reps),
                SqlValue::Integer(c.lapses),
                SqlValue::Integer(c.odue),
                SqlValue::Integer(c.odid),
                SqlValue::Text(c.data.into()),
            ],
        );
    }

    fn rows(&self, sql: &str, params: &[Param]) -> Vec<Vec<SqlValue>> {
        let reader = self.db.open_reader(ReaderRole::Query).unwrap();
        reader.execute_raw(sql, &values(params)).unwrap().rows
    }

    fn ids(&self, sql: &str, params: &[Param]) -> Vec<i64> {
        self.rows(sql, params)
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                ref other => panic!("not an id: {other:?}"),
            })
            .collect()
    }

    fn plan(&self, text: &str, mode: SearchMode) -> Plan {
        Plan::build(&parse(text).unwrap(), mode, &context()).unwrap()
    }

    fn sorted(&self, text: &str, mode: SearchMode, key: &str, direction: SortDirection) -> Vec<i64> {
        let sort = Sort::new(mode, key, direction).unwrap();
        let query = SortedQuery::for_plan(&self.plan(text, mode), &sort, &context()).unwrap();
        self.ids(query.sql(), query.params())
    }

    fn asc(&self, mode: SearchMode, key: &str) -> Vec<i64> {
        self.sorted("", mode, key, SortDirection::Ascending)
    }

    fn desc(&self, mode: SearchMode, key: &str) -> Vec<i64> {
        self.sorted("", mode, key, SortDirection::Descending)
    }

    fn sorted_ids(&self, ids: &[i64], mode: SearchMode, key: &str, direction: SortDirection) -> Vec<i64> {
        let sort = Sort::new(mode, key, direction).unwrap();
        let query = SortedQuery::for_ids(&sort, &context()).unwrap();
        self.ids(query.sql(), &[SortedQuery::ids_param(ids)])
    }

    /// The sorted ids grouped into the runs of `runs_sql`.
    fn runs(&self, mode: SearchMode, key: &str, direction: SortDirection) -> Vec<Vec<i64>> {
        let sort = Sort::new(mode, key, direction).unwrap();
        let query = SortedQuery::for_plan(&self.plan("", mode), &sort, &context()).unwrap();
        let mut runs: Vec<(i64, Vec<i64>)> = Vec::new();
        for row in self.rows(query.runs_sql(), query.params()) {
            let (SqlValue::Integer(id), SqlValue::Integer(run)) = (&row[0], &row[1]) else {
                panic!("{row:?}");
            };
            match runs.last_mut() {
                Some((key, ids)) if key == run => ids.push(*id),
                _ => runs.push((*run, vec![*id])),
            }
        }
        runs.into_iter().map(|(_, ids)| ids).collect()
    }
}

/// A collection that sets every column apart somewhere: decks, note types
/// and templates with ranks, notes with numbers and texts in `sfld`, card
/// states, card data, a noteless card and a note without cards.
fn mixed() -> Db {
    let db = Db::new();
    db.deck(1, "Default");
    db.deck(2, "B");
    db.deck(3, "a\u{1f}x");
    db.deck(4, "Cram");
    db.notetype(10, "Zed", &["Card 1", "Card 2"]);
    db.notetype(11, "alpha", &["card 1"]);
    db.notetype(12, "Cloze", &["Cloze"]);
    db.note(100, 10, SqlValue::Text("Banana".into()), " b ", 50);
    db.note(101, 11, SqlValue::Integer(12), "", 40);
    db.note(102, 12, SqlValue::Text("apple".into()), " A z ", 50);
    db.note(103, 10, SqlValue::Text("Apple".into()), " a ", 60);
    db.note(104, 11, SqlValue::Text(String::new()), "", 10);
    let card = |id, nid| C { id, nid, ..CARD };
    db.card(C { due: 5, ..card(1, 100) });
    db.card(C { ord: 1, ctype: 2, queue: 2, due: -3, ivl: 10, factor: 2500, reps: 4, lapses: 1, modified: 7, did: 2, data: r#"{"s":10,"d":5}"#, ..card(2, 100) });
    db.card(C { ctype: 1, queue: 1, due: NOW + 600, factor: 0, reps: 1, modified: 7, did: 3, ..card(3, 101) });
    db.card(C { ord: 1, ctype: 2, queue: -1, due: 2, ivl: 3, factor: 2300, reps: 9, lapses: 3, modified: 9, data: r#"{"s":2.5,"d":3,"lrt":1894600000}"#, ..card(4, 102) });
    db.card(C { ord: 9, ctype: 2, queue: 2, due: -100000, odue: 4, odid: 2, did: 4, ivl: 20, factor: 2500, reps: 2, modified: 1, data: r#"{"pos":3}"#, ..card(5, 102) });
    db.card(C { due: 2, queue: -2, data: r#"{"s":1}"#, ..card(6, 103) });
    db.card(C { ctype: 3, queue: 3, due: 1, ivl: 0, factor: 1300, reps: 20, lapses: 5, modified: 3, ..card(7, 103) });
    // A card whose note row is missing (S9).
    db.card(C { due: 9, ..card(8, 999) });
    db
}

const MODES: [SearchMode; 2] = [SearchMode::Cards, SearchMode::Notes];

fn sortable(mode: SearchMode) -> impl Iterator<Item = Column> {
    Column::ALL.into_iter().filter(move |column| column.sortable(mode))
}

#[test]
fn reversing_an_ascending_sort_gives_the_descending_one_s8() {
    let db = mixed();
    for mode in MODES {
        for column in sortable(mode) {
            let mut ascending = db.asc(mode, column.key());
            ascending.reverse();
            assert_eq!(ascending, db.desc(mode, column.key()), "{mode:?} {column:?}");
        }
    }
}

#[test]
fn every_order_is_total_and_keeps_the_result_set_s8_s9() {
    let db = mixed();
    let note_sorts = [
        Column::SortField,
        Column::CardType,
        Column::Created,
        Column::NoteModified,
        Column::Notetype,
        Column::Tags,
    ];
    for mode in MODES {
        let mut all = db.ids(db.plan("", mode).sql(), &[]);
        all.sort_unstable();
        for column in sortable(mode) {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                let order = db.sorted("", mode, column.key(), direction);
                let mut set = order.clone();
                set.sort_unstable();
                set.dedup();
                assert_eq!(set.len(), order.len(), "{mode:?} {column:?}: each id once");
                // S9: only the note-column sorts leave out card 8.
                let expected: Vec<i64> = if mode == SearchMode::Cards && note_sorts.contains(&column) {
                    all.iter().copied().filter(|id| *id != 8).collect()
                } else {
                    all.clone()
                };
                assert_eq!(set, expected, "{mode:?} {column:?} {direction:?}");
                // The same ids in any order sort to the same order (H8).
                let mut shuffled = all.clone();
                shuffled.reverse();
                shuffled.push(123_456);
                assert_eq!(
                    db.sorted_ids(&shuffled, mode, column.key(), direction),
                    order,
                    "{mode:?} {column:?} {direction:?}"
                );
            }
        }
    }
}

#[test]
fn rows_equal_on_every_key_follow_the_final_id_key_s8() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(10, "N", &["T"]);
    for (id, nid) in [(30, 300), (10, 100), (20, 200)] {
        db.note(nid, 10, SqlValue::Text("same".into()), "", 5);
        db.card(C { id, nid, modified: 4, ..CARD });
    }
    assert_eq!(db.asc(SearchMode::Cards, "cardMod"), [10, 20, 30]);
    assert_eq!(db.desc(SearchMode::Cards, "cardMod"), [30, 20, 10]);
    assert_eq!(db.runs(SearchMode::Cards, "cardMod", SortDirection::Descending), [vec![30, 20, 10]]);
    assert_eq!(db.asc(SearchMode::Notes, "noteFld"), [100, 200, 300]);
}

#[test]
fn no_sort_creates_a_schema_object_s10() {
    let db = mixed();
    let objects = |db: &Db| {
        db.rows(
            "SELECT type, name FROM sqlite_master UNION ALL SELECT type, name FROM sqlite_temp_master ORDER BY 1, 2",
            &[],
        )
    };
    let before = objects(&db);
    for mode in MODES {
        for column in sortable(mode) {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                // Each runs on a query_only reader (storage/open-and-connections).
                db.sorted("", mode, column.key(), direction);
                db.sorted_ids(&[1, 2, 100], mode, column.key(), direction);
                let sort = Sort::new(mode, column.key(), direction).unwrap();
                let query = SortedQuery::for_plan(&db.plan("", mode), &sort, &context()).unwrap();
                db.rows(query.runs_sql(), query.params());
            }
        }
    }
    assert_eq!(objects(&db), before);
}

#[test]
fn sort_field_orders_numbers_first_then_nocase_text_s2() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(10, "N", &["T"]);
    let fields = [
        SqlValue::Text("éclair".into()),
        SqlValue::Text("zebra".into()),
        SqlValue::Text("banana".into()),
        SqlValue::Text("Banana".into()),
        SqlValue::Text("Apple".into()),
        SqlValue::Text("apple".into()),
        SqlValue::Text(String::new()),
        SqlValue::Integer(12),
        SqlValue::Integer(10),
        SqlValue::Integer(9),
        SqlValue::Real(1.5),
        SqlValue::Integer(-4),
    ];
    for (k, field) in fields.into_iter().enumerate() {
        let id = k as i64 + 1;
        db.note(100 + id, 10, field, "", 0);
        db.card(C { id, nid: 100 + id, ..CARD });
    }
    // -4, 1.5, 9, 10, 12, empty, apple and Apple (equal: card id), Banana
    // and banana (equal), zebra, éclair.
    assert_eq!(db.asc(SearchMode::Cards, "noteFld"), [12, 11, 10, 9, 8, 7, 5, 6, 3, 4, 2, 1]);
    assert_eq!(
        db.runs(SearchMode::Cards, "noteFld", SortDirection::Ascending),
        [vec![12], vec![11], vec![10], vec![9], vec![8], vec![7], vec![5, 6], vec![3, 4], vec![2], vec![1]]
    );
}

#[test]
fn ranks_order_names_under_unicase_with_subdecks_after_their_parent_s3() {
    let db = Db::new();
    db.notetype(10, "N", &["T"]);
    db.note(100, 10, SqlValue::Text("x".into()), "", 0);
    let names = ["Ä", "Z", "B\u{1f}c", "B", "AB", "a2", "A B", "A\u{1f}B", "A", "_x"];
    for (k, name) in names.iter().enumerate() {
        let id = k as i64 + 1;
        db.deck(id, name);
        db.card(C { id, nid: 100, did: id, ord: id, ..CARD });
    }
    // A card in a deck that does not exist has no rank: NULL first.
    db.card(C { id: 50, nid: 100, did: 77, ord: 50, ..CARD });
    assert_eq!(db.asc(SearchMode::Cards, "deck"), [50, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
}

#[test]
fn card_type_ranks_templates_then_ordinal_so_cloze_2_precedes_cloze_10_s4() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(20, "Std B", &["card 1"]);
    db.notetype(10, "Std A", &["Card 1", "Back"]);
    db.notetype(30, "Cloze", &["Cloze"]);
    db.note(200, 20, SqlValue::Text("x".into()), "", 0);
    db.note(100, 10, SqlValue::Text("x".into()), "", 0);
    db.note(300, 30, SqlValue::Text("x".into()), "", 0);
    db.card(C { id: 1, nid: 200, ..CARD });
    db.card(C { id: 2, nid: 100, ..CARD });
    db.card(C { id: 3, nid: 100, ord: 1, ..CARD });
    db.card(C { id: 4, nid: 300, ord: 9, ..CARD });
    db.card(C { id: 5, nid: 300, ord: 1, ..CARD });
    // Back; Card 1 of note type 10 before card 1 of 20 (by note type id);
    // Cloze by ordinal (the template at ordinal 0).
    assert_eq!(db.asc(SearchMode::Cards, "template"), [3, 2, 1, 5, 4]);
    assert_eq!(db.desc(SearchMode::Cards, "template"), [4, 5, 1, 2, 3]);
}

#[test]
fn due_puts_new_positions_timestamps_and_days_on_one_scale_s6() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(10, "N", &["T"]);
    db.note(100, 10, SqlValue::Text("x".into()), "", 0);
    let cards = [
        C { due: 5, ..CARD },
        C { ctype: 1, queue: 1, due: NOW + 600, ..CARD },
        C { ctype: 2, queue: 2, due: -3, ..CARD },
        C { ctype: 2, queue: 2, due: 0, ..CARD },
        C { ctype: 1, queue: 3, due: 1, ..CARD },
        C { ctype: 2, queue: 2, due: -100_000, odid: 1, odue: 3, ..CARD },
        C { due: -99_999, odid: 1, odue: 2, ..CARD },
        C { ctype: 2, queue: -1, due: 1, ..CARD },
    ];
    for (k, card) in cards.into_iter().enumerate() {
        db.card(C { id: k as i64 + 1, nid: 100, ..card });
    }
    assert_eq!(db.asc(SearchMode::Cards, "cardDue"), [6, 7, 1, 3, 4, 2, 5, 8]);
    let sort = Sort::new(SearchMode::Cards, "cardDue", SortDirection::Ascending).unwrap();
    let query = SortedQuery::for_plan(&db.plan("", SearchMode::Cards), &sort, &context()).unwrap();
    assert!(query.reads().contains(ContextValue::Today) && query.reads().contains(ContextValue::Now));
}

#[test]
fn notes_mode_aggregates_over_all_cards_and_null_notes_come_first_ascending_s5_s9() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(10, "N", &["T", "U"]);
    for nid in [100, 200, 300] {
        db.note(nid, 10, SqlValue::Text("x".into()), "", 0);
    }
    // 100: only new cards (no Due); 200: a review due in 2 days and a new
    // card; 300: a review due today. Note 400 has no card.
    db.note(400, 10, SqlValue::Text("x".into()), "", 0);
    db.card(C { id: 1, nid: 100, ..CARD });
    db.card(C { id: 2, nid: 200, ..CARD });
    db.card(C { id: 3, nid: 200, ord: 1, ctype: 2, queue: 2, due: 2, factor: 2500, reps: 3, ..CARD });
    db.card(C { id: 4, nid: 300, ctype: 2, queue: 2, due: 0, factor: 2500, reps: 1, ..CARD });
    assert_eq!(db.asc(SearchMode::Notes, "cardDue"), [100, 400, 300, 200]);
    assert_eq!(db.desc(SearchMode::Notes, "cardDue"), [200, 300, 400, 100]);
    assert_eq!(
        db.runs(SearchMode::Notes, "cardDue", SortDirection::Ascending),
        [vec![100, 400], vec![300], vec![200]]
    );
    // S9: `is:new` matches notes 100 and 200; 200 still sorts by its review card.
    assert_eq!(
        db.sorted("is:new", SearchMode::Notes, "cardDue", SortDirection::Descending),
        [200, 100]
    );
    // Equal aggregates keep note id order: 200 and 300 both average 2500.
    assert_eq!(db.asc(SearchMode::Notes, "cardEase"), [100, 400, 200, 300]);
    assert_eq!(db.asc(SearchMode::Notes, "template"), [400, 100, 300, 200]);
}

#[test]
fn position_uses_the_first_card_of_the_note_s5_1() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(10, "N", &["T", "U"]);
    for nid in [100, 200, 300] {
        db.note(nid, 10, SqlValue::Text("x".into()), "", 0);
    }
    // 100's first card is new at 7; 200's first card is a review without
    // `pos` (P 0); 300's first card carries `pos` 3.
    db.card(C { id: 1, nid: 100, due: 7, ..CARD });
    db.card(C { id: 2, nid: 200, ctype: 2, queue: 2, due: 4, ..CARD });
    db.card(C { id: 3, nid: 300, ctype: 2, queue: 2, due: 4, data: r#"{"pos":3}"#, ..CARD });
    db.card(C { id: 4, nid: 100, ord: 1, due: 1, ..CARD });
    db.card(C { id: 5, nid: -5, due: -100_000, ..CARD });
    assert_eq!(db.asc(SearchMode::Cards, "originalPosition"), [5, 2, 3, 1, 4]);
    assert_eq!(db.asc(SearchMode::Notes, "originalPosition"), [200, 300, 100]);
}

#[test]
fn unsortable_columns_and_unknown_keys_are_refused_but_field_columns_are_not_s1() {
    for mode in MODES {
        for key in ["question", "answer", "bogus", "", "NoteFld", "fld"] {
            assert_eq!(
                Sort::new(mode, key, SortDirection::Ascending),
                Err(Refusal::UnsortableColumn),
                "{key}"
            );
        }
        for key in ["fld:Front", "fld:", "fld:a:b"] {
            let sort = Sort::new(mode, key, SortDirection::Descending).unwrap();
            assert_eq!(
                sort.column(),
                &SortColumn::Field(key.strip_prefix("fld:").unwrap().to_owned())
            );
            assert!(matches!(
                SortedQuery::for_ids(&sort, &context()),
                Err(SortError::FieldColumnNotYetSupported { .. })
            ));
        }
    }
    for key in ["stability", "difficulty", "retrievability"] {
        assert!(Sort::new(SearchMode::Cards, key, SortDirection::Ascending).is_ok());
        assert_eq!(
            Sort::new(SearchMode::Notes, key, SortDirection::Ascending),
            Err(Refusal::UnsortableColumn)
        );
    }
}

#[test]
fn memory_state_keys_read_card_data_on_their_own_s4_s7() {
    let db = Db::new();
    db.deck(1, "Default");
    db.notetype(10, "N", &["T"]);
    db.note(100, 10, SqlValue::Text("x".into()), "", 0);
    let cards = [
        (1, r#"{"s":5}"#),
        (2, ""),
        (3, r#"{"s":2,"d":7,"lrt":1894615200}"#),
        (4, r#"{"s":20,"d":2,"lrt":1894615200}"#),
        (5, r#"{"s":5,"d":5,"cd":5}"#),
    ];
    for (id, data) in cards {
        db.card(C { id, nid: 100, ctype: 2, queue: 2, due: 0, ivl: 1, data, ..CARD });
    }
    // `s` without `d` sorts by `s`; a non-string `cd` reads as no keys.
    assert_eq!(db.asc(SearchMode::Cards, "stability"), [2, 5, 3, 1, 4]);
    assert_eq!(db.asc(SearchMode::Cards, "difficulty"), [1, 2, 5, 4, 3]);
    // One day elapsed: the lower stability has the lower retrievability;
    // no memory state is NULL.
    assert_eq!(db.asc(SearchMode::Cards, "retrievability"), [1, 2, 5, 3, 4]);
}
