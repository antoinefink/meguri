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
        for (queue, time) in [
            (CardQueue::New, 5000),
            (CardQueue::Review, 7000),
            (CardQueue::Learning, 100000),
            (CardQueue::InterdayLearning, 1000),
        ] {
            DeckSched::record_answer(&mut deck.common, 0, queue, time, 60);
        }
        fn counters(deck: &meguri_model::deck::Deck, expected: &serde_json::Value) {
            let actual: serde_json::Value = serde_json::from_str(&deck_to_json(deck)).unwrap();
            for key in ["newToday", "revToday", "lrnToday", "timeToday"] {
                assert_eq!(actual[key], expected[key], "{key}");
            }
        }
        counters(&deck, &steps[10]["legacy_json"]["decks"][name]);
        let bytes = deck.common.to_bytes().into_owned();
        assert_eq!(DeckSched::new(None, &deck.common).studied(1), (0, 0));
        assert_eq!(deck.common.to_bytes(), bytes);
        counters(&deck, &steps[14]["legacy_json"]["decks"][name]);
        DeckSched::record_answer(&mut deck.common, 1, CardQueue::New, 2000, 60);
        counters(&deck, &steps[17]["legacy_json"]["decks"][name]);
        if name == "Parent" {
            DeckSched::record_answer(&mut deck.common, 1, CardQueue::New, 3000, 60);
        }
        counters(&deck, &steps[19]["legacy_json"]["decks"][name]);
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
            .chunks_exact(2)
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
