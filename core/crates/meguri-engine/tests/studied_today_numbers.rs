//! Today's numbers through a real collection (`stats/studied-today` T12):
//! N6's row rule for the studied-today window and the seven-day window,
//! their starts at `S` and `S − 6` days, and a rollover moving them.

mod common;

use std::sync::Arc;

use common::day::{self, Timing};
use meguri_engine::studied::{StudiedTodayNumbers, studied_today_numbers};
use meguri_engine::{Collection, Ports};
use meguri_model::clock::{StepClock, UtcOffset};

/// 2026-10-12 22:00 +02:00 (Europe/Paris): day 0 runs to N.
const NOW: i64 = 1_791_835_200;
/// 2026-10-13 04:00 +02:00.
const N: i64 = 1_791_856_800;
const DAY: i64 = 86_400;
/// `S`, day 0's first millisecond.
const S: i64 = (N - DAY) * 1000;

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

fn row(id: i64, cid: i64, ease: i64, kind: i64, time: i64) -> String {
    format!("INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES ({id}, {cid}, 0, {ease}, 1, 1, 2500, {time}, {kind})")
}

#[test]
fn an_empty_log_gives_zeros() {
    let paris = UtcOffset::from_seconds(7_200).unwrap();
    let clock = Arc::new(StepClock::new(NOW * 1000, paris, "Europe/Paris"));
    let (_dir, path) = day::new_collection(&*clock, Timing::New);
    let (_engine, collection) = day::open(&path, clock.clone(), Ports::default());
    let read = studied_today_numbers(&collection).unwrap();
    assert_eq!(read, StudiedTodayNumbers { generation: collection.generation().0, ..Default::default() });
}

#[test]
fn rows_windows_and_rollover_follow_n6() {
    let paris = UtcOffset::from_seconds(7_200).unwrap();
    let clock = Arc::new(StepClock::new(NOW * 1000, paris, "Europe/Paris"));
    let (_dir, path) = day::new_collection(&*clock, Timing::New);
    let (_engine, collection) = day::open(&path, clock.clone(), Ports::default());
    raw(
        &collection,
        vec![
            // Today (`id > S`, B2): counted, whatever the ease, deleted card (cid 99) included.
            row(S + 1, 1, 3, 1, 1_000),
            row(S + 10, 99, 0, 0, 2_000),
            row(S + 20, 2, 1, 2, 4_000),
            row(S + 30, 3, 4, 3, 8_000),
            // Card 2 again: one more answer, no more cards.
            row(S + 35, 2, 3, 1, 0),
            // Types 4 and 5 never count.
            row(S + 40, 1, 0, 4, 100_000),
            row(S + 50, 1, 0, 5, 100_000),
            // Exactly S: yesterday (B2), in the seven days only.
            row(S, 1, 3, 1, 16_000),
            // The seven-day window: its first millisecond in, its start out.
            row(S - 6 * DAY * 1000 + 1, 2, 3, 1, 32_000),
            row(S - 6 * DAY * 1000, 2, 3, 1, 64_000),
        ],
    );
    let read = studied_today_numbers(&collection).unwrap();
    assert_eq!((read.answers, read.time_ms), (5, 15_000));
    // Cards 1, 99, 2 and 3: four different cards; card 1's type 4 and 5 rows do not count.
    assert_eq!(read.cards, 4);
    assert_eq!((read.recent_answers, read.recent_time_ms), (7, 63_000));
    assert_eq!(read.generation, collection.generation().0);

    // Past N the day moves: today's rows become yesterday's.
    clock.set_now_ms((N + 60) * 1000);
    let next = studied_today_numbers(&collection).unwrap();
    assert_eq!((next.answers, next.time_ms), (0, 0));
    // S moved by a day, so the oldest counted row (S − 6 days + 1 ms) left the window.
    assert_eq!((next.recent_answers, next.recent_time_ms), (6, 31_000));
}

#[test]
fn a_sum_that_overflows_is_an_error() {
    let paris = UtcOffset::from_seconds(7_200).unwrap();
    let clock = Arc::new(StepClock::new(NOW * 1000, paris, "Europe/Paris"));
    let (_dir, path) = day::new_collection(&*clock, Timing::New);
    let (_engine, collection) = day::open(&path, clock.clone(), Ports::default());
    raw(&collection, vec![row(S + 1, 1, 3, 1, i64::MAX), row(S + 2, 1, 3, 1, 1)]);
    // (The same card twice: had it not overflowed, one card and two answers.)
    assert!(studied_today_numbers(&collection).is_err());
}
