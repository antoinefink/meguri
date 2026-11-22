//! The Statistics page's request through its real exports on an opened
//! collection (U103, U292; `stats/history-reads` H15, H16; G8.3, G9.5):
//! the stream of one request ending with its `End` before its
//! `JobFinished`, a cancel through the handle's token seen within H16's
//! 50 ms with nothing after it, `ContextChanged` after a commit while a
//! part waits, and no listener kept once a request has ended. Names and
//! values are made up.

use std::path::Path;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::{Duration, Instant};

use meguri_ffi::collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock};
use meguri_ffi::stats_page::*;
use meguri_ffi::{CoreEvent, CoreEventKind, CoreEventListener, CoreError, JobOutcome};

/// 2030-01-15T10:00:00Z.
const NOW_MS: i64 = 1_894_701_600_000;
const DAY_MS: i64 = 86_400_000;
/// The schema-18 fixture's deck `Lang` and one of its notes.
const LANG: i64 = 1_788_249_600_212;
const NOTE: i64 = 1_788_249_600_238;
const WAIT: Duration = Duration::from_secs(120);

struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        NOW_MS
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space;
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        DiskCapacity::Available { bytes: u64::MAX }
    }
}
struct Events(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Events {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}

fn finished(events: &mpsc::Receiver<CoreEvent>, id: u64) -> JobOutcome {
    loop {
        let event = events.recv_timeout(Duration::from_secs(60)).expect("no JobFinished");
        if let CoreEventKind::JobFinished { id: got, outcome } = event.kind
            && got == id
        {
            return outcome;
        }
    }
}

/// Opens a copy of the schema-18 fixture after `statements` ran on it.
fn open(dir: &tempfile::TempDir, statements: &[String]) -> mpsc::Receiver<CoreEvent> {
    let directory = dir.path().join("collection");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    let path = directory.join("collection.anki2");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    if !statements.is_empty() {
        let clock = meguri_model::clock::FixedClock::new(NOW_MS, meguri_model::clock::UtcOffset::UTC);
        let random = meguri_model::rng::SeededRandom::new(1);
        let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(&clock, &random)).unwrap();
        for statement in statements {
            opened.writer().execute_raw(statement, &[]).unwrap();
        }
    }
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Events(tx))).unwrap();
    let handle = collection::start_open(
        CollectionLocation {
            manual_transfer_directory: dir.path().join("ManualTransfers").to_string_lossy().into_owned(),
            collection_directory: directory.to_str().unwrap().into(),
            backups_directory: backups.to_str().unwrap().into(),
            app_group_snapshot: dir.path().join("widget.snapshot").to_str().unwrap().into(),
        },
        Arc::new(Clock),
        Arc::new(Space),
        7,
    )
    .unwrap();
    assert_eq!(finished(&rx, handle.id), JobOutcome::Succeeded);
    rx
}

/// `cards` new review cards in `Lang` and `rows` review-log rows over the
/// last 900 days: enough for the history build to report progress.
fn big(cards: i64, rows: i64) -> Vec<String> {
    let first = NOW_MS - 900 * DAY_MS;
    vec![
        "DELETE FROM revlog".to_owned(),
        format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {cards}) \
             INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, \
             odid, flags, data) SELECT {first} + i, {NOTE}, {LANG}, 0, 0, -1, 2, 2, 100, i % 60, 2500, 1, 0, 0, 0, 0, 0, \
             '{{}}' FROM n"
        ),
        format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {rows}) \
             INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) SELECT {first} + i * 60000, \
             {first} + 1 + (i % {cards}), -1, 1 + i % 4, i % 50 - 5, 1, 2500, 5000, i % 3 FROM n"
        ),
    ]
}

/// Sends each message to the test, with the instant it arrived. With
/// `hold_first_progress`, the first progress waits until the test lets it
/// go, so the request is still waiting on its part then.
struct Messages {
    tx: Mutex<mpsc::Sender<(StatsPageMessage, Instant)>>,
    hold_first_progress: Mutex<Option<mpsc::Receiver<()>>>,
}

impl StatsPageListener for Messages {
    fn on_message(&self, message: StatsPageMessage) {
        let progress = matches!(message, StatsPageMessage::Progress { .. });
        let _ = self.tx.lock().unwrap().send((message, Instant::now()));
        if progress && let Some(go) = self.hold_first_progress.lock().unwrap().take() {
            go.recv().unwrap();
        }
    }
}

fn listener(hold: Option<mpsc::Receiver<()>>) -> (Arc<Messages>, mpsc::Receiver<(StatsPageMessage, Instant)>) {
    let (tx, rx) = mpsc::channel();
    let messages = Arc::new(Messages {
        tx: Mutex::new(tx),
        hold_first_progress: Mutex::new(hold),
    });
    (messages, rx)
}

fn every_part() -> Vec<StatsPart> {
    vec![
        StatsPart::CardCounts,
        StatsPart::FutureDue,
        StatsPart::Intervals,
        StatsPart::Stability,
        StatsPart::Difficulty,
        StatsPart::Retrievability,
        StatsPart::StudiedToday,
        StatsPart::Reviews,
        StatsPart::ByAnswer,
        StatsPart::Retention,
        StatsPart::RetentionByDay,
        StatsPart::Hours,
        StatsPart::Buttons,
        StatsPart::Added,
        StatsPart::CardStatesOverTime,
        StatsPart::Started,
        StatsPart::ByDeck,
        StatsPart::ActivityDays,
        StatsPart::KnowledgeOverTime,
    ]
}

fn request(scope: StatsScope, period: StatsPeriod) -> StatsPageRequest {
    StatsPageRequest {
        scope,
        period,
        parts: every_part(),
        refresh: 3,
    }
}

/// Every message until the `End`, then nothing more.
fn until_end(rx: &mpsc::Receiver<(StatsPageMessage, Instant)>) -> Vec<(StatsPageMessage, Instant)> {
    let mut messages = Vec::new();
    loop {
        let (message, at) = rx.recv_timeout(WAIT).expect("the request never ended");
        let end = matches!(message, StatsPageMessage::End { .. });
        messages.push((message, at));
        if end {
            return messages;
        }
    }
}

/// The listener is released once the job has ended: the request keeps
/// nothing of the caller's.
fn released(weak: &Weak<Messages>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while weak.upgrade().is_some() {
        assert!(Instant::now() < deadline, "the request kept its listener");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_request_streams_its_parts_and_ends_before_its_job() {
    let dir = tempfile::tempdir().unwrap();
    let events = open(&dir, &[]);
    let (messages, rx) = listener(None);
    let weak = Arc::downgrade(&messages);
    let handle = start_stats_page(request(StatsScope::AllDecks, StatsPeriod::All), messages).unwrap();
    let got = until_end(&rx);
    assert_eq!(finished(&events, handle.id), JobOutcome::Succeeded);
    assert!(rx.try_recv().is_err(), "nothing after the End");
    released(&weak);

    let StatsPageMessage::Header { header } = &got[0].0 else {
        panic!("the header first: {:?}", got[0].0);
    };
    assert_eq!(header.refresh, 3);
    assert_eq!(header.history, StatsHistory::AllHistory);
    assert_eq!(header.context.now_ms, NOW_MS);
    assert_eq!(header.context.zone, "UTC");
    assert_eq!(header.browse_scope, StatsBrowseScope::None);
    let parts: Vec<StatsPart> = got
        .iter()
        .filter_map(|(message, _)| match message {
            StatsPageMessage::Part { part, .. } => Some(*part),
            _ => None,
        })
        .collect();
    let mut sorted = parts.clone();
    sorted.sort_by_key(|part| every_part().iter().position(|p| p == part));
    assert_eq!(sorted, every_part(), "each part once");
    assert_eq!(parts[0], StatsPart::CardCounts, "the present tier first");
    assert_eq!(parts.last(), Some(&StatsPart::KnowledgeOverTime), "knowledge last");
    let (StatsPageMessage::End { end }, _) = got.last().unwrap() else { unreachable!() };
    assert_eq!(end, &StatsPageEnd::Complete);

    // Within the year, a deck's request reads the same history for every
    // period but All.
    for period in [StatsPeriod::OneMonth, StatsPeriod::SixMonths, StatsPeriod::OneYear] {
        let (messages, rx) = listener(None);
        let handle = start_stats_page(request(StatsScope::Deck { id: LANG }, period), messages).unwrap();
        let got = until_end(&rx);
        assert_eq!(finished(&events, handle.id), JobOutcome::Succeeded);
        let StatsPageMessage::Header { header } = &got[0].0 else { panic!() };
        assert_eq!(header.history, StatsHistory::Last12Months, "{period:?}");
    }

    // The two cache calls run with a collection open.
    release_stats_caches().unwrap();
    stats_home_idle().unwrap();
}

#[test]
fn h16_a_cancel_through_the_handle_ends_the_request_within_50_ms_with_nothing_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let events = open(&dir, &big(30_000, 240_000));
    let (go, hold) = mpsc::channel();
    let (messages, rx) = listener(Some(hold));
    let weak = Arc::downgrade(&messages);
    let handle = start_stats_page(request(StatsScope::AllDecks, StatsPeriod::All), messages).unwrap();
    // The first progress: the request waits on the history build.
    loop {
        let (message, _) = rx.recv_timeout(WAIT).expect("no progress");
        assert!(!matches!(message, StatsPageMessage::End { .. }), "ended before the build reported progress");
        if matches!(message, StatsPageMessage::Progress { .. }) {
            break;
        }
    }
    let cancelled_at = Instant::now();
    handle.cancel.cancel().unwrap();
    go.send(()).unwrap();
    let rest = until_end(&rx);
    let (StatsPageMessage::End { end }, ended_at) = rest.last().unwrap() else { unreachable!() };
    assert_eq!(end, &StatsPageEnd::Cancelled);
    let took = ended_at.saturating_duration_since(cancelled_at);
    assert!(took <= Duration::from_millis(50), "ended {took:?} after the cancel");
    assert_eq!(rest.len(), 1, "nothing but the End after the cancel: {:?}", rest);
    assert_eq!(finished(&events, handle.id), JobOutcome::Cancelled);
    released(&weak);
}

#[test]
fn g8_3_a_commit_while_a_part_waits_ends_the_request_with_context_changed() {
    let dir = tempfile::tempdir().unwrap();
    let events = open(&dir, &big(30_000, 240_000));
    let (go, hold) = mpsc::channel();
    let (messages, rx) = listener(Some(hold));
    let handle = start_stats_page(request(StatsScope::AllDecks, StatsPeriod::All), messages).unwrap();
    loop {
        let (message, _) = rx.recv_timeout(WAIT).expect("no progress");
        assert!(!matches!(message, StatsPageMessage::End { .. }), "ended before the build reported progress");
        if matches!(message, StatsPageMessage::Progress { .. }) {
            break;
        }
    }
    // A commit after the request's snapshot, while it waits.
    meguri_ffi::decks::add_deck("Elsewhere".into()).unwrap();
    go.send(()).unwrap();
    let rest = until_end(&rx);
    let (StatsPageMessage::End { end }, _) = rest.last().unwrap() else { unreachable!() };
    assert_eq!(end, &StatsPageEnd::ContextChanged);
    assert_eq!(finished(&events, handle.id), JobOutcome::Succeeded);
}

#[test]
fn a_request_with_no_collection_open_is_refused_before_it_starts() {
    struct Nobody;
    impl StatsPageListener for Nobody {
        fn on_message(&self, _: StatsPageMessage) {}
    }
    assert!(matches!(
        start_stats_page(request(StatsScope::AllDecks, StatsPeriod::All), Arc::new(Nobody)),
        Err(CoreError::Unavailable { .. })
    ));
}
