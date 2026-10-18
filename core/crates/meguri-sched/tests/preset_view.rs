use meguri_model::{
    deck_config::DeckConfig,
    ids::DeckConfigId,
    proto::{ProtoBlob, deck_config::DeckConfigMessage},
    time::EpochSecs,
    usn::Usn,
};
use meguri_sched::model::preset::PresetView;

fn row(message: DeckConfigMessage) -> DeckConfig {
    DeckConfig {
        id: DeckConfigId(42),
        name: "Scheduling test".into(),
        mtime: EpochSecs(123),
        usn: Usn(7),
        config: ProtoBlob::new(message),
    }
}

#[test]
fn bounds_are_a_read_view_and_do_not_fill_unbounded_missing_fields() {
    let mut message = DeckConfigMessage::default();
    message.desired_retention = f32::NAN;
    message.hard_multiplier = f32::INFINITY;
    let raw = row(message);
    let bytes = raw.config.to_bytes();
    let view = PresetView::new(&raw);
    assert_eq!(view.fields().new_per_day, 0);
    assert_eq!(view.fields().maximum_review_interval, 36500);
    assert_eq!(view.fields().desired_retention, 0.9);
    assert_eq!(view.fields().hard_multiplier, 1.2);
    assert!(view.fields().learn_steps.is_empty());
    assert!(!view.fields().wait_for_audio);
    assert_eq!(raw.config.to_bytes(), bytes);
}

#[test]
fn an_equal_edit_keeps_invalid_source_bytes_but_a_real_edit_normalizes_bounds() {
    let mut message = DeckConfigMessage::default();
    message.desired_retention = f32::NAN;
    let raw = row(message);
    let bytes = raw.config.to_bytes();
    let view = PresetView::new(&raw);
    assert!(view.edit(|m| m.desired_retention = 0.9).unwrap().is_none());
    assert_eq!(raw.config.to_bytes(), bytes);
    let changed = view.edit(|m| m.desired_retention = 0.85).unwrap().unwrap();
    assert_eq!(changed.config.desired_retention, 0.85);
    assert_eq!(changed.config.maximum_review_interval, 36500);
    assert_eq!(changed.config.cap_answer_time_to_secs, 60);
    assert_eq!(changed.mtime, raw.mtime);
    assert_eq!(changed.usn, raw.usn);
}

#[test]
fn parameter_decay_uses_stored_value_before_fsrs_model_clamping() {
    let mut message = DeckConfigMessage::default();
    message.set_fsrs_params_6(vec![0.5; 21]);
    let mut params = message.fsrs_params_6().to_vec();
    params[20] = 0.05;
    message.set_fsrs_params_6(params);
    let raw = row(message);
    let view = PresetView::new(&raw);
    assert_eq!(view.decay(), 0.05);
    assert_eq!(view.parameters(), raw.config.fsrs_params_6());
    let empty = row(DeckConfigMessage::default());
    assert_eq!(PresetView::new(&empty).decay(), 0.1542);
}

fn hex(bytes: &str) -> Vec<u8> {
    bytes
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn bounded_legacy_reads_and_edits_match_golden_blob_pairs() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/goldens/sched/data/preset-bounds.json"
    ))
    .unwrap();
    let steps = &golden["observations"]["steps"];
    for raw in steps[4]["db_exec"]["rows"].as_array().unwrap() {
        let name = raw[0].as_str().unwrap();
        let expected = &steps[5]["legacy_json"]["presets"][name];
        if expected.is_null() {
            continue;
        }
        let mut source = row(DeckConfigMessage::default());
        source.id = DeckConfigId(expected["id"].as_i64().unwrap());
        source.name = name.into();
        source.mtime = EpochSecs(raw[1].as_i64().unwrap());
        source.usn = Usn(raw[2].as_i64().unwrap() as i32);
        let bytes = hex(raw[3].as_str().unwrap());
        source.config = ProtoBlob::decode(&bytes).unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&meguri_model::schema11::deck_config_to_json(&source.for_use())).unwrap();
        assert_eq!(&actual, expected, "{name}");
        let view = PresetView::new(&source);
        assert!(view.edit(|_| {}).unwrap().is_none());
        assert_eq!(source.config.to_bytes(), bytes);
        if name == "Bounds" {
            let edited = view.edit(|m| m.desired_retention = 0.85).unwrap().unwrap();
            let expected = steps[17]["db_exec"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r[0] == "Bounds")
                .unwrap();
            assert_eq!(edited.config.to_bytes(), hex(expected[3].as_str().unwrap()));
        }
    }
}

#[test]
fn retention_patch_keeps_older_parameters_sm2_and_opaque_bytes_exactly() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/goldens/sched/data/preset-keep.json"
    ))
    .unwrap();
    let steps = &golden["observations"]["steps"];
    let mut source = row(DeckConfigMessage::default());
    source.config = ProtoBlob::decode(&hex(steps[1]["db_exec"]["rows"][0][0].as_str().unwrap())).unwrap();
    let view = PresetView::new(&source);
    assert_eq!(view.parameters().len(), 19);
    assert_eq!(view.decay(), 0.5);
    assert!(view.allows_short_term());
    let changed = view.edit(|m| m.desired_retention = 0.85).unwrap().unwrap();
    assert_eq!(
        changed.config.to_bytes(),
        hex(steps[4]["db_exec"]["rows"][0][2].as_str().unwrap())
    );
    assert_eq!(changed.config.other(), source.config.other());
    assert!(view.edit(|m| *m = DeckConfigMessage::default()).is_err());
}

#[test]
fn unknown_enum_codes_act_as_zero_and_survive_an_unrelated_edit() {
    use meguri_sched::model::preset::*;
    let mut source = row(DeckConfigMessage::builtin_defaults());
    source.config.new_card_insert_order = 9;
    source.config.leech_action = -1;
    source.config.new_mix = 3;
    source.config.interday_learning_mix = 4;
    source.config.new_card_sort_order = 5;
    source.config.review_order = 13;
    source.config.new_card_gather_priority = 6;
    source.config.question_action = 2;
    source.config.answer_action = 5;
    let view = PresetView::new(&source);
    assert_eq!(view.insert_order(), InsertOrder::Sequential);
    assert_eq!(view.leech_action(), LeechAction::Suspend);
    assert_eq!(view.new_mix(), ReviewMix::Mixed);
    assert_eq!(view.interday_learning_mix(), ReviewMix::Mixed);
    assert_eq!(view.new_sort_order(), NewSortOrder::CardTypeThenGathered);
    assert_eq!(view.review_order(), ReviewOrder::DueThenRandom);
    assert_eq!(view.gather_priority(), GatherPriority::Deck);
    assert_eq!(view.question_action(), QuestionAction::ShowAnswer);
    assert_eq!(view.answer_action(), AnswerAction::Bury);
    let changed = view.edit(|m| m.desired_retention = 0.85).unwrap().unwrap();
    assert_eq!(changed.config.review_order, 13);
    assert_eq!(changed.config.leech_action, -1);
}

#[test]
fn parameter_lists_select_first_nonempty_even_when_its_length_is_invalid() {
    fn params(lists: &[(u8, Vec<f32>)]) -> DeckConfigMessage {
        let mut bytes = Vec::new();
        for (field, values) in lists {
            bytes.push(field << 3 | 2);
            bytes.push((values.len() * 4) as u8);
            for value in values {
                bytes.extend(value.to_le_bytes());
            }
        }
        use meguri_model::proto::BlobMessage;
        DeckConfigMessage::decode(&bytes).unwrap()
    }
    for (lists, count, decay, short) in [
        (vec![], 0, 0.1542, true),
        (vec![(3, vec![0.2; 17])], 17, 0.5, false),
        (vec![(3, vec![0.2; 17]), (5, vec![0.3; 19])], 19, 0.5, true),
        (vec![(5, vec![0.3; 19]), (6, vec![0.4; 21])], 21, 0.4, true),
        (vec![(5, vec![0.3; 19]), (6, vec![0.4; 1])], 1, 0.5, false),
        (vec![(6, vec![0.0; 19])], 19, 0.5, false),
    ] {
        let source = row(params(&lists));
        let view = PresetView::new(&source);
        assert_eq!(view.parameters().len(), count);
        assert_eq!(view.decay(), decay);
        assert_eq!(view.allows_short_term(), short);
    }
}
