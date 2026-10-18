use meguri_sched::memory::ignore_before::cutoff_millis;
mod common;
use meguri_model::{
    card::CardType,
    ids::{CardId, RevlogId},
    revlog::{RevlogEntry, RevlogKind},
};
use meguri_sched::memory::{
    items::{History, HistoryForm, PresetTraining, TrainingItem},
    resolve::{last_review_time, repair_last_review_time},
};

fn row(id: i64, kind: RevlogKind, button: u8, interval: i32, factor: u32) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(id),
        card_id: CardId(1),
        usn: Default::default(),
        button,
        interval,
        last_interval: 0,
        ease_factor: factor,
        answer_millis: 0,
        kind,
    }
}

fn history_h() -> Vec<RevlogEntry> {
    use RevlogKind::*;
    [
        ("2025-10-01", 9 * 3600, Learning, 3),
        ("2025-10-01", 9 * 3600 + 600, Learning, 3),
        ("2025-10-02", 10 * 3600, Review, 3),
        ("2025-10-05", 10 * 3600, Review, 3),
        ("2025-10-14", 10 * 3600, Review, 1),
        ("2025-10-14", 10 * 3600 + 600, Relearning, 3),
        ("2025-10-16", 10 * 3600, Review, 3),
        ("2025-10-22", 10 * 3600, Review, 4),
        ("2025-11-11", 10 * 3600, Review, 2),
    ]
    .into_iter()
    .map(|(date, time, kind, rating)| row(cutoff_millis(date).unwrap() + time * 1000, kind, rating, 3, 2500))
    .collect()
}

#[test]
fn complete_history_replays_the_pinned_h_reviews_and_training_prefixes() {
    let rows = history_h();
    let next_day = cutoff_millis("2026-03-03").unwrap() / 1000 + 4 * 3600;
    let history = History::select(&rows, 0, HistoryForm::Memory).unwrap();
    assert!(history.complete);
    assert_eq!(
        history
            .reviews(next_day)
            .iter()
            .map(|r| (r.rating, r.delta_t))
            .collect::<Vec<_>>(),
        [(3, 0), (3, 0), (3, 1), (3, 3), (1, 9), (3, 0), (3, 2), (4, 6), (2, 20)]
    );
    let memory = history.memory(&fsrs::FSRS::new(&[]).unwrap(), next_day, 0.9).unwrap();
    assert!((memory.stability - 39.451553).abs() < 1e-5);
    assert!((memory.difficulty - 7.6402645).abs() < 1e-5);
    let mut training = PresetTraining::default();
    training.add_card(&rows, 0, next_day);
    training.sort();
    assert_eq!(
        training.items.iter().map(|i| i.item.reviews.len()).collect::<Vec<_>>(),
        [3, 4, 5, 7, 8, 9]
    );
    assert_eq!(training.review_count, 9);
    assert_eq!(
        training.items.iter().map(|i| i.review_id).collect::<Vec<_>>(),
        [2, 3, 4, 6, 7, 8].map(|i| rows[i].id)
    );
    assert_eq!(
        training.owned_bytes(),
        training.items.capacity() * std::mem::size_of::<TrainingItem>()
            + training
                .items
                .iter()
                .map(|i| i.item.reviews.capacity() * std::mem::size_of::<fsrs::FSRSReview>())
                .sum::<usize>()
    );
    let mut excluded = PresetTraining::default();
    excluded.add_card(&rows, cutoff_millis("2025-10-02").unwrap(), next_day);
    assert!(excluded.items.is_empty());
    assert_eq!(excluded.review_count, 0);
}

#[test]
fn reset_cram_manual_and_cutoff_rules_select_the_last_learning_run() {
    use RevlogKind::*;
    let rows = vec![
        row(1000, Learning, 3, 1, 2500),
        row(2000, Filtered, 3, 1, 0),
        row(3000, Learning, 3, 1, 2500),
        row(4000, Review, 3, 3, 2500),
        row(5000, Manual, 0, 0, 0),
        row(6000, Learning, 3, 1, 2500),
        row(7000, Manual, 0, 1, 2500),
        row(8000, Learning, 3, 1, 2500),
        row(9000, Review, 3, 3, 2500),
    ];
    let ids = |h: History<'_>| h.kept().map(|r| r.id.0).collect::<Vec<_>>();
    assert_eq!(
        ids(History::select(&rows, 0, HistoryForm::Memory).unwrap()),
        [8000, 9000]
    );
    let exact = History::select(&rows, 8000, HistoryForm::Memory).unwrap();
    assert!(exact.complete);
    let partial = History::select(&rows, 8001, HistoryForm::Memory).unwrap();
    assert!(!partial.complete);
    assert_eq!(ids(partial), [9000]);
    assert!(History::select(&rows, 9000, HistoryForm::Memory).is_none());
    assert!(History::select(&rows, 8001, HistoryForm::Training).is_none());
    let cram = vec![
        row(1000, Learning, 3, 1, 2500),
        row(2000, Filtered, 3, 1, 0),
        row(3000, Learning, 3, 1, 2500),
    ];
    assert_eq!(
        ids(History::select(&cram, 0, HistoryForm::Memory).unwrap()),
        [1000, 3000]
    );
    assert!(History::select(&cram[..1], 5000, HistoryForm::Memory).unwrap().complete);
    assert!(History::select(&cram[..2], 5000, HistoryForm::Memory).is_none());
}

#[test]
fn incomplete_single_review_uses_the_history_factor_rules() {
    let model = fsrs::FSRS::new(&[]).unwrap();
    for (factor, difficulty) in [(2500, 5.003576), (820, 7.4799995), (0, 5.003576)] {
        let rows = [row(1000, RevlogKind::Review, 3, 100, factor)];
        let history = History::select(&rows, 0, HistoryForm::Memory).unwrap();
        assert!(!history.complete);
        let memory = history.memory(&model, 86400, 0.9).unwrap();
        assert_eq!(memory.stability, 100.0);
        assert!((memory.difficulty - difficulty).abs() < 1e-5);
    }
}

#[test]
fn fallback_and_repair_have_distinct_reset_and_button_rules() {
    use RevlogKind::*;
    let mut card = common::card();
    card.card_type = CardType::Review;
    card.data.last_review_secs = Some(100);
    let mut rows = vec![row(-1999, Review, 3, 1, 2500), row(1000, Manual, 0, 0, 0)];
    assert_eq!(last_review_time(&rows), Some(-1));
    assert!(repair_last_review_time(&mut card, &rows));
    assert_eq!(card.data.last_review_secs, None);
    rows.push(row(2999, Manual, 3, 0, 0));
    assert!(repair_last_review_time(&mut card, &rows));
    assert_eq!(card.data.last_review_secs, Some(2));
    rows.push(row(3999, Rescheduled, 5, 1, 2500));
    assert_eq!(last_review_time(&rows), Some(2));
    assert!(repair_last_review_time(&mut card, &rows));
    assert_eq!(card.data.last_review_secs, Some(3));
    assert!(!repair_last_review_time(&mut card, &rows));
    card.card_type = CardType::New;
    card.data.last_review_secs = Some(100);
    assert!(!repair_last_review_time(&mut card, &rows));
    assert_eq!(card.data.last_review_secs, Some(100));
    card.card_type = CardType::Learning;
    assert!(!repair_last_review_time(&mut card, &[]));
}

#[test]
fn on_the_fly_preserves_stored_memory_and_refuses_only_used_cutoffs() {
    use meguri_model::{
        deck_config::DeckConfig,
        ids::DeckConfigId,
        proto::{ProtoBlob, deck_config::DeckConfigMessage},
    };
    use meguri_sched::{
        memory::resolve::{MemoryError, on_the_fly},
        model::preset::PresetView,
        states::fsrs_glue::FsrsCache,
    };
    let preset = |date: &str| {
        let mut fields = DeckConfigMessage::builtin_defaults();
        fields.ignore_revlogs_before_date = date.into();
        PresetView::new(&DeckConfig {
            id: DeckConfigId(42),
            name: "test".into(),
            mtime: Default::default(),
            usn: Default::default(),
            config: ProtoBlob::new(fields),
        })
    };
    let bad = preset("bad");
    let mut cache = FsrsCache::default();
    let mut card = common::card();
    assert_eq!(on_the_fly(&mut cache, &bad, &card, &[], 86400).unwrap(), None);
    card.card_type = CardType::Review;
    card.data.memory_state = Some(meguri_model::card_data::MemoryState {
        stability: 30.,
        difficulty: 6.,
    });
    assert_eq!(
        on_the_fly(&mut cache, &bad, &card, &[], 86400).unwrap(),
        card.data.memory_state
    );
    card.data.memory_state = None;
    assert!(matches!(
        on_the_fly(&mut cache, &bad, &card, &[], 86400),
        Err(MemoryError::InvalidDate {
            preset: DeckConfigId(42)
        })
    ));
    let valid = preset("");
    assert_eq!(on_the_fly(&mut cache, &valid, &card, &[], 86400).unwrap(), None);
    card.interval = 20;
    for factor in [0, 820] {
        card.ease_factor = factor;
        let memory = on_the_fly(&mut cache, &valid, &card, &[], 86400).unwrap().unwrap();
        assert_eq!(memory.stability, 20.);
        assert_eq!(memory.difficulty, 10.);
    }
}

#[test]
fn cutoff_accepts_the_pinned_date_language_and_uses_utc_midnight() {
    for (text, expected) in [
        ("", 0),
        ("1970-01-01", 0),
        ("1969-12-31", -86_400_000),
        ("2026-01-01", 1_767_225_600_000),
        ("2026-1-1", 1_767_225_600_000),
        ("\u{2003}2026-\t1-\n1", 1_767_225_600_000),
        ("+000002026-01-01", 1_767_225_600_000),
    ] {
        assert_eq!(cutoff_millis(text).unwrap(), expected, "{text:?}");
    }
    assert!(cutoff_millis("0000-02-29").is_ok());
    assert!(cutoff_millis("-262143-01-01").is_ok());
    assert!(cutoff_millis("+262142-12-31").is_ok());
    for year in [-262000, -10000, -400, -1, 0, 1600, 2000, 260000] {
        let a = cutoff_millis(&format!("{year:+}-03-01")).unwrap();
        let b = cutoff_millis(&format!("{:+}-03-01", year + 400)).unwrap();
        assert_eq!(b - a, 146_097 * 86_400_000);
    }
}

#[test]
fn cutoff_rejects_invalid_dates_signs_widths_and_trailing_text() {
    for text in [
        " ",
        "2026-01-03 ",
        "02026-01-03",
        "2026-02-30",
        "1900-02-29",
        "2026-13-01",
        "2026-00-01",
        "2026-01-00",
        "2026/01/03",
        "2026-01-03T00:00",
        "2026-001-03",
        "2026-1-003",
        "2026-+1-3",
        "2026-1--3",
        "2026 -01-03",
        "２０２６-01-03",
        "2026-٠١-03",
        "+262143-01-01",
        "-262144-01-01",
        "+999999999999999999999999999-01-01",
        "--2026-01-01",
        "garbage",
    ] {
        assert!(cutoff_millis(text).is_err(), "{text:?}");
    }
}
