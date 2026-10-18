use meguri_model::card::{CardQueue, CardType};
use meguri_sched::model::card_sched::queue_for_type;

fn card() -> meguri_model::card::Card {
    use meguri_model::{
        card::{Card, CardQueue, CardType},
        ids::{CardId, DeckId, NoteId},
    };
    Card {
        id: CardId(1),
        note_id: NoteId(1),
        deck_id: DeckId(1),
        ord: 0,
        mtime: Default::default(),
        usn: Default::default(),
        card_type: CardType::New,
        queue: CardQueue::New,
        due: 0,
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: Default::default(),
    }
}

#[test]
fn queue_interpretation_keeps_unusual_pairs_and_filtered_restoration_order() {
    use meguri_model::{card::Card, ids::DeckId};
    use meguri_sched::model::card_sched::*;
    let mut card = Card {
        card_type: CardType::Review,
        queue: CardQueue::New,
        due: -1,
        remaining_steps: 2002,
        ..card()
    };
    assert_eq!(CardSched::new(&card).due(), Due::Position(u32::MAX));
    assert_eq!(CardSched::new(&card).remaining_steps(), 2);
    assert!(!restore_negative_queue(&mut card));
    card.card_type = CardType::Learning;
    card.queue = CardQueue::Review;
    card.deck_id = DeckId(7);
    card.original_deck_id = DeckId(2);
    card.original_due = 1_700_000_000;
    assert_eq!(CardSched::new(&card).home_deck(), DeckId(2));
    let stamps = (card.mtime, card.usn);
    assert!(return_from_filtered(&mut card));
    assert_eq!(
        (
            card.deck_id,
            card.original_deck_id,
            card.original_due,
            card.due,
            card.queue
        ),
        (DeckId(2), DeckId(0), 0, 1_700_000_000, CardQueue::Learning)
    );
    assert_eq!((card.mtime, card.usn), stamps);
    card.original_deck_id = DeckId(2);
    card.original_due = 450;
    card.queue = CardQueue::Suspended;
    assert!(return_from_filtered(&mut card));
    assert_eq!(card.queue, CardQueue::Suspended);
    assert!(restore_negative_queue(&mut card));
    assert_eq!(card.queue, CardQueue::InterdayLearning);
}

#[test]
fn saved_positions_follow_each_operations_distinct_negative_and_filtered_rules() {
    use meguri_model::{card::Card, ids::DeckId};
    use meguri_sched::model::position::*;
    let mut card = Card { due: -1, ..card() };
    save_answer_position(&mut card, CardType::Review, false, false);
    assert_eq!(card.data.new_position, Some(0));
    save_set_due_position(&mut card);
    assert_eq!(card.data.new_position, Some(u32::MAX));
    card.original_deck_id = DeckId(9);
    card.original_due = 0;
    card.due = 55;
    save_answer_position(&mut card, CardType::Learning, false, true);
    assert_eq!(card.data.new_position, Some(55));
    save_set_due_position(&mut card);
    assert_eq!(card.data.new_position, Some(0));
    card.data.new_position = None;
    save_answer_position(&mut card, CardType::Learning, true, true);
    assert_eq!(card.data.new_position, None);
    card.original_due = 99;
    save_answer_position(&mut card, CardType::Learning, false, true);
    assert_eq!(card.data.new_position, Some(99));
    card.card_type = CardType::Review;
    card.data = meguri_model::card_data::CardData::parse(
        r#"{"pos":99,"s":1,"d":2,"dr":0.9,"decay":0.5,"lrt":23,"cd":"{\"x\":true}","xyz":[1, 2]}"#,
    );
    assert_eq!(forget_memory_and_position(&mut card), Some(99));
    assert_eq!(
        card.data.to_text(),
        r#"{"dr":0.9,"decay":0.5,"lrt":23,"cd":"{\"x\":true}","xyz":[1, 2]}"#
    );
}

#[test]
fn queue_restoration_uses_the_strict_timestamp_boundary_for_both_learning_types() {
    for ty in [CardType::Learning, CardType::Relearning] {
        assert_eq!(queue_for_type(ty, 1_000_000_000), CardQueue::InterdayLearning);
        assert_eq!(queue_for_type(ty, 1_000_000_001), CardQueue::Learning);
        assert_eq!(queue_for_type(ty, -1), CardQueue::InterdayLearning);
    }
    assert_eq!(queue_for_type(CardType::New, 1_700_000_000), CardQueue::New);
    assert_eq!(queue_for_type(CardType::Review, 1_700_000_000), CardQueue::Review);
}
