//! The page request's relay and encoding (U103, U292), on a schema-18
//! fixture opened by the production host: every message and outcome kind
//! as the bridge carries it, the header once before the first part, the
//! `End` last, G6's bytes of every value, and the cancel rule. Names and
//! values are made up.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use meguri_engine::CancelToken;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_stats::graphs::encoding::{Decoder, Encoder, MemberKind};
use meguri_stats::graphs::{Columns, Graph, History, Scope, ScopeError, StatsError, Unreadable};
use meguri_stats::history::{CardStatesOverTime, KnowledgeOverTime, KnowledgeSample, Progress, StartedPart, StateCounts};
use meguri_stats::parts::activity::ActivityDaysPart;
use meguri_stats::parts::by_deck::{ByDeckPart, DeckRow, DeckValues, DeckWindows};
use meguri_stats::parts::retention_by_day::{RetentionByDayPart, RetentionDay};
use meguri_stats::parts::studied_today::StudiedTodayPart;
use meguri_stats::page::{
    self, ByAnswerPart, OldestDay, PageEnd, PageHeader, PageMessage, PageRequest, Part, PartFailure, PartMessage,
    PartOutcome, PartValue, Period,
};
use meguri_testkit::bench::fixtures::CoreHost;

use super::*;

/// 2030-01-15T10:00:00Z.
const NOW_MS: i64 = 1_894_701_600_000;
const DAY_MS: i64 = 86_400_000;
/// The fixture's deck `Lang` and two of its cards.
const LANG: i64 = 1_788_249_600_212;
const REVIEW_CARD: i64 = 1_788_249_600_238;
const LEARN_CARD: i64 = 1_788_249_600_240;

struct Fixture {
    _dir: tempfile::TempDir,
    host: CoreHost,
}

/// The schema-18 fixture with forty review-log rows over the last forty
/// days, of every answer and an unrated one, written before the host opens
/// it.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection").join("collection.anki2");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    {
        let clock = meguri_model::clock::FixedClock::new(NOW_MS, UtcOffset::UTC);
        let random = meguri_model::rng::SeededRandom::new(1);
        let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(&clock, &random)).unwrap();
        for day in 1..=40i64 {
            let card = if day % 2 == 0 { REVIEW_CARD } else { LEARN_CARD };
            let statement = format!(
                "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES ({}, {card}, -1, {}, \
                 {}, {}, 2500, {}, 1)",
                NOW_MS - day * DAY_MS,
                day % 5,
                day % 30 + 1,
                day % 40,
                1_000 * day,
            );
            opened.writer().execute_raw(&statement, &[]).unwrap();
        }
    }
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host = meguri_testkit::bench::fixtures::open_engine_collection(&path, clock).unwrap();
    Fixture { _dir: dir, host }
}

fn request(scope: Scope, history: History) -> PageRequest {
    PageRequest {
        scope,
        history,
        parts: Part::all().collect(),
        refresh: 9,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    }
}

/// Keeps every message, and fires `cancel_on_first_part` when the first
/// part arrives.
#[derive(Default)]
struct Recorder {
    messages: Mutex<Vec<StatsPageMessage>>,
    cancel_on_first_part: Option<Arc<CancelToken>>,
}

impl StatsPageListener for Recorder {
    fn on_message(&self, message: StatsPageMessage) {
        if let (Some(token), StatsPageMessage::Part { .. }) = (&self.cancel_on_first_part, &message) {
            token.cancel();
        }
        self.messages.lock().unwrap().push(message);
    }
}

impl Recorder {
    fn taken(&self) -> Vec<StatsPageMessage> {
        std::mem::take(&mut self.messages.lock().unwrap())
    }
}

/// Runs a real request through the relay: the core's messages, the relayed
/// ones and the `End`.
fn relay(fixture: &Fixture, request: &PageRequest) -> (Vec<PageMessage>, Vec<StatsPageMessage>, StatsPageEnd) {
    let collection = fixture.host.collection().unwrap();
    let recorder = Recorder::default();
    let mut core = Vec::new();
    let end = run(request, &recorder, |sink| {
        page::request(fixture.host.stats(), &collection, fixture.host.derived(), request, &mut |message| {
            core.push(message.clone());
            sink(message);
        })
    });
    (core, recorder.taken(), end)
}

fn record_bytes(value: &PartValue) -> Vec<u8> {
    match value {
        PartValue::Record(record) => {
            let mut out = Encoder::new();
            record.encode(&mut out);
            out.into_bytes()
        }
        other => panic!("not a record: {other:?}"),
    }
}

fn optional(decoder: &mut Decoder<'_>) -> Option<i32> {
    let items = decoder.list(|decoder| decoder.integer()).unwrap();
    assert!(items.len() <= 1, "an optional value is a list of none or one item");
    items.first().map(|day| i32::try_from(*day).unwrap())
}

fn decode_by_answer(bytes: &[u8]) -> ByAnswerPart {
    let mut decoder = Decoder::new(bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers; 8]).unwrap();
    let unrated = u64::try_from(decoder.integer().unwrap()).unwrap();
    assert!(decoder.is_done(), "no byte after `unrated`");
    let mut part = ByAnswerPart {
        unrated,
        ..ByAnswerPart::default()
    };
    for (at, key) in map.keys.iter().enumerate() {
        let day = i32::try_from(*key).unwrap();
        part.count_by_answer.insert(day, std::array::from_fn(|answer| map.integers[answer][at]));
        part.time_by_answer.insert(day, std::array::from_fn(|answer| map.integers[4 + answer][at]));
    }
    part
}

fn decode_card_states(bytes: &[u8]) -> CardStatesOverTime {
    let mut decoder = Decoder::new(bytes);
    let start: [i64; 5] = std::array::from_fn(|_| decoder.integer().unwrap());
    let map = decoder.sparse_map(&[MemberKind::Integers; 5]).unwrap();
    let left_out = u64::try_from(decoder.integer().unwrap()).unwrap();
    let first_day = optional(&mut decoder);
    assert!(decoder.is_done(), "no byte after `first_day`");
    CardStatesOverTime {
        start: StateCounts::from(start),
        deltas: map
            .keys
            .iter()
            .enumerate()
            .map(|(at, key)| {
                let delta: [i64; 5] = std::array::from_fn(|state| map.integers[state][at]);
                (i32::try_from(*key).unwrap(), StateCounts::from(delta))
            })
            .collect(),
        left_out,
        first_day,
    }
}

fn decode_started(bytes: &[u8]) -> StartedPart {
    let mut decoder = Decoder::new(bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers]).unwrap();
    let first_day = optional(&mut decoder);
    assert!(decoder.is_done(), "no byte after `first_day`");
    StartedPart {
        days: map
            .keys
            .iter()
            .zip(&map.integers[0])
            .map(|(key, count)| (i32::try_from(*key).unwrap(), u64::try_from(*count).unwrap()))
            .collect(),
        first_day,
    }
}

fn decode_knowledge(bytes: &[u8]) -> KnowledgeOverTime {
    let mut decoder = Decoder::new(bytes);
    let samples = decoder
        .list(|decoder| {
            Ok(KnowledgeSample {
                day: i32::try_from(decoder.integer()?).unwrap(),
                instant_ms: decoder.integer()?,
                knowledge: decoder.f64()?,
                studied: u64::try_from(decoder.integer()?).unwrap(),
            })
        })
        .unwrap();
    let failed = u64::try_from(decoder.integer().unwrap()).unwrap();
    let left_out = u64::try_from(decoder.integer().unwrap()).unwrap();
    assert!(decoder.is_done(), "no byte after `left_out`");
    KnowledgeOverTime {
        samples,
        failed,
        left_out,
    }
}

fn decode_studied_today(bytes: &[u8]) -> StudiedTodayPart {
    let mut decoder = Decoder::new(bytes);
    let part = StudiedTodayPart {
        answers: u64::try_from(decoder.integer().unwrap()).unwrap(),
        cards: u64::try_from(decoder.integer().unwrap()).unwrap(),
        time_ms: decoder.integer().unwrap(),
    };
    assert!(decoder.is_done());
    part
}

fn decode_retention_by_day(bytes: &[u8]) -> RetentionByDayPart {
    let mut decoder = Decoder::new(bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers; 5]).unwrap();
    let sums = decoder.list(|decoder| decoder.f64()).unwrap();
    assert_eq!(sums.len(), map.keys.len(), "one sum per day");
    let retentions = decoder.list(|decoder| decoder.f32()).unwrap();
    let several = match decoder.integer().unwrap() {
        0 => false,
        1 => true,
        other => panic!("several {other}"),
    };
    assert!(decoder.is_done());
    let count = |member: usize, at: usize| u64::try_from(map.integers[member][at]).unwrap();
    RetentionByDayPart {
        days: map
            .keys
            .iter()
            .enumerate()
            .map(|(at, key)| {
                (i32::try_from(*key).unwrap(), RetentionDay {
                    young_passed: count(0, at),
                    young_failed: count(1, at),
                    mature_passed: count(2, at),
                    mature_failed: count(3, at),
                    desired_sum: sums[at],
                    desired_rows: count(4, at),
                })
            })
            .collect(),
        retentions,
        several,
    }
}

fn decode_deck_values(decoder: &mut Decoder<'_>) -> Result<DeckValues, meguri_stats::graphs::encoding::DecodeError> {
    Ok(DeckValues {
        reviews: u64::try_from(decoder.integer()?).unwrap(),
        time_ms: decoder.integer()?,
        passed: u64::try_from(decoder.integer()?).unwrap(),
        failed: u64::try_from(decoder.integer()?).unwrap(),
    })
}

fn decode_by_deck(bytes: &[u8]) -> ByDeckPart {
    let mut decoder = Decoder::new(bytes);
    let rows = decoder
        .list(|decoder| {
            let deck = meguri_model::ids::DeckId(decoder.integer()?);
            let own_cards = decoder.integer()? == 1;
            let cards = u64::try_from(decoder.integer()?).unwrap();
            let one_month = decode_deck_values(decoder)?;
            let three_months = decode_deck_values(decoder)?;
            let six_months = decode_deck_values(decoder)?;
            let one_year = decode_deck_values(decoder)?;
            let all_time = decoder.list(decode_deck_values)?;
            assert!(all_time.len() <= 1);
            Ok(DeckRow {
                deck,
                own_cards,
                cards,
                windows: DeckWindows {
                    one_month,
                    three_months,
                    six_months,
                    one_year,
                    all_time: all_time.into_iter().next(),
                },
            })
        })
        .unwrap();
    assert!(decoder.is_done());
    ByDeckPart { rows }
}

fn decode_activity_days(bytes: &[u8]) -> ActivityDaysPart {
    let mut decoder = Decoder::new(bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers]).unwrap();
    assert!(decoder.is_done());
    ActivityDaysPart {
        days: map
            .keys
            .iter()
            .zip(&map.integers[0])
            .map(|(key, count)| (i32::try_from(*key).unwrap(), u64::try_from(*count).unwrap()))
            .collect(),
    }
}

fn first_header(core: &[PageMessage]) -> Arc<PageHeader> {
    core.iter()
        .find_map(|message| match message {
            PageMessage::Part(part) => Some(Arc::clone(&part.header)),
            PageMessage::Progress { .. } => None,
        })
        .expect("a part")
}

fn ends(messages: &[StatsPageMessage]) -> Vec<&StatsPageEnd> {
    messages
        .iter()
        .filter_map(|message| match message {
            StatsPageMessage::End { end } => Some(end),
            _ => None,
        })
        .collect()
}

#[test]
fn every_value_crosses_as_g6_bytes_after_one_header_and_before_the_end() {
    let fixture = fixture();
    for (scope, history) in [
        (Scope::Collection, History::AllHistory),
        (Scope::Deck(meguri_model::ids::DeckId(LANG)), History::Last12Months),
    ] {
        let request = request(scope.clone(), history);
        let (core, relayed, end) = relay(&fixture, &request);
        assert_eq!(end, StatsPageEnd::Complete, "{scope:?}");
        assert_eq!(ends(&relayed), [&StatsPageEnd::Complete], "one End");
        assert!(matches!(relayed.last(), Some(StatsPageMessage::End { .. })), "the End is last");

        // The header once, first, as the request computed it.
        let header = first_header(&core);
        let headers: Vec<_> = relayed
            .iter()
            .enumerate()
            .filter(|(_, message)| matches!(message, StatsPageMessage::Header { .. }))
            .collect();
        assert_eq!(headers.len(), 1, "{scope:?}");
        let first_part = relayed.iter().position(|m| matches!(m, StatsPageMessage::Part { .. })).unwrap();
        assert!(headers[0].0 < first_part, "the header comes before the first part");
        let StatsPageMessage::Header { header: sent } = headers[0].1 else { unreachable!() };
        assert_eq!(sent, &StatsPageHeader::new(&header, history));
        assert_eq!(sent.refresh, 9);
        assert_eq!(sent.history, StatsHistory::from(history));
        assert_eq!(Context::from(sent.context.clone()), header.context, "the context goes back unchanged");

        // Each part once, in the core's order, with its value's bytes.
        let parts: Vec<(StatsPart, &StatsPartOutcome)> = relayed
            .iter()
            .filter_map(|message| match message {
                StatsPageMessage::Part { part, outcome } => Some((*part, outcome)),
                _ => None,
            })
            .collect();
        let core_parts: Vec<&PartMessage> = core
            .iter()
            .filter_map(|message| match message {
                PageMessage::Part(part) => Some(part),
                PageMessage::Progress { .. } => None,
            })
            .collect();
        assert_eq!(parts.len(), Part::all().count(), "{scope:?}");
        for ((part, outcome), core) in parts.iter().zip(&core_parts) {
            assert_eq!(Part::from(*part), core.part);
            match (&core.outcome, outcome) {
                (PartOutcome::Data(value), StatsPartOutcome::Data { bytes }) => match value {
                    PartValue::Record(_) => assert_eq!(bytes, &record_bytes(value), "{part:?}"),
                    PartValue::ByAnswer(by_answer) => assert_eq!(&decode_by_answer(bytes), by_answer),
                    PartValue::CardStates(states) => assert_eq!(&decode_card_states(bytes), states),
                    PartValue::Started(started) => assert_eq!(&decode_started(bytes), started),
                    PartValue::Knowledge(knowledge) => assert_eq!(&decode_knowledge(bytes), knowledge),
                    PartValue::StudiedToday(part) => assert_eq!(&decode_studied_today(bytes), part),
                    PartValue::RetentionByDay(part) => assert_eq!(&decode_retention_by_day(bytes), part),
                    PartValue::ByDeck(part) => assert_eq!(&decode_by_deck(bytes), part),
                    PartValue::ActivityDays(part) => assert_eq!(&decode_activity_days(bytes), part),
                },
                (PartOutcome::Failed(failure), StatsPartOutcome::Failed { failure: sent }) => {
                    assert_eq!(sent, &StatsPartFailure::from(failure), "{part:?}");
                }
                other => panic!("{part:?}: {other:?}"),
            }
        }
        // The fixture gives the history parts and By Answer values to read
        // back, not empty ones.
        let value = |wanted: Part| match core_parts.iter().find(|p| p.part == wanted).map(|p| &p.outcome) {
            Some(PartOutcome::Data(value)) => value.clone(),
            other => panic!("{wanted:?}: {other:?}"),
        };
        let PartValue::ByAnswer(by_answer) = value(Part::ByAnswer) else { panic!() };
        assert!(!by_answer.count_by_answer.is_empty() && by_answer.unrated > 0, "{by_answer:?}");
        let PartValue::CardStates(states) = value(Part::CardStatesOverTime) else { panic!() };
        assert!(!states.deltas.is_empty() && states.first_day.is_some(), "{states:?}");
        let PartValue::Started(started) = value(Part::Started) else { panic!() };
        assert!(started.first_day.is_some(), "{started:?}");
        let PartValue::Knowledge(knowledge) = value(Part::KnowledgeOverTime) else { panic!() };
        assert!(!knowledge.samples.is_empty(), "{knowledge:?}");
        let PartValue::RetentionByDay(retention) = value(Part::RetentionByDay) else { panic!() };
        assert!(!retention.days.is_empty() && !retention.retentions.is_empty(), "{retention:?}");
        let PartValue::ByDeck(decks) = value(Part::ByDeck) else { panic!() };
        assert!(!decks.rows.is_empty(), "{decks:?}");
        let PartValue::ActivityDays(activity) = value(Part::ActivityDays) else { panic!() };
        assert!(!activity.days.is_empty(), "{activity:?}");
        assert!(matches!(value(Part::StudiedToday), PartValue::StudiedToday(_)));
        // H9's oldest day arrives as a day.
        assert!(matches!(sent.oldest_day, StatsOldestDay::Day { .. }), "{:?}", sent.oldest_day);
    }
}

#[test]
fn every_failure_progress_and_end_kind_maps_and_the_header_follows_its_value() {
    let fixture = fixture();
    let (core, _, _) = relay(&fixture, &request(Scope::Collection, History::AllHistory));
    let header = first_header(&core);
    let other_header = Arc::new(PageHeader {
        oldest_day: OldestDay::Day(-300),
        ..(*header).clone()
    });
    let bad = Unreadable {
        table: "revlog",
        column: "ease",
        key: 5,
    };
    let failed = |part: Part, header: &Arc<PageHeader>, failure: PartFailure| {
        PageMessage::Part(PartMessage {
            part,
            header: Arc::clone(header),
            outcome: PartOutcome::Failed(failure),
        })
    };
    let sent = vec![
        PageMessage::Progress {
            part: Part::Started,
            progress: Progress::Fraction(0.25),
        },
        failed(Part::CardCounts, &header, PartFailure::UnsupportedScope),
        failed(Part::Started, &header, PartFailure::UnreadableRow(bad)),
        PageMessage::Progress {
            part: Part::KnowledgeOverTime,
            progress: Progress::Indeterminate,
        },
        failed(Part::ByDeck, &header, PartFailure::NotBuilt),
        failed(Part::Hours, &other_header, PartFailure::Failed("disk full".into())),
    ];
    let request = request(Scope::Collection, History::AllHistory);
    let recorder = Recorder::default();
    let end = run(&request, &recorder, |sink| {
        for message in sent {
            sink(message);
        }
        Ok(PageEnd::ContextChanged)
    });
    assert_eq!(end, StatsPageEnd::ContextChanged);
    let row = StatsUnreadable {
        table: "revlog".into(),
        column: "ease".into(),
        key: 5,
    };
    let failure = |part: StatsPart, failure: StatsPartFailure| StatsPageMessage::Part {
        part,
        outcome: StatsPartOutcome::Failed { failure },
    };
    assert_eq!(recorder.taken(), vec![
        StatsPageMessage::Progress {
            part: StatsPart::Started,
            progress: StatsProgress::Fraction { fraction: 0.25 },
        },
        StatsPageMessage::Header {
            header: StatsPageHeader::new(&header, History::AllHistory),
        },
        failure(StatsPart::CardCounts, StatsPartFailure::UnsupportedScope),
        failure(StatsPart::Started, StatsPartFailure::UnreadableRow { row: row.clone() }),
        StatsPageMessage::Progress {
            part: StatsPart::KnowledgeOverTime,
            progress: StatsProgress::Indeterminate,
        },
        failure(StatsPart::ByDeck, StatsPartFailure::NotBuilt),
        // A part computed for another header brings it first.
        StatsPageMessage::Header {
            header: StatsPageHeader::new(&other_header, History::AllHistory),
        },
        failure(StatsPart::Hours, StatsPartFailure::Failed {
            message: "disk full".into()
        }),
        StatsPageMessage::End {
            end: StatsPageEnd::ContextChanged
        },
    ]);

    // H9's three values.
    for (day, expected) in [
        (OldestDay::Day(-300), StatsOldestDay::Day { day: -300 }),
        (OldestDay::NoRows, StatsOldestDay::NoRows),
        (OldestDay::Unavailable(PartFailure::NotBuilt), StatsOldestDay::Unavailable {
            failure: StatsPartFailure::NotBuilt,
        }),
        (OldestDay::Unavailable(PartFailure::UnreadableRow(bad)), StatsOldestDay::Unavailable {
            failure: StatsPartFailure::UnreadableRow { row: row.clone() },
        }),
    ] {
        assert_eq!(StatsOldestDay::from(&day), expected);
    }

    // Every end, and an error before any part.
    for (result, expected) in [
        (Ok(PageEnd::Complete), StatsPageEnd::Complete),
        (Ok(PageEnd::ScopeError(ScopeError::UnknownDeck)), StatsPageEnd::ScopeError {
            error: StatsScopeError::UnknownDeck,
        }),
        (Ok(PageEnd::UnreadableRow(bad)), StatsPageEnd::UnreadableRow { row: row.clone() }),
        (Ok(PageEnd::ContextChanged), StatsPageEnd::ContextChanged),
        (Ok(PageEnd::Cancelled), StatsPageEnd::Cancelled),
        (Err(StatsError::NotBuilt(Graph::Calendar)), StatsPageEnd::Failed {
            error: CoreError::Unsupported {
                feature: "the calendar graph".into(),
            },
        }),
    ] {
        let recorder = Recorder::default();
        let request = self::request(Scope::Collection, History::Last12Months);
        assert_eq!(run(&request, &recorder, |_| result), expected);
        assert_eq!(recorder.taken(), vec![StatsPageMessage::End { end: expected }], "only the End");
    }
}

#[test]
fn scope_errors_and_a_search_scope_cross_with_their_kinds() {
    let fixture = fixture();
    let (_, relayed, end) = relay(&fixture, &request(Scope::Deck(meguri_model::ids::DeckId(42)), History::AllHistory));
    assert_eq!(end, StatsPageEnd::ScopeError {
        error: StatsScopeError::UnknownDeck
    });
    assert_eq!(relayed.len(), 1, "no part before a scope error: {relayed:?}");

    // A search error's range counts UTF-16 units of the scope's text.
    let (_, relayed, end) = relay(&fixture, &request(Scope::Search("\u{1D11E} prop:foo<".into()), History::AllHistory));
    let StatsPageEnd::ScopeError {
        error: StatsScopeError::Search { error },
    } = end
    else {
        panic!("{end:?}");
    };
    assert!(!error.kind.is_empty() && error.start >= 3 && error.end <= 13, "{error:?}");
    assert_eq!(relayed.len(), 1);

    // A search runs Anki's records; the page's own parts refuse it (H18).
    let (_, relayed, end) = relay(&fixture, &request(Scope::Search("deck:*".into()), History::AllHistory));
    assert_eq!(end, StatsPageEnd::Complete);
    for message in &relayed {
        if let StatsPageMessage::Part { part, outcome } = message {
            let own = matches!(
                part,
                StatsPart::StudiedToday
                    | StatsPart::RetentionByDay
                    | StatsPart::CardStatesOverTime
                    | StatsPart::Started
                    | StatsPart::ByDeck
                    | StatsPart::ActivityDays
                    | StatsPart::KnowledgeOverTime
            );
            if own {
                assert_eq!(
                    outcome,
                    &StatsPartOutcome::Failed {
                        failure: StatsPartFailure::UnsupportedScope
                    },
                    "{part:?}"
                );
            } else {
                assert!(matches!(outcome, StatsPartOutcome::Data { .. }), "{part:?}: {outcome:?}");
            }
        }
    }
}

#[test]
fn nothing_but_the_cancelled_end_follows_a_cancel() {
    let fixture = fixture();
    // Cancelled before it starts: the End alone.
    let request = request(Scope::Collection, History::AllHistory);
    request.cancel.cancel();
    let (_, relayed, end) = relay(&fixture, &request);
    assert_eq!(end, StatsPageEnd::Cancelled);
    assert_eq!(relayed, vec![StatsPageMessage::End {
        end: StatsPageEnd::Cancelled
    }]);

    // Cancelled by the receiver of the first part: no part after it.
    let request = self::request(Scope::Collection, History::AllHistory);
    let recorder = Recorder {
        cancel_on_first_part: Some(Arc::clone(&request.cancel)),
        ..Recorder::default()
    };
    let collection = fixture.host.collection().unwrap();
    let end = run(&request, &recorder, |sink| {
        page::request(fixture.host.stats(), &collection, fixture.host.derived(), &request, sink)
    });
    assert_eq!(end, StatsPageEnd::Cancelled);
    let relayed = recorder.taken();
    let parts = relayed.iter().filter(|m| matches!(m, StatsPageMessage::Part { .. })).count();
    assert_eq!(parts, 1, "{relayed:?}");
    assert_eq!(relayed.last(), Some(&StatsPageMessage::End {
        end: StatsPageEnd::Cancelled
    }));

    // A token fired after the last part makes the End Cancelled too.
    let request = self::request(Scope::Collection, History::AllHistory);
    let recorder = Recorder::default();
    let end = run(&request, &recorder, |_| {
        request.cancel.cancel();
        Ok(PageEnd::Complete)
    });
    assert_eq!(end, StatsPageEnd::Cancelled);
}

#[test]
fn periods_read_their_history_and_parts_map_one_to_one() {
    for (period, history) in [
        (StatsPeriod::OneMonth, History::Last12Months),
        (StatsPeriod::ThreeMonths, History::Last12Months),
        (StatsPeriod::SixMonths, History::Last12Months),
        (StatsPeriod::OneYear, History::Last12Months),
        (StatsPeriod::All, History::AllHistory),
    ] {
        assert_eq!(Period::from(period).history(), history, "{period:?}");
    }
    let core: Vec<Part> = Part::all().collect();
    let ffi: Vec<StatsPart> = core.iter().map(|part| StatsPart::from(*part)).collect();
    assert_eq!(ffi.len(), PARTS.len());
    assert_eq!(ffi.iter().map(|part| Part::from(*part)).collect::<Vec<_>>(), core);
}

#[test]
fn the_exports_need_an_open_collection() {
    assert!(matches!(start_release_stats_caches(), Err(CoreError::Unavailable { .. })));
    assert!(matches!(stats_home_idle(), Err(CoreError::Unavailable { .. })));
    struct Nobody;
    impl StatsPageListener for Nobody {
        fn on_message(&self, _: StatsPageMessage) {}
    }
    let request = StatsPageRequest {
        scope: StatsScope::AllDecks,
        period: StatsPeriod::All,
        parts: vec![StatsPart::CardCounts],
        refresh: 1,
    };
    assert!(matches!(
        start_stats_page(request, Arc::new(Nobody)),
        Err(CoreError::Unavailable { .. })
    ));
}
