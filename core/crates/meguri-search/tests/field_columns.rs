//! Field columns (`specs/browser/workspace.md` V3, V4.2): the cell values
//! and V4.2's order on a mixed collection through the product SQL (numbers,
//! texts, empties, rows without the field, both directions, both modes, the
//! final key), from a plan and from an id list.

use std::{fs, path::Path};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::DeckId;
use meguri_model::rng::SeededRandom;
use meguri_search::browser::cells::Cell;
use meguri_search::browser::fields;
use meguri_search::config::SortDirection::{self, Ascending, Descending};
use meguri_search::{FoldMode, Param, Plan, SearchContext, SearchMode, SearchNotetypes, Sort, SortedQuery, parse};
use meguri_storage::{Initializers, OpenOptions, OpenedCollection, Reader, ReaderRole, open_collection, raw::SqlValue};

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

#[test]
fn numbers_are_ascii_digits_with_an_optional_sign_and_fraction() {
    for (text, value) in [
        ("0", 0.0),
        ("-0", 0.0),
        ("007", 7.0),
        ("-2.5", -2.5),
        ("2.50", 2.5),
        ("10", 10.0),
        ("123456789012345678901", 1.2345678901234568e20),
    ] {
        assert_eq!(fields::number(text), Some(value), "{text}");
    }
    for text in ["5.", ".5", "-", "", "+1", "1e3", "1,5", "--1", "1.2.3", " 1", "١", "0x1", "inf", "NaN"] {
        assert_eq!(fields::number(text), None, "{text:?}");
    }
}

#[test]
fn the_line_is_the_sort_field_cells_text() {
    for value in [
        "plain",
        "  padded\t",
        "a\nb",
        "<b>bold</b> &amp; more",
        "[sound:a.mp3] word",
        "x[[type:Back]]y",
        "\u{3000}wide\u{a0}",
        "<img src=\"cat.png\">",
    ] {
        assert_eq!(
            fields::line(value),
            meguri_text::html::text_line_keep_media(value),
            "{value:?}"
        );
    }
}

#[test]
fn cells_show_a_dash_nothing_media_or_the_line() {
    let media = |sound: Option<&str>, image: Option<&str>| Cell::Media {
        sound: sound.map(str::to_owned),
        image: image.map(str::to_owned),
    };
    assert_eq!(fields::cell(None), Cell::NoField);
    for empty in ["", " ", "<br>", "<div></div>", " <BR/>\n"] {
        assert_eq!(fields::cell(Some(empty)), Cell::Empty, "{empty:?}");
    }
    assert_eq!(fields::cell(Some("&nbsp;")), Cell::Text(String::new()), "not empty (E1)");
    assert_eq!(fields::cell(Some("<b>Dog</b>")), Cell::Text("Dog".into()));
    assert_eq!(
        fields::cell(Some("one [sound:a.mp3] [sound:b.mp3]")),
        media(Some("a.mp3"), None)
    );
    assert_eq!(
        fields::cell(Some(r#"<IMG SRC="a&amp;b.png"> <img src="c.png">"#)),
        media(None, Some("a&b.png"))
    );
    assert_eq!(
        fields::cell(Some(r#"<audio src="x.mp3"> text"#)),
        Cell::Text("x.mp3  text".into()),
        "an audio tag is no image"
    );
    assert_eq!(
        fields::cell(Some("<img src=\"p.png\">[sound:s.mp3]")),
        media(Some("s.mp3"), Some("p.png"))
    );
}

/// Fields drop in order: the reader, the collection, then the directory.
struct Db {
    reader: Reader,
    db: OpenedCollection,
    _dir: tempfile::TempDir,
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

impl Db {
    /// A copy of the schema-18 fixture with no cards, notes, note types or
    /// fields.
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
        let reader = db.open_reader(ReaderRole::Query).unwrap();
        let db = Db {
            reader,
            db,
            _dir: dir,
        };
        for table in ["cards", "notes", "notetypes", "templates", "fields"] {
            db.exec(&format!("DELETE FROM {table}"), &[]);
        }
        db
    }

    fn exec(&self, sql: &str, params: &[SqlValue]) {
        self.db.writer().execute_raw(sql, params).unwrap();
    }

    fn notetype(&self, id: i64, name: &str, field_names: &[&str]) {
        self.exec(
            "INSERT INTO notetypes (id, name, mtime_secs, usn, config) VALUES (?1, ?2, 0, 0, X'')",
            &[SqlValue::Integer(id), SqlValue::Text(name.into())],
        );
        for (ord, field) in field_names.iter().enumerate() {
            self.exec(
                "INSERT INTO fields (ntid, ord, name, config) VALUES (?1, ?2, ?3, X'')",
                &[
                    SqlValue::Integer(id),
                    SqlValue::Integer(ord as i64),
                    SqlValue::Text((*field).into()),
                ],
            );
        }
    }

    /// A note and one card per id in `cards`.
    fn note(&self, id: i64, mid: i64, fields: &[&str], cards: &[i64]) {
        self.exec(
            "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
             VALUES (?1, ?2, ?3, 0, 0, '', ?4, '', 0, 0, '')",
            &[
                SqlValue::Integer(id),
                SqlValue::Text(format!("g{id}")),
                SqlValue::Integer(mid),
                SqlValue::Text(fields.join("\u{1f}")),
            ],
        );
        for &card in cards {
            self.card(card, id);
        }
    }

    fn card(&self, id: i64, nid: i64) {
        self.exec(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (?1, ?2, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, '')",
            &[SqlValue::Integer(id), SqlValue::Integer(nid)],
        );
    }

    fn ids(&self, query: &SortedQuery) -> Vec<i64> {
        self.reader
            .execute_raw(query.sql(), &values(query.params()))
            .unwrap()
            .rows
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                ref other => panic!("not an id: {other:?}"),
            })
            .collect()
    }

    /// Every row of the mode, unsorted.
    fn unsorted(&self, mode: SearchMode) -> Vec<i64> {
        let plan = self.plan(mode);
        self.reader
            .execute_raw(plan.sql(), &values(plan.params()))
            .unwrap()
            .rows
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                ref other => panic!("not an id: {other:?}"),
            })
            .collect()
    }

    fn plan(&self, mode: SearchMode) -> Plan {
        Plan::build(&parse("").unwrap(), mode, &context(), &SearchNotetypes::default()).unwrap()
    }

    fn sorted(&self, mode: SearchMode, key: &str, direction: SortDirection) -> Vec<i64> {
        let sort = Sort::new(mode, key, direction).unwrap();
        let from_plan = self.ids(&SortedQuery::for_plan(&self.plan(mode), &sort, &context()).unwrap());
        let mut all = self.unsorted(mode);
        all.reverse();
        let from_ids = self.ids(&SortedQuery::for_ids(&all, &sort, &context()).unwrap());
        assert_eq!(from_plan, from_ids, "{mode:?} {key} {direction:?}: a resort gives the search's order");
        from_plan
    }

    /// The sorted ids grouped into the equal-key runs of `runs_sql`.
    fn runs(&self, mode: SearchMode, key: &str) -> Vec<Vec<i64>> {
        let sort = Sort::new(mode, key, Ascending).unwrap();
        let query = SortedQuery::for_plan(&self.plan(mode), &sort, &context()).unwrap();
        let mut runs: Vec<(i64, Vec<i64>)> = Vec::new();
        for row in self.reader.execute_raw(query.runs_sql(), &values(query.params())).unwrap().rows {
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

/// Note types A (Front, Back), B (Back), C (Front, back: another name) and
/// notes holding numbers, texts and empty values in Back, a note of a note
/// type that is gone, and a card whose note is gone.
fn mixed() -> Db {
    let db = Db::new();
    db.notetype(10, "A", &["Front", "Back"]);
    db.notetype(11, "B", &["Back"]);
    db.notetype(12, "C", &["Front", "back"]);
    let a = |id: i64, back: &str| db.note(id, 10, &["f", back], &[id * 10]);
    let b = |id: i64, back: &str| db.note(id, 11, &[back], &[id * 10]);
    db.note(101, 10, &["f", "10"], &[1010, 1011]);
    a(102, "9");
    b(103, "-2.5");
    b(104, "2.50");
    a(105, "2.5");
    a(106, "<b>3</b>");
    b(107, " 4 ");
    a(108, "&nbsp;");
    a(109, ".5");
    a(110, "5.");
    b(111, "apple");
    b(112, "Apple");
    a(113, "Banana");
    a(114, "éclair");
    a(115, "");
    b(116, "<br>");
    db.note(117, 12, &["f", "ZZZ"], &[1170]);
    // A damaged note: no value at Back's position.
    db.note(118, 10, &["f"], &[1180]);
    // A note whose note type is gone.
    db.note(119, 99, &["f", "1"], &[1190]);
    // A card whose note is gone: listed in cards mode, as a row without the
    // field (S9 leaves it out only under its six note-column sorts; H8).
    db.card(9990, 999);
    db
}

/// V4.2's ascending order of `mixed`'s notes by Back.
const NOTES_ASCENDING: [i64; 19] = [
    // Numbers, by value; equal numbers by text ("2.5" before "2.50").
    103, 105, 104, 106, 107, 102, 101,
    // Texts: the empty text (`&nbsp;`), then by unicase, then code point.
    108, 109, 110, 112, 111, 113, 114,
    // Empty values (E1), by id.
    115, 116, 118,
    // Rows whose note type has no field Back, by id.
    117, 119,
];

#[test]
fn rows_sort_by_presence_emptiness_numbers_then_texts_v4_2() {
    let db = mixed();
    assert_eq!(db.sorted(SearchMode::Notes, "fld:Back", Ascending), NOTES_ASCENDING);
    let mut descending = NOTES_ASCENDING.to_vec();
    descending.reverse();
    assert_eq!(
        db.sorted(SearchMode::Notes, "fld:Back", Descending),
        descending,
        "reversing reverses the whole order"
    );

    // Cards mode: each card by its note's value, then by card id.
    let mut cards: Vec<i64> = NOTES_ASCENDING
        .iter()
        .flat_map(|&note| if note == 101 { vec![1010, 1011] } else { vec![note * 10] })
        .collect();
    cards.push(9990);
    assert!(db.unsorted(SearchMode::Cards).contains(&9990));
    assert_eq!(db.sorted(SearchMode::Cards, "fld:Back", Ascending), cards);
    let mut descending = cards.clone();
    descending.reverse();
    assert_eq!(db.sorted(SearchMode::Cards, "fld:Back", Descending), descending);
}

#[test]
fn equal_keys_form_one_run_before_the_final_key_s8() {
    let db = mixed();
    let runs = db.runs(SearchMode::Notes, "fld:Back");
    assert!(runs.contains(&vec![115, 116, 118]), "{runs:?}");
    assert!(runs.contains(&vec![117, 119]), "{runs:?}");
    assert_eq!(runs.len(), 14 + 2, "every value apart: {runs:?}");
    let runs = db.runs(SearchMode::Cards, "fld:Back");
    assert!(runs.contains(&vec![1010, 1011]), "{runs:?}");
    assert!(runs.contains(&vec![1170, 1190, 9990]), "{runs:?}");
}

#[test]
fn the_name_is_matched_exactly_and_bound() {
    let db = mixed();
    db.notetype(13, "D", &["it's \"q\"; --"]);
    db.note(120, 13, &["x"], &[1200]);
    let order = db.sorted(SearchMode::Notes, "fld:it's \"q\"; --", Ascending);
    assert_eq!(order[0], 120);
    assert_eq!(order.len(), 20);
    // `back` is another name than `Back`: only note type C has it.
    let order = db.sorted(SearchMode::Notes, "fld:back", Ascending);
    assert_eq!(order[0], 117);
    // A name no note type has: every row is without the field, by id.
    let mut by_id = db.sorted(SearchMode::Notes, "fld:Nowhere", Ascending);
    let sorted = {
        let mut ids = by_id.clone();
        ids.sort();
        ids
    };
    assert_eq!(by_id, sorted);
    by_id.reverse();
    assert_eq!(db.sorted(SearchMode::Notes, "fld:Nowhere", Descending), by_id);
}

/// Folding that is not lower-casing, equal numbers and a number past the
/// binary64 range, through the product SQL.
#[test]
fn equal_folds_order_by_code_point_and_equal_numbers_by_text() {
    let db = Db::new();
    db.notetype(10, "A", &["Back"]);
    let long = format!("1{}", "0".repeat(400));
    let notes: [(i64, &str); 9] = [
        (1, "strasse"),
        (2, "Stra\u{df}e"),
        (3, "STRASSE"),
        (4, "\u{212a}"),
        (5, "k"),
        (6, "0"),
        (7, "-0"),
        (8, &long),
        (9, "99"),
    ];
    for (id, back) in notes {
        db.note(id, 10, &[back], &[id * 10]);
    }
    // Numbers: -0 equals 0, then by text ("-0" first); the long one is
    // infinite. Texts: equal folds, then code points.
    let ascending = [7, 6, 9, 8, 5, 4, 3, 2, 1];
    assert_eq!(db.sorted(SearchMode::Notes, "fld:Back", Ascending), ascending);
    let mut descending = ascending.to_vec();
    descending.reverse();
    assert_eq!(db.sorted(SearchMode::Notes, "fld:Back", Descending), descending);
}

/// A search whose plan binds parameters: the name's parameter follows them.
#[test]
fn a_sorted_search_binds_the_name_after_the_plans_parameters() {
    let db = mixed();
    let plan = Plan::build(
        &parse("deck:Default").unwrap(),
        SearchMode::Notes,
        &context(),
        &SearchNotetypes::default(),
    )
    .unwrap();
    assert!(!plan.params().is_empty(), "the predicate binds a parameter");
    let sort = Sort::new(SearchMode::Notes, "fld:Back", Ascending).unwrap();
    let query = SortedQuery::for_plan(&plan, &sort, &context()).unwrap();
    assert_eq!(query.params().len(), plan.params().len() + 1);
    assert_eq!(db.ids(&query), NOTES_ASCENDING);
}
