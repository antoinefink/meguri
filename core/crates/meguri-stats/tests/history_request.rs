//! The Statistics page's request (U293): `stats/history-reads` H5, H15 to
//! H18 and the round 11 lines of `stats/graphs-common` G3, G5, G8.3. The
//! request shape has no oracle: these tests check it by construction (H20's
//! H15, H16 and H17 lines), against the graph engine's and the history
//! read's own results for the same context. Names and values are made up.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meguri_engine::derived_host::Invalidation;
use meguri_engine::{CancelToken, Readiness};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_stats::graphs::encoding::Encoder;
use meguri_stats::graphs::records::card_counts::CardCountsRecord;
use meguri_stats::graphs::records::hourly::HourlyRecord;
use meguri_stats::graphs::records::reviews::ReviewsRecord;
use meguri_stats::graphs::{Columns, History, Record, Scope, ScopeError, Unreadable};
use meguri_stats::history::{self, CardStatesOverTime, HISTORY_COMPONENT, HistoryRequest, HistoryResult, Progress};
use meguri_stats::page::{
    self, PageEnd, PageMessage, PageRequest, Part, PartFailure, PartMessage, PartOutcome, PartValue, Period, Tier,
};
use meguri_storage::{Cell, ConfigValue, RowKey, Table};
use meguri_testkit::bench::fixtures::CoreHost;

use crate::graphs_common::{Fixture, LANG, NOTE, NOW_MS, VERBS, card, revlog, revlog_cells};

const DAY: i64 = 86_400;
const DAY_MS: i64 = DAY * 1_000;
const WAIT: Duration = Duration::from_secs(120);

/// The parts whose units have not landed: none now.
const PENDING: [Part; 0] = [];

fn every_part() -> Vec<Part> {
    Part::all().collect()
}

fn page_request(scope: Scope, history: History, parts: &[Part]) -> PageRequest {
    PageRequest {
        scope,
        history,
        parts: parts.to_vec(),
        refresh: 7,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    }
}

/// The messages a request sent, and how it ended.
fn run(host: &CoreHost, request: &PageRequest) -> (Vec<PageMessage>, PageEnd) {
    let collection = host.collection().unwrap();
    let mut messages = Vec::new();
    let end = page::request(host.stats(), &collection, host.derived(), request, &mut |message| messages.push(message))
        .unwrap();
    (messages, end)
}

fn parts_of(messages: &[PageMessage]) -> Vec<&PartMessage> {
    messages
        .iter()
        .filter_map(|message| match message {
            PageMessage::Part(part) => Some(part),
            PageMessage::Progress { .. } => None,
        })
        .collect()
}

fn outcome(messages: &[PageMessage], part: Part) -> &PartOutcome {
    let found: Vec<_> = parts_of(messages).into_iter().filter(|m| m.part == part).collect();
    assert_eq!(found.len(), 1, "{part:?} returned once: {found:?}");
    &found[0].outcome
}

fn value(messages: &[PageMessage], part: Part) -> &PartValue {
    match outcome(messages, part) {
        PartOutcome::Data(value) => value,
        other => panic!("{part:?}: {other:?}"),
    }
}

fn record<T: 'static + Clone>(messages: &[PageMessage], part: Part) -> T {
    match value(messages, part) {
        PartValue::Record(record) => record.as_any().downcast_ref::<T>().expect("its record").clone(),
        other => panic!("{part:?}: {other:?}"),
    }
}

fn card_states(messages: &[PageMessage]) -> CardStatesOverTime {
    match value(messages, Part::CardStatesOverTime) {
        PartValue::CardStates(states) => states.clone(),
        other => panic!("{other:?}"),
    }
}

fn bytes(record: &Record) -> Vec<u8> {
    let mut out = Encoder::new();
    record.encode(&mut out);
    out.into_bytes()
}

/// A part's value as bytes or debug text, to compare two requests.
fn comparable(value: &PartValue) -> String {
    match value {
        PartValue::Record(record) => format!("{:?}", bytes(record)),
        other => format!("{other:?}"),
    }
}

fn progress_of(messages: &[PageMessage], part: Part) -> Vec<Progress> {
    messages
        .iter()
        .filter_map(|message| match message {
            PageMessage::Progress { part: p, progress } if *p == part => Some(*progress),
            _ => None,
        })
        .collect()
}

/// Rows on three cards of `Lang` and its subdeck, today and up to 400 days
/// ago.
fn studied() -> Fixture {
    let f = Fixture::new();
    f.write(|scope| {
        scope.put(&card(100, LANG))?;
        scope.put(&card(101, VERBS))?;
        for (n, day) in [0, 3, 40, 200, 400].into_iter().enumerate() {
            let id = NOW_MS - day * DAY_MS - n as i64;
            scope.insert_revlog(&revlog(id, if n % 2 == 0 { 100 } else { 101 }))?;
        }
        Ok(())
    });
    f
}

#[test]
fn h15_every_part_once_by_tier_with_one_header() {
    let f = studied();
    for history in [History::Last12Months, History::AllHistory] {
        let request = page_request(Scope::Collection, history, &every_part());
        let (messages, end) = run(&f.host, &request);
        assert_eq!(end, PageEnd::Complete);
        let parts = parts_of(&messages);
        assert_eq!(parts.len(), Part::all().count());
        let tiers: Vec<Tier> = parts.iter().map(|m| m.part.tier()).collect();
        assert!(tiers.windows(2).all(|w| w[0] <= w[1]), "{tiers:?}");
        // One header for every Data message: the context token of the first.
        let header = &parts[0].header;
        assert!(parts.iter().all(|m| Arc::ptr_eq(&m.header, header)));
        assert_eq!(header.token.refresh(), 7);
        assert_eq!(header.context.next_day_at, f.next_day_at());
        for part in Part::all() {
            match outcome(&messages, part) {
                PartOutcome::Failed(PartFailure::NotBuilt) => assert!(PENDING.contains(&part), "{part:?}"),
                PartOutcome::Data(_) => assert!(!PENDING.contains(&part), "{part:?}"),
                other => panic!("{part:?}: {other:?}"),
            }
        }
    }
    // A part named twice comes once; only the parts named come.
    let request = page_request(Scope::Collection, History::Last12Months, &[Part::Added, Part::CardCounts, Part::Added]);
    let (messages, end) = run(&f.host, &request);
    assert_eq!(end, PageEnd::Complete);
    let named: Vec<Part> = parts_of(&messages).iter().map(|m| m.part).collect();
    assert_eq!(named, [Part::CardCounts, Part::Added]);
    // No part: the scope is still resolved, and the request completes.
    assert_eq!(run(&f.host, &page_request(Scope::Collection, History::AllHistory, &[])).1, PageEnd::Complete);
}

#[test]
fn h15_parts_equal_the_engine_and_the_history_read_for_their_context() {
    let f = studied();
    // The column caches built, so Cached reads them and Snapshot does not.
    meguri_stats::graphs::stats_home_idle(f.host.derived());
    f.host.derived().request_refresh(meguri_stats::graphs::COMPONENT);
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while f.host.derived().readiness(meguri_stats::graphs::COMPONENT).unwrap().readiness != Readiness::Ready {
        assert!(meguri_engine::monotonic::now() < deadline, "the column caches never became ready");
        std::thread::sleep(Duration::from_millis(5));
    }
    f.host.derived().flush().unwrap();
    assert!(f.data(Scope::Collection, History::AllHistory).scope.from_caches());
    for scope in [Scope::Collection, Scope::Deck(meguri_model::ids::DeckId(LANG)), Scope::Deck(meguri_model::ids::DeckId(VERBS))] {
        for history in [History::Last12Months, History::AllHistory] {
            let request = page_request(scope.clone(), history, &every_part());
            let (cached, _) = run(&f.host, &request);
            let (fresh, _) = run(&f.host, &PageRequest { columns: Columns::Snapshot, ..request.clone() });
            for part in Part::all().filter(|p| !PENDING.contains(p)) {
                assert_eq!(comparable(value(&cached, part)), comparable(value(&fresh, part)), "{scope:?} {history:?} {part:?}");
            }
            // The graph engine's records for the same context: the second
            // request of G8.3 with the page's token.
            let header = Arc::clone(&parts_of(&cached)[0].header);
            let engine = meguri_stats::graphs::GraphRequest {
                scope: scope.clone(),
                history,
                graphs: meguri_stats::graphs::Graph::all()
                    .filter(|g| *g != meguri_stats::graphs::Graph::Calendar && *g != meguri_stats::graphs::Graph::Today)
                    .collect(),
                refresh: header.token.refresh(),
                context: Some(header.token.clone()),
                cancel: CancelToken::new(),
                columns: Columns::Cached,
            };
            let collection = f.collection();
            let records = match f.host.stats().request(&collection, f.host.derived(), &engine).unwrap() {
                meguri_stats::graphs::GraphResult::Data(data) => data.records,
                other => panic!("{other:?}"),
            };
            assert_eq!(records.len(), 11);
            for record in &records {
                let part = Part::all()
                    .filter(|p| !PENDING.contains(p))
                    .find(|p| {
                        matches!(value(&cached, *p), PartValue::Record(r) if r.graph() == record.graph())
                    })
                    .unwrap();
                match value(&cached, part) {
                    PartValue::Record(mine) => assert_eq!(bytes(mine), bytes(record), "{scope:?} {history:?} {part:?}"),
                    other => panic!("{other:?}"),
                }
            }
            // By Answer counts the Reviews record's rows (H6): every rated
            // row on its day, the others in `unrated`.
            let reviews: ReviewsRecord = record(&cached, Part::Reviews);
            match value(&cached, Part::ByAnswer) {
                PartValue::ByAnswer(by_answer) => {
                    let rated: i64 = by_answer.count_by_answer.values().flatten().sum();
                    let counted: i64 = reviews.count.values().map(|kinds| kinds.values().iter().sum::<i64>()).sum();
                    assert_eq!(rated + by_answer.unrated as i64, counted, "{scope:?} {history:?}");
                    assert!(by_answer.count_by_answer.keys().all(|day| reviews.count.contains_key(day)));
                }
                other => panic!("{other:?}"),
            }
            // H2's parts are the history read's for the header's context.
            let cold = history::card_states_cold(
                &collection,
                &header.context,
                &HistoryRequest {
                    scope: scope.clone(),
                    history,
                    cancel: CancelToken::new(),
                },
            )
            .unwrap();
            match cold {
                HistoryResult::Data(cold) => {
                    assert_eq!(card_states(&cached), cold.card_states);
                    assert!(matches!(value(&cached, Part::Started), PartValue::Started(s) if *s == cold.started));
                }
                other => panic!("{other:?}"),
            }
            // H3's part is the knowledge read's for the header's context.
            let cold = history::knowledge_cold(
                &collection,
                &header.context,
                &history::KnowledgeRequest {
                    scope: scope.clone(),
                    history,
                    oldest_day: None,
                    cancel: CancelToken::new(),
                },
            )
            .unwrap();
            match (cold, value(&cached, Part::KnowledgeOverTime)) {
                (history::KnowledgeResult::Data(cold), PartValue::Knowledge(mine)) => assert_eq!(*mine, cold.knowledge),
                other => panic!("{other:?}"),
            }
        }
    }
}

#[test]
fn g2_1_a_deck_scope_holds_its_subdecks() {
    let f = studied();
    let deck = |id| Scope::Deck(meguri_model::ids::DeckId(id));
    let total = |scope: Scope| {
        let (messages, end) = run(&f.host, &page_request(scope, History::AllHistory, &[Part::CardCounts, Part::CardStatesOverTime]));
        assert_eq!(end, PageEnd::Complete);
        let counts: CardCountsRecord = record(&messages, Part::CardCounts);
        let all = counts.including_inactive;
        (
            all.new + all.learning + all.relearning + all.young + all.mature + all.suspended + all.buried,
            card_states(&messages).on(0).total(),
        )
    };
    let (lang, lang_states) = total(deck(LANG));
    let (verbs, verbs_states) = total(deck(VERBS));
    // The subdeck's cards are in its parent's scope.
    f.write(|scope| {
        scope.put(&card(102, VERBS))?;
        scope.insert_revlog(&revlog(NOW_MS - 5 * DAY_MS, 102))?;
        Ok(())
    });
    assert_eq!(total(deck(LANG)), (lang + 1, lang_states + 1));
    assert_eq!(total(deck(VERBS)), (verbs + 1, verbs_states + 1));
    assert!(lang > verbs);
    // A deck id that names no deck ends the request before any part.
    let (messages, end) = run(&f.host, &page_request(deck(NOTE), History::Last12Months, &every_part()));
    assert_eq!(end, PageEnd::ScopeError(ScopeError::UnknownDeck));
    assert!(messages.is_empty());
}

#[test]
fn g3_the_period_picks_the_history_and_all_reads_every_row() {
    assert_eq!(Period::All.history(), History::AllHistory);
    for period in [Period::OneMonth, Period::ThreeMonths, Period::SixMonths, Period::OneYear] {
        assert_eq!(period.history(), History::Last12Months);
    }
    let f = studied();
    let parts = [Part::Reviews, Part::Hours, Part::CardStatesOverTime];
    let (year, _) = run(&f.host, &page_request(Scope::Collection, History::Last12Months, &parts));
    let (all, _) = run(&f.host, &page_request(Scope::Collection, History::AllHistory, &parts));
    // The row 400 days ago is outside the year.
    let year_reviews: ReviewsRecord = record(&year, Part::Reviews);
    let all_reviews: ReviewsRecord = record(&all, Part::Reviews);
    let rows = |r: &ReviewsRecord| r.count.values().map(|k| k.values().iter().sum::<i64>()).sum::<i64>();
    assert_eq!(rows(&all_reviews), rows(&year_reviews) + 1);
    // All-history buckets only with All (G3).
    assert!(record::<HourlyRecord>(&year, Part::Hours).all_time.is_none());
    assert!(record::<HourlyRecord>(&all, Part::Hours).all_time.is_some());
    // H2.4: `start` is zero with All; with the year it holds what entered
    // before the window, and both end on the same day.
    let year_states = card_states(&year);
    let all_states = card_states(&all);
    assert_eq!(all_states.start.total(), 0);
    assert!(year_states.start.total() >= 1);
    assert_eq!(year_states.on(0), all_states.on(0));
}

#[test]
fn h18_a_search_scope_fails_only_the_meguri_parts() {
    let f = studied();
    let (messages, end) = run(&f.host, &page_request(Scope::Search("deck:*".into()), History::AllHistory, &every_part()));
    assert_eq!(end, PageEnd::Complete);
    let meguri_parts = [
        Part::StudiedToday,
        Part::RetentionByDay,
        Part::CardStatesOverTime,
        Part::Started,
        Part::ByDeck,
        Part::ActivityDays,
        Part::KnowledgeOverTime,
    ];
    for part in Part::all() {
        match outcome(&messages, part) {
            PartOutcome::Failed(PartFailure::UnsupportedScope) => assert!(meguri_parts.contains(&part), "{part:?}"),
            PartOutcome::Data(_) => assert!(!meguri_parts.contains(&part), "{part:?}"),
            other => panic!("{part:?}: {other:?}"),
        }
    }
    // White space only is the Collection scope (G2).
    let (messages, _) = run(&f.host, &page_request(Scope::Search("  ".into()), History::AllHistory, &[Part::Started]));
    assert!(matches!(outcome(&messages, Part::Started), PartOutcome::Data(_)));
    // A search error ends the request before any part.
    let (messages, end) = run(&f.host, &page_request(Scope::Search("prop:foo<".into()), History::AllHistory, &every_part()));
    assert!(matches!(end, PageEnd::ScopeError(ScopeError::Search(_))), "{end:?}");
    assert!(messages.is_empty());
}

#[test]
fn g2_a_white_space_search_is_all_decks_for_every_part_history_and_knowledge_included() {
    let f = studied();
    let (all, end) = run(&f.host, &page_request(Scope::Collection, History::AllHistory, &every_part()));
    assert_eq!(end, PageEnd::Complete);
    let (blank, end) = run(&f.host, &page_request(Scope::Search(" \t ".into()), History::AllHistory, &every_part()));
    assert_eq!(end, PageEnd::Complete);
    for part in Part::all() {
        match (outcome(&all, part), outcome(&blank, part)) {
            (PartOutcome::Data(a), PartOutcome::Data(b)) => assert_eq!(comparable(a), comparable(b), "{part:?}"),
            other => panic!("{part:?}: {other:?}"),
        }
    }
}

#[test]
fn h15_an_unreadable_row_in_the_window_fails_every_part_outside_only_the_history_ones() {
    let f = Fixture::new();
    let old = NOW_MS - 400 * DAY_MS;
    let cells = revlog_cells(old, 100, Cell::Integer(300), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1));
    f.write_raw(move |scope| {
        scope.put(&card(100, LANG))?;
        scope.restore(Table::Revlog, &RowKey::Id(old), Some(&cells))?;
        Ok(())
    });
    let bad = Unreadable {
        table: "revlog",
        column: "ease",
        key: old,
    };
    let (messages, end) = run(&f.host, &page_request(Scope::Collection, History::AllHistory, &every_part()));
    assert_eq!(end, PageEnd::UnreadableRow(bad));
    assert!(messages.is_empty());
    let (messages, end) = run(&f.host, &page_request(Scope::Collection, History::Last12Months, &every_part()));
    assert_eq!(end, PageEnd::Complete);
    // H18: the parts that read all history fail alone (H2, H11), and the
    // header carries no `oldest_day` (H9).
    let all_history = |part: Part| {
        matches!(part, Part::CardStatesOverTime | Part::Started | Part::ActivityDays | Part::KnowledgeOverTime)
    };
    for part in Part::all().filter(|p| !PENDING.contains(p)) {
        match outcome(&messages, part) {
            PartOutcome::Failed(PartFailure::UnreadableRow(row)) => {
                assert!(all_history(part), "{part:?}");
                assert_eq!(*row, bad);
            }
            PartOutcome::Data(_) => assert!(!all_history(part), "{part:?}"),
            other => panic!("{part:?}: {other:?}"),
        }
    }
    let header = &parts_of(&messages)[0].header;
    assert_eq!(header.oldest_day, page::OldestDay::Unavailable(PartFailure::UnreadableRow(bad)));
}

#[test]
fn h17_no_part_and_no_build_writes_the_collection() {
    let f = studied();
    // The day clock's own check (G1) runs first; its writes are its own.
    run(&f.host, &page_request(Scope::Collection, History::AllHistory, &[]));
    let before = f.dump();
    let deck = Scope::Deck(meguri_model::ids::DeckId(LANG));
    for part in Part::all() {
        for scope in [Scope::Collection, deck.clone()] {
            for history in [History::Last12Months, History::AllHistory] {
                run(&f.host, &page_request(scope.clone(), history, &[part]));
                assert_eq!(f.dump(), before, "{part:?} {scope:?} {history:?}");
            }
        }
    }
    // A rebuild of the history component, which a request waits for.
    f.host.derived().invalidate(Invalidation::CheckDatabase);
    let (messages, end) = run(&f.host, &page_request(Scope::Collection, History::AllHistory, &every_part()));
    assert_eq!(end, PageEnd::Complete);
    assert!(matches!(outcome(&messages, Part::CardStatesOverTime), PartOutcome::Data(_)));
    assert_eq!(f.dump(), before);
}

#[test]
fn h16_a_request_cancelled_before_it_starts_sends_nothing() {
    let f = studied();
    let request = page_request(Scope::Collection, History::AllHistory, &every_part());
    request.cancel.cancel();
    let (messages, end) = run(&f.host, &request);
    assert_eq!(end, PageEnd::Cancelled);
    assert!(messages.is_empty());
}

/// A collection of `cards` cards in `Lang` with `rows` review rows spread
/// over them, written before the host opens it: its history build takes
/// long enough to report progress.
struct Big {
    _dir: tempfile::TempDir,
    host: CoreHost,
}

fn big(cards: i64, rows: i64) -> Big {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::graphs_common::fixture_path(dir.path());
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    let clock = meguri_model::clock::FixedClock::new(NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(&clock, &random)).unwrap();
    let first = NOW_MS - 900 * DAY_MS;
    for statement in [
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
    ] {
        opened.writer().execute_raw(&statement, &[]).unwrap();
    }
    drop(opened);
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host = meguri_testkit::bench::fixtures::open_engine_collection(&path, clock).unwrap();
    Big { _dir: dir, host }
}

/// Waits until the history component is `Ready`, failing if it fails.
fn until_ready(host: &CoreHost) {
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness != Readiness::Ready {
        assert!(!host.derived().has_failed(HISTORY_COMPONENT), "the build failed");
        assert!(meguri_engine::monotonic::now() < deadline, "the build stopped");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn h15_the_first_tiers_return_while_the_build_runs_then_kept_values_come_at_once() {
    let big = big(30_000, 240_000);
    let host = &big.host;
    let request = page_request(Scope::Collection, History::AllHistory, &every_part());
    let collection = host.collection().unwrap();
    // What had returned when the first progress came.
    let mut seen: Vec<PageMessage> = Vec::new();
    let mut before_progress: Option<Vec<Part>> = None;
    let end = page::request(host.stats(), &collection, host.derived(), &request, &mut |message| {
        if matches!(message, PageMessage::Progress { .. }) && before_progress.is_none() {
            before_progress = Some(parts_of(&seen).iter().map(|m| m.part).collect());
        }
        seen.push(message);
    })
    .unwrap();
    assert_eq!(end, PageEnd::Complete);
    // Partial readiness: every part that does not wait for the build had
    // returned, in its tier, before the history parts' first progress.
    let before = before_progress.expect("a build this size reports progress");
    let ready_at_once: Vec<Part> = Part::all()
        .filter(|p| p.tier() <= Tier::ReviewLog && !matches!(p, Part::CardStatesOverTime | Part::Started))
        .collect();
    let mut sorted = before.clone();
    sorted.sort();
    assert_eq!(sorted, ready_at_once);
    // Progress for both history parts: fractions in [0, 1], never going back.
    for part in [Part::CardStatesOverTime, Part::Started] {
        let mut last = 0.0;
        let progress = progress_of(&seen, part);
        assert!(progress.iter().any(|p| matches!(p, Progress::Fraction(_))), "{part:?}: {progress:?}");
        for p in progress {
            if let Progress::Fraction(fraction) = p {
                assert!((0.0..=1.0).contains(&fraction), "{fraction}");
                assert!(fraction >= last, "{fraction} after {last}");
                last = fraction;
            }
        }
    }
    // No progress after a part has returned.
    let returned_at = seen
        .iter()
        .position(|m| matches!(m, PageMessage::Part(p) if p.part == Part::CardStatesOverTime))
        .unwrap();
    assert!(!seen[returned_at..].iter().any(|m| matches!(m, PageMessage::Progress { .. })));
    let first = card_states(&seen);

    // The component is kept: the next request reads it at once, with no
    // progress, and gives the same values.
    let (again, end) = run(host, &request);
    assert_eq!(end, PageEnd::Complete);
    assert!(again.iter().all(|m| matches!(m, PageMessage::Part(_))));
    assert_eq!(card_states(&again), first);

    // A commit, then a refresh: its values follow the commit and equal a
    // computation from the snapshot.
    let id = NOW_MS - 900 * DAY_MS + 9;
    collection
        .run(meguri_engine::OpSpec::named("answer", meguri_model::changes::OpSource::UserUndoable), move |ctx| {
            let mut c = meguri_storage::ReadRows::card(&*ctx.scope(), meguri_model::ids::CardId(id))?.unwrap();
            c.card_type = meguri_model::card::CardType::Relearning;
            ctx.scope().put(&c)?;
            let mut row = revlog(NOW_MS - 1_000, id);
            row.button = 1;
            row.interval = -600;
            row.kind = meguri_model::revlog::RevlogKind::Relearning;
            ctx.scope().insert_revlog(&row)?;
            Ok(())
        })
        .unwrap();
    let (refreshed, end) = run(host, &request);
    assert_eq!(end, PageEnd::Complete);
    let (fresh, _) = run(host, &PageRequest { columns: Columns::Snapshot, ..request.clone() });
    assert_ne!(card_states(&refreshed), first);
    assert_eq!(card_states(&refreshed), card_states(&fresh));
    assert_eq!(card_states(&refreshed).on(0).relearning, first.on(0).relearning + 1);
}

#[test]
fn h16_a_cancel_during_the_build_stops_the_request_within_50_ms_and_the_build_goes_on() {
    let big = big(30_000, 240_000);
    let host = Arc::new(big.host);
    let request = page_request(Scope::Collection, History::AllHistory, &every_part());
    let cancel = Arc::clone(&request.cancel);
    let waiting = Arc::new(AtomicBool::new(false));
    let after_cancel = Arc::new(Mutex::new(Vec::new()));
    let (release, go) = std::sync::mpsc::channel::<()>();
    let reader = {
        let (host, waiting, after_cancel) = (Arc::clone(&host), Arc::clone(&waiting), Arc::clone(&after_cancel));
        std::thread::spawn(move || {
            let collection = host.collection().unwrap();
            // The first progress holds the request until the test has
            // cancelled it, so it is still waiting then.
            let mut held = Some(go);
            let end = page::request(host.stats(), &collection, host.derived(), &request, &mut |message| {
                if waiting.load(Ordering::SeqCst) {
                    after_cancel.lock().unwrap().push(message);
                    return;
                }
                if matches!(message, PageMessage::Progress { .. }) {
                    waiting.store(true, Ordering::SeqCst);
                    if let Some(go) = held.take() {
                        go.recv().unwrap();
                    }
                }
            })
            .unwrap();
            (end, meguri_engine::monotonic::now())
        })
    };
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while !waiting.load(Ordering::SeqCst) {
        assert!(meguri_engine::monotonic::now() < deadline, "the request never waited for the build");
        std::thread::sleep(Duration::from_millis(1));
    }
    let cancelled_at = meguri_engine::monotonic::now();
    cancel.cancel();
    release.send(()).unwrap();
    let (end, returned_at) = reader.join().unwrap();
    assert_eq!(end, PageEnd::Cancelled);
    let took = returned_at.saturating_sub(cancelled_at);
    assert!(took <= Duration::from_millis(50), "returned {took:?} after the cancel");
    assert!(after_cancel.lock().unwrap().is_empty(), "nothing is sent after a cancel");
    // The build was not cancelled with it, and the next request reads it.
    until_ready(&host);
    let (messages, end) = run(&host, &page_request(Scope::Collection, History::AllHistory, &[Part::Started]));
    assert_eq!(end, PageEnd::Complete);
    assert!(messages.iter().all(|m| matches!(m, PageMessage::Part(_))), "no wait: the build is kept");
}

#[test]
fn g8_3_a_commit_while_a_part_waits_ends_the_request_with_context_changed() {
    let big = big(30_000, 240_000);
    let host = &big.host;
    let request = page_request(Scope::Collection, History::AllHistory, &every_part());
    let collection = host.collection().unwrap();
    let mut seen: Vec<PageMessage> = Vec::new();
    let mut committed = false;
    let end = page::request(host.stats(), &collection, host.derived(), &request, &mut |message| {
        if matches!(message, PageMessage::Progress { .. }) && !committed {
            committed = true;
            collection
                .run(meguri_engine::OpSpec::named("set_config", meguri_model::changes::OpSource::UserUndoable), |ctx| {
                    ctx.scope().set_config("firstDayOfWeek", &ConfigValue::Integer(1))?;
                    Ok(())
                })
                .unwrap();
        }
        seen.push(message);
    })
    .unwrap();
    assert!(committed);
    assert_eq!(end, PageEnd::ContextChanged);
    // The parts that had returned hold for their context; the waiting ones
    // and the third tier are dropped.
    let returned: Vec<Part> = parts_of(&seen).iter().map(|m| m.part).collect();
    assert!(returned.contains(&Part::CardCounts) && returned.contains(&Part::Reviews), "{returned:?}");
    for dropped in [Part::CardStatesOverTime, Part::Started, Part::KnowledgeOverTime] {
        assert!(!returned.contains(&dropped), "{dropped:?}");
    }
    // The next refresh takes a new context and completes.
    until_ready(host);
    let (messages, end) = run(host, &request);
    assert_eq!(end, PageEnd::Complete);
    assert!(matches!(outcome(&messages, Part::CardStatesOverTime), PartOutcome::Data(_)));
}
