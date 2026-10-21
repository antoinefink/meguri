use meguri_model::{
    card::{Card, CardQueue, CardType},
    card_data::MemoryState,
    ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId},
    time::EpochSecs,
    usn::Usn,
};
use meguri_sched::{
    model::preset::PresetView,
    session::token::{StateInputs, effective_home_fingerprint, render_key, state_token},
};

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
        effective_home: [0; 32],
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
    input.history_last_review = None;
    input.effective_home = [1; 32];
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
        render_key(&card, EpochSecs(3), NotetypeId(4), EpochSecs(5), &[0; 32]),
        render_key(&flagged, EpochSecs(3), NotetypeId(4), EpochSecs(5), &[0; 32])
    );
    flagged.flags = 0;
    assert_eq!(
        render_key(&card, EpochSecs(3), NotetypeId(4), EpochSecs(5), &[0; 32]),
        render_key(&flagged, EpochSecs(3), NotetypeId(4), EpochSecs(5), &[0; 32])
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

fn preset(fields: meguri_model::proto::deck_config::DeckConfigMessage) -> PresetView {
    PresetView::new(&meguri_model::deck_config::DeckConfig {
        id: DeckConfigId(1),
        name: "token test".into(),
        mtime: EpochSecs(3),
        usn: Usn(0),
        config: meguri_model::proto::ProtoBlob::new(fields),
    })
}
#[test]
fn c5_effective_home_uses_provider_bounds_and_enum_defaults() {
    use meguri_model::proto::deck_config::DeckConfigMessage;
    let mut a = DeckConfigMessage::builtin_defaults();
    a.cap_answer_time_to_secs = 0;
    a.maximum_review_interval = 0;
    a.leech_action = 123;
    let mut b = a.clone();
    b.cap_answer_time_to_secs = 10000;
    b.maximum_review_interval = 100000;
    b.leech_action = 0;
    let (a, b) = (preset(a), preset(b));
    assert_eq!(a.fields().cap_answer_time_to_secs, b.fields().cap_answer_time_to_secs);
    assert_eq!(a.fields().maximum_review_interval, b.fields().maximum_review_interval);
    assert_eq!(effective_home_fingerprint(&a, 0.9), effective_home_fingerprint(&b, 0.9));
}

#[test]
fn c5_effective_home_distinguishes_selected_kind_length_bits_support_and_retention() {
    use meguri_model::proto::{ProtoBlob, deck_config::DeckConfigMessage};
    let mut a = DeckConfigMessage::builtin_defaults();
    a.set_fsrs_params_6(vec![1.0; 21]);
    let a = preset(a);
    let digest = effective_home_fingerprint(&a, 0.9);
    assert_ne!(
        digest,
        effective_home_fingerprint(&a, f32::from_bits(0.9f32.to_bits() + 1))
    );
    for values in [vec![1.0; 19], vec![1.0; 20], vec![2.0; 21], vec![f32::NAN; 21]] {
        let mut p = a.source().clone();
        p.config.set_fsrs_params_6(values);
        assert_ne!(digest, effective_home_fingerprint(&PresetView::new(&p), 0.9));
    }
    let mut p = a.source().clone();
    let mut bytes = p.config.to_bytes().into_owned();
    bytes.extend_from_slice(&[0x3a, 0x01, 0x01]);
    p.config = ProtoBlob::decode(&bytes).unwrap();
    assert_ne!(digest, effective_home_fingerprint(&PresetView::new(&p), 0.9));
    // Equal values/length selected from a different list still name a different kind.
    let mut p = a.source().clone();
    p.config.set_fsrs_params_6(vec![1.0; 17]);
    let six = effective_home_fingerprint(&PresetView::new(&p), 0.9);
    let mut legacy: serde_json::Value =
        serde_json::from_str(&meguri_model::schema11::deck_config::deck_config_to_json(&p)).unwrap();
    legacy["fsrsParams6"] = serde_json::json!([]);
    legacy["fsrsWeights"] = serde_json::json!(vec![1.0; 17]);
    let four = meguri_model::schema11::deck_config::deck_config_from_json(&legacy.to_string()).unwrap();
    assert_ne!(six, effective_home_fingerprint(&PresetView::new(&four), 0.9));
}
