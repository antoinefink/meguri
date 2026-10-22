//! Product SQL for search/state-and-flags S1-S4 and query-model Q4-Q9.
//! Fresh synthetic rows in a copied schema-18 collection; no stand-in matcher.
use std::{fs, path::Path};

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
};
use meguri_search::{ContextReads, ContextValue, FoldMode, Param, Plan, SearchContext, SearchMode, parse};
use meguri_storage::{OpenOptions, OpenedCollection, open_collection, raw::SqlValue};

fn context() -> SearchContext {
    SearchContext {
        now: 2_000_000_000,
        today: 12,
        // S1: the intraday cutoff crosses this boundary without changing today.
        next_day_at: 2_000_000_100,
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
    let db = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    db.writer().execute_raw("DELETE FROM cards", &[]).unwrap();
    db.writer().execute_raw("DELETE FROM notes", &[]).unwrap();
    (dir, db)
}

/// id, nid, type, queue, due, odue, odid, did. Raw integers retain the full
/// domain and unusual type/queue pairs instead of normalizing them as models.
fn card(db: &OpenedCollection, values: [i64; 8]) {
    db.writer()
        .execute_raw(
            "INSERT INTO cards (id,nid,type,queue,due,odue,odid,did,ord,mod,usn,ivl,factor,reps,lapses,left,flags,data)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0,0,0,0,0,0,0,0,0,'')",
            &values.map(SqlValue::Integer),
        )
        .unwrap();
}

fn plan(text: &str, mode: SearchMode, ctx: &SearchContext) -> Plan {
    Plan::build(&parse(text).unwrap(), mode, ctx).unwrap()
}

fn ids(db: &OpenedCollection, text: &str, mode: SearchMode, ctx: &SearchContext) -> Vec<i64> {
    let plan = plan(text, mode, ctx);
    let params = plan
        .params()
        .iter()
        .map(|p| match p {
            Param::Integer(v) => SqlValue::Integer(*v),
            Param::Real(v) => SqlValue::Real(*v),
            Param::Text(v) => SqlValue::Text(v.clone()),
        })
        .collect::<Vec<_>>();
    let mut ids = db
        .writer()
        .execute_raw(plan.sql(), &params)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| {
            let SqlValue::Integer(id) = row[0] else {
                panic!("noninteger id: {row:?}")
            };
            id
        })
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

#[test]
fn stored_queues_and_inclusive_due_cutoffs_ignore_type_deck_and_original_due() {
    let (_dir, db) = collection();
    let mut due_ids = Vec::new();
    let mut buried_ids = Vec::new();
    let mut id = 1000;
    for (queue, due, matches_due, matches_buried) in [
        (2, 11, true, false),
        (2, 12, true, false),
        (2, 13, false, false),
        (3, 11, true, false),
        (3, 12, true, false),
        (3, 13, false, false),
        (1, 2_000_001_199, true, false),
        (1, 2_000_001_200, true, false),
        (1, 2_000_001_201, false, false),
        (4, 2_000_001_199, true, false),
        (4, 2_000_001_200, true, false),
        (4, 2_000_001_201, false, false),
        (0, 0, false, false),
        (-1, 0, false, false),
        (-2, 0, false, true),
        (-3, i64::MAX, false, true),
        (99, 0, false, false),
        (-4, i64::MIN, false, false),
        (2, -1, true, false),
        (1, -1, true, false),
    ] {
        for card_type in [-7, 0, 1, 2, 3, 99] {
            for odid in [0, 80] {
                // The opposite cutoff side would invert results if odue were read.
                let odue = if matches_due { i64::MAX } else { i64::MIN };
                card(&db, [id, id, card_type, queue, due, odue, odid, -900]);
                if matches_due {
                    due_ids.push(id);
                }
                if matches_buried {
                    buried_ids.push(id);
                }
                id += 1;
            }
        }
    }
    let before = db
        .writer()
        .execute_raw("SELECT * FROM cards ORDER BY id", &[])
        .unwrap()
        .rows;
    assert_eq!(ids(&db, "is:due", SearchMode::Cards, &context()), due_ids);
    assert_eq!(ids(&db, "is:buried", SearchMode::Cards, &context()), buried_ids);
    assert_eq!(
        db.writer()
            .execute_raw("SELECT * FROM cards ORDER BY id", &[])
            .unwrap()
            .rows,
        before
    );
}

#[test]
fn full_integer_context_domain_has_no_wrap_float_promotion_or_day_cap() {
    let (_dir, db) = collection();
    for (id, queue, due) in [
        (1, 2, 4_294_967_295),
        (2, 3, 4_294_967_296),
        (3, 1, i64::MAX),
        (4, 4, i64::MAX),
        (5, 2, -1),
        (6, 1, 0),
        (7, 4, 1),
        (8, 1, i64::MIN),
        (9, 4, i64::MIN + i64::from(u32::MAX)),
        (10, 4, i64::MIN + i64::from(u32::MAX) + 1),
    ] {
        card(&db, [id, id, 99, queue, due, 0, 0, 999]);
    }
    let mut ctx = context();
    ctx.today = u32::MAX;
    ctx.now = i64::MAX;
    ctx.learn_ahead_secs = u32::MAX;
    assert_eq!(
        plan("is:due", SearchMode::Cards, &ctx).params(),
        [Param::Integer(4_294_967_295), Param::Integer(i64::MAX)]
    );
    assert_eq!(
        ids(&db, "is:due", SearchMode::Cards, &ctx),
        [1, 3, 4, 5, 6, 7, 8, 9, 10]
    );
    ctx.today = 0;
    ctx.now = 0;
    ctx.learn_ahead_secs = 0;
    assert_eq!(ids(&db, "is:due", SearchMode::Cards, &ctx), [5, 6, 8, 9, 10]);
    ctx.now = i64::MIN;
    ctx.learn_ahead_secs = u32::MAX;
    assert_eq!(ids(&db, "is:due", SearchMode::Cards, &ctx), [5, 8, 9]);
}

#[test]
fn boolean_card_and_note_queries_share_per_card_truth_and_exact_context_reads() {
    let (_dir, db) = collection();
    db.writer()
        .execute_raw(
            "INSERT INTO notes (id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) VALUES
         (1,'a',1,0,0,'','a','a',0,0,''),(2,'b',1,0,0,'','b','b',0,0,''),(3,'c',1,0,0,'','c','c',0,0,'')",
            &[],
        )
        .unwrap();
    for (id, nid, queue, due) in [
        (10, 1, 2, 12),
        (11, 1, -2, 0),
        (20, 2, -3, 0),
        (21, 2, 0, 0),
        (40, 4, 4, 0),
    ] {
        card(&db, [id, nid, 0, queue, due, 0, 0, 0]);
    }
    let ctx = context();
    for (query, cards, notes) in [
        ("is:due", vec![10, 40], vec![1]),
        ("is:buried", vec![11, 20], vec![1, 2]),
        ("-is:buried", vec![10, 21, 40], vec![1, 2]),
        ("is:due is:buried", vec![], vec![]),
        ("is:due OR is:buried", vec![10, 11, 20, 40], vec![1, 2]),
        ("-(is:due OR is:buried)", vec![21], vec![2]),
        ("(is:due OR is:buried) -is:buried", vec![10, 40], vec![1]),
        ("is:buried OR -is:buried", vec![10, 11, 20, 21, 40], vec![1, 2]),
    ] {
        assert_eq!(ids(&db, query, SearchMode::Cards, &ctx), cards, "{query}");
        assert_eq!(ids(&db, query, SearchMode::Notes, &ctx), notes, "{query}");
    }
    let due_reads = [ContextValue::Now, ContextValue::Today, ContextValue::LearnAhead]
        .into_iter()
        .collect::<ContextReads>();
    for mode in [SearchMode::Cards, SearchMode::Notes] {
        for query in ["is:due", "-is:due", "(is:due OR is:buried) -is:buried"] {
            let plan = plan(query, mode, &ctx);
            assert_eq!(plan.reads(), due_reads);
            assert!(!plan.is_slow());
        }
        for query in ["is:buried", "-is:buried", "(is:buried OR -is:buried)"] {
            assert_eq!(plan(query, mode, &ctx).reads(), ContextReads::NONE);
        }
    }
    let cards = plan("is:due OR is:buried", SearchMode::Cards, &ctx);
    assert!(!cards.sql().contains("notes"));
    assert_eq!(ids(&db, "", SearchMode::Notes, &ctx), [1, 2, 3]);
}
