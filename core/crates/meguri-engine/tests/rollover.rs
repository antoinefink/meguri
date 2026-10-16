//! The engine's day clock (`specs/sched/day-clock.md` D5 to D10, D12.3,
//! D14.3): `check_rollover`, its writes and unbury in `Rollover` ops, the
//! rollover op's undo clear, the events, the cache and its fast path, the
//! open-time check, the generation change and the parked check. The day
//! numbers against the oracle are `day_conformance.rs`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::day::{self, Timing};
use common::steps::{self, UPDATE_NOTE};
use common::{col_mod, drain};
use meguri_engine::{
    Collection, DayReport, DisabledReason, Engine, EngineError, Event, EventKind, LongOp, OpSpec, Ports, Subscription,
    UndoScope, UndoStatus,
};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, StampMode};
use tempfile::TempDir;

/// 2030-01-15 10:00 +01:00: the collections' creation.
const T0: i64 = 1_894_698_000;
/// 2030-01-15 04:00 +01:00: their `crt` (D11).
const CRT: i64 = 1_894_676_400;
/// 2030-01-20 10:00 and 20:00 +01:00: day 5.
const DAY5_MORNING: i64 = 1_895_130_000;
const DAY5_EVENING: i64 = 1_895_166_000;
/// 2030-01-21 04:00 +01:00: day 6 starts.
const BOUNDARY: i64 = 1_895_194_800;
const DAY: i64 = 86_400;
const CET: i32 = 3600;

fn offset(seconds: i32) -> UtcOffset {
    UtcOffset::from_seconds(seconds).unwrap()
}

/// A collection created in Paris at `T0`, open on a steppable clock.
struct Day {
    _dir: TempDir,
    path: std::path::PathBuf,
    clock: Arc<StepClock>,
    engine: Engine,
    c: Collection,
}

impl Day {
    fn paris(timing: Timing) -> Day {
        Day::paris_with(timing, &[])
    }

    /// With `statements` run on the new collection before the engine opens
    /// it.
    fn paris_with(timing: Timing, statements: &[(&str, Vec<SqlValue>)]) -> Day {
        let clock = Arc::new(StepClock::new(T0 * 1000, offset(CET), "Europe/Paris"));
        let (dir, path) = day::new_collection(clock.as_ref(), timing);
        day::raw(&path, clock.as_ref(), statements);
        let (engine, c) = day::open(&path, clock.clone(), Ports::default());
        Day {
            _dir: dir,
            path,
            clock,
            engine,
            c,
        }
    }

    fn at(&self, secs: i64) {
        self.clock.set_now_ms(secs * 1000);
    }

    fn check(&self) -> DayReport {
        self.c.check_rollover().unwrap()
    }

    fn subscribe(&self) -> Subscription {
        self.engine.stream().subscribe()
    }

    fn config(&self, key: &str) -> Option<(i64, i64, String)> {
        let reader = self.c.reader(ReaderRole::Query).unwrap();
        let entry = reader.config_entry(key).unwrap()?;
        Some((
            i64::from(entry.entry.usn.0),
            entry.entry.mtime.0,
            String::from_utf8(entry.entry.value).unwrap(),
        ))
    }

    fn card(&self, id: CardId) -> Card {
        let reader = self.c.reader(ReaderRole::Query).unwrap();
        reader.card(id).unwrap().unwrap()
    }

    fn peek(&self) -> UndoStatus {
        self.c.peek_undo(UndoScope::Global).unwrap()
    }
}

fn report(today: u32, next_day_at: i64, rolled_over_from: Option<u32>) -> DayReport {
    DayReport {
        today,
        next_day_at,
        rolled_over_from,
    }
}

/// The day clock's events among `events`.
fn day_events(events: &[Arc<Event>]) -> Vec<EventKind> {
    events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::RolledOver { .. } | EventKind::TimeZoneChanged { .. } | EventKind::NextDayAtChanged { .. }
            )
        })
        .map(|e| e.kind.clone())
        .collect()
}

fn rolled(old_day: u32, new_day: u32) -> EventKind {
    EventKind::RolledOver { old_day, new_day }
}

fn next_at(next_day_at: i64) -> EventKind {
    EventKind::NextDayAtChanged { next_day_at }
}

/// Renders day clock events for comparison (EventKind has no PartialEq).
fn shown(events: &[EventKind]) -> Vec<String> {
    events.iter().map(|e| format!("{e:?}")).collect()
}

/// A note with two buried cards, written as given (`mod` and `usn` kept)
/// by a skip-undo op: a review card buried by the scheduler and an
/// interday learning card buried by the user (D7's table).
fn buried_cards(c: &Collection) -> (NoteId, CardId, CardId) {
    let card = |nid, card_type, queue, due, mtime| Card {
        id: CardId(0),
        note_id: nid,
        deck_id: DeckId(1),
        ord: 0,
        mtime: EpochSecs(mtime),
        usn: Usn(3),
        card_type,
        queue,
        due,
        interval: 3,
        ease_factor: 2500,
        reps: 4,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: Default::default(),
    };
    c.run(
        OpSpec::named("setup_cards", OpSource::UserNonUndoable)
            .skip_undo()
            .stamp(StampMode::CallerSupplied),
        move |ctx| {
            let mut note = common::note(&["river", "rio"]);
            note.usn = Usn(3);
            let nid = ctx.scope().add_note(&note)?;
            let review = ctx.scope().add_card(&card(
                nid,
                CardType::Review,
                CardQueue::BuriedByScheduler,
                5,
                1_700_000_001,
            ))?;
            let interday = ctx.scope().add_card(&card(
                nid,
                CardType::Learning,
                CardQueue::BuriedByUser,
                6,
                1_700_000_002,
            ))?;
            Ok((nid, review, interday))
        },
    )
    .unwrap()
    .value
}

/// A `SyncIncoming` op setting config keys as given.
fn sync_config(c: &Collection, values: &[(&'static str, ConfigValue)]) {
    let values = values.to_vec();
    c.run(OpSpec::named("sync_merge", OpSource::SyncIncoming), move |ctx| {
        for (key, value) in values {
            ctx.scope().set_config(key, &value)?;
        }
        Ok(())
    })
    .unwrap();
}

/// Waits until the subscription has delivered `count` day clock events.
fn wait_day_events(subscription: &Subscription, count: usize) -> Vec<EventKind> {
    let mut seen = Vec::new();
    while day_events(&seen).len() < count {
        match subscription.recv_timeout(Duration::from_secs(10)) {
            Some(event) => seen.push(event),
            None => panic!("only {:?}", shown(&day_events(&seen))),
        }
    }
    seen.extend(drain(subscription, Duration::from_millis(100)));
    day_events(&seen)
}

#[test]
fn ae24_at_four_the_day_rolls_over_both_buried_kinds_return_and_undo_reports_the_rollover() {
    let d = Day::paris(Timing::New);
    d.at(DAY5_EVENING);
    assert_eq!(d.check(), report(5, BOUNDARY, Some(0)));
    let (nid, review, interday) = buried_cards(&d.c);
    steps::edit(&d.c, None, nid, "river bank").unwrap();
    let mod_before = col_mod(&d.c);
    let subscription = d.subscribe();

    d.at(BOUNDARY - 1);
    assert_eq!(d.check(), report(5, BOUNDARY, None), "03:59:59 is still day 5");
    assert_eq!(d.card(review).queue, CardQueue::BuriedByScheduler);
    assert_eq!(d.peek().name(), Some(UPDATE_NOTE));

    d.at(BOUNDARY);
    assert_eq!(d.check(), report(6, BOUNDARY + DAY, Some(5)));
    for (id, queue, mtime) in [
        (review, CardQueue::Review, 1_700_000_001),
        (interday, CardQueue::InterdayLearning, 1_700_000_002),
    ] {
        let card = d.card(id);
        assert_eq!(card.queue, queue);
        assert_eq!((card.mtime, card.usn), (EpochSecs(mtime), Usn(3)), "mod and usn kept");
    }
    assert_eq!(d.config("lastUnburied"), Some((-1, BOUNDARY, "6".to_owned())));
    assert_eq!(col_mod(&d.c), mod_before, "col.mod unchanged");
    assert_eq!(d.peek(), UndoStatus::Unavailable(DisabledReason::ClearedByRollover));

    // The rollover op's change set, then RolledOver and the new boundary,
    // caused by it.
    let events = drain(&subscription, Duration::from_millis(200));
    let changes = events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::Changes(changes) => Some((e.seq, changes.clone())),
            _ => None,
        })
        .expect("the rollover op's change set");
    assert_eq!(changes.1.source, OpSource::Rollover);
    assert_eq!(
        changes.1.cards.iter().map(|c| c.id).collect::<Vec<_>>(),
        [review, interday]
    );
    assert_eq!(
        changes.1.config_keys.iter().map(|k| k.key.as_str()).collect::<Vec<_>>(),
        ["lastUnburied"]
    );
    let after: Vec<Arc<Event>> = events.iter().filter(|e| e.seq > changes.0).cloned().collect();
    assert_eq!(
        shown(&day_events(&after)),
        shown(&[rolled(5, 6), next_at(BOUNDARY + DAY)])
    );
    assert!(
        after
            .iter()
            .filter(|e| !matches!(e.kind, EventKind::Changes(_)))
            .all(|e| e.cause_seq == Some(changes.0))
    );
    assert!(
        events
            .iter()
            .filter(|e| e.seq < changes.0)
            .all(|e| day_events(&[Arc::clone(e)]).is_empty()),
        "the events follow the rollover op's commit"
    );
}

#[test]
fn a_second_check_on_the_same_day_reports_nothing_and_skips_the_writer() {
    let d = Day::paris(Timing::New);
    let subscription = d.subscribe();
    d.at(DAY5_MORNING);
    assert_eq!(d.check(), report(5, BOUNDARY, Some(0)));
    let seq = d.c.last_seq();
    assert_eq!(d.check(), report(5, BOUNDARY, None));
    d.at(DAY5_EVENING);
    assert_eq!(d.check(), report(5, BOUNDARY, None));
    assert_eq!(d.c.last_seq(), seq, "no op ran");
    assert_eq!(
        shown(&day_events(&drain(&subscription, Duration::from_millis(100)))),
        shown(&[rolled(0, 5), next_at(BOUNDARY)])
    );
    assert_eq!(d.c.today().unwrap(), 5);
    assert_eq!(d.c.next_day_at().unwrap(), BOUNDARY);
}

#[test]
fn the_first_check_at_open_writes_and_reports_nothing() {
    let clock = Arc::new(StepClock::new(T0 * 1000, offset(CET), "Europe/Paris"));
    let (_dir, path) = day::new_collection(clock.as_ref(), Timing::New);
    let engine = Engine::new(meguri_engine::EngineConfig::default()).unwrap();
    let subscription = engine.stream().subscribe();
    let c = engine
        .open_raw(
            &path,
            meguri_engine::OpenConfig::new(clock.clone(), Arc::new(meguri_model::rng::SeededRandom::new(7))),
        )
        .unwrap();
    let events = drain(&subscription, Duration::from_millis(100));
    assert!(day_events(&events).is_empty(), "{:?}", shown(&day_events(&events)));
    let rows = c
        .reader(ReaderRole::Query)
        .unwrap()
        .execute_raw(
            "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY IN ('creationOffset', \
             'lastUnburied', 'localOffset', 'rollover') ORDER BY KEY",
            &[],
        )
        .unwrap()
        .rows;
    let row = |key: &str, usn: i64, mtime: i64, val: &str| {
        vec![
            SqlValue::Text(key.to_owned()),
            SqlValue::Integer(usn),
            SqlValue::Integer(mtime),
            SqlValue::Text(val.to_owned()),
        ]
    };
    assert_eq!(
        rows,
        [
            row("creationOffset", 0, 0, "-60"),
            row("localOffset", -1, T0, "-60"),
            row("rollover", -1, T0, "4"),
        ]
    );
    assert_eq!(c.reader(ReaderRole::Query).unwrap().col().unwrap().created.0, CRT);
    assert_eq!(c.check_rollover().unwrap(), report(0, CRT + DAY, None));
}

#[test]
fn a_synced_rollover_change_recomputes_at_once_and_reports_once() {
    let d = Day::paris(Timing::New);
    d.at(DAY5_MORNING);
    d.check();
    let subscription = d.subscribe();
    sync_config(&d.c, &[("rollover", ConfigValue::from(6_i64))]);
    // The check the change set started reports the new boundary by itself.
    let events = wait_day_events(&subscription, 1);
    assert_eq!(shown(&events), shown(&[next_at(BOUNDARY + 2 * 3600)]));
    assert_eq!(d.check(), report(5, BOUNDARY + 2 * 3600, None));
    assert!(day_events(&drain(&subscription, Duration::from_millis(100))).is_empty());
}

#[test]
fn a_new_offset_writes_local_offset_once_and_keeps_the_undo_history() {
    let d = Day::paris(Timing::New);
    assert_eq!(d.config("localOffset"), Some((-1, T0, "-60".to_owned())));
    let (nid, _, _) = buried_cards(&d.c);
    steps::edit(&d.c, None, nid, "edited").unwrap();
    d.at(T0 + 60);
    d.check();
    assert_eq!(
        d.config("localOffset"),
        Some((-1, T0, "-60".to_owned())),
        "unchanged offset"
    );

    let subscription = d.subscribe();
    d.clock.set_utc_offset(offset(2 * 3600));
    d.at(T0 + 120);
    // 11:02 local on the same date: the same day, a boundary an hour earlier.
    assert_eq!(d.check(), report(0, CRT + DAY - 3600, None));
    assert_eq!(d.config("localOffset"), Some((-1, T0 + 120, "-120".to_owned())));
    d.at(T0 + 180);
    d.check();
    assert_eq!(
        d.config("localOffset"),
        Some((-1, T0 + 120, "-120".to_owned())),
        "written once"
    );
    assert_eq!(
        shown(&day_events(&drain(&subscription, Duration::from_millis(100)))),
        shown(&[next_at(CRT + DAY - 3600)])
    );
    assert_eq!(
        d.peek().name(),
        Some(UPDATE_NOTE),
        "an offset write keeps the history (D10)"
    );
}

#[test]
fn travel_east_past_the_rollover_hour_starts_the_next_day_at_the_next_check() {
    let d = Day::paris(Timing::New);
    // 2030-01-16 03:30 in Paris: still day 0 until 04:00 +01:00.
    d.at(1_894_761_000);
    assert_eq!(d.check(), report(0, 1_894_762_800, None));
    let subscription = d.subscribe();
    // The same instant in Helsinki is 04:30: day 1 (R464).
    d.clock.set_utc_offset(offset(2 * 3600));
    d.clock.set_zone("Europe/Helsinki");
    assert_eq!(d.check(), report(1, 1_894_845_600, Some(0)));
    assert_eq!(
        shown(&day_events(&drain(&subscription, Duration::from_millis(100)))),
        shown(&[
            EventKind::TimeZoneChanged {
                zone_id: "Europe/Helsinki".to_owned()
            },
            rolled(0, 1),
            next_at(1_894_845_600)
        ])
    );
}

#[test]
fn paris_to_ceuta_reports_the_zone_and_no_day_change() {
    let d = Day::paris(Timing::New);
    let subscription = d.subscribe();
    d.clock.set_zone("Africa/Ceuta");
    assert_eq!(d.check(), report(0, CRT + DAY, None));
    assert_eq!(
        shown(&day_events(&drain(&subscription, Duration::from_millis(100)))),
        shown(&[EventKind::TimeZoneChanged {
            zone_id: "Africa/Ceuta".to_owned()
        }])
    );
    assert_eq!(d.check(), report(0, CRT + DAY, None));
    assert!(day_events(&drain(&subscription, Duration::from_millis(100))).is_empty());
}

#[test]
fn last_unburied_more_than_a_week_ahead_unburies_at_the_next_check() {
    let d = Day::paris(Timing::New);
    d.at(DAY5_MORNING);
    d.check();
    let (_, review, interday) = buried_cards(&d.c);
    // A week ahead is still inside the window.
    sync_config(&d.c, &[("lastUnburied", ConfigValue::from(12_i64))]);
    d.check();
    assert_eq!(d.card(review).queue, CardQueue::BuriedByScheduler);
    // Ten days ahead is not.
    sync_config(&d.c, &[("lastUnburied", ConfigValue::from(15_i64))]);
    assert_eq!(d.check(), report(5, BOUNDARY, None));
    assert_eq!(d.card(review).queue, CardQueue::Review);
    assert_eq!(d.card(interday).queue, CardQueue::InterdayLearning);
    assert_eq!(d.config("lastUnburied"), Some((-1, DAY5_MORNING, "5".to_owned())));
}

#[test]
fn closing_on_day_n_and_opening_on_day_n_plus_one_unburies_before_the_collection_is_ready() {
    let d = Day::paris(Timing::New);
    d.at(DAY5_EVENING);
    d.check();
    let (_, review, interday) = buried_cards(&d.c);
    d.engine.close().unwrap();

    d.at(1_895_211_000); // 2030-01-21 08:30
    let engine = Engine::new(meguri_engine::EngineConfig::default()).unwrap();
    let subscription = engine.stream().subscribe();
    let c = engine
        .open_raw(
            &d.path,
            meguri_engine::OpenConfig::new(d.clock.clone(), Arc::new(meguri_model::rng::SeededRandom::new(7))),
        )
        .unwrap();
    {
        let reader = c.reader(ReaderRole::Query).unwrap();
        assert_eq!(reader.card(review).unwrap().unwrap().queue, CardQueue::Review);
        assert_eq!(
            reader.card(interday).unwrap().unwrap().queue,
            CardQueue::InterdayLearning
        );
        assert_eq!(
            reader.config_value("lastUnburied").unwrap(),
            Some(ConfigValue::from(6_i64))
        );
    }
    // An open records the day; it reports no change.
    assert!(day_events(&drain(&subscription, Duration::from_millis(100))).is_empty());
    assert_eq!(c.check_rollover().unwrap(), report(6, BOUNDARY + DAY, None));
}

#[test]
fn legacy_timing_survives_a_card_write_and_a_sync_without_creation_offset_and_switches_with_one() {
    // D4's example: crt 2030-01-14 23:30 +01:00 seen on 2030-06-01 10:00
    // +02:00 is day 137 in legacy timing, 138 with creationOffset -60.
    let d = Day::paris_with(Timing::Legacy, &[("UPDATE col SET crt = 1894660200", Vec::new())]);
    d.clock.set_utc_offset(offset(2 * 3600));
    d.at(1_906_531_200);
    let next = 1_906_596_000;
    assert_eq!(d.check().today, 137);
    // The rows an answer writes (card and revlog), by a stand-in: no
    // scheduler answer path exists yet. U44 runs a real answer in
    // `sched/day/legacy-timing-after-answer` (plan amendment, review-log
    // U40 A2).
    let (_, review, _) = buried_cards(&d.c);
    steps::answer(&d.c, None, review).unwrap();
    assert_eq!(d.check(), report(137, next, None));
    assert_eq!(d.config("creationOffset"), None);

    // A sync that leaves creationOffset absent (it moves `col.ls`).
    let subscription = d.subscribe();
    d.c.run(OpSpec::named("sync_merge", OpSource::SyncIncoming), |ctx| {
        let mut col = ctx.scope().col()?;
        col.last_sync = meguri_model::time::EpochMillis(1_906_531_200_000);
        ctx.scope().put_col(&col)?;
        ctx.scope().set_config("collapseTime", &ConfigValue::from(600_i64))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(d.check(), report(137, next, None));
    assert_eq!(d.config("creationOffset"), None);
    assert!(day_events(&drain(&subscription, Duration::from_millis(100))).is_empty());

    // One that brings creationOffset switches the count at once (D12.3).
    sync_config(&d.c, &[("creationOffset", ConfigValue::from(-60_i64))]);
    assert_eq!(shown(&wait_day_events(&subscription, 1)), shown(&[rolled(137, 138)]));
    assert_eq!(d.check(), report(138, next, None));
}

#[test]
fn a_check_refused_during_a_one_way_sync_runs_once_the_collection_is_ready() {
    let d = Day::paris(Timing::New);
    let subscription = d.subscribe();
    let slot = d.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    d.at(DAY5_MORNING);
    assert!(matches!(
        d.c.check_rollover(),
        Err(EngineError::CollectionUnavailable { .. })
    ));
    assert!(matches!(
        d.c.check_rollover(),
        Err(EngineError::CollectionUnavailable { .. })
    ));
    std::thread::sleep(Duration::from_millis(100));
    assert!(day_events(&drain(&subscription, Duration::from_millis(50))).is_empty());
    drop(exclusive);
    drop(slot);
    assert_eq!(
        shown(&wait_day_events(&subscription, 2)),
        shown(&[rolled(0, 5), next_at(BOUNDARY)]),
        "the parked check ran once"
    );
    assert_eq!(d.check(), report(5, BOUNDARY, None));
}

/// The replacement collection of the generation tests: created 2030-01-13
/// in Paris (day 7 at `DAY5_EVENING`), with a review card buried by the
/// user on day 3 (`lastUnburied` 3).
fn replacement() -> (TempDir, std::path::PathBuf) {
    let earlier = StepClock::new(1_894_525_200_000, offset(CET), "Europe/Paris");
    let (dir, source) = day::new_collection(&earlier, Timing::New);
    day::raw(
        &source,
        &earlier,
        &[
            (
                "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data) VALUES \
                 (1700000000000, 'g1', (SELECT id FROM notetypes ORDER BY id LIMIT 1), 1700000000, 3, '', \
                 'one' || char(31) || 'uno', 'one', 0, 0, '')",
                Vec::new(),
            ),
            (
                "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, \
                 odue, odid, flags, data) VALUES (1700000000001, 1700000000000, 1, 0, 1700000001, 3, 2, -3, 5, 3, \
                 2500, 4, 0, 0, 0, 0, 0, '')",
                Vec::new(),
            ),
            (
                "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('lastUnburied', 0, 0, CAST('3' AS BLOB))",
                Vec::new(),
            ),
        ],
    );
    (dir, source)
}

/// The replacement's buried card.
const REPLACEMENT_CARD: CardId = CardId(1_700_000_000_001);

/// Replaces the collection with `source` through a restore's exclusive
/// phase and returns the new generation's privileged handle.
fn replace_with(engine: &Engine, source: std::path::PathBuf) -> Collection {
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    exclusive
        .replace(move |live| {
            std::fs::copy(&source, live)
                .map(drop)
                .map_err(|e| EngineError::Internal { message: e.to_string() })
        })
        .unwrap()
}

#[test]
fn a_generation_change_restarts_the_day_clock_and_unburies_the_new_collection() {
    let d = Day::paris(Timing::New);
    d.at(DAY5_EVENING);
    assert_eq!(d.check().today, 5);
    let (_source_dir, source) = replacement();
    let subscription = d.subscribe();
    let slot = d.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    let new = exclusive
        .replace(move |live| {
            std::fs::copy(&source, live)
                .map(drop)
                .map_err(|e| EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    {
        let reader = new.reader(ReaderRole::Query).unwrap();
        let card = reader.card(REPLACEMENT_CARD).unwrap().unwrap();
        assert_eq!(
            (card.queue, card.mtime, card.usn),
            (CardQueue::Review, EpochSecs(1_700_000_001), Usn(3))
        );
        assert_eq!(
            reader.config_value("lastUnburied").unwrap(),
            Some(ConfigValue::from(7_i64))
        );
    }
    drop(exclusive);
    drop(slot);
    let c = d.engine.collection().unwrap();
    assert_eq!(c.check_rollover().unwrap(), report(7, BOUNDARY, None));
    let events = drain(&subscription, Duration::from_millis(100));
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, EventKind::CollectionReplaced { .. }))
    );
    assert!(
        day_events(&events).is_empty(),
        "a new generation records its day: {:?}",
        shown(&day_events(&events))
    );
}

/// What a listener heard of the new generation: the day a check reported
/// and the replacement's buried card's queue, or the error it got.
type Heard = Result<(u32, CardQueue), String>;

/// Checks the day and reads the card through a handle of the current
/// generation, as an app would on hearing of a replacement.
fn look(engine: &Engine) -> Heard {
    let c = engine.collection().map_err(|e| e.to_string())?;
    let today = c.check_rollover().map_err(|e| e.to_string())?.today;
    let reader = c.reader(ReaderRole::Query).map_err(|e| e.to_string())?;
    let card = reader.card(REPLACEMENT_CARD).map_err(|e| e.to_string())?;
    Ok((today, card.ok_or("no card")?.queue))
}

/// A `GenerationListener` that looks at the new generation when told.
#[derive(Default)]
struct Looking {
    engine: std::sync::OnceLock<Engine>,
    heard: std::sync::Mutex<Vec<Heard>>,
}

impl meguri_engine::GenerationListener for Looking {
    fn name(&self) -> &str {
        "looking"
    }

    fn collection_replaced(&self, _old: meguri_engine::Generation, _new: meguri_engine::Generation) {
        let heard = look(self.engine.get().expect("the engine"));
        self.heard.lock().unwrap().push(heard);
    }
}

#[test]
fn whoever_hears_of_a_generation_change_sees_its_first_check_committed() {
    // L8, D8: the new generation's first check commits before
    // `CollectionReplaced` is delivered, before any `GenerationListener` is
    // told and before its readers open. Either sees the replacement's day
    // (7) and its card unburied, never the old collection's cached day (5)
    // or the card still buried.
    let listener = Arc::new(Looking::default());
    let clock = Arc::new(StepClock::new(T0 * 1000, offset(CET), "Europe/Paris"));
    let (_dir, path) = day::new_collection(clock.as_ref(), Timing::New);
    let (engine, c) = day::open(
        &path,
        clock.clone(),
        Ports::default().generation_listener(listener.clone()),
    );
    assert!(listener.engine.set(engine.clone()).is_ok());
    clock.set_now_ms(DAY5_EVENING * 1000);
    assert_eq!(c.check_rollover().unwrap().today, 5);

    let subscription = engine.stream().subscribe();
    let subscriber = {
        let engine = engine.clone();
        std::thread::spawn(move || {
            loop {
                let event = subscription
                    .recv_timeout(Duration::from_secs(10))
                    .expect("CollectionReplaced");
                if matches!(event.kind, EventKind::CollectionReplaced { .. }) {
                    // Looks before taking any later event.
                    let heard = look(&engine);
                    let later = drain(&subscription, Duration::from_millis(200));
                    return (event, heard, later);
                }
            }
        })
    };
    let (_source_dir, source) = replacement();
    let new = replace_with(&engine, source);
    let (replaced, heard, later) = subscriber.join().unwrap();

    assert_eq!(*listener.heard.lock().unwrap(), [Ok((7, CardQueue::Review))]);
    assert_eq!(heard, Ok((7, CardQueue::Review)));
    // The first check's change set follows `CollectionReplaced` on the
    // stream, in the new generation.
    let first = later
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::Changes(changes) => Some((e, changes.clone())),
            _ => None,
        })
        .expect("the first check's change set");
    assert_eq!(first.1.source, OpSource::Rollover);
    assert_eq!(
        first.1.cards.iter().map(|card| card.id).collect::<Vec<_>>(),
        [REPLACEMENT_CARD]
    );
    assert!(first.0.seq > replaced.seq);
    assert_eq!(first.0.generation, new.generation());
    assert_eq!(replaced.generation, new.generation());
    assert!(day_events(&later).is_empty(), "{:?}", shown(&day_events(&later)));
}

#[test]
fn the_core_never_wakes_at_the_boundary_by_itself() {
    let d = Day::paris(Timing::New);
    d.at(BOUNDARY - 1);
    d.check();
    let subscription = d.subscribe();
    d.at(BOUNDARY + 10);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        drain(&subscription, Duration::from_millis(50)).is_empty(),
        "no timer fired"
    );
    assert_eq!(d.check().rolled_over_from, Some(5));
}

#[test]
fn an_op_never_runs_the_day_query_itself() {
    let d = Day::paris(Timing::New);
    let inner = d.c.clone();
    let result =
        d.c.run(OpSpec::named("answer_card", OpSource::UserUndoable), move |_| {
            Ok(inner.check_rollover())
        })
        .unwrap()
        .value;
    assert!(matches!(result, Err(EngineError::InvalidArgument { .. })), "{result:?}");
}

#[test]
fn a_raw_write_drops_the_cached_day() {
    let d = Day::paris(Timing::New);
    d.at(DAY5_MORNING);
    assert_eq!(d.check().today, 5);
    // crt a day later, written outside any op: the cached day stays until
    // the day clock hears of it (D6).
    d.c.submit_maintenance(
        meguri_engine::executor::catalog::ANALYZE,
        Duration::from_millis(1),
        |writer| Ok(writer.execute_raw(&format!("UPDATE col SET crt = {}", CRT + DAY), &[])?),
    )
    .wait()
    .unwrap();
    assert_eq!(d.check().today, 5, "cached");
    d.c.drop_cached_day().unwrap();
    assert_eq!(d.check(), report(4, BOUNDARY, Some(5)));
}
