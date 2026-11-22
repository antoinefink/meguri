//! `stats/history-reads` H12's deck scope of `stats/studied-today` T12.1:
//! N6's rows of the cards of a deck set by `stats/graphs-common` G2.1, and
//! a plan that never scans the review log. Names and values are made up.
mod common;

use common::{empty_collection, open, plain};
use meguri_model::ids::DeckId;
use meguri_storage::queries::studied::{STUDIED_SQL, Studied, studied_in_decks_sql};
use meguri_storage::rusqlite::{Connection, params};
use meguri_storage::{ReadRows, ReaderRole};

/// A start `S` of the studied-today window, in epoch milliseconds.
const S: i64 = 1_791_770_400_000;

fn insert_card(conn: &Connection, id: i64, did: i64, odid: i64) {
    conn.execute(
        "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) \
         VALUES (?1, ?1, ?2, 0, 0, 0, 2, 2, 5, 1, 2500, 1, 0, 0, 0, ?3, 0, '')",
        params![id, did, odid],
    )
    .unwrap();
}

fn insert_row(conn: &Connection, id: i64, cid: i64, time: i64, kind: i64) {
    conn.execute(
        "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES (?1, ?2, 0, 3, 1, 1, 2500, ?3, ?4)",
        params![id, cid, time, kind],
    )
    .unwrap();
}

#[test]
fn a_deck_set_counts_the_rows_of_cards_by_did_or_a_non_zero_odid() {
    let (_dir, path) = empty_collection();
    {
        let conn = plain(&path);
        // Card 1 in deck 10; card 2 in filtered deck 40 from deck 10; card 3
        // in deck 20; card 4 in deck 10 from deck 20 (its `did` counts).
        insert_card(&conn, 1, 10, 0);
        insert_card(&conn, 2, 40, 10);
        insert_card(&conn, 3, 20, 0);
        insert_card(&conn, 4, 10, 20);
        // B2: a row at `S` is out, one after it in; types 4 and 5 never count.
        insert_row(&conn, S, 1, 1_000, 1);
        insert_row(&conn, S + 1, 1, 2_000, 1);
        insert_row(&conn, S + 2, 1, 3_000, 0);
        insert_row(&conn, S + 3, 2, 4_000, 3);
        insert_row(&conn, S + 4, 3, 5_000, 1);
        insert_row(&conn, S + 5, 4, 6_000, 2);
        insert_row(&conn, S + 6, 1, 7_000, 4);
        insert_row(&conn, S + 7, 2, 8_000, 5);
        // A deleted card's row counts for the collection only.
        insert_row(&conn, S + 8, 99, 9_000, 1);
    }
    let collection = open(&path).unwrap();
    let reader = collection.reader_factory().open(ReaderRole::Query).unwrap();
    let ten = reader.studied_since_in(S, &[DeckId(10)]).unwrap();
    assert_eq!(ten, Studied { answers: 4, cards: 3, time_ms: 15_000 });
    let twenty = reader.studied_since_in(S, &[DeckId(20)]).unwrap();
    assert_eq!(twenty, Studied { answers: 2, cards: 2, time_ms: 11_000 });
    // A card in both halves of a set counts once.
    let both = reader.studied_since_in(S, &[DeckId(10), DeckId(20)]).unwrap();
    assert_eq!(both, Studied { answers: 5, cards: 4, time_ms: 20_000 });
    assert_eq!(reader.studied_since_in(S, &[]).unwrap(), Studied::default());
    let all = reader.studied_since(S).unwrap();
    assert_eq!(all, Studied { answers: 6, cards: 5, time_ms: 29_000 });
}

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let n = statement.parameter_count();
    let values: Vec<i64> = (1..=n).map(|i| if i == 1 { S } else { 10 }).collect();
    statement
        .query_map(meguri_storage::rusqlite::params_from_iter(values), |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn the_deck_statement_never_scans_the_review_log_or_the_cards() {
    let (_dir, path) = empty_collection();
    let conn = plain(&path);
    for sql in [STUDIED_SQL.to_owned(), studied_in_decks_sql(1), studied_in_decks_sql(3)] {
        let lines = plan(&conn, &sql);
        assert!(
            !lines.iter().any(|l| l.starts_with("SCAN revlog") || l.starts_with("SCAN cards")),
            "{sql}: {lines:#?}"
        );
        assert!(lines.iter().any(|l| l.contains("SEARCH revlog")), "{sql}: {lines:#?}");
    }
    let deck = plan(&conn, &studied_in_decks_sql(1));
    assert!(deck.iter().any(|l| l.contains("ix_cards_sched")), "{deck:#?}");
    assert!(deck.iter().any(|l| l.contains("idx_cards_odid")), "{deck:#?}");
}
