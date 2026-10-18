use meguri_model::{
    deck_config::DeckConfig,
    ids::DeckConfigId,
    proto::{BlobMessage, ProtoBlob, deck_config::DeckConfigMessage},
};
use meguri_sched::{
    model::preset::PresetView,
    states::fsrs_glue::{FsrsCache, UnsupportedParams},
};

fn preset(params: Vec<f32>) -> PresetView {
    let mut fields = DeckConfigMessage::builtin_defaults();
    fields.set_fsrs_params_6(params);
    PresetView::new(&DeckConfig {
        id: DeckConfigId(1),
        name: String::new(),
        mtime: Default::default(),
        usn: Default::default(),
        config: ProtoBlob::new(fields),
    })
}

#[test]
fn supported_models_are_reused_without_changing_any_crate_output_bits() {
    let mut cache = FsrsCache::default();
    for params in [vec![], vec![0.5; 17], vec![0.5; 19], vec![0.5; 21]] {
        let preset = preset(params);
        let first = cache.model(&preset).unwrap() as *const _;
        let second = cache.model(&preset).unwrap() as *const _;
        assert_eq!(first, second);
        let actual = cache.model(&preset).unwrap().next_states(None, 0.9, 0).unwrap();
        let expected = fsrs::FSRS::new(preset.parameters())
            .unwrap()
            .next_states(None, 0.9, 0)
            .unwrap();
        for (a, b) in [
            (&actual.again, &expected.again),
            (&actual.hard, &expected.hard),
            (&actual.good, &expected.good),
            (&actual.easy, &expected.easy),
        ] {
            assert_eq!(
                (
                    a.interval.to_bits(),
                    a.memory.stability.to_bits(),
                    a.memory.difficulty.to_bits()
                ),
                (
                    b.interval.to_bits(),
                    b.memory.stability.to_bits(),
                    b.memory.difficulty.to_bits()
                )
            );
        }
    }
}

#[test]
fn unsupported_lengths_nonfinite_values_and_later_markers_are_typed_refusals() {
    let mut cache = FsrsCache::default();
    for params in [
        vec![0.5; 34],
        vec![0.5; 1],
        vec![f32::NAN; 21],
        vec![f32::INFINITY; 19],
        vec![f32::NEG_INFINITY; 17],
    ] {
        assert!(matches!(cache.model(&preset(params)), Err(UnsupportedParams)));
    }
    let fields = DeckConfigMessage::decode(&[0x38, 0]).unwrap();
    let view = PresetView::new(&DeckConfig {
        id: DeckConfigId(1),
        name: String::new(),
        mtime: Default::default(),
        usn: Default::default(),
        config: ProtoBlob::new(fields),
    });
    assert!(matches!(cache.model(&view), Err(UnsupportedParams)));
    // Refusal of one preset never poisons the previously cached default.
    assert!(cache.model(&preset(vec![])).unwrap().next_states(None, 0.9, 0).is_ok());
}
