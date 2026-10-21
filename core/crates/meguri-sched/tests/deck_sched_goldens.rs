use meguri_model::{
    card::CardQueue,
    proto::{
        ProtoBlob,
        deck::{DeckKind, DeckKindMessage},
    },
    schema11::{deck_from_json, deck_to_json},
};
use meguri_sched::model::deck_sched::DeckSched;

#[test]
fn deck_counter_model_matches_recorded_answer_and_rollover_pairs() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/goldens/sched/data/deck-counters.json"
    ))
    .unwrap();
    let steps = &golden["observations"]["steps"];
    for name in ["Parent", "Parent::Child"] {
        let mut deck = deck_from_json(&steps[4]["legacy_json"]["decks"][name].to_string()).unwrap();
        let stored_learning_count = deck.common.learning_studied;
        assert_eq!(stored_learning_count, if name == "Parent::Child" { 5 } else { 0 });
        let counters = |deck: &meguri_model::deck::Deck, step: usize| {
            let expected = &steps[step]["legacy_json"]["decks"][name];
            let actual: serde_json::Value = serde_json::from_str(&deck_to_json(deck)).unwrap();
            for key in ["newToday", "revToday", "lrnToday", "timeToday"] {
                if name == "Parent::Child" && matches!(step, 17 | 19) && key == "lrnToday" {
                    // A6/D4, K1/P8 (R322): only Child's count differs at steps 17/19.
                    assert_eq!(expected[key], serde_json::json!([1, 0]));
                    assert_eq!(
                        actual[key],
                        serde_json::json!([expected[key][0], stored_learning_count]),
                        "{name} step {step} {key}"
                    );
                } else {
                    assert_eq!(actual[key], expected[key], "{name} step {step} {key}");
                }
            }
        };
        for (queue, time) in [
            (CardQueue::New, 5000),
            (CardQueue::Review, 7000),
            (CardQueue::Learning, 100000),
            (CardQueue::InterdayLearning, 1000),
        ] {
            DeckSched::record_answer(&mut deck.common, 0, queue, time, 60);
        }
        counters(&deck, 10);
        let bytes = deck.common.to_bytes().into_owned();
        assert_eq!(DeckSched::new(None, &deck.common).studied(1), (0, 0));
        assert_eq!(deck.common.to_bytes(), bytes);
        counters(&deck, 14);
        DeckSched::record_answer(&mut deck.common, 1, CardQueue::New, 2000, 60);
        counters(&deck, 17);
        if name == "Parent" {
            DeckSched::record_answer(&mut deck.common, 1, CardQueue::New, 3000, 60);
        }
        counters(&deck, 19);
    }
}

#[test]
fn desired_retention_resolves_recorded_optional_deck_values_without_editor_bounds() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/goldens/sched/data/desired-retention.json"
    ))
    .unwrap();
    let common = meguri_model::proto::deck::DeckCommon::default();
    for (step, retention) in [(1, 0.85), (4, 0.5), (7, 0.9)] {
        let text = golden["observations"]["steps"][step]["db_exec"]["rows"][0][0]
            .as_str()
            .unwrap();
        let bytes: Vec<u8> = text
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
        let kind = ProtoBlob::<DeckKindMessage>::decode(&bytes).unwrap();
        let DeckKind::Normal(normal) = &kind.kind else {
            panic!("normal deck fixture")
        };
        assert_eq!(DeckSched::new(Some(normal), &common).retention(0.9), retention);
        assert_eq!(kind.to_bytes(), bytes);
    }
}
