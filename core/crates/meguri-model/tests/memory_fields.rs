use meguri_model::card_data::{CardData, MemoryState};
#[test]
fn memory_comparison_uses_the_codec_rounding_and_retains_presence() {
    let stored = CardData::parse(
        r#"{"s":1.23456,"d":7.6401,"dr":0.851,"decay":0.1542,"pos":31,"lrt":1234,"cd":"{}","unknown": [1, 2]}"#,
    );
    let mut desired = CardData::default();
    desired.memory_state = Some(MemoryState {
        stability: 1.2346,
        difficulty: 7.64,
    });
    desired.desired_retention = Some(0.85);
    desired.decay = Some(0.154);
    assert!(stored.same_memory_values(&desired));
    desired.memory_state.as_mut().unwrap().stability = 1.2347;
    assert!(!stored.same_memory_values(&desired));
    desired.memory_state = stored.memory_state;
    desired.decay = None;
    assert!(!stored.same_memory_values(&desired));
    let zero = CardData::parse(r#"{"dr":0.0}"#);
    let minus = CardData::parse(r#"{"dr":-0.0}"#);
    assert!(zero.same_memory_values(&minus));
    assert!(!zero.same_memory_values(&CardData::default()));
}
