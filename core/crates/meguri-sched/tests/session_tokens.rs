use meguri_model::{
    card::{Card, CardQueue, CardType},
    card_data::MemoryState,
    ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId},
    time::EpochSecs,
    usn::Usn,
};
use meguri_sched::session::token::{StateInputs, render_key, state_token};

fn card() -> Card {
    Card {
        id: CardId(1),
        note_id: NoteId(2),
        deck_id: DeckId(1),
        ord: 0,
        mtime: EpochSecs(10),
        usn: Usn::PENDING,
        card_type: CardType::Review,
        queue: CardQueue::Review,
        due: 1,
        interval: 5,
        ease_factor: 2500,
        reps: 1,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: Default::default(),
    }
}
fn inputs(card: &Card) -> StateInputs<'_> {
    StateInputs {
        card,
        raw_data: b"{}",
        preset_id: DeckConfigId(1),
        preset_mtime: Some(EpochSecs(3)),
        home_retention: None,
        short_term_with_steps: false,
        learn_ahead: 120,
        today: 1,
        next_day_at: 1000,
        history_memory: None,
        history_last_review: None,
    }
}

#[test]
fn token_covers_boundary_and_history_without_card_row_edits() {
    let card = card();
    let mut input = inputs(&card);
    let original = state_token(&input);
    input.next_day_at += 3600;
    assert_ne!(state_token(&input), original, "same day, changed boundary");
    input.next_day_at = 1000;
    input.history_memory = Some(MemoryState {
        stability: 3.0,
        difficulty: 4.0,
    });
    assert_ne!(state_token(&input), original);
    input.history_memory = None;
    input.history_last_review = Some(500);
    assert_ne!(state_token(&input), original);
}

#[test]
fn flag_and_usn_do_not_change_state_but_flag_changes_render_key() {
    let card = card();
    let mut flagged = card.clone();
    flagged.flags = 7;
    flagged.usn = Usn(6);
    assert_eq!(state_token(&inputs(&card)), state_token(&inputs(&flagged)));
    assert_ne!(
        render_key(&card, EpochSecs(3), NotetypeId(4), EpochSecs(5)),
        render_key(&flagged, EpochSecs(3), NotetypeId(4), EpochSecs(5))
    );
    flagged.flags = 0;
    assert_eq!(
        render_key(&card, EpochSecs(3), NotetypeId(4), EpochSecs(5)),
        render_key(&flagged, EpochSecs(3), NotetypeId(4), EpochSecs(5))
    );
}

#[test]
fn exact_data_and_missing_preset_markers_are_distinct() {
    let card = card();
    let mut input = inputs(&card);
    let original = state_token(&input);
    input.raw_data = b"{ }";
    assert_ne!(state_token(&input), original);
    input.raw_data = b"{}";
    input.preset_mtime = None;
    assert_ne!(state_token(&input), original);
    input.preset_mtime = Some(EpochSecs(0));
    let zero = state_token(&input);
    input.preset_mtime = None;
    assert_ne!(state_token(&input), zero);
}
