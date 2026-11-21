//! Product SQL for search/text-matching, fields and case-folding on a copied
//! schema-18 collection with fresh notes; the oracle scenarios
//! (`search/text`, `regex`, `fields`, `folding`) pin the same rules against
//! Anki. The note types are given to the plan as [`SearchNotetypes`] (T15).
use std::{fs, path::Path};

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::{DeckId, NotetypeId},
    rng::SeededRandom,
};
use meguri_search::{
    BuildError, ContextReads, ContextValue, ErrorKind, FieldShape, FoldMode, NotetypeShape, Param, Plan, SearchContext,
    SearchFailure, SearchMode, SearchNotetypes, parse,
};
use meguri_storage::{Initializers, OpenOptions, OpenedCollection, open_collection, raw::SqlValue};

#[derive(Clone, Copy)]
struct Options {
    normalize: bool,
    ignore_accents: bool,
    fold: FoldMode,
}

const UNICODE: Options = Options {
    normalize: true,
    ignore_accents: false,
    fold: FoldMode::Unicode,
};

const ASCII: Options = Options {
    fold: FoldMode::AsciiCompat,
    ..UNICODE
};

const ACCENTS: Options = Options {
    ignore_accents: true,
    ..UNICODE
};

fn context(options: Options) -> SearchContext {
    SearchContext {
        now: 2_000_000_000,
        today: 12,
        next_day_at: 2_000_000_100,
        learn_ahead_secs: 1200,
        current_deck: DeckId(1),
        normalize_note_text: options.normalize,
        ignore_accents: options.ignore_accents,
        fold: options.fold,
    }
}

/// A collection without notes or cards, and the note types the plan is
/// given. Notes are written with the sort text the save path stores.
struct Fixture {
    _dir: tempfile::TempDir,
    db: OpenedCollection,
    types: Vec<NotetypeShape>,
}

fn fixture() -> Fixture {
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
    for table in ["cards", "notes"] {
        db.writer().execute_raw(&format!("DELETE FROM {table}"), &[]).unwrap();
    }
    Fixture {
        _dir: dir,
        db,
        types: Vec::new(),
    }
}

#[derive(Clone, Copy)]
struct Kind<'a> {
    fields: &'a [&'a str],
    excluded: &'a [usize],
    sort_field: u32,
    cloze: bool,
}

fn standard<'a>(fields: &'a [&'a str]) -> Kind<'a> {
    Kind {
        fields,
        excluded: &[],
        sort_field: 0,
        cloze: false,
    }
}

impl Fixture {
    fn notetype(&mut self, id: i64, name: &str, kind: Kind<'_>) {
        self.types.push(NotetypeShape {
            id: NotetypeId(id),
            name: name.to_owned(),
            cloze: kind.cloze,
            sort_field: kind.sort_field,
            fields: kind
                .fields
                .iter()
                .enumerate()
                .map(|(k, name)| FieldShape {
                    name: (*name).to_owned(),
                    excluded: kind.excluded.contains(&k),
                })
                .collect(),
        });
    }

    fn note(&self, id: i64, mid: i64, fields: &[&str]) {
        let sort_field = self
            .types
            .iter()
            .find(|shape| shape.id.0 == mid)
            .map_or(0, |shape| shape.sort_field as usize);
        let sort_text = meguri_text::checksum::sort_text_of(fields, sort_field).into_owned();
        self.db
            .writer()
            .execute_raw(
                "INSERT INTO notes (id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) VALUES (?1,?1,?2,0,0,'',?3,?4,0,0,'')",
                &[
                    SqlValue::Integer(id),
                    SqlValue::Integer(mid),
                    SqlValue::Text(fields.join("\u{1f}")),
                    SqlValue::Text(sort_text),
                ],
            )
            .unwrap();
    }

    fn card(&self, id: i64, nid: i64) {
        self.db
            .writer()
            .execute_raw(
                "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data)
                 VALUES (?1,?2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
                &[SqlValue::Integer(id), SqlValue::Integer(nid)],
            )
            .unwrap();
    }

    /// T8.1 through the product's function.
    fn combining_free(&self, text: &str) -> String {
        let rows = self
            .db
            .writer()
            .execute_raw("SELECT meguri_transform(?1, 1)", &[SqlValue::Text(text.to_owned())])
            .unwrap();
        match &rows.rows[0][0] {
            SqlValue::Text(text) => text.clone(),
            other => panic!("{other:?}"),
        }
    }

    fn notetypes(&self) -> SearchNotetypes {
        SearchNotetypes::new(self.types.clone())
    }

    fn build(&self, text: &str, mode: SearchMode, options: Options) -> Result<Plan, BuildError> {
        Plan::build(&parse(text).unwrap(), mode, &context(options), &self.notetypes())
    }

    fn run(&self, plan: &Plan, text: &str) -> Vec<i64> {
        let params = plan
            .params()
            .iter()
            .map(|p| match p {
                Param::Integer(v) => SqlValue::Integer(*v),
                Param::Real(v) => SqlValue::Real(*v),
                Param::Text(v) => SqlValue::Text(v.clone()),
            })
            .collect::<Vec<_>>();
        let mut ids: Vec<i64> = self
            .db
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

    fn found(&self, text: &str, options: Options) -> Vec<i64> {
        let plan = self
            .build(text, SearchMode::Notes, options)
            .unwrap_or_else(|e| panic!("{text}: {e:?}"));
        self.run(&plan, text)
    }

    fn failure(&self, text: &str) -> ErrorKind {
        match self.build(text, SearchMode::Notes, UNICODE) {
            Err(BuildError::Search(error)) => error.kind().clone(),
            Ok(plan) => panic!("{text}: built {}", plan.sql()),
        }
    }

    /// Each (search, expected notes) row in notes mode.
    fn table(&self, options: Options, rows: &[(&str, &[i64])]) {
        for (text, expected) in rows {
            assert_eq!(self.found(text, options), *expected, "{text}");
        }
    }
}

fn failed(cause: SearchFailure) -> ErrorKind {
    ErrorKind::SearchFailed(cause)
}

/// One note type `Basic` (`Front`, `Back`); the notes hold `(front, back)`.
fn basic(notes: &[(&str, &str)]) -> Fixture {
    let mut f = fixture();
    f.notetype(1, "Basic", standard(&["Front", "Back"]));
    for (k, (front, back)) in notes.iter().enumerate() {
        f.note(k as i64 + 1, 1, &[front, back]);
    }
    f
}

// T5.2's table.
#[test]
fn plain_text_matches_the_sort_text_or_the_field_string() {
    let f = basic(&[
        ("<b>hot</b>dog", "x"),   // 1
        ("dog house", "x"),       // 2
        ("hot<b>dog</b>", "x"),   // 3: the sort field
        ("x", "hot<b>dog</b>"),   // 4: another field
        ("Tom &amp; Jerry", "x"), // 5
        ("x", "a &lt; b"),        // 6
        ("c &lt; d", "x"),        // 7: the sort field
        ("<b>x</b>", "y"),        // 8
        ("dosomethingg", "y"),    // 9
        ("dig", "y"),             // 10
        ("dg", "y"),              // 11
        ("a*b", "y"),             // 12
        ("a_b", "y"),             // 13
        ("a\\b", "y"),            // 14
        ("a%b", "y"),             // 15
        ("dog", "cat"),           // 16
        ("dog", ""),              // 17
    ]);
    f.table(
        UNICODE,
        &[
            ("dog", &[1, 2, 3, 4, 16, 17]),
            ("hotdog", &[1, 3]),
            ("&amp;", &[5]),
            ("&", &[5, 6, 7]),
            ("<", &[1, 3, 4, 7, 8]),
            ("do*g", &[1, 2, 3, 4, 9, 16, 17]),
            ("d_g", &[1, 2, 3, 4, 10, 16, 17]),
            (r"a\*b", &[12]),
            (r"a\_b", &[13]),
            (r"a\\b", &[14]),
            ("a%b", &[15]),
            ("dog*cat", &[16]),
            ("dog_cat", &[16]),
            (r#""dog cat""#, &[]),
            ("dog_", &[1, 2, 3, 4, 16, 17]),
        ],
    );
}

// T2: the sort text is the text SQLite gives back for the stored column.
#[test]
fn the_sort_text_is_read_as_sqlite_gives_it_back() {
    let f = basic(&[("<b>1</b>.50", "x"), ("<b>1</b>e3", "x"), ("<i>0</i>07", "x")]);
    f.table(
        UNICODE,
        &[
            ("1.50", &[]),
            ("1.5", &[1]),
            ("1e3", &[]),
            ("1000", &[2]),
            ("007", &[]),
            ("7", &[3]),
        ],
    );
}

// T4: stored text keeps the form it was saved in.
#[test]
fn normalization_applies_to_the_search_text_only() {
    let f = basic(&[("café", "x"), ("cafe\u{301}", "x")]);
    let off = Options {
        normalize: false,
        ..UNICODE
    };
    assert_eq!(f.found("café", off), [1]);
    assert_eq!(f.found("cafe\u{301}", off), [2]);
    assert_eq!(f.found("café", UNICODE), [1]);
    assert_eq!(f.found("cafe\u{301}", UNICODE), [1]);
    assert_eq!(f.found("front:cafe\u{301}", off), [2]);
    assert_eq!(f.found("front:cafe\u{301}", UNICODE), [1]);
}

// T3.3: the limit counts the whole pattern as tested, run patterns of note
// types without notes included, and waits for every statement check.
#[test]
fn patterns_over_fifty_thousand_bytes_fail_at_plan_time() {
    let f = basic(&[]);
    let a = |n: usize| "a".repeat(n);
    assert!(f.build(&a(49_998), SearchMode::Notes, UNICODE).is_ok());
    assert_eq!(f.failure(&a(49_999)), failed(SearchFailure::PatternTooLong));
    assert_eq!(
        f.failure(&format!("front:{}", a(49_999))),
        failed(SearchFailure::PatternTooLong)
    );
    assert!(
        f.build(&format!("front:{}", a(49_998)), SearchMode::Notes, UNICODE)
            .is_ok()
    );
    assert_eq!(
        f.failure(&format!("back:{}", a(49_999))),
        failed(SearchFailure::PatternTooLong)
    );
    // A field no note type has tests no pattern.
    assert!(
        f.build(&format!("nosuch:{}", a(60_000)), SearchMode::Notes, UNICODE)
            .is_ok()
    );
    let mut f = fixture();
    f.notetype(
        2,
        "Memo",
        Kind {
            excluded: &[2],
            ..standard(&["Front", "Back", "Notes"])
        },
    );
    assert!(f.build(&a(49_996), SearchMode::Notes, UNICODE).is_ok());
    assert_eq!(f.failure(&a(49_997)), failed(SearchFailure::PatternTooLong));
    assert_eq!(
        f.failure(&format!("{} a -and", a(60_000))),
        failed(SearchFailure::NegatedOperator)
    );
    match f.build(&format!("x {}", a(60_000)), SearchMode::Notes, UNICODE) {
        Err(BuildError::Search(error)) => assert_eq!(error.range(), 2..60_002),
        other => panic!("{other:?}"),
    }
}

/// `search/text/excluded-fields`: `Notes` is excluded in Memo, MemoSort
/// (whose sort field it is) and the cloze type Gap; Plain excludes nothing.
fn excluded_fields() -> Fixture {
    let mut f = fixture();
    let memo = ["Front", "Back", "Notes"];
    f.notetype(
        1,
        "Memo",
        Kind {
            excluded: &[2],
            ..standard(&memo)
        },
    );
    f.notetype(
        2,
        "MemoSort",
        Kind {
            excluded: &[1],
            sort_field: 1,
            ..standard(&["Front", "Notes"])
        },
    );
    f.notetype(3, "Plain", standard(&["Front", "Back"]));
    f.notetype(
        4,
        "Gap",
        Kind {
            excluded: &[1],
            cloze: true,
            ..standard(&["Text", "Notes"])
        },
    );
    f.note(1, 1, &["dog", "x", "secret"]);
    f.note(2, 2, &["dog", "secret"]);
    f.note(3, 3, &["secret", "x"]);
    f.note(4, 1, &["x", "dog", "cat"]);
    f.note(5, 3, &["dog", "cat"]);
    f.note(6, 1, &["hot<b>dog</b>", "x", "y"]);
    f.note(7, 2, &["a", "hot<b>dog</b>"]);
    f.note(8, 4, &["{{c1::a}}", "secret"]);
    f.note(9, 1, &["x", "y", "dog"]);
    f
}

// T7.2, T7.3, T7.4.
#[test]
fn excluded_fields_are_left_out_of_unqualified_searches() {
    excluded_fields().table(
        UNICODE,
        &[
            ("secret", &[3]),
            ("notes:secret", &[1, 2, 8]),
            ("re:secret", &[3]),
            ("w:secret", &[3]),
            ("nc:secret", &[3]),
            ("sc:secret", &[]),
            ("*:secret", &[1, 2, 3, 8]),
            ("hotdog", &[6]),
            ("dog*cat", &[5]),
            ("dog_cat", &[5]),
            ("x*y", &[9]),
            ("y*dog", &[]),
            ("re:^dog", &[1, 2, 4, 5]),
            ("re:^x", &[1, 4, 6, 9]),
            ("re:dog.cat", &[5]),
            ("re:x$", &[1, 3, 4, 6, 9]),
            ("w:dog", &[1, 2, 4, 5, 6]),
            ("dog", &[1, 2, 4, 5, 6]),
        ],
    );
}

// T7.2, T7.3, T16: the note types that exclude every field.
#[test]
fn excluding_every_field_fails_the_terms_that_need_one() {
    let hidden = Kind {
        excluded: &[0],
        ..standard(&["Only"])
    };
    let mut f = fixture();
    f.notetype(1, "Hidden", hidden);
    f.notetype(2, "Plain", standard(&["Front", "Back"]));
    let all_excluded = failed(SearchFailure::AllFieldsExcluded {
        notetype: "Hidden".to_owned(),
    });
    assert_eq!(f.failure("re:secret"), all_excluded);
    assert_eq!(f.failure("w:secret"), all_excluded);
    f.note(1, 1, &["secret"]);
    f.note(2, 2, &["secret", "x"]);
    assert_eq!(f.failure("-re:secret"), all_excluded);
    f.table(
        UNICODE,
        &[
            ("secret", &[2]),
            ("nc:secret", &[2]),
            ("only:secret", &[1]),
            ("only:re:secret", &[1]),
            ("*:secret", &[1, 2]),
            ("*:re:secret", &[1, 2]),
        ],
    );
    assert_eq!(f.failure("sc:secret"), failed(SearchFailure::NoClozeNoteType));

    // No note type keeps a field: plain, nc: and sc: fail too.
    let mut f = fixture();
    f.notetype(1, "Hidden", hidden);
    f.notetype(
        2,
        "Gone",
        Kind {
            excluded: &[0],
            cloze: true,
            ..standard(&["Text"])
        },
    );
    for text in ["secret", "-secret", "nc:secret", "sc:secret", "-sc:secret"] {
        assert_eq!(f.failure(text), failed(SearchFailure::NoSearchableField), "{text}");
    }
    assert!(f.build("front:secret", SearchMode::Notes, UNICODE).is_ok());
    // T16: the first by unicase name, then id.
    let mut f = fixture();
    for (id, name) in [(5, "B2"), (9, "b")] {
        f.notetype(id, name, hidden);
    }
    assert_eq!(
        f.failure("re:x"),
        failed(SearchFailure::AllFieldsExcluded {
            notetype: "b".to_owned()
        })
    );
}

// T8.3's table and the transform of the whole pattern.
#[test]
fn nc_matches_the_combining_free_texts() {
    let notes = [
        "über",
        "Über",
        "uber",
        "ば",
        "ぱ",
        "ﾊﾞ",
        "ÆON",
        "ｄｏｇ",
        "İzmir",
        "ﬁne",
        "Straße",
        "a％b",
        "azb",
        "þaw ayakkabı €xit",
        "a＼b",
        "a\\b",
        "ab",
    ];
    let mut f = fixture();
    f.notetype(1, "One", standard(&["Front"]));
    for (k, text) in notes.iter().enumerate() {
        f.note(k as i64 + 1, 1, &[text]);
    }
    f.table(
        UNICODE,
        &[
            ("nc:uber", &[1, 2, 3]),
            ("nc:über", &[1, 2, 3]),
            ("nc:ÜBER", &[1, 2, 3]),
            ("nc:は", &[4, 5]),
            ("nc:ﾊ", &[6]),
            ("nc:ハ", &[6]),
            ("nc:aeon", &[7]),
            ("nc:dog", &[8]),
            ("nc:izmir", &[9]),
            ("nc:fine", &[10]),
            ("nc:strasse", &[]),
            ("nc:strase", &[11]),
            ("nc:straße", &[11]),
            ("nc:a％b", &[12, 13, 14, 15, 16, 17]),
            ("nc:a＼b", &[14, 17]),
            ("nc:a＼＼b", &[15, 16]),
        ],
    );
}

/// Basic notes, a cloze note type and its notes.
fn cloze_fixture() -> Fixture {
    let mut f = fixture();
    f.notetype(1, "Basic", standard(&["Front", "Back"]));
    f.notetype(
        2,
        "Cloze",
        Kind {
            cloze: true,
            ..standard(&["Text", "Back Extra"])
        },
    );
    f.note(1, 2, &["{{c1::capital}} city", ""]);
    f.note(2, 1, &["capital city", ""]);
    f.note(3, 1, &["other", ""]);
    f.note(4, 2, &["{{c1::x", "y}} z"]);
    f.note(5, 2, &["{{c1::one::hint}} two", ""]);
    f.note(6, 2, &["{{c1::über}} all", ""]);
    f
}

// T9: cloze note types only, markup removed from each text alone, and a
// `-` directly before the term.
#[test]
fn sc_matches_cloze_notes_without_their_markup() {
    cloze_fixture().table(
        UNICODE,
        &[
            ("sc:capital", &[1]),
            (r#"sc:"capital city""#, &[1]),
            ("sc:hint", &[]),
            ("sc:one", &[5]),
            (r#"sc:"x_y z""#, &[4]),
            (r#"sc:"c1::""#, &[4]),
            ("-sc:capital", &[2]),
            (r#"-sc:"capital city""#, &[2]),
            ("-sc:capital -sc:city", &[2]),
            ("-(sc:capital)", &[2, 3, 4, 5, 6]),
            ("-sc:other", &[3]),
            ("-\"sc:other\"", &[3]),
        ],
    );
    let f = basic(&[("x", "y")]);
    for text in ["sc:x", "-sc:x", "-(sc:x)", "re:[ sc:x"] {
        assert_eq!(f.failure(text), failed(SearchFailure::NoClozeNoteType), "{text}");
    }
}

// T10's table.
#[test]
fn w_matches_whole_words_of_the_field_string() {
    let f = basic(&[
        ("the dog.", ""),    // 1
        ("x<b>dog</b>", ""), // 2
        ("hotdog", ""),      // 3
        ("dogs", ""),        // 4
        ("Dog_house", ""),   // 5
        ("dog", "cat"),      // 6
        ("a.b", ""),         // 7
        ("c+d", ""),         // 8
        ("日本語", ""),      // 9
        ("Über", ""),        // 10
        ("", ""),            // 11
    ]);
    let rows: &[(&str, &[i64])] = &[
        ("w:dog", &[1, 2, 6]),
        ("w:DOG", &[1, 2, 6]),
        ("w:dog*", &[1, 2, 4, 5, 6]),
        ("w:*og", &[1, 2, 3, 6]),
        ("w:dog*cat", &[6]),
        ("w:dog_cat", &[6]),
        ("w:a.b", &[7]),
        ("w:c+d", &[8]),
        ("w:日本", &[]),
        ("w:über", &[10]),
        ("w:[", &[]),
        ("w:", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
    ];
    f.table(UNICODE, rows);
    f.table(ASCII, rows);
}

// T11's table.
#[test]
fn re_matches_the_whole_field_string() {
    let f = basic(&[
        ("Dog", "x"),                // 1
        ("hotdog", "y"),             // 2
        ("<b>dog</b>", "y"),         // 3
        ("x", "dog"),                // 4
        ("a\"b", "z"),               // 5
        ("line one\nline two", "z"), // 6
    ]);
    f.table(
        UNICODE,
        &[
            ("re:dog", &[1, 2, 3, 4]),
            (r#""re:(?-i)Dog""#, &[1]),
            ("re:^dog", &[1]),
            ("re:dog$", &[4]),
            ("re:^x$", &[]),
            ("re:g.x", &[1]),
            (r#""re:\x1f""#, &[1, 2, 3, 4, 5, 6]),
            (r#"re:a\"b"#, &[5]),
            ("re:one.line", &[]),
            (r#""re:(?s)one.line""#, &[6]),
            ("re:", &[1, 2, 3, 4, 5, 6]),
        ],
    );
}

// T12: kinds, timing and ranges.
#[test]
fn invalid_expressions_fail_at_plan_time_wherever_they_stand() {
    let empty = basic(&[]);
    let invalid = |pattern: &str, reason: &str| ErrorKind::InvalidRegex {
        pattern: pattern.to_owned(),
        reason: reason.to_owned(),
    };
    assert_eq!(empty.failure(r#""re:(""#), invalid("(", "unclosed group"));
    assert_eq!(empty.failure("re:["), invalid("[", "unclosed character class"));
    for text in [
        "re:a{2,1}",
        r"re:\p{Nope}",
        "re:*",
        "front:re:[",
        "nosuchfield:re:[",
        "-re:[",
        "*:re:[",
    ] {
        assert!(matches!(empty.failure(text), ErrorKind::InvalidRegex { .. }), "{text}");
    }
    assert_eq!(
        empty.failure("re:a{1000}{1000}"),
        failed(SearchFailure::ExpressionTooLarge {
            pattern: "a{1000}{1000}".to_owned()
        })
    );
    let nested = |n: usize| format!("\"re:{}{}\"", "(".repeat(n), ")".repeat(n));
    assert!(empty.build(&nested(249), SearchMode::Notes, UNICODE).is_ok());
    assert!(matches!(empty.failure(&nested(250)), ErrorKind::InvalidRegex { .. }));
    assert!(empty.build("w:[", SearchMode::Notes, UNICODE).is_ok());
    assert!(empty.build("w:＼q", SearchMode::Notes, UNICODE).is_ok());
    assert!(matches!(
        empty.build("w:＼q", SearchMode::Notes, ACCENTS),
        Err(BuildError::Search(error)) if matches!(error.kind(), ErrorKind::InvalidRegex { .. })
    ));
    // F2 step 3: a field-name pattern over the size limit.
    let name = format!("{}:x", "_".repeat(30_000));
    assert_eq!(
        empty.failure(&name),
        failed(SearchFailure::ExpressionTooLarge {
            pattern: "_".repeat(30_000)
        })
    );
    // E4 step 3: statement checks first, then the patterns in reading order,
    // each with its whole term's range.
    assert_eq!(empty.failure("re:[ sc:x"), failed(SearchFailure::NoClozeNoteType));
    assert_eq!(
        empty.failure(r#"re:[ "re:(""#),
        invalid("[", "unclosed character class")
    );
    match empty.build("dog front:re:[", SearchMode::Notes, UNICODE) {
        Err(BuildError::Search(error)) => assert_eq!(error.range(), 4..14),
        other => panic!("{other:?}"),
    }
}

/// One-field note types named as F2's table needs, one note each.
fn named_fields() -> Fixture {
    let mut f = fixture();
    for (id, name) in [
        (1, "STRASSE"),
        (2, "Café"),
        (3, "Ünit"),
        (4, "a_b"),
        (5, "axb"),
        (6, "*"),
    ] {
        f.notetype(id, &format!("T{id}"), standard(&[name]));
        f.note(id, id, &["v"]);
    }
    f
}

// F2's table.
#[test]
fn field_names_resolve_against_every_note_type() {
    named_fields().table(
        UNICODE,
        &[
            ("straße:v", &[1]),
            ("straß*:v", &[]),
            ("stras*:v", &[1]),
            ("cafe\u{301}:v", &[2]),
            ("CAFÉ:v", &[2]),
            ("ÜNIT:v", &[3]),
            ("ÜN*:v", &[3]),
            ("a_b:v", &[4, 5]),
            (r"a\_b:v", &[4]),
            (r"\*:v", &[6]),
            ("**:v", &[1, 2, 3, 4, 5, 6]),
            ("nosuchfield:v", &[]),
            ("nosuchfield:*", &[]),
            ("-nosuchfield:v", &[1, 2, 3, 4, 5, 6]),
            ("-straße:v", &[2, 3, 4, 5, 6]),
        ],
    );
}

// F4.1 and F9.
#[test]
fn a_one_field_run_must_equal_the_whole_field() {
    let f = basic(&[("dog", ""), ("DOG", "x"), ("dig", "x"), ("hot dog", "x"), ("über", "x")]);
    f.table(
        UNICODE,
        &[
            ("front:dog", &[1, 2]),
            ("front:DOG", &[1, 2]),
            ("front:d_g", &[1, 2, 3]),
            ("front:*dog*", &[1, 2, 4]),
            ("back:", &[1]),
            ("back:_*", &[2, 3, 4, 5]),
            ("back:*_", &[2, 3, 4, 5]),
            ("back:*", &[1, 2, 3, 4, 5]),
        ],
    );
    assert_eq!(f.found("front:uber", ACCENTS), Vec::<i64>::new());
    assert_eq!(f.found("*:uber", ACCENTS), Vec::<i64>::new());
}

// F4.2's tables.
#[test]
fn neighbouring_fields_form_one_run() {
    let mut f = fixture();
    f.notetype(1, "A", standard(&["A1", "A2", "B"]));
    for (id, fields) in [
        (1, ["x", "dog", "z"]),
        (2, ["dog", "x", "z"]),
        (3, ["x", "y", "dog"]),
        (4, ["dog", "dog", "z"]),
    ] {
        f.note(id, 1, &fields);
    }
    f.table(
        UNICODE,
        &[
            ("a*:dog", &[2, 4]),
            ("a_:dog", &[2, 4]),
            ("a*:nc:dog", &[2, 4]),
            ("a1:dog", &[2, 4]),
            ("a2:dog", &[1, 4]),
            ("a*:re:dog", &[1, 2, 4]),
            ("a*:*", &[1, 2, 3, 4]),
            ("a*:_*", &[1, 2, 3, 4]),
            ("a*:*dog*", &[1, 2, 4]),
            ("a*:dog_x", &[2]),
            ("**:*dog*", &[1, 2, 3, 4]),
        ],
    );
    let mut f = fixture();
    f.notetype(1, "Y", standard(&["X", "Y1", "Y2"]));
    f.notetype(2, "Q", standard(&["P", "Q1", "Q2", "R"]));
    f.note(1, 1, &["x", "y", "dog"]);
    f.note(2, 1, &["x", "dog", "z"]);
    f.note(3, 2, &["p", "dog", "q", "r"]);
    f.note(4, 2, &["p", "q", "dog", "r"]);
    f.table(UNICODE, &[("y*:dog", &[1]), ("q*:dog", &[3, 4])]);
}

// F5, F6, F7.
#[test]
fn field_modes_and_all_fields_names() {
    let f = basic(&[
        ("apple", "x"),              // 1
        ("x", "apple"),              // 2
        ("über", ""),                // 3
        ("Über", "y"),               // 4
        ("uber", "y"),               // 5
        ("line one\nline two", "y"), // 6
    ]);
    f.table(
        UNICODE,
        &[
            ("front:re:^a", &[1]),
            ("back:re:^a", &[2]),
            ("front:re:", &[1, 2, 3, 4, 5, 6]),
            ("front:nc:uber", &[3, 4, 5]),
            ("front:nc:Uber", &[3, 4, 5]),
            ("front:nc:ÜBER", &[3, 4, 5]),
            ("front:nc:ub*", &[3, 4, 5]),
            ("*:nc:uber", &[5]),
            ("*:über", &[3, 4]),
            ("*:ÜBER", &[3, 4]),
            ("*:", &[3]),
            ("_*:", &[3]),
            ("*:line*two", &[6]),
            ("*:re:^a", &[1, 2]),
        ],
    );
}

// T6: ignore accents, and what it leaves alone.
#[test]
fn ignoring_accents_is_the_combining_free_match() {
    let mut f = fixture();
    f.notetype(1, "Basic", standard(&["Front"]));
    f.notetype(
        2,
        "Cloze",
        Kind {
            cloze: true,
            ..standard(&["Text"])
        },
    );
    for (id, mid, text) in [
        (1, 1, "über"),
        (2, 1, "ÜBER"),
        (3, 1, "uber"),
        (4, 2, "{{c1::über}} all"),
        (5, 1, "Straße"),
        (6, 1, "Привет"),
    ] {
        f.note(id, mid, &[text]);
    }
    f.table(
        ACCENTS,
        &[
            ("uber", &[1, 2, 3, 4]),
            ("über", &[1, 2, 3, 4]),
            ("w:uber", &[1, 2, 3, 4]),
            ("w:ÜBER", &[1, 2, 3, 4]),
            ("sc:uber", &[4]),
            ("front:uber", &[3]),
            ("re:uber", &[3]),
            ("*:uber", &[3]),
            ("strasse", &[]),
            ("strase", &[5]),
            ("привет", &[6]),
        ],
    );
    assert_eq!(f.found("uber", UNICODE), [3]);
    assert_eq!(f.found("sc:uber", UNICODE), Vec::<i64>::new());
}

// C5's differential table: AsciiCompat is Anki's result, Unicode Meguri's.
#[test]
fn unicode_mode_folds_every_alphabet() {
    let mut f = fixture();
    f.notetype(1, "Basic", standard(&["Front"]));
    f.notetype(
        2,
        "Cloze",
        Kind {
            cloze: true,
            ..standard(&["Text"])
        },
    );
    let stored = [
        "Über",
        "{{c1::Über}}",
        "\u{1e9e}",
        "ß",
        "STRASSE",
        "Σ",
        "ς",
        "σ",
        "ПРИВЕТ",
        "ＡＢＣ",
        "abc",
        "ǅ",
        "ſ",
        "\u{212a}",
        "İ",
        "ı",
        "ハ",
    ];
    for (k, text) in stored.iter().enumerate() {
        f.note(k as i64 + 1, if k == 1 { 2 } else { 1 }, &[text]);
    }
    let off = |fold| Options {
        normalize: false,
        ignore_accents: false,
        fold,
    };
    let rows: &[(&str, &[i64], &[i64])] = &[
        ("über", &[], &[1, 2]),
        ("ß", &[4], &[3, 4]),
        ("ς", &[7], &[6, 7, 8]),
        ("привет", &[], &[9]),
        ("ａｂｃ", &[], &[10]),
        ("ǆ", &[], &[12]),
        ("s", &[5], &[5, 13]),
        ("k", &[], &[14]),
        ("front:über", &[], &[1]),
        ("front:σ", &[8], &[6, 7, 8]),
        ("sc:über", &[], &[2]),
        ("nc:привет", &[], &[9]),
        ("nc:σ", &[8], &[6, 7, 8]),
        ("ss", &[5], &[5]),
        ("nc:ss", &[5], &[5]),
        ("abc", &[11], &[11]),
        ("i", &[], &[]),
        ("I", &[], &[]),
        ("nc:i", &[15, 16], &[15, 16]),
        ("ハ", &[17], &[17]),
        ("は", &[], &[]),
        ("re:σ", &[6, 7, 8], &[6, 7, 8]),
        ("*:σ", &[6, 7, 8], &[6, 7, 8]),
        ("re:s", &[5, 13], &[5, 13]),
        ("re:k", &[14], &[14]),
        ("re:ǆ", &[12], &[12]),
        ("re:ａｂｃ", &[10], &[10]),
        ("w:über", &[1, 2], &[1, 2]),
        ("nc:uber", &[1, 2], &[1, 2]),
    ];
    for (text, ascii, unicode) in rows {
        assert_eq!(f.found(text, off(FoldMode::AsciiCompat)), *ascii, "{text} ascii");
        assert_eq!(f.found(text, off(FoldMode::Unicode)), *unicode, "{text} unicode");
    }
}

/// A small deterministic generator (SplitMix64).
struct Seq(u64);

impl Seq {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }

    fn text(&mut self, items: &[&str], max: u64) -> String {
        let len = 1 + self.next() % max;
        (0..len).map(|_| self.pick(items)).collect()
    }
}

/// C3 equality of two characters, through the `regex` crate's `(?i)`.
fn simple_fold_equal(a: char, b: char) -> bool {
    regex::Regex::new(&format!("(?i)^{}$", regex::escape(&a.to_string())))
        .unwrap()
        .is_match(&b.to_string())
}

// C7 (`fold_props`): for a text term that is not negated, the Unicode result
// contains the AsciiCompat result, and every note found only in Unicode mode
// holds a character equal to one of the term's characters under C3 but not
// under C6. Under negation the inclusion goes the other way.
#[test]
fn fold_props() {
    let letters = [
        "a", "A", "k", "K", "\u{212a}", "s", "S", "ſ", "ß", "\u{1e9e}", "σ", "Σ", "ς", "ü", "Ü", "x", "é", "É",
    ];
    let pattern_pieces = [
        "a", "A", "k", "K", "s", "S", "ß", "σ", "Σ", "ü", "Ü", "x", "é", "*", "_", "\\*", "\\_",
    ];
    let mut f = fixture();
    f.notetype(1, "Pair", standard(&["Front", "Back"]));
    let mut seq = Seq(0x5EED_F01D);
    let mut stored = Vec::new();
    for id in 1..=60 {
        let fields = [seq.text(&letters, 4), seq.text(&letters, 3)];
        f.note(id, 1, &[&fields[0], &fields[1]]);
        stored.push(fields.join("\u{1f}"));
    }
    let ascii = Options {
        fold: FoldMode::AsciiCompat,
        ..UNICODE
    };
    let mut extra_seen = 0;
    for case in 0..400 {
        let value = seq.text(&pattern_pieces, 3);
        let prefix = ["", "nc:", "front:", "front:nc:", "front:*", "back:*"][case % 6];
        let text = format!("\"{prefix}{value}\"");
        let narrow = f.found(&text, ascii);
        let wide = f.found(&text, UNICODE);
        assert!(
            narrow.iter().all(|id| wide.contains(id)),
            "{text}: {narrow:?} not in {wide:?}"
        );
        for id in wide.iter().filter(|id| !narrow.contains(id)) {
            extra_seen += 1;
            // nc: compares the combining-free texts.
            let (note, value) = if prefix.contains("nc:") {
                (f.combining_free(&stored[(*id - 1) as usize]), f.combining_free(&value))
            } else {
                (stored[(*id - 1) as usize].clone(), value.clone())
            };
            let holds = note.chars().any(|c| {
                value
                    .chars()
                    .any(|p| simple_fold_equal(p, c) && !c.eq_ignore_ascii_case(&p))
            });
            assert!(holds, "{text} found {note:?} in Unicode mode only");
        }
        let negated = format!("-{text}");
        let narrow_not = f.found(&negated, ascii);
        let wide_not = f.found(&negated, UNICODE);
        assert!(wide_not.iter().all(|id| narrow_not.contains(id)), "{negated}");
    }
    assert!(
        extra_seen > 20,
        "the generator found too few Unicode-only matches: {extra_seen}"
    );
}

// T1, T15: what each term reads, and which terms are slow.
#[test]
fn plans_record_reads_and_slow_terms() {
    let f = basic(&[]);
    let check = |text: &str, options: Options, slow: bool, values: &[ContextValue]| {
        let plan = f.build(text, SearchMode::Notes, options).unwrap();
        assert_eq!(plan.is_slow(), slow, "{text}");
        assert_eq!(plan.reads(), values.iter().copied().collect::<ContextReads>(), "{text}");
    };
    use ContextValue::{Fold, IgnoreAccents, NormalizeNoteText};
    check("dog", UNICODE, true, &[NormalizeNoteText, IgnoreAccents, Fold]);
    check("dog", ASCII, false, &[NormalizeNoteText, IgnoreAccents, Fold]);
    check(
        "dog",
        Options {
            fold: FoldMode::AsciiCompat,
            ..ACCENTS
        },
        true,
        &[NormalizeNoteText, IgnoreAccents, Fold],
    );
    check("nc:dog", ASCII, true, &[NormalizeNoteText, Fold]);
    check("re:dog", UNICODE, true, &[NormalizeNoteText]);
    check("w:dog", UNICODE, true, &[NormalizeNoteText, IgnoreAccents]);
    check("front:dog", UNICODE, true, &[NormalizeNoteText, Fold]);
    check("front:dog", ASCII, false, &[NormalizeNoteText, Fold]);
    check("front:re:dog", UNICODE, true, &[NormalizeNoteText]);
    check("nosuch:dog", UNICODE, false, &[NormalizeNoteText, Fold]);
    check("*:dog", ASCII, true, &[NormalizeNoteText, Fold]);
}

// Q4, Q5: text terms read the note, in cards mode through its cards.
#[test]
fn cards_mode_returns_the_cards_of_matching_notes() {
    let f = basic(&[("dog", "x"), ("cat", "x")]);
    f.card(11, 1);
    f.card(12, 1);
    f.card(21, 2);
    let plan = f.build("dog OR front:cat", SearchMode::Cards, UNICODE).unwrap();
    assert_eq!(f.run(&plan, "dog OR front:cat"), [11, 12, 21]);
    let plan = f.build("dog -re:cat", SearchMode::Cards, UNICODE).unwrap();
    assert_eq!(f.run(&plan, "dog -re:cat"), [11, 12]);
}

// T14: the functions on their own.
#[test]
fn the_text_functions_follow_t14() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    meguri_search::sqlfns::register(&conn).unwrap();
    let one = |sql: &str| -> Option<i64> { conn.query_row(sql, [], |row| row.get(0)).unwrap() };
    let text = |sql: &str| -> Option<String> { conn.query_row(sql, [], |row| row.get(0)).unwrap() };
    assert_eq!(one("SELECT meguri_like('Über', '%über%', 1)"), Some(1));
    assert_eq!(one("SELECT meguri_like('Über', '%über%', 0)"), Some(0));
    assert_eq!(one("SELECT meguri_like(NULL, '%', 1)"), None);
    assert_eq!(one("SELECT meguri_like(1.5, '1.5', 1)"), Some(1));
    assert_eq!(one("SELECT meguri_like(7, '7', 0)"), Some(1));
    assert_eq!(one("SELECT meguri_like('a\\', 'a\\', 1)"), Some(0));
    assert_eq!(
        one("SELECT meguri_regex_fields('(?i)^b$', 'a' || char(31) || 'B')"),
        Some(1)
    );
    assert_eq!(
        one("SELECT meguri_regex_fields('(?i)^b$', 'a' || char(31) || 'B', 0)"),
        Some(0)
    );
    assert_eq!(
        one("SELECT meguri_regex_fields('(?i)^b$', 'a' || char(31) || 'B', 0, 1)"),
        Some(1)
    );
    assert_eq!(
        one("SELECT meguri_regex_fields('(?i)^b$', 'a' || char(31) || 'B', 5)"),
        Some(0)
    );
    assert_eq!(one("SELECT meguri_regex_fields('(?i)b', NULL, 0)"), None);
    assert_eq!(
        text("SELECT meguri_transform('{{c1::Über::hint}} x', 2)").as_deref(),
        Some("Über x")
    );
    assert_eq!(
        text("SELECT meguri_transform('{{c1::Über::hint}} x', 3)").as_deref(),
        Some("Uber x")
    );
    assert_eq!(text("SELECT meguri_transform(1.5, 0)").as_deref(), Some("1.5"));
    assert!(
        conn.query_row("SELECT meguri_transform('x', 4)", [], |row| row.get::<_, String>(0))
            .is_err()
    );
}

// T14: a REAL argument reads as SQLite's own text for it.
#[test]
fn real_arguments_read_as_sqlite_casts_them() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    meguri_search::sqlfns::register(&conn).unwrap();
    for value in [
        0.0,
        -0.0,
        1.5,
        0.1,
        1.0 / 3.0,
        1e15,
        1e16,
        1234567890123456.0,
        1e20,
        1e-5,
        9.99999999999999e-5,
        5e-324,
        f64::MAX,
        f64::INFINITY,
    ] {
        let (cast, transformed, matched): (String, String, i64) = conn
            .query_row(
                "SELECT CAST(?1 AS TEXT), meguri_transform(?1, 1), meguri_like(?1, CAST(?1 AS TEXT), 1)",
                [value],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(transformed, cast, "{value:e}");
        assert_eq!(matched, 1, "{value:e}");
    }
}

// T7.1, T15: the note types as the collection stores them.
#[test]
fn note_types_read_from_the_collection() {
    let f = fixture();
    let read = SearchNotetypes::read(f.db.writer()).unwrap();
    assert!(!read.types().is_empty());
    assert!(read.types().windows(2).all(|pair| pair[0].id.0 < pair[1].id.0));
    for shape in read.types() {
        assert!(!shape.fields.is_empty(), "{}", shape.name);
        assert!(shape.fields.iter().all(|field| !field.excluded), "{}", shape.name);
    }
}
