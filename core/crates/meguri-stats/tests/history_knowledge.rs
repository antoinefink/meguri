//! `stats/history-reads` H3 and its part of H14 (U290): Knowledge Over
//! Time, kept per deck key in the `stats_history` component. There is no
//! oracle (H20): hand-built logs checked against Card Info's per-row states
//! (C6.3) and curve (C7.4), the scheduler's own memory items for the cutoff,
//! and the component against a cold replay after random changes. Names and
//! values are made up.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use meguri_engine::activity::{civil, today_day_number};
use meguri_engine::derived_host::Invalidation;
use meguri_engine::{CancelToken, Readiness};
use meguri_model::card::{CardQueue, CardType};
use meguri_model::card_data::MemoryState;
use meguri_model::clock::{Clock, UtcOffset};
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{CardId, DeckConfigId, DeckId, RevlogId};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::DeckKind;
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_sched::memory::items::{History as Items, HistoryForm};
use meguri_sched::model::preset::PresetView;
use meguri_stats::graphs::{History, Scope};
use meguri_stats::history::{
    self, CardMemory, HISTORY_COMPONENT, KnowledgeData, KnowledgeOverTime, KnowledgeRequest, KnowledgeResult, Progress,
};
use meguri_stats::{CardInfoOutcome, card_info};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};

use crate::graphs_common::{CRAM, Fixture, LANG, VERBS, card};
use crate::history_states::{Mix, REBUILDS, add_deck, at, big, current, dump, random_collection, row, scopes, shaped, stale_events, stored};

const DAY: i64 = 86_400;
const WAIT: Duration = Duration::from_secs(120);
const LEARN: RevlogKind = RevlogKind::Learning;
const REVIEW: RevlogKind = RevlogKind::Review;
const RELEARN: RevlogKind = RevlogKind::Relearning;

fn request(scope: Scope, history: History, oldest_day: Option<i32>) -> KnowledgeRequest {
    KnowledgeRequest {
        scope,
        history,
        oldest_day,
        cancel: CancelToken::new(),
    }
}

fn read_with(f: &Fixture, request: &KnowledgeRequest) -> KnowledgeResult {
    let collection = f.collection();
    history::knowledge_over_time(f.host.stats(), &collection, f.host.derived(), request, &mut |_| {}).unwrap()
}

fn data(result: KnowledgeResult) -> Box<KnowledgeData> {
    match result {
        KnowledgeResult::Data(data) => data,
        other => panic!("not data: {other:?}"),
    }
}

/// The part as the component serves it.
fn warm(f: &Fixture, scope: Scope, history: History) -> KnowledgeOverTime {
    let data = data(read_with(f, &request(scope, history, None)));
    assert!(data.from_component);
    data.knowledge
}

/// The samples by day, today's point under day 0.
fn by_day(part: &KnowledgeOverTime) -> BTreeMap<i32, (f64, u64)> {
    part.samples.iter().map(|s| (s.day, (s.knowledge, s.studied))).collect()
}

/// C7.4's curve, binary64.
fn curve(x: f64, stability: f64, decay: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let factor = 0.9f64.powf(1.0 / -decay) - 1.0;
    (x / stability * factor + 1.0).powf(-decay)
}

/// A kept sum is whole 2^-40 cards: within that of the exact value.
fn close(actual: f64, expected: f64, what: &str) {
    let tolerance = expected.abs() * 1e-9 + 1e-11;
    assert!((actual - expected).abs() <= tolerance, "{what}: {actual} against {expected}");
}

/// A preset with these FSRS values, and a deck on it.
fn preset_deck(f: &Fixture, name: &str, params: Option<Vec<f32>>, cutoff: &str) -> (i64, i64) {
    let mut message = DeckConfigMessage::builtin_defaults();
    if let Some(params) = params {
        message.set_fsrs_params_6(params);
    }
    message.ignore_revlogs_before_date = cutoff.to_owned();
    let config = DeckConfig {
        id: DeckConfigId(0),
        name: format!("{name} options"),
        mtime: Default::default(),
        usn: Default::default(),
        config: ProtoBlob::new(message),
    };
    let mut deck = meguri_engine::decks::new_deck(name.to_owned());
    let (tx, rx) = std::sync::mpsc::channel();
    f.write(move |scope| {
        let preset = scope.add_deck_config(&config)?;
        if let DeckKind::Normal(normal) = &mut deck.kind.kind {
            normal.config_id = preset.0;
        }
        tx.send((preset.0, scope.add_deck(&deck)?.0)).unwrap();
        Ok(())
    });
    rx.recv().unwrap()
}

/// Edits preset `id` in one op.
fn edit_preset(f: &Fixture, id: i64, edit: impl FnOnce(&mut DeckConfigMessage) + Send + 'static) {
    f.write(move |scope| {
        let mut preset = ReadRows::deck_config(&*scope, DeckConfigId(id))?.unwrap();
        edit(&mut preset.config);
        scope.put(&preset)?;
        Ok(())
    });
}

fn preset_view(f: &Fixture, id: i64) -> PresetView {
    let collection = f.collection();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    PresetView::for_config(&*snapshot, DeckConfigId(id)).unwrap()
}

/// A card in `deck` created on `created` with these rows.
fn put_card(f: &Fixture, deck: i64, id: i64, rows: Vec<RevlogEntry>) {
    let c = card(id, deck);
    f.write(move |scope| {
        scope.put(&c)?;
        for r in &rows {
            scope.insert_revlog(r)?;
        }
        Ok(())
    });
}

/// FSRS-6's default parameters with the 21st (decay) changed.
fn params_with_decay(decay: f32) -> Vec<f32> {
    let mut params = fsrs::DEFAULT_PARAMETERS.to_vec();
    params[20] = decay;
    params
}

/// The kept rows of Card Info's history, oldest first: (instant, state).
fn kept_rows(f: &Fixture, id: i64) -> (Vec<(i64, MemoryState)>, Option<MemoryState>) {
    let collection = f.collection();
    let CardInfoOutcome::Found(info) = card_info(&collection, CardId(id)).unwrap() else { panic!("no card info") };
    let mut kept: Vec<(i64, MemoryState)> = info
        .history
        .iter()
        .filter(|entry| entry.rating >= 1 && !(entry.kind == RevlogKind::Filtered && entry.factor == 0))
        .map(|entry| (entry.instant, entry.memory_state.expect("a kept row has a state")))
        .collect();
    kept.reverse();
    (kept, info.raw.memory_state)
}

#[test]
fn h3_states_and_recall_follow_card_info_for_kept_rows() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let (_, custom) = preset_deck(&f, "Tuned", Some(params_with_decay(0.35)), "");
    let complete = add_deck(&f, "Complete");
    let started = add_deck(&f, "Started");
    let mixed = add_deck(&f, "Mixed");
    let cases: Vec<(i64, i64, Vec<RevlogEntry>)> = vec![
        // A learning run, reviews, a lapse.
        (complete, at(n, -420, 1), {
            let id = at(n, -420, 1);
            vec![
                row(at(n, -410, 10), id, 3, -600, 2500, LEARN),
                row(at(n, -410, 20) + 600_000, id, 3, 1, 2500, LEARN),
                row(at(n, -408, 30), id, 3, 3, 2500, REVIEW),
                row(at(n, -401, 40), id, 3, 9, 2500, REVIEW),
                row(at(n, -380, 50), id, 1, -600, 2500, REVIEW),
                row(at(n, -380, 60) + 600_000, id, 3, 5, 2500, RELEARN),
                row(at(n, -300, 70), id, 4, 40, 2500, REVIEW),
                row(at(n, -150, 80), id, 2, 60, 2500, REVIEW),
                row(at(n, -20, 90), id, 3, 120, 2500, REVIEW),
            ]
        }),
        // An incomplete history: reviews from before FSRS.
        (started, at(n, -600, 2), {
            let id = at(n, -600, 2);
            vec![
                row(at(n, -500, 11), id, 3, 4, 2300, REVIEW),
                row(at(n, -496, 21), id, 3, 10, 2400, REVIEW),
                row(at(n, -380, 31), id, 1, -600, 2200, REVIEW),
                row(at(n, -40, 41), id, 3, 30, 2200, REVIEW),
            ]
        }),
        // Rows that are not kept between kept ones: Set Due Date, cram, an
        // unrated review.
        (mixed, at(n, -300, 3), {
            let id = at(n, -300, 3);
            vec![
                row(at(n, -290, 12), id, 3, -600, 2500, LEARN),
                row(at(n, -289, 22), id, 3, 2, 2500, LEARN),
                row(at(n, -280, 32), id, 0, 15, 2500, RevlogKind::Manual),
                row(at(n, -270, 42), id, 3, 20, 0, RevlogKind::Filtered),
                row(at(n, -250, 52), id, 3, 25, 2500, REVIEW),
                row(at(n, -200, 62), id, 0, 0, 2500, REVIEW),
                row(at(n, -100, 72), id, 1, -600, 2500, REVIEW),
                row(at(n, -99, 82), id, 3, 4, 2500, RELEARN),
            ]
        }),
        // The same as the first, on a preset of its own decay.
        (custom, at(n, -420, 4), {
            let id = at(n, -420, 4);
            vec![
                row(at(n, -410, 13), id, 3, -600, 2500, LEARN),
                row(at(n, -409, 23), id, 3, 1, 2500, LEARN),
                row(at(n, -300, 33), id, 4, 30, 2500, REVIEW),
                row(at(n, -5, 43), id, 3, 90, 2500, REVIEW),
            ]
        }),
    ];
    for (deck, id, rows) in &cases {
        put_card(&f, *deck, *id, rows.clone());
    }
    let now = f.clock.now_ms();
    let collection = f.collection();
    for (deck, id, _) in &cases {
        let preset = {
            let lease = collection.reader(ReaderRole::Query).unwrap();
            let snapshot = lease.snapshot().unwrap();
            let home = ReadRows::deck(&*snapshot, DeckId(*deck)).unwrap().unwrap();
            let DeckKind::Normal(normal) = &home.kind.kind else { panic!() };
            normal.config_id
        };
        let decay = f64::from(preset_view(&f, preset).decay());
        let (kept, computed) = kept_rows(&f, *id);
        let part = warm(&f, Scope::Deck(DeckId(*deck)), History::AllHistory);
        assert_eq!(part.failed, 0);
        let mut seen = 0;
        for sample in &part.samples {
            let instant = sample.instant_ms.div_euclid(1_000);
            let last = kept.iter().rev().find(|(t, _)| *t <= instant);
            match last {
                None => assert_eq!(sample.studied, 0, "deck {deck} day {}", sample.day),
                Some((t, state)) => {
                    seen += 1;
                    assert_eq!(sample.studied, 1, "deck {deck} day {}", sample.day);
                    let expected = curve((instant - t) as f64 / 86_400.0, f64::from(state.stability), decay);
                    close(sample.knowledge, expected, &format!("deck {deck} day {}", sample.day));
                    assert!(sample.knowledge <= 1.0 && sample.knowledge >= 0.0);
                }
            }
        }
        assert!(seen > 280, "deck {deck}: {seen} samples");
        // Today's point is the replay at now, from the newest kept row.
        let today = part.samples.last().unwrap();
        assert_eq!((today.day, today.instant_ms), (0, now));
        // The kept memory over every row is Card Info's computed state.
        let lease = collection.reader(ReaderRole::Query).unwrap();
        f.host.derived().flush().unwrap();
        let snapshot = lease.snapshot().unwrap();
        let records = history::kept_memory_records(f.host.derived(), &snapshot).unwrap().expect("current");
        let record = records.iter().find(|m| m.id == *id).unwrap();
        let computed = computed.expect("a computed state");
        let (t, _) = kept.last().unwrap();
        assert_eq!(
            record.with_cutoff,
            CardMemory::State {
                stability: computed.stability,
                difficulty: computed.difficulty,
                last: *t,
            }
        );
        assert_eq!(record.preset, Some(preset));
    }
}

#[test]
fn h3_a_sample_uses_only_the_rows_it_had_seen() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let deck = add_deck(&f, "Causal");
    let id = at(n, -200, 1);
    put_card(
        &f,
        deck,
        id,
        vec![
            row(at(n, -190, 1), id, 3, -600, 2500, LEARN),
            row(at(n, -189, 2), id, 3, 3, 2500, LEARN),
            row(at(n, -150, 3), id, 3, 30, 2500, REVIEW),
        ],
    );
    let scope = Scope::Deck(DeckId(deck));
    let before = by_day(&warm(&f, scope.clone(), History::AllHistory));
    let past = |part: &BTreeMap<i32, (f64, u64)>, until: i32| -> Vec<(i32, (u64, u64))> {
        part.iter()
            .filter(|(day, _)| **day < until)
            .map(|(day, (k, s))| (*day, (k.to_bits(), *s)))
            .collect()
    };
    // An answer now, then a Reset marker: no past sample moves.
    let answer = f.clock.now_ms() - 1_000;
    f.write(move |scope| {
        scope.insert_revlog(&row(answer, id, 3, 60, 2500, REVIEW))?;
        Ok(())
    });
    let answered = by_day(&warm(&f, scope.clone(), History::AllHistory));
    assert_eq!(past(&answered, 0), past(&before, 0));
    assert_ne!(answered[&0].0.to_bits(), before[&0].0.to_bits(), "today's point follows the answer");
    let reset = f.clock.now_ms() - 500;
    f.write(move |scope| {
        scope.insert_revlog(&row(reset, id, 0, 0, 0, RevlogKind::Manual))?;
        Ok(())
    });
    let after_reset = by_day(&warm(&f, scope.clone(), History::AllHistory));
    assert_eq!(past(&after_reset, 0), past(&before, 0));
    assert_eq!(after_reset[&0].1, 0, "after a Reset the card has no item");
    // A sync bringing an older row moves the samples from its day on only.
    let older = at(n, -100, 4);
    f.write_raw(move |scope| {
        scope.insert_revlog(&row(older, id, 1, -600, 2500, REVIEW))?;
        Ok(())
    });
    let synced = by_day(&warm(&f, scope, History::AllHistory));
    assert_eq!(past(&synced, -100), past(&before, -100));
    assert_ne!(synced[&-100].0.to_bits(), before[&-100].0.to_bits());
}

/// The UTC date of whole second `t`, as I1.3's cutoff text.
fn date_text(t: i64) -> String {
    let date = civil(t.div_euclid(DAY));
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

#[test]
fn h3_1_the_cutoff_applies_only_from_its_instant() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let cutoff_text = date_text(n - 200 * DAY);
    let cutoff_ms = (n - 200 * DAY).div_euclid(DAY) * DAY * 1_000;
    let (preset, deck) = preset_deck(&f, "Cut", None, &cutoff_text);
    let id = at(n, -320, 1);
    let rows = vec![
        row(at(n, -310, 1), id, 3, -600, 2500, LEARN),
        row(at(n, -310, 2) + 600_000, id, 3, 3, 2500, LEARN),
        row(at(n, -305, 3), id, 3, 8, 2500, REVIEW),
        row(at(n, -290, 4), id, 2, 15, 2500, REVIEW),
        row(at(n, -150, 5), id, 3, 30, 2300, REVIEW),
        row(at(n, -90, 6), id, 3, 70, 2300, REVIEW),
    ];
    put_card(&f, deck, id, rows.clone());
    let view = preset_view(&f, preset);
    let model = fsrs::FSRS::new(view.parameters()).unwrap();
    let retention = view.fields().historical_retention;
    let decay = f64::from(view.decay());
    let part = warm(&f, Scope::Deck(DeckId(deck)), History::AllHistory);
    let (mut before, mut after) = (0, 0);
    for sample in &part.samples {
        let instant = sample.instant_ms.div_euclid(1_000);
        let seen: Vec<RevlogEntry> = rows.iter().filter(|r| r.id.0.div_euclid(1_000) <= instant).cloned().collect();
        let with_cutoff = instant * 1_000 >= cutoff_ms;
        let items = Items::select(&seen, if with_cutoff { cutoff_ms } else { i64::MIN }, HistoryForm::Memory);
        match items {
            None => assert_eq!(sample.studied, 0, "day {}", sample.day),
            Some(items) => {
                let state = items.memory(&model, n, retention).unwrap();
                let last = items.kept().last().unwrap().id.0.div_euclid(1_000);
                let expected = curve((instant - last) as f64 / 86_400.0, f64::from(state.stability), decay);
                assert_eq!(sample.studied, 1, "day {}", sample.day);
                close(sample.knowledge, expected, &format!("day {}", sample.day));
                if with_cutoff {
                    after += 1;
                    // From the cutoff on, the history starts at the first
                    // interday row after it.
                    assert!(!items.complete, "day {}", sample.day);
                } else {
                    before += 1;
                    assert!(items.complete, "day {}", sample.day);
                }
            }
        }
    }
    assert!(before > 50 && after > 50, "{before} before and {after} after the cutoff");
    // No sample between the cutoff and the first row after it has an item.
    let gap = part.samples.iter().filter(|s| s.instant_ms >= cutoff_ms && s.instant_ms < at(n, -150, 5)).collect::<Vec<_>>();
    assert!(!gap.is_empty() && gap.iter().all(|s| s.studied == 0));
}

#[test]
fn h3_1_cards_without_an_item_add_nothing_and_crate_errors_go_to_failed() {
    let f = Fixture::new();
    let n = f.next_day_at();
    // Row ids follow the card's, so no two cards share one.
    let graded = |id: i64| {
        let k = id % 1_000 * 10;
        vec![
            row(at(n, -50, k + 1), id, 3, -600, 2500, LEARN),
            row(at(n, -49, k + 2), id, 3, 4, 2500, LEARN),
            row(at(n, -20, k + 3), id, 3, 12, 2500, REVIEW),
        ]
    };
    // A list the crate refuses, and a cutoff text the reader refuses.
    let (_, refused) = preset_deck(&f, "Refused", Some(vec![0.5; 5]), "");
    let (_, bad_date) = preset_deck(&f, "BadDate", None, "garbage");
    for (deck, k) in [(refused, 1), (bad_date, 2)] {
        put_card(&f, deck, at(n, -60, k), graded(at(n, -60, k)));
        // A suspended card is left out, not failed.
        let suspended = shaped(at(n, -60, k + 10), deck, CardType::Review, CardQueue::Suspended, 12);
        let rows = graded(suspended.id.0);
        f.write(move |scope| {
            scope.put(&suspended)?;
            for r in &rows {
                scope.insert_revlog(r)?;
            }
            Ok(())
        });
    }
    for deck in [refused, bad_date] {
        let part = warm(&f, Scope::Deck(DeckId(deck)), History::AllHistory);
        assert_eq!((part.failed, part.left_out), (1, 1), "deck {deck}");
        assert!(part.samples.is_empty(), "no estimate: {part:?}");
    }
    // Cards that give no item: new, only Set Due Date rows, imported with
    // a schedule and no log, a stored memory state without rows, a log
    // ending in a Reset.
    let quiet = add_deck(&f, "Quiet");
    let new = shaped(at(n, -30, 1), quiet, CardType::New, CardQueue::New, 0);
    let set_due = shaped(at(n, -30, 2), quiet, CardType::Review, CardQueue::Review, 10);
    let imported = shaped(at(n, -30, 3), quiet, CardType::Review, CardQueue::Review, 40);
    let mut stored_state = shaped(at(n, -30, 4), quiet, CardType::Review, CardQueue::Review, 40);
    stored_state.data.memory_state = Some(MemoryState {
        stability: 40.0,
        difficulty: 5.0,
    });
    let set_due_rows = vec![row(at(n, -25, 5), set_due.id.0, 0, 10, 2500, RevlogKind::Manual)];
    f.write(move |scope| {
        for c in [&new, &set_due, &imported, &stored_state] {
            scope.put(c)?;
        }
        for r in &set_due_rows {
            scope.insert_revlog(r)?;
        }
        Ok(())
    });
    let part = warm(&f, Scope::Deck(DeckId(quiet)), History::AllHistory);
    assert_eq!(part, KnowledgeOverTime::default(), "no estimate");
    // A Reset at the end: studied before it, not after.
    let reset_card = at(n, -30, 6);
    let mut rows = graded(reset_card);
    rows.push(row(at(n, -10, 7), reset_card, 0, 0, 0, RevlogKind::Manual));
    put_card(&f, quiet, reset_card, rows);
    let days = by_day(&warm(&f, Scope::Deck(DeckId(quiet)), History::AllHistory));
    assert_eq!(days[&-11].1, 1);
    assert_eq!(days[&-10].1, 0);
    assert_eq!(days[&0].1, 0);
    // A deck whose preset id names no preset replays with the built-in
    // defaults (`sched/presets` V1).
    let orphan = add_deck(&f, "Orphan");
    f.write(move |scope| {
        let mut deck = ReadRows::deck(&*scope, DeckId(orphan))?.unwrap();
        if let DeckKind::Normal(normal) = &mut deck.kind.kind {
            normal.config_id = 987_654;
        }
        scope.put(&deck)?;
        Ok(())
    });
    put_card(&f, orphan, at(n, -60, 8), graded(at(n, -60, 8)));
    let part = warm(&f, Scope::Deck(DeckId(orphan)), History::AllHistory);
    assert_eq!(part.failed, 0);
    assert!(by_day(&part)[&-1].1 == 1);
    // Bounds: each card adds at most one.
    let all = warm(&f, Scope::Collection, History::AllHistory);
    assert!(all.samples.iter().all(|s| s.knowledge >= 0.0 && s.knowledge <= s.studied as f64 + 1e-9));
    assert_eq!(all.failed, 2);
}

#[test]
fn h1_a_row_at_a_day_end_counts_on_that_day_and_months_follow_local_dates() {
    let mut f = Fixture::new();
    let n = f.next_day_at();
    let on_end = add_deck(&f, "OnEnd");
    let after_end = add_deck(&f, "AfterEnd");
    let end = n - 10 * DAY;
    for (deck, second, k) in [(on_end, end, 1), (after_end, end + 1, 2)] {
        let id = at(n, -400, k);
        put_card(
            &f,
            deck,
            id,
            vec![row(second * 1_000 + 999, id, 3, 5, 2500, LEARN), row(at(n, -2, k), id, 3, 9, 2500, REVIEW)],
        );
    }
    let on = by_day(&warm(&f, Scope::Deck(DeckId(on_end)), History::Last12Months));
    assert_eq!(on[&-11].1, 0);
    // At the row's own instant: exactly one card's recall.
    assert_eq!(on[&-10], (1.0, 1));
    let after = by_day(&warm(&f, Scope::Deck(DeckId(after_end)), History::Last12Months));
    assert_eq!(after[&-10].1, 0);
    assert_eq!(after[&-9].1, 1);
    // `last12Months`: −365 to −1, then today.
    let days: Vec<i32> = on.keys().copied().collect();
    assert_eq!(days, (-365..=0).collect::<Vec<_>>());
    // In a zone far from UTC with a rollover hour, the month samples are
    // the last day of each local month, back to the oldest day's month.
    f.write(|scope| {
        scope.set_config("rollover", &ConfigValue::Integer(4))?;
        Ok(())
    });
    f.clock.set_utc_offset(UtcOffset::from_seconds(13 * 3_600).unwrap());
    let n = f.next_day_at();
    let zone = 13 * 3_600;
    let today = today_day_number(n, 4, zone);
    let oldest = -700;
    let result = data(read_with(&f, &request(Scope::Collection, History::AllHistory, Some(oldest))));
    let part = result.knowledge;
    let months: Vec<i32> = part.samples.iter().map(|s| s.day).filter(|day| *day < -365).collect();
    assert!(!months.is_empty());
    for day in &months {
        let next = civil(today + i64::from(*day) + 1);
        assert_eq!(next.day, 1, "day {day} is not a month end");
        let sample = part.samples.iter().find(|s| s.day == *day).unwrap();
        assert_eq!(sample.instant_ms, (n + i64::from(*day) * DAY) * 1_000);
    }
    let first = civil(today + i64::from(months[0]));
    let oldest_date = civil(today + i64::from(oldest));
    assert_eq!((first.year, first.month), (oldest_date.year, oldest_date.month));
    // Every month between: one sample each.
    for pair in months.windows(2) {
        let (a, b) = (civil(today + i64::from(pair[0])), civil(today + i64::from(pair[1])));
        assert_eq!(i32::from(b.month) + 12 * b.year, i32::from(a.month) + 12 * a.year + 1);
    }
    // The zone change rebuilt the component on the new grid: it equals a
    // cold replay.
    compare(&f, &[Scope::Collection, Scope::Deck(DeckId(on_end))], "a zone change");
    f.reopen();
    compare(&f, &[Scope::Collection], "reopened");
}

#[test]
fn h1_deck_scopes_hold_their_subdecks_and_add_up() {
    let f = Fixture::new();
    let solo = random_collection(&f, 41, 300);
    for history in [History::Last12Months, History::AllHistory] {
        let all = by_day(&warm(&f, Scope::Collection, history));
        let lang = by_day(&warm(&f, Scope::Deck(DeckId(LANG)), history));
        let verbs = by_day(&warm(&f, Scope::Deck(DeckId(VERBS)), history));
        let alone = by_day(&warm(&f, Scope::Deck(DeckId(solo)), history));
        assert!(all.values().any(|(_, studied)| *studied > 0));
        for (day, (knowledge, studied)) in &all {
            let (a, b) = (lang.get(day).copied().unwrap_or_default(), alone.get(day).copied().unwrap_or_default());
            // Every card's home deck is under `Lang` or is `Solo` (G2.1).
            assert_eq!(*studied, a.1 + b.1, "day {day}");
            close(*knowledge, a.0 + b.0, &format!("day {day}"));
            let v = verbs.get(day).copied().unwrap_or_default();
            assert!(v.1 <= a.1 && v.0 <= a.0 + 1e-9, "day {day}");
        }
    }
}

/// Every warm part equals a cold replay of the same snapshot and context,
/// exactly: kept sums are whole 2^-40 cards.
fn compare(f: &Fixture, scopes: &[Scope], step: &str) {
    let collection = f.collection();
    let derived = f.host.derived();
    for scope in scopes {
        for history in [History::Last12Months, History::AllHistory] {
            let request = request(scope.clone(), history, Some(-900));
            let context = history::page_context(&collection, derived).unwrap();
            let warm = history::knowledge_in(f.host.stats(), &collection, derived, &context, &request, &mut |_| {}).unwrap();
            let cold = history::knowledge_cold(&collection, &context, &request).unwrap();
            match (warm, cold) {
                (KnowledgeResult::Data(w), KnowledgeResult::Data(c)) => {
                    assert!(w.from_component, "{step}: {scope:?} {history:?}");
                    assert_eq!(w.knowledge, c.knowledge, "{step}: {scope:?} {history:?}");
                    for sample in &w.knowledge.samples {
                        assert!(sample.knowledge <= sample.studied as f64 + 1e-9, "{step}: {sample:?}");
                    }
                }
                (w, c) => assert_eq!(w, c, "{step}: {scope:?} {history:?}"),
            }
        }
    }
}

/// The steps of [`Mix`] after which a knowledge read rebuilds, beside
/// U289's: a preset's FSRS values or a deck's preset changed, or more days
/// passed than the kept samples reach.
const KNOWLEDGE_REBUILDS: [&str; 3] = ["preset parameters", "preset cutoff", "deck preset"];

fn knowledge_mix(seed: u64, steps: usize) {
    let mut f = Fixture::new();
    let solo = random_collection(&f, seed, 150);
    let (alt, alt_deck) = preset_deck(&f, "Alt", Some(params_with_decay(0.3)), "");
    let n = f.next_day_at();
    let random = SeededRandom::new(seed ^ 0xa17);
    // A few cards on the other preset.
    let mut extra = Vec::new();
    for k in 0..20 {
        let id = at(n, -((random.next_u64() % 400) as i64), 900 + k);
        let mut rows = Vec::new();
        for j in 0..(random.next_u64() % 8) {
            let day = -((random.next_u64() % 400) as i64);
            let (ease, ivl, kind) = [(3, -600, LEARN), (3, 10, REVIEW), (1, -600, REVIEW), (0, 0, RevlogKind::Manual)]
                [(random.next_u64() % 4) as usize];
            rows.push(row(at(n, day, 2_000 * j as i64 + k), id, ease, ivl, 2500, kind));
        }
        extra.push(id);
        put_card(&f, alt_deck, id, rows);
    }
    let (cards, _) = stored(&f);
    let mut mix = Mix {
        random: SeededRandom::new(seed ^ 0x5eed),
        next_id: 0,
        cards: cards.iter().map(|c| c.0).collect(),
        removed: Vec::new(),
        bad_rows: Vec::new(),
        decks: vec![LANG, VERBS, solo, alt_deck],
        offset_hours: 0,
    };
    let mut all_scopes: Vec<Scope> = scopes(solo).into_iter().map(|(scope, _)| scope).collect();
    all_scopes.push(Scope::Deck(DeckId(alt_deck)));
    compare(&f, &all_scopes, "built");
    let mut events = f.host.engine().stream().subscribe();
    for at_step in 0..steps {
        let name = match mix.pick(8) {
            0 => {
                let decay = 0.1 + (mix.pick(50) as f32) / 100.0;
                edit_preset(&f, alt, move |preset| preset.set_fsrs_params_6(params_with_decay(decay)));
                "preset parameters"
            }
            1 => {
                let days = mix.pick(500) as i64;
                let text = date_text(f.next_day_at() - days * DAY);
                edit_preset(&f, alt, move |preset| preset.ignore_revlogs_before_date = text);
                "preset cutoff"
            }
            2 => {
                let retention = 0.8 + (mix.pick(15) as f32) / 100.0;
                edit_preset(&f, alt, move |preset| preset.desired_retention = retention);
                "desired retention"
            }
            3 => {
                let to = if mix.pick(2) == 0 { alt } else { 1 };
                f.write(move |scope| {
                    let mut deck = ReadRows::deck(&*scope, DeckId(VERBS))?.unwrap();
                    if let DeckKind::Normal(normal) = &mut deck.kind.kind {
                        normal.config_id = to;
                    }
                    scope.put(&deck)?;
                    Ok(())
                });
                "deck preset"
            }
            4 => {
                // An answer on a card of the other preset.
                let id = extra[mix.pick(extra.len())];
                let row_id = f.clock.now_ms() - 2_000 - mix.pick(1_000) as i64;
                f.write(move |scope| {
                    if ReadRows::card(&*scope, CardId(id))?.is_none() {
                        return Ok(());
                    }
                    let mut c = current(scope, id);
                    c.interval = 20;
                    scope.put(&c)?;
                    scope.insert_revlog(&row(row_id, id, 3, 20, 2500, REVIEW))?;
                    Ok(())
                });
                "answer on the other preset"
            }
            _ => mix.step(&mut f),
        };
        let step = format!("step {at_step} ({name})");
        compare(&f, &all_scopes, &step);
        if REBUILDS.contains(&name) || KNOWLEDGE_REBUILDS.contains(&name) {
            events = f.host.engine().stream().subscribe();
        } else {
            assert_eq!(stale_events(&events), 0, "{step} rebuilt the component");
        }
    }
    // Many rollovers at once: past the kept days, the next read rebuilds,
    // and still equals a cold replay.
    f.clock.step_ms(120 * DAY * 1_000);
    compare(&f, &all_scopes, "120 days later");
}

#[test]
fn h14_4_kept_knowledge_equals_a_cold_replay_after_random_changes() {
    for seed in [5, 6, 7] {
        knowledge_mix(seed, 60);
    }
}

#[test]
fn h14_3_a_preset_change_recomputes_only_that_presets_cards() {
    let f = Fixture::new();
    let solo = random_collection(&f, 13, 200);
    let (alt, alt_deck) = preset_deck(&f, "Alt", Some(params_with_decay(0.3)), "");
    let n = f.next_day_at();
    for k in 0..30 {
        let id = at(n, -300, 700 + k);
        put_card(
            &f,
            alt_deck,
            id,
            vec![
                row(at(n, -290 + k, 1), id, 3, -600, 2500, LEARN),
                row(at(n, -280 + k, 2), id, 3, 5, 2500, REVIEW),
                row(at(n, -200 + k, 3), id, 3, 20, 2500, REVIEW),
            ],
        );
    }
    let others = [Scope::Collection, Scope::Deck(DeckId(LANG)), Scope::Deck(DeckId(solo)), Scope::Deck(DeckId(CRAM))];
    let alt_scope = Scope::Deck(DeckId(alt_deck));
    let read_all = |f: &Fixture| -> Vec<KnowledgeOverTime> {
        others
            .iter()
            .chain([&alt_scope])
            .map(|scope| warm(f, scope.clone(), History::AllHistory))
            .collect()
    };
    let before = read_all(&f);
    // A desired-retention change touches neither series (H14.3).
    let mut events = f.host.engine().stream().subscribe();
    edit_preset(&f, alt, |preset| preset.desired_retention = 0.85);
    f.write(move |scope| {
        let mut deck = ReadRows::deck(&*scope, DeckId(alt_deck))?.unwrap();
        if let DeckKind::Normal(normal) = &mut deck.kind.kind {
            normal.desired_retention = Some(0.8);
        }
        scope.put(&deck)?;
        Ok(())
    });
    assert_eq!(read_all(&f), before);
    assert_eq!(stale_events(&events), 0, "a desired retention change rebuilt the component");
    // A parameter change moves that preset's cards only; the other scopes
    // hold no card of it and stay as they were, bit for bit.
    edit_preset(&f, alt, |preset| preset.set_fsrs_params_6(params_with_decay(0.5)));
    let after = read_all(&f);
    let (alt_before, alt_after) = (before.last().unwrap(), after.last().unwrap());
    assert_ne!(alt_before, alt_after);
    // The Collection holds the Alt cards: its sum moves by theirs.
    for (at, scope) in others.iter().enumerate().skip(1) {
        assert_eq!(after[at], before[at], "{scope:?}");
    }
    compare(&f, &[Scope::Collection, alt_scope.clone()], "parameters changed");
    events = f.host.engine().stream().subscribe();
    // H2 is unchanged by it.
    let states = |f: &Fixture| {
        let collection = f.collection();
        let request = history::HistoryRequest {
            scope: Scope::Collection,
            history: History::AllHistory,
            cancel: CancelToken::new(),
        };
        match history::card_states(f.host.stats(), &collection, f.host.derived(), &request, &mut |_| {}).unwrap() {
            history::HistoryResult::Data(data) => data.card_states,
            other => panic!("{other:?}"),
        }
    };
    let h2 = states(&f);
    edit_preset(&f, alt, |preset| preset.set_fsrs_params_6(params_with_decay(0.2)));
    assert_eq!(states(&f), h2);
    let _ = stale_events(&events);
}

#[test]
fn h17_knowledge_builds_and_reads_write_nothing_to_the_collection() {
    let f = Fixture::new();
    let solo = random_collection(&f, 31, 120);
    f.next_day_at();
    let before = dump(&f.host);
    for (scope, _) in scopes(solo) {
        warm(&f, scope.clone(), History::AllHistory);
        let collection = f.collection();
        let context = history::page_context(&collection, f.host.derived()).unwrap();
        history::knowledge_cold(&collection, &context, &request(scope, History::Last12Months, None)).unwrap();
    }
    f.host.derived().invalidate(Invalidation::CheckDatabase);
    warm(&f, Scope::Collection, History::Last12Months);
    assert_eq!(dump(&f.host), before);
}

#[test]
fn h18_errors_of_a_knowledge_read() {
    let f = Fixture::new();
    assert_eq!(
        read_with(&f, &request(Scope::Deck(DeckId(424_242)), History::AllHistory, None)),
        KnowledgeResult::UnknownDeck
    );
    assert_eq!(
        read_with(&f, &request(Scope::Search("deck:Lang".into()), History::AllHistory, None)),
        KnowledgeResult::UnsupportedScope
    );
}

#[test]
fn h15_a_knowledge_read_reports_fractions_that_never_go_back() {
    let big = big(30_000, 240_000);
    let collection = big.host.collection().unwrap();
    let mut seen = Vec::new();
    let result = history::knowledge_over_time(
        big.host.stats(),
        &collection,
        big.host.derived(),
        &request(Scope::Collection, History::AllHistory, None),
        &mut |progress| seen.push(progress),
    )
    .unwrap();
    let data = data(result);
    assert!(data.from_component);
    assert!(data.knowledge.samples.iter().any(|s| s.studied > 0));
    let mut last = 0.0;
    for progress in &seen {
        if let Progress::Fraction(fraction) = progress {
            assert!((0.0..=1.0).contains(fraction) && *fraction >= last, "{seen:?}");
            last = *fraction;
        }
    }
    assert!(!seen.is_empty(), "a build this size reports progress");
}

#[test]
fn h16_a_cancelled_knowledge_read_returns_within_50_ms_and_the_build_goes_on() {
    let big = big(30_000, 240_000);
    let host = Arc::new(big.host);
    let cancel = CancelToken::new();
    let waiting = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (release, go) = std::sync::mpsc::channel::<()>();
    let reader = {
        let (host, cancel, waiting) = (Arc::clone(&host), Arc::clone(&cancel), Arc::clone(&waiting));
        std::thread::spawn(move || {
            let collection = host.collection().unwrap();
            let request = KnowledgeRequest {
                scope: Scope::Collection,
                history: History::AllHistory,
                oldest_day: None,
                cancel,
            };
            let mut held = Some(go);
            let result = history::knowledge_over_time(host.stats(), &collection, host.derived(), &request, &mut |_| {
                waiting.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(go) = held.take() {
                    go.recv().unwrap();
                }
            })
            .unwrap();
            (result, meguri_engine::monotonic::now())
        })
    };
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while !waiting.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(meguri_engine::monotonic::now() < deadline, "the read never waited for the build");
        std::thread::sleep(Duration::from_millis(1));
    }
    let cancelled_at = meguri_engine::monotonic::now();
    cancel.cancel();
    release.send(()).unwrap();
    let (result, returned_at) = reader.join().unwrap();
    assert_eq!(result, KnowledgeResult::Cancelled);
    let took = returned_at.saturating_sub(cancelled_at);
    assert!(took <= Duration::from_millis(50), "returned {took:?} after the cancel");
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness != Readiness::Ready {
        assert!(!host.derived().has_failed(HISTORY_COMPONENT));
        assert!(meguri_engine::monotonic::now() < deadline, "the build stopped");
        std::thread::sleep(Duration::from_millis(5));
    }
    let collection = host.collection().unwrap();
    let again = history::knowledge_over_time(
        host.stats(),
        &collection,
        host.derived(),
        &request(Scope::Collection, History::AllHistory, None),
        &mut |_| {},
    )
    .unwrap();
    assert!(data(again).from_component);
}

#[test]
fn h3_today_point_is_computed_at_each_requests_now() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let deck = add_deck(&f, "Now");
    let id = at(n, -40, 1);
    put_card(
        &f,
        deck,
        id,
        vec![row(at(n, -30, 1), id, 3, -600, 2500, LEARN), row(at(n, -29, 2), id, 3, 10, 2500, REVIEW)],
    );
    let first = warm(&f, Scope::Deck(DeckId(deck)), History::Last12Months);
    // An hour later, the same day: the kept samples are the same, today's
    // point is lower.
    f.clock.step_ms(3_600_000);
    let later = warm(&f, Scope::Deck(DeckId(deck)), History::Last12Months);
    let (a, b) = (first.samples.last().unwrap(), later.samples.last().unwrap());
    assert_eq!(b.instant_ms - a.instant_ms, 3_600_000);
    assert!(b.knowledge < a.knowledge);
    assert_eq!(first.samples[..first.samples.len() - 1], later.samples[..later.samples.len() - 1]);
    let (kept, _) = kept_rows(&f, id);
    let (t, state) = kept.last().unwrap();
    let decay = f64::from(preset_view(&f, 1).decay());
    let expected = curve((b.instant_ms.div_euclid(1_000) - t) as f64 / 86_400.0, f64::from(state.stability), decay);
    close(b.knowledge, expected, "today's point");
    // The removed answer of an undo takes today's point back.
    let undone = RevlogId(at(n, -29, 2));
    f.write(move |scope| {
        scope.remove_revlog(undone)?;
        Ok(())
    });
    let undo = warm(&f, Scope::Deck(DeckId(deck)), History::Last12Months);
    assert!(undo.samples.last().unwrap().knowledge < b.knowledge);
    compare(&f, &[Scope::Deck(DeckId(deck))], "an undo");
}

#[test]
fn h3_1_a_refused_preset_fails_only_cards_with_an_item_and_a_nan_list_is_kept() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let mut nan = fsrs::DEFAULT_PARAMETERS.to_vec();
    nan[20] = f32::NAN;
    let (_, refused) = preset_deck(&f, "NaN", Some(nan), "");
    // A graded row that gives no item at any cut (no learning run, no
    // interday row): not failed.
    let quiet = at(n, -40, 1);
    put_card(&f, refused, quiet, vec![row(at(n, -30, 1), quiet, 3, 0, 2500, REVIEW)]);
    let part = warm(&f, Scope::Deck(DeckId(refused)), History::AllHistory);
    assert_eq!(part, KnowledgeOverTime::default());
    // One that gives an item fails from its first row on.
    let failing = at(n, -40, 2);
    put_card(
        &f,
        refused,
        failing,
        vec![row(at(n, -30, 2), failing, 3, -600, 2500, LEARN), row(at(n, -29, 3), failing, 3, 3, 2500, LEARN)],
    );
    let part = warm(&f, Scope::Deck(DeckId(refused)), History::AllHistory);
    assert_eq!((part.failed, part.samples.len()), (1, 0));
    compare(&f, &[Scope::Collection, Scope::Deck(DeckId(refused))], "a refused list");
}

#[test]
fn h3_1_todays_point_counts_a_later_dated_row_from_the_requests_day() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let deck = add_deck(&f, "Ahead");
    let id = at(n, -20, 1);
    // A row stamped two days after `N` by another device's clock.
    let rows = vec![
        row(at(n, -10, 1), id, 3, -600, 2500, LEARN),
        row(at(n, -9, 2), id, 3, 2, 2500, LEARN),
        row((n + 2 * DAY) * 1_000, id, 1, -600, 2500, REVIEW),
    ];
    put_card(&f, deck, id, rows.clone());
    let part = warm(&f, Scope::Deck(DeckId(deck)), History::Last12Months);
    let view = preset_view(&f, 1);
    let model = fsrs::FSRS::new(view.parameters()).unwrap();
    let items = Items::select(&rows, 0, HistoryForm::Memory).unwrap();
    let state = items.memory(&model, n, view.fields().historical_retention).unwrap();
    let now = f.clock.now_ms().div_euclid(1_000);
    let raw = curve((now - (n + 2 * DAY)) as f64 / 86_400.0, f64::from(state.stability), f64::from(view.decay()));
    let expected = if raw.is_nan() { 0.0 } else { raw.clamp(0.0, 1.0) };
    let today = part.samples.last().unwrap();
    assert_eq!(today.studied, 1);
    close(today.knowledge, expected, "today's point");
    compare(&f, &[Scope::Deck(DeckId(deck))], "a later dated row");
}
