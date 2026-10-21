//! U44 values are checked independently of the persistence operation.
use meguri_model::{
    card::{CardQueue, CardType},
    card_data::MemoryState,
    deck_config::DeckConfig,
    ids::*,
    proto::{ProtoBlob, deck_config::DeckConfigMessage},
    time::EpochSecs,
    usn::Usn,
};
use meguri_sched::{
    answer::{plan, siblings},
    model::preset::PresetView,
    states::{
        engine::{AnswerState, ReviewState, RevlogPreview, State},
        interval_limits::Delay,
        labels::format_seconds,
    },
};

fn card() -> meguri_model::card::Card {
    use meguri_storage::rows::{Cell, Record};

    // Decode a complete row in model/row-records R4 column order.
    let mut cells: Vec<Cell> = [
        // id, nid, did, ord, mod, usn
        11,
        7,
        1,
        0,
        4,
        2,
        // type, queue, due
        CardType::New.stored(),
        CardQueue::New.stored(),
        3,
        // ivl, factor, reps, lapses, left, odue, odid, flags
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        5,
    ]
    .into_iter()
    .map(Cell::Integer)
    .collect();
    cells.push(Cell::text(r#"{"cd":"{ \"client\": 1 }","unknown": [ 1, 2 ]}"#));
    meguri_model::card::Card::from_cells(&cells).expect("valid answer fixture row")
}
fn preset() -> PresetView {
    PresetView::new(&DeckConfig {
        id: DeckConfigId(1),
        name: "p".into(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        config: ProtoBlob::new(DeckConfigMessage::builtin_defaults()),
    })
}
fn chosen() -> AnswerState {
    AnswerState {
        state: State::Review(ReviewState {
            scheduled_days: 5,
            elapsed_days: 0,
            ease_factor: 2.35,
            lapses: 0,
            leeched: false,
            memory_state: Some(MemoryState {
                stability: 2.3065,
                difficulty: 2.118104,
            }),
        }),
        delay: Delay::Days(5),
        left: 0,
        factor: 2350,
        revlog: RevlogPreview {
            interval: 5,
            last_interval: 0,
            kind: 0,
            factor: 224,
        },
        label: format_seconds(432000, 1200),
    }
}
#[test]
fn applies_selected_values_and_keeps_custom_and_unknown_data() {
    let before = card();
    let p = plan(
        &before,
        State::New { position: 3 },
        &chosen(),
        &preset(),
        0.88,
        3,
        100_000,
        95_000,
    )
    .unwrap();
    assert_eq!(
        (p.card.due, p.card.interval, p.card.ease_factor, p.card.reps),
        (8, 5, 2350, 1)
    );
    assert_eq!((p.card.mtime.0, p.card.usn.0, p.elapsed_ms), (100, -1, 60000));
    assert_eq!(p.card.flags, 5);
    assert_eq!(
        p.card.data.to_text(),
        r#"{"pos":3,"s":2.3065,"d":2.118,"dr":0.88,"decay":0.154,"lrt":100,"cd":"{ \"client\": 1 }","unknown":[ 1, 2 ]}"#
    );
}
#[test]
fn sibling_permissions_follow_the_stored_answer_queue() {
    let mut fields = DeckConfigMessage::builtin_defaults();
    fields.bury_new = true;
    fields.bury_reviews = true;
    fields.bury_interday_learning = true;
    for (q, new, review, interday) in [
        (CardQueue::New, true, false, false),
        (CardQueue::Review, true, true, false),
        (CardQueue::Learning, true, true, true),
        (CardQueue::InterdayLearning, true, true, true),
        (CardQueue::Suspended, true, false, false),
    ] {
        assert_eq!(siblings::allowed(q, CardQueue::New, &fields), new);
        assert_eq!(siblings::allowed(q, CardQueue::Review, &fields), review);
        assert_eq!(siblings::allowed(q, CardQueue::InterdayLearning, &fields), interday);
        assert!(!siblings::allowed(q, CardQueue::Learning, &fields));
        assert!(!siblings::allowed(q, CardQueue::Suspended, &fields));
    }
}
#[test]
fn refuses_a_non_answer_state() {
    let mut value = chosen();
    value.state = State::New { position: 3 };
    assert!(
        plan(
            &card(),
            State::New { position: 3 },
            &value,
            &preset(),
            0.9,
            3,
            100_000,
            0
        )
        .is_err()
    );
}
