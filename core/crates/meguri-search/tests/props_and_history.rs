//! Product SQL for search/properties P2-P16 and search/history H1-H10 on a
//! copied schema-18 collection with fresh rows. The `search/props` and
//! `search/history` scenarios pin the same rules against Anki.
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

/// 2030-01-15 04:00 UTC, day 0's start (H9).
const B0: i64 = 1_894_680_000;
const DAY: i64 = 86_400;

/// Day 10 (P4) or day 2 (H9), one second before its end at `next_day_at`.
fn context(today: u32, next_day_at: i64) -> SearchContext {
    SearchContext {
        now: next_day_at - 1,
        today,
        next_day_at,
        learn_ahead_secs: 1200,
        current_deck: DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

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
    for table in ["cards", "notes", "revlog"] {
        db.writer().execute_raw(&format!("DELETE FROM {table}"), &[]).unwrap();
    }
    (dir, db)
}

/// The scheduling columns of a card; the rest are 0 and `data` is given.
#[derive(Clone, Copy, Default)]
struct Card {
    card_type: i64,
    queue: i64,
    due: i64,
    odue: i64,
    ivl: i64,
    factor: i64,
    reps: i64,
    lapses: i64,
}

fn card(db: &OpenedCollection, id: i64, card: Card, data: &str) {
    let mut values: Vec<SqlValue> = [
        id,
        card.card_type,
        card.queue,
        card.due,
        card.odue,
        card.ivl,
        card.factor,
        card.reps,
        card.lapses,
    ]
    .map(SqlValue::Integer)
    .to_vec();
    values.push(SqlValue::Text(data.to_owned()));
    db.writer()
        .execute_raw(
            "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,odue,ivl,factor,reps,lapses,left,odid,flags,data)
             VALUES (?1,?1,1,0,0,0,?2,?3,?4,?5,?6,?7,?8,?9,0,0,0,?10)",
            &values,
        )
        .unwrap();
}

fn revlog(db: &OpenedCollection, id: i64, cid: i64, ease: i64) {
    db.writer()
        .execute_raw(
            "INSERT INTO revlog (id,cid,usn,ease,ivl,lastIvl,factor,time,type) VALUES (?1,?2,0,?3,0,0,0,0,0)",
            &[id, cid, ease].map(SqlValue::Integer),
        )
        .unwrap();
}

fn plan(text: &str, ctx: &SearchContext) -> Plan {
    Plan::build(&parse(text).unwrap(), SearchMode::Cards, ctx).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn ids_in(db: &OpenedCollection, text: &str, mode: SearchMode, ctx: &SearchContext) -> Vec<i64> {
    let plan = Plan::build(&parse(text).unwrap(), mode, ctx).unwrap_or_else(|e| panic!("{text}: {e:?}"));
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

fn ids(db: &OpenedCollection, text: &str, ctx: &SearchContext) -> Vec<i64> {
    ids_in(db, text, SearchMode::Cards, ctx)
}

fn review(due: i64) -> Card {
    Card {
        card_type: 2,
        queue: 2,
        due,
        ..Card::default()
    }
}

#[test]
fn column_properties_compare_every_card_type() {
    let (_dir, db) = collection();
    let ctx = context(10, B0 + 11 * DAY);
    for (id, ivl, factor, reps, lapses) in [(1, 0, 0, 0, 0), (2, 5, 2500, 3, 1), (3, 30, 1300, 9, 0), (4, 5, 1299, 1, 2)] {
        card(
            &db,
            id,
            Card {
                ivl,
                factor,
                reps,
                lapses,
                ..Card::default()
            },
            "",
        );
    }
    for (query, expected) in [
        ("prop:ivl=0", vec![1]),
        ("prop:ivl>=5", vec![2, 3, 4]),
        ("prop:reps>1", vec![2, 3]),
        ("prop:lapses!=0", vec![2, 4]),
        ("prop:ease=2.5", vec![2]),
        ("prop:ease=1.3", vec![3]),
        ("prop:ease>=1.2999", vec![2, 3, 4]),
        ("prop:ease<1", vec![1]),
        ("prop:ease!=2.5", vec![1, 3, 4]),
        ("prop:ease>inf", vec![]),
        ("prop:ease>nan", vec![2, 3, 4]),
        ("prop:ease<-1", vec![]),
    ] {
        assert_eq!(ids(&db, query, &ctx), expected, "{query}");
        let plan = plan(query, &ctx);
        assert!(!plan.is_slow() && plan.reads().is_empty(), "{query}");
    }
}

#[test]
fn due_days_use_the_home_due_and_whole_days_from_the_boundary() {
    let (_dir, db) = collection();
    let next_day_at = B0 + 11 * DAY;
    let ctx = context(10, next_day_at);
    let learning = |due: i64, odue: i64| Card {
        card_type: 1,
        queue: 1,
        due,
        odue,
        ..Card::default()
    };
    // P4's table, day 10, one second before the boundary.
    card(&db, 1, review(9), "");
    card(&db, 2, review(10), "");
    card(&db, 3, review(11), "");
    card(&db, 4, learning(next_day_at - 86_401, 0), "");
    card(&db, 5, learning(next_day_at - 86_400, 0), "");
    card(&db, 6, learning(ctx.now + 600, 0), "");
    card(&db, 7, Card { odue: 9, ..review(-100_000) }, "");
    card(&db, 8, learning(5, next_day_at - 86_401), "");
    card(&db, 9, learning(next_day_at - 18 * 3600, 0), "");
    card(
        &db,
        10,
        Card {
            card_type: 1,
            queue: 3,
            due: 9,
            ..Card::default()
        },
        "",
    );
    // New, suspended and buried cards never match.
    card(&db, 11, Card { queue: 0, ..review(10) }, "");
    card(&db, 12, Card { queue: -1, ..review(10) }, "");
    card(&db, 13, Card { queue: -3, ..review(10) }, "");
    for (query, expected) in [
        ("prop:due=-1", vec![1, 4, 5, 7, 8, 10]),
        ("prop:due=0", vec![2, 6, 9]),
        ("prop:due=1", vec![3]),
        ("prop:due>=0", vec![2, 3, 6, 9]),
    ] {
        assert_eq!(ids(&db, query, &ctx), expected, "{query}");
    }
    assert_eq!(
        plan("prop:due=0", &ctx).reads(),
        ContextReads::NONE.with(ContextValue::Today).with(ContextValue::NextDayAt)
    );
    // The 32-bit wrap: review days are compared with -2147483638.
    let ctx = context(11, next_day_at);
    assert_eq!(ids(&db, "prop:due<2147483647", &ctx), [4, 5, 6, 8, 9]);
}

#[test]
fn due_extremes_follow_sqlite_arithmetic_without_wrapping() {
    let (_dir, db) = collection();
    let ctx = context(10, B0 + 11 * DAY);
    card(
        &db,
        1,
        Card {
            card_type: 1,
            queue: 1,
            due: -9_223_372_036_854_775_000,
            ..Card::default()
        },
        "",
    );
    card(
        &db,
        2,
        Card {
            card_type: 1,
            queue: 1,
            due: i64::MAX,
            ..Card::default()
        },
        "",
    );
    assert_eq!(ids(&db, "prop:due<0", &ctx), [1]);
    assert_eq!(ids(&db, "prop:due>=0", &ctx), [2]);
    assert_eq!(ids(&db, "prop:due<-2147483648", &ctx), [1]);
    // A negative next-day-at overflows the upper end instead.
    let ctx = SearchContext {
        next_day_at: -1_000_000,
        ..ctx
    };
    assert_eq!(ids(&db, "prop:due<0", &ctx), [1]);
    assert_eq!(ids(&db, "prop:due>2147483647", &ctx), [2]);
}

#[test]
fn positions_are_new_cards_by_type_on_their_home_due() {
    let (_dir, db) = collection();
    let ctx = context(10, B0 + 11 * DAY);
    let new = |queue: i64, due: i64, odue: i64| Card {
        queue,
        due,
        odue,
        ..Card::default()
    };
    card(&db, 1, new(0, 5, 0), "");
    card(&db, 2, new(-1, 7, 0), "");
    card(&db, 3, new(-3, 9, 0), "");
    card(&db, 4, new(0, -100_000, 6), "");
    card(&db, 5, review(5), r#"{"pos":5}"#);
    assert_eq!(ids(&db, "prop:pos<=6", &ctx), [1, 4]);
    assert_eq!(ids(&db, "prop:pos>6", &ctx), [2, 3]);
    assert_eq!(ids(&db, "prop:pos=5", &ctx), [1]);
}

#[test]
fn memory_state_properties_compare_with_the_operand_decimal() {
    let (_dir, db) = collection();
    let ctx = context(30, B0 + 31 * DAY);
    for (id, data) in [
        (1, r#"{"s":2.3,"d":3.7}"#),
        (2, r#"{"s":0.1,"d":2}"#),
        (3, r#"{"s":5}"#),
        (4, r#"{"s":1e40,"d":1}"#),
        (5, "{}"),
        (6, "garbage"),
        (7, r#"{"s":5,"d":5,"cd":5}"#),
    ] {
        card(&db, id, Card::default(), data);
    }
    // P7's table; unknown rows are found by neither the term nor -term.
    for (query, expected, negated) in [
        ("prop:s>1", vec![1, 3, 4], vec![2]),
        ("prop:s=2.3", vec![], vec![1, 2, 3, 4]),
        ("prop:s<2.3", vec![1, 2], vec![3, 4]),
        ("prop:d>0.3", vec![1], vec![2, 4]),
    ] {
        assert_eq!(ids(&db, query, &ctx), expected, "{query}");
        assert_eq!(ids(&db, &format!("-{query}"), &ctx), negated, "-{query}");
        let plan = plan(query, &ctx);
        assert!(plan.is_slow() && plan.reads().is_empty(), "{query}");
    }
    // P6: the stored 0.1 (0.10000000149...) lies above the decimal 0.1.
    assert_eq!(ids(&db, "prop:s>0.1", &ctx), [1, 2, 3, 4]);
}

#[test]
fn retrievability_is_false_for_new_cards_and_unknown_without_memory() {
    let (_dir, db) = collection();
    let ctx = context(30, B0 + 31 * DAY);
    let lrt = ctx.now - DAY;
    card(&db, 1, Card::default(), &format!(r#"{{"s":21,"d":5,"lrt":{lrt}}}"#));
    card(&db, 2, review(35), &format!(r#"{{"s":21,"d":5,"lrt":{lrt}}}"#));
    card(&db, 3, review(35), "{}");
    card(&db, 4, review(35), r#"{"s":5}"#);
    assert_eq!(ids(&db, "prop:r>0.99", &ctx), [2]);
    assert_eq!(ids(&db, "prop:r<0.99", &ctx), Vec::<i64>::new());
    assert_eq!(ids(&db, "-prop:r<0.9", &ctx), [1, 2]);
    assert_eq!(
        plan("prop:r<0.9", &ctx).reads(),
        ContextReads::NONE.with(ContextValue::Now).with(ContextValue::Today)
    );
}

#[test]
fn custom_data_values_are_text_or_numbers() {
    let (_dir, db) = collection();
    let ctx = context(10, B0 + 11 * DAY);
    let rows = [
        r#"{"k":"7","v":"abc"}"#,
        r#"{"k":5}"#,
        r#"{"k":1.5}"#,
        r#"{"k":"5x"}"#,
        r#"{"k":" 7"}"#,
        r#"{"k":"abc"}"#,
        r#"{"k":true}"#,
        r#"{"k":null}"#,
        r#"{"k":""}"#,
        r#"{"k":"1","k":"9"}"#,
        r#"{"K":1}"#,
        r#"{"other":1}"#,
        "[1,2]",
        r#"{"k":1e2}"#,
    ];
    for (index, cd) in rows.iter().enumerate() {
        let data = format!(r#"{{"cd":{}}}"#, serde_json::to_string(cd).unwrap());
        card(&db, index as i64 + 1, Card::default(), &data);
    }
    card(&db, 20, Card::default(), "{}");
    for (query, expected) in [
        ("prop:cdn:k>=5", vec![1, 2, 4, 5, 10, 14]),
        ("prop:cdn:k=0", vec![6, 7, 8, 9]),
        ("has-cd:k", vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 14]),
        ("-has-cd:k", vec![11, 12, 13, 20]),
        ("prop:cds:k=true", vec![7]),
        ("prop:cds:k=null", vec![8]),
        ("prop:cds:k=", vec![9]),
        ("\"prop:cds:k= 7\"", vec![5]),
        ("prop:cds:k=100.0", vec![14]),
        ("prop:cds:k=100", vec![]),
        ("prop:cdn:k=100", vec![14]),
        ("prop:cds:k>5", vec![1, 4, 6, 7, 8, 10]),
        ("has-cd:K", vec![11]),
    ] {
        assert_eq!(ids(&db, query, &ctx), expected, "{query}");
        let plan = plan(query, &ctx);
        assert!(plan.is_slow() && plan.reads().is_empty(), "{query}");
    }
}

#[test]
fn non_finite_operands_fail_the_plan_with_the_term_text() {
    let ctx = context(10, B0 + 11 * DAY);
    let failure = |text: &str| match Plan::build(&parse(text).unwrap(), SearchMode::Cards, &ctx) {
        Err(BuildError::Search(error)) => (error.kind().clone(), error.range()),
        other => panic!("{text}: {other:?}"),
    };
    let out_of_range = |term: &str| ErrorKind::SearchFailed(SearchFailure::NumberOutOfRange { term: term.to_owned() });
    assert_eq!(failure("prop:s>inf"), (out_of_range("prop:s>inf"), 0..10));
    assert_eq!(failure("a -prop:r<-inf"), (out_of_range("prop:r<-inf"), 3..14));
    assert_eq!(failure("\"prop:s>nan\""), (out_of_range("\"prop:s>nan\""), 0..12));
    assert_eq!(failure("prop:cdn:k>inf").0, out_of_range("prop:cdn:k>inf"));
    assert_eq!(failure("prop:d>1e38").0, out_of_range("prop:d>1e38"));
    // E4 step 3: a statement check before an earlier term's pattern check.
    assert_eq!(failure("tag:re:[ prop:s>inf").0, out_of_range("prop:s>inf"));
}

#[test]
fn history_windows_count_back_from_the_next_day_boundary() {
    let (_dir, db) = collection();
    // H9: searches on day 2, one second before B3.
    let (b1, b2, b3) = (B0 + DAY, B0 + 2 * DAY, B0 + 3 * DAY);
    let ctx = context(2, b3);
    let ms = |secs: i64| secs * 1000;
    for cid in [1, 2, 3, 4, 5, 6, 7, 10, 11] {
        card(&db, cid, review(0), "");
    }
    revlog(&db, ms(b2), 1, 3);
    revlog(&db, ms(b2) - 1, 2, 3);
    revlog(&db, ms(b2 + 5), 3, 0);
    revlog(&db, ms(b1 + 100), 4, 2);
    revlog(&db, ms(B0 + 100), 5, 3);
    revlog(&db, ms(b2 + 100), 5, 3);
    revlog(&db, ms(b2 + 200), 6, 1);
    revlog(&db, ms(B0 + 300), 7, 0);
    revlog(&db, ms(b2 + 300), 7, 4);
    revlog(&db, ms(b3 + 100), 10, 3);
    revlog(&db, ms(b3 + 200), 10, 0);
    // H1: other clients' buttons; 5 is an answer, -1 only a first-answer
    // candidate.
    revlog(&db, ms(b2 + 400), 11, -1);
    revlog(&db, ms(b2 + 500), 11, 5);
    for (query, expected) in [
        ("rated:1", vec![1, 5, 6, 7, 10, 11]),
        ("rated:0", vec![1, 5, 6, 7, 10, 11]),
        ("introduced:1", vec![6, 7, 10, 11]),
        ("resched:1", vec![3, 10]),
        ("resched:0", vec![10]),
        ("prop:rated=0", vec![1, 5, 6, 7, 11]),
        ("prop:rated!=0", vec![2, 4, 5, 10]),
        ("prop:rated>-1", vec![1, 5, 6, 7, 10, 11]),
        ("prop:rated=-1", vec![2, 4]),
        ("prop:rated<-1", vec![5]),
        ("prop:rated<=-1", vec![2, 4, 5]),
        ("prop:rated>=-1", vec![1, 2, 4, 5, 6, 7, 10, 11]),
        ("prop:rated=-1:2", vec![4]),
        ("rated:2:2", vec![4]),
        ("rated:1:1", vec![6]),
        ("prop:resched=0", vec![3]),
        ("prop:resched=-2", vec![7]),
        ("-introduced:1", vec![1, 2, 3, 4, 5]),
    ] {
        assert_eq!(ids(&db, query, &ctx), expected, "{query}");
        let plan = plan(query, &ctx);
        assert_eq!(plan.reads(), ContextReads::NONE.with(ContextValue::NextDayAt), "{query}");
        assert!(!plan.is_slow(), "{query}");
    }
    // At B3 the window moves a day.
    let ctx = context(3, B0 + 4 * DAY);
    assert_eq!(ids(&db, "rated:1", &ctx), [10]);
    assert_eq!(ids(&db, "prop:rated=-1", &ctx), [1, 5, 6, 7, 11]);
}

#[test]
fn added_and_edited_exclude_the_boundary_instant() {
    let (_dir, db) = collection();
    let (b1, b2) = (B0 + DAY, B0 + 2 * DAY);
    let ctx = context(1, b2);
    for (id, mod_secs) in [(1, b1 - 1), (2, b1), (3, b1 + 1)] {
        db.writer()
            .execute_raw(
                "INSERT INTO notes (id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) VALUES (?1,?1,1,?2,0,'','f','',0,0,'')",
                &[SqlValue::Integer(id), SqlValue::Integer(mod_secs)],
            )
            .unwrap();
    }
    for (id, nid) in [(b1 * 1000 - 1000, 1), (b1 * 1000, 2), (b1 * 1000 + 1000, 3)] {
        db.writer()
            .execute_raw(
                "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,odue,ivl,factor,reps,lapses,left,odid,flags,data)
                 VALUES (?1,?2,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,'')",
                &[SqlValue::Integer(id), SqlValue::Integer(nid)],
            )
            .unwrap();
    }
    assert_eq!(ids(&db, "added:1", &ctx), [b1 * 1000 + 1000]);
    assert_eq!(ids_in(&db, "added:1", SearchMode::Notes, &ctx), [3]);
    assert_eq!(ids_in(&db, "edited:1", SearchMode::Notes, &ctx), [3]);
    assert_eq!(ids_in(&db, "edited:2", SearchMode::Notes, &ctx), [1, 2, 3]);
    assert_eq!(ids_in(&db, "added:0", SearchMode::Notes, &ctx), [3]);
}
