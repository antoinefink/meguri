//! The activity heatmap through a real collection (`stats/activity-heatmap`
//! AH3, AH4, AH6): the day clock's boundary, `T0`, deck subtrees by name and
//! a rollover moving every row one day back.

mod common;

use std::sync::Arc;

use common::day::{self, Timing};
use meguri_engine::activity::{CivilDate, activity_heatmap};
use meguri_engine::{Collection, Ports};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::DeckId;

/// 2026-10-12 22:00 +02:00 (Europe/Paris): day 0 runs to N.
const NOW: i64 = 1_791_835_200;
/// 2026-10-13 04:00 +02:00.
const N: i64 = 1_791_856_800;
const DAY: i64 = 86_400;

fn raw(collection: &Collection, statements: Vec<String>) {
    collection
        .submit_maintenance(
            meguri_engine::executor::catalog::ANALYZE,
            std::time::Duration::from_millis(10),
            move |w| {
                for sql in &statements {
                    w.execute_raw(sql, &[])?;
                }
                Ok(())
            },
        )
        .wait()
        .unwrap();
}

fn deck(collection: &Collection, name: &str) -> DeckId {
    meguri_engine::decks::ops::add(collection, name.into()).unwrap().value.id
}

fn nonzero(counts: &[u32]) -> Vec<(usize, u32)> {
    counts.iter().enumerate().filter(|(_, n)| **n > 0).map(|(k, n)| (k, *n)).collect()
}

#[test]
fn days_scopes_and_today_follow_the_collection() {
    let paris = UtcOffset::from_seconds(7_200).unwrap();
    let clock = Arc::new(StepClock::new(NOW * 1000, paris, "Europe/Paris"));
    let (_dir, path) = day::new_collection(&*clock, Timing::New);
    let (_engine, collection) = day::open(&path, clock.clone(), Ports::default());

    let lang = deck(&collection, "Lang");
    let sub = deck(&collection, "Lang::Sub");
    let other = deck(&collection, "Language");
    // Cards 1 to 3 in Lang, Lang::Sub and Language; card 4 sits in Language
    // with Lang as its home deck (a filtered deck's card, by `odid`).
    let mut sql = Vec::new();
    for (id, did, odid) in [(1, lang.0, 0), (2, sub.0, 0), (3, other.0, 0), (4, other.0, lang.0)] {
        sql.push(format!(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) \
             VALUES ({id}, {id}, {did}, 0, 0, 0, 2, 2, 1, 1, 2500, 1, 0, 0, 0, {odid}, 0, '')"
        ));
    }
    // Card c reviewed c days ago (an hour before that day's end), rated; one
    // unrated Set Due Date row; a deleted card's row today.
    for (id, cid, ease, kind, at) in [
        (1, 1, 3, 1, N - DAY - 3_600),
        (2, 2, 1, 2, N - 2 * DAY - 3_600),
        (3, 3, 4, 1, N - 3 * DAY - 3_600),
        (4, 4, 2, 0, N - 4 * DAY - 3_600),
        (5, 1, 0, 4, N - 3_600),
        (6, 99, 3, 1, N - 3_600),
    ] {
        sql.push(format!(
            "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES ({}, {cid}, 0, {ease}, 1, 1, 2500, 1000, {kind})",
            at * 1000 + id
        ));
    }
    raw(&collection, sql);

    let all = activity_heatmap(&collection, None).unwrap();
    assert_eq!(all.next_day_at, N);
    assert_eq!(all.generation, collection.generation().0);
    assert_eq!(all.today_date, CivilDate { year: 2026, month: 10, day: 12, weekday: 1 });
    assert_eq!(all.counts.len(), 371);
    assert_eq!(nonzero(&all.counts), [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1)]);
    // The subtree by name: Lang::Sub is in, Language is not (but card 4,
    // whose home is Lang, is).
    let lang_counts = activity_heatmap(&collection, Some(lang)).unwrap();
    assert_eq!(nonzero(&lang_counts.counts), [(1, 1), (2, 1), (4, 1)]);
    assert_eq!(nonzero(&activity_heatmap(&collection, Some(sub)).unwrap().counts), [(2, 1)]);
    assert_eq!(nonzero(&activity_heatmap(&collection, Some(other)).unwrap().counts), [(3, 1)]);
    assert!(nonzero(&activity_heatmap(&collection, Some(DeckId(424_242))).unwrap().counts).is_empty());

    // Past the boundary the day clock moves N; every row is a day older.
    clock.set_now_ms((N + 60) * 1000);
    let next = activity_heatmap(&collection, None).unwrap();
    assert_eq!(next.next_day_at, N + DAY);
    assert_eq!(next.today, all.today + 1);
    assert_eq!(next.today_date, CivilDate { year: 2026, month: 10, day: 13, weekday: 2 });
    assert_eq!(nonzero(&next.counts), [(1, 1), (2, 1), (3, 1), (4, 1), (5, 1)]);
}
