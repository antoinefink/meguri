use meguri_sched::model::card_data::{CardData, MemoryState};
use rand::{Rng, SeedableRng, rngs::StdRng};

fn raw_value(rng: &mut StdRng, depth: u8) -> String {
    match rng.random_range(0..if depth == 0 { 5 } else { 7 }) {
        0 => "null".into(),
        1 => rng.random::<bool>().to_string(),
        2 => format!(
            "{}.{}e+{}",
            rng.random_range(0..9999),
            rng.random_range(0..9999),
            rng.random_range(0..200)
        ),
        3 => serde_json::to_string(&format!("quote\"slash\\ newline\n日本語 {}", rng.random::<u64>())).unwrap(),
        4 => rng.random::<i64>().to_string(),
        5 => format!("[ {},  {} ]", raw_value(rng, depth - 1), raw_value(rng, depth - 1)),
        _ => format!(
            "{{ \"nested\" : {}, \"z\": {} }}",
            raw_value(rng, depth - 1),
            raw_value(rng, depth - 1)
        ),
    }
}

#[test]
fn ten_thousand_answer_rewrites_preserve_unknown_raw_json_and_custom_data() {
    let mut rng = StdRng::seed_from_u64(0x42_1955);
    for case in 0..10_000 {
        let custom = format!("{{\"k\":true,\"n\":[1,2],\"text\":\"日本語-{case}\"}}");
        let unknown: Vec<(String, String)> = (0..rng.random_range(1..=5))
            .map(|i| (format!("xyz_{case}_{i}"), raw_value(&mut rng, 3)))
            .collect();
        let mut input = format!("{{\"cd\":{}", serde_json::to_string(&custom).unwrap());
        for (key, raw) in &unknown {
            input.push_str(&format!(",\"{key}\":{raw}"));
        }
        input.push('}');
        let mut data = CardData::parse(&input);
        data.memory_state = Some(MemoryState {
            stability: rng.random_range(0.01..1000.0),
            difficulty: rng.random_range(1.0..10.0),
        });
        data.last_review_secs = Some(1_894_701_600 + case);
        let output = data.to_text();
        let reread = CardData::parse(&output);
        assert_eq!(
            reread
                .unknown_keys()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect::<Vec<_>>(),
            unknown,
            "case {case}"
        );
        assert_eq!(reread.custom_data(), Some(custom.as_str()), "case {case}");
        let next = CardData::parse(&reread.to_text());
        assert_eq!(
            next.unknown_keys().collect::<Vec<_>>(),
            reread.unknown_keys().collect::<Vec<_>>(),
            "repeated rewrite: {case}"
        );
        assert_eq!(next.custom_data(), reread.custom_data());
    }
}

#[test]
fn fresh_answer_fields_encode_to_exact_oracle_bytes() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/goldens/sched/data/answer-data.json"
    ))
    .unwrap();
    let states = [
        (2.3065, 2.118, 0.85, 0.3),
        (2.3065, 2.118, 0.9, 0.05),
        (3.173, 5.282, 0.9, 0.5),
        (3.7145, 5.82, 0.9, 0.5),
        (3.173, 5.282, 0.9, 0.5),
        (2.3065, 2.118, 0.9, 0.3),
        (2.3065, 2.118, 0.9, 0.1542),
    ];
    for (index, (s, d, dr, decay)) in states.into_iter().enumerate() {
        let mut data = CardData::default();
        data.new_position = Some(index as u32 + 1);
        data.memory_state = Some(MemoryState {
            stability: s,
            difficulty: d,
        });
        data.desired_retention = Some(dr);
        data.decay = Some(decay);
        data.last_review_secs = Some(1_894_701_600);
        assert_eq!(
            data.to_text(),
            golden["observations"]["steps"][12]["db_exec"]["rows"][index][2]
                .as_str()
                .unwrap()
        );
    }
}
