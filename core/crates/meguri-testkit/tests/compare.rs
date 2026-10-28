//! Comparison rules (specs/conformance/observations.md O3, O6, O7).

use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::observe;
use meguri_testkit::suite::Deviation;
use serde_json::{Value, json};

fn memory_states(stability: f64) -> Value {
    json!({"steps": [{"next_states": {"#c1": {
        "current": {"kind": "new", "position": 1},
        "again": {"kind": "learning", "remaining_steps": 2, "scheduled_secs": 60, "elapsed_secs": 0,
                  "memory_state": {"stability": stability, "difficulty": 6.4133}},
        "hard": {"kind": "relearning",
                 "learning": {"remaining_steps": 1, "scheduled_secs": 600, "elapsed_secs": 0,
                              "memory_state": {"stability": stability, "difficulty": 5.0}},
                 "review": {"scheduled_days": 1, "elapsed_days": 0, "ease_factor": 2.3, "lapses": 1,
                            "leeched": false, "memory_state": {"stability": stability, "difficulty": 5.0}}},
        "good": {"kind": "review", "scheduled_days": 5, "elapsed_days": 0, "ease_factor": 2.5, "lapses": 0,
                 "leeched": false, "memory_state": {"stability": stability, "difficulty": 2.1}},
        "easy": {"kind": "review", "scheduled_days": 9, "elapsed_days": 0, "ease_factor": 2.5, "lapses": 0,
                 "leeched": false}
    }}}]})
}

fn plain() -> CompareOptions<'static> {
    CompareOptions::default()
}

#[test]
fn memory_state_stability_has_a_relative_tolerance_of_1e_5() {
    let expected = memory_states(11.262237);
    let close = memory_states(11.262237 * (1.0 + 5e-6));
    let far = memory_states(11.262237 * (1.0 + 2e-5));
    assert!(diff::compare("s/x", &expected, &close, &plain()).differences.is_empty());
    let result = diff::compare("s/x", &expected, &far, &plain());
    let pointers: Vec<&str> = result.differences.iter().map(|d| d.pointer.as_str()).collect();
    assert_eq!(
        pointers,
        [
            "/steps/0/next_states/#c1/again/memory_state/stability",
            "/steps/0/next_states/#c1/good/memory_state/stability",
            "/steps/0/next_states/#c1/hard/learning/memory_state/stability",
            "/steps/0/next_states/#c1/hard/review/memory_state/stability",
        ]
    );
}

#[test]
fn values_outside_declared_tolerances_compare_exactly() {
    // The same relative 5e-6 on a card row and an ease factor is a difference.
    let card = |due: f64| json!({"steps": [{"card": {"#c1": {"data": {"s": due}}}}]});
    assert_eq!(
        diff::compare("s/x", &card(2.3065), &card(2.3065 * (1.0 + 5e-6)), &plain())
            .differences
            .len(),
        1
    );
    let mut eased = memory_states(1.0);
    eased["steps"][0]["next_states"]["#c1"]["good"]["ease_factor"] = json!(2.5000125);
    assert_eq!(
        diff::compare("s/x", &memory_states(1.0), &eased, &plain())
            .differences
            .len(),
        1
    );
    // 1 and 1.0 differ; integers never get a tolerance, even inside stats.
    let stats = |v: Value| json!({"steps": [{"stats": {"count": v}}]});
    assert_eq!(
        diff::compare("s/x", &stats(json!(1)), &stats(json!(1.0)), &plain())
            .differences
            .len(),
        1
    );
    assert_eq!(
        diff::compare("s/x", &stats(json!(1_000_000)), &stats(json!(1_000_001)), &plain())
            .differences
            .len(),
        1
    );
    assert!(
        diff::compare("s/x", &stats(json!(0.25)), &stats(json!(0.25 * (1.0 + 5e-7))), &plain())
            .differences
            .is_empty()
    );
    assert_eq!(
        diff::compare("s/x", &stats(json!(0.25)), &stats(json!(0.25 * (1.0 + 2e-6))), &plain())
            .differences
            .len(),
        1
    );
}

#[test]
fn differences_name_pointers_absent_members_and_the_first_divergent_step() {
    let expected = json!({"steps": [{}, {}, {"timing": {"now": 1}}, {"ids": {"#c1": 5, "a/b~c": 1}}]});
    let actual = json!({"steps": [{}, {}, {"timing": {"now": 1}}, {"ids": {"#c1": 4, "#c2": 6}}, {}]});
    let result = diff::compare("s/x", &expected, &actual, &plain());
    let shown: Vec<(String, Value, Value)> = result
        .differences
        .iter()
        .map(|d| (d.pointer.clone(), diff::shown(&d.expected), diff::shown(&d.actual)))
        .collect();
    assert_eq!(
        shown,
        [
            ("/steps/3/ids/#c1".to_owned(), json!(5), json!(4)),
            ("/steps/3/ids/#c2".to_owned(), json!("<absent>"), json!(6)),
            ("/steps/3/ids/a~1b~0c".to_owned(), json!(1), json!("<absent>")),
            ("/steps/4".to_owned(), json!("<absent>"), json!({})),
        ]
    );
    assert_eq!(diff::first_divergence(&result.differences), Some(3));
    // A literal "<absent>" string is still unequal to a missing member.
    let a = json!({"steps": [{"x": "<absent>"}]});
    let b = json!({"steps": [{}]});
    assert_eq!(diff::compare("s/x", &a, &b, &plain()).differences.len(), 1);
}

#[test]
fn set_comparison_ignores_order_only_where_declared() {
    let expected = json!({"steps": [{"search": {"ids": ["#n1", "#n2", "#n3"]}}, {"queue": ["#c1", "#c2"]}]});
    let actual = json!({"steps": [{"search": {"ids": ["#n3", "#n1", "#n2"]}}, {"queue": ["#c2", "#c1"]}]});
    let set = vec!["/steps/*/search/ids".to_owned()];
    let options = CompareOptions {
        set: &set,
        ..CompareOptions::default()
    };
    let result = diff::compare("s/x", &expected, &actual, &options);
    let pointers: Vec<&str> = result.differences.iter().map(|d| d.pointer.as_str()).collect();
    assert_eq!(pointers, ["/steps/1/queue/0", "/steps/1/queue/1"]);
    // A multiset: a repeated item is not the same as a single one.
    let dup = json!({"steps": [{"search": {"ids": ["#n1", "#n1", "#n2"]}}, {"queue": ["#c1", "#c2"]}]});
    assert!(!diff::compare("s/x", &expected, &dup, &options).differences.is_empty());
}

#[test]
fn set_comparison_tells_integers_from_fractions_and_ignores_order() {
    // JCS writes 1 and 1.0 alike; the multiset still holds two different
    // values, and reordering them is no difference.
    let set = vec!["/steps/*/rows".to_owned()];
    let options = CompareOptions {
        set: &set,
        ..CompareOptions::default()
    };
    let expected = json!({"steps": [{"rows": [[1], [1.0], [{"a": 2}], [{"a": 2.0}]]}]});
    let actual = json!({"steps": [{"rows": [[{"a": 2.0}], [1.0], [{"a": 2}], [1]]}]});
    let result = diff::compare("s/x", &expected, &actual, &options);
    assert!(result.differences.is_empty(), "{:?}", result.differences);
    // Two 1.0s are not a 1 and a 1.0.
    let other = json!({"steps": [{"rows": [[{"a": 2.0}], [1.0], [{"a": 2}], [1.0]]}]});
    assert_eq!(diff::compare("s/x", &expected, &other, &options).differences.len(), 1);
}

fn fold_deviation() -> Deviation {
    serde_json::from_value(json!({
        "id": "search-unicode-case-folding",
        "requirement": "R1036",
        "summary": "Plain searches fold non-ASCII letters, so Meguri can match more notes.",
        "test": "search/folding/unicode-mode",
        "scenarios": ["search/folding/*"],
        "pointer": "/steps/*/search/ids",
        "transform": {"replace": ["#n1", "#n2"]}
    }))
    .unwrap()
}

#[test]
fn a_listed_deviation_turns_its_difference_into_a_pass_and_nothing_else() {
    let golden = json!({"steps": [{}, {"search": {"ids": ["#n1"]}}]});
    let meguri = json!({"steps": [{}, {"search": {"ids": ["#n1", "#n2"]}}]});
    let deviations = vec![fold_deviation()];
    let with = CompareOptions {
        deviations: &deviations,
        ..CompareOptions::default()
    };

    let result = diff::compare("search/folding/umlaut", &golden, &meguri, &with);
    assert!(result.differences.is_empty(), "{:?}", result.differences);
    assert_eq!(result.deviations_applied, ["search-unicode-case-folding"]);

    // Without the entry, or for a scenario it does not list, the difference fails.
    assert_eq!(
        diff::compare("search/folding/umlaut", &golden, &meguri, &plain())
            .differences
            .len(),
        1
    );
    let other = diff::compare("search/plain/umlaut", &golden, &meguri, &with);
    assert_eq!(other.differences.len(), 1);
    assert!(other.deviations_applied.is_empty());
    // The transform only covers its pointer: another difference still fails.
    let mut worse = meguri.clone();
    worse["steps"][0] = json!({"timing": {"now": 1}});
    assert_eq!(
        diff::compare("search/folding/umlaut", &golden, &worse, &with)
            .differences
            .len(),
        1
    );
}

#[test]
fn exact_json_representable_integer_lexemes_keep_integer_kind() {
    for (lexeme, expected) in [
        ("0", json!(0)),
        ("1", json!(1)),
        ("-9223372036854775808", json!(i64::MIN)),
        ("9223372036854775807", json!(i64::MAX)),
        ("12345678901234567890", json!(12_345_678_901_234_567_890_u64)),
        ("18446744073709551615", json!(u64::MAX)),
    ] {
        let parsed = observe::parse_json(&format!(r#"{{"nested":[{{"value":{lexeme}}}]}}"#)).unwrap();
        let value = &parsed["nested"][0]["value"];
        assert_eq!(value, &expected, "{lexeme}");
        assert!(!value.is_f64(), "{lexeme}");
        assert_eq!(value.to_string(), expected.to_string(), "{lexeme}");
    }
}

#[test]
fn exact_json_outside_64_bit_integers_keep_c3_float_fallback() {
    for lexeme in ["18446744073709551616", "-9223372036854775809"] {
        let document = format!(r#"{{"nested":[{{"value":{lexeme}}}]}}"#);
        for value in [
            observe::parse_json(lexeme).unwrap(),
            observe::parse_json(&document).unwrap()["nested"][0]["value"].clone(),
            meguri_testkit::scenario::parse_observation_json(&document).unwrap()["nested"][0]["value"].clone(),
        ] {
            assert!(value.is_f64(), "{lexeme}");
            assert_eq!(
                value.as_f64().unwrap().to_bits(),
                lexeme.parse::<f64>().unwrap().to_bits()
            );
        }
        assert!(meguri_testkit::scenario::parse_json(&document).is_err(), "{lexeme}");
    }
}

#[test]
fn authored_json_bounds_raw_integer_lexemes_before_conversion() {
    for lexeme in [
        "9007199254740993",
        "-9007199254740993",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "170141183460469231731687303715884105728",
        "-170141183460469231731687303715884105729",
    ] {
        for document in [lexeme.to_owned(), format!(r#"{{"nested":[{{"value":{lexeme}}}]}}"#)] {
            let error = meguri_testkit::scenario::parse_json(&document).unwrap_err();
            assert!(error.to_string().contains("outside ±2^53"), "{lexeme}: {error}");
        }
    }
}

#[test]
fn exact_json_float_lexemes_keep_kind_and_precision() {
    for lexeme in [
        "-0",
        "1.0",
        "1e0",
        "1E0",
        "-0.0",
        "18446744073709551616.0",
        "-9223372036854775809.0",
        "1.5e300",
        "1.2345678901234567",
    ] {
        let parsed = observe::parse_json(&format!(r#"{{"nested":[{lexeme}]}}"#)).unwrap();
        let value = &parsed["nested"][0];
        assert!(value.is_f64(), "{lexeme}");
        if matches!(lexeme, "-0" | "-0.0") {
            assert!(value.as_f64().unwrap().is_sign_negative(), "{lexeme}");
        }
        assert_eq!(
            value.as_f64().unwrap().to_bits(),
            lexeme.parse::<f64>().unwrap().to_bits(),
            "{lexeme}"
        );
        let authored = meguri_testkit::scenario::parse_json(&format!(r#"{{"nested":[{lexeme}]}}"#)).unwrap();
        assert!(authored["nested"][0].is_f64(), "{lexeme}");
        assert_eq!(
            authored["nested"][0].as_f64().unwrap().to_bits(),
            value.as_f64().unwrap().to_bits(),
            "{lexeme}"
        );
        let round_trip = observe::parse_json(&observe::golden_text(&parsed)).unwrap();
        assert!(round_trip["nested"][0].is_f64(), "{lexeme}");
        assert_eq!(
            round_trip["nested"][0].as_f64().unwrap().to_bits(),
            value.as_f64().unwrap().to_bits(),
            "{lexeme}"
        );
    }
}

#[test]
fn browser_config_json_negative_zero_is_float_but_authored_yaml_is_integer() {
    // storage/config-table C3 and browser-config-ops B2 preserve JSON -0;
    // conformance/dsl D2 resolves the authored YAML token as an integer.
    let observation =
        meguri_testkit::scenario::parse_observation_json(r#"{"steps":[{"browser_config":{"sortType":-0}}]}"#).unwrap();
    let observed = &observation["steps"][0]["browser_config"]["sortType"];
    assert!(observed.is_f64());
    assert_eq!(observed.as_f64(), Some(0.0));
    assert!(observed.as_f64().unwrap().is_sign_negative());

    let authored = meguri_testkit::scenario::parse_yaml("setup:\n  config:\n    sortType: -0\n").unwrap();
    let configured = &authored["setup"]["config"]["sortType"];
    assert_eq!(configured.as_i64(), Some(0));
    assert!(!configured.is_f64());
}

#[test]
fn f32_values_are_written_as_their_shortest_round_trip_text() {
    assert_eq!(observe::f32_value(11.262_237_f32).unwrap().to_string(), "11.262237");
    assert_eq!(observe::f32_value(2.3065_f32).unwrap().to_string(), "2.3065");
    assert_eq!(observe::f32_value(1.0_f32).unwrap(), json!(1.0));
    assert!(observe::f32_value(1.0_f32).unwrap().is_f64());
    assert_eq!(observe::f32_value(0.1_f32).unwrap().to_string(), "0.1");
    assert_eq!(observe::f32_value(1e-10_f32).unwrap(), json!(1e-10));
    assert!(observe::f32_value(f32::NAN).is_err());
    // The widened f64 is what f32 printing must avoid.
    assert_ne!(
        observe::f32_value(11.262_237_f32).unwrap(),
        json!(f64::from(11.262_237_f32))
    );
}

#[test]
fn golden_text_matches_the_oracle_layout() {
    let value = json!({"b": [1, 2.5, "x"], "a": {"z": [], "y": {}, "x": [{"k": 1e-5}]}, "c": 1e22, "d": 0.0001});
    assert_eq!(
        observe::golden_text(&value),
        "{\n  \"a\": {\n    \"x\": [\n      {\n        \"k\": 1e-05\n      }\n    ],\n    \"y\": {},\n    \"z\": []\n  },\n  \
         \"b\": [1, 2.5, \"x\"],\n  \"c\": 1e+22,\n  \"d\": 0.0001\n}\n"
    );
    // Committed goldens are already in this layout.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/goldens/smoke/fsrs-new-card.json");
    let text = std::fs::read_to_string(path).unwrap();
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(observe::golden_text(&parsed), text);
}

#[test]
fn card_stats_tolerates_only_declared_memory_and_retrievability_paths_at_the_boundary() {
    let base = json!({"steps":[{"card_stats":{"#c1":{
        "memory_state":{"stability":1.0,"difficulty":1.0},"fsrs_retrievability":1.0,
        "average_secs":1.0,"total_secs":1.0,"desired_retention":1.0,"fsrs_params":[1.0],
        "revlog":[{"memory_state":{"stability":1.0,"difficulty":1.0},"taken_secs":1.0,"ease":1.0}],
        "reviews":1000000,"interval":1000000,"ease":1.0
    }}}]});
    let paths = [
        "/memory_state/stability",
        "/memory_state/difficulty",
        "/fsrs_retrievability",
        "/revlog/0/memory_state/stability",
        "/revlog/0/memory_state/difficulty",
    ];
    for path in paths {
        let pointer = format!("/steps/0/card_stats/#c1{path}");
        for (relative, passes) in [
            (0.999e-5, true),
            (1.001e-5, false),
            (-0.999e-5, true),
            (-1.001e-5, false),
        ] {
            let mut actual = base.clone();
            *actual.pointer_mut(&pointer).unwrap() = json!(1.0 + relative);
            let diff = diff::compare("s/x", &base, &actual, &plain());
            assert_eq!(diff.differences.is_empty(), passes, "{path}: {relative}");
        }
    }
    for path in [
        "/average_secs",
        "/total_secs",
        "/desired_retention",
        "/fsrs_params/0",
        "/revlog/0/taken_secs",
        "/revlog/0/ease",
        "/ease",
    ] {
        let pointer = format!("/steps/0/card_stats/#c1{path}");
        let mut actual = base.clone();
        *actual.pointer_mut(&pointer).unwrap() = json!(1.000000001);
        assert_eq!(
            diff::compare("s/x", &base, &actual, &plain()).differences.len(),
            1,
            "{path}"
        );
    }
    for path in ["/reviews", "/interval"] {
        let mut actual = base.clone();
        *actual.pointer_mut(&format!("/steps/0/card_stats/#c1{path}")).unwrap() = json!(1000001);
        assert_eq!(diff::compare("s/x", &base, &actual, &plain()).differences.len(), 1);
    }
    // Similar field names at unrelated depths/kinds have no card-info tolerance.
    for pointer in [
        "/steps/0/card_stats/#c1/other/memory_state/stability",
        "/steps/0/card_stats/#c1/revlog/0/fsrs_retrievability",
        "/steps/0/card_stats/#c1/other/fsrs_retrievability",
        "/steps/0/card/#c1/memory_state/stability",
    ] {
        let segments = pointer
            .trim_start_matches('/')
            .split('/')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(meguri_testkit::observe::tolerance(&segments), None, "{pointer}");
    }
}

#[test]
fn typed_stats_lists_keep_the_existing_float_tolerance_and_exact_counts_and_keys() {
    let base = json!({"steps":[{"stats":[{"future_due":{"daily_load":1.0,"future_due":{"0":1000000}},"difficulty":{"average":1.0}}]}]});
    for relative in [0.999e-6, 1.001e-6] {
        let mut actual = base.clone();
        actual["steps"][0]["stats"][0]["difficulty"]["average"] = json!(1.0 + relative);
        assert_eq!(
            diff::compare("s/x", &base, &actual, &plain()).differences.is_empty(),
            relative < 1e-6
        );
    }
    let mut count = base.clone();
    count["steps"][0]["stats"][0]["future_due"]["future_due"]["0"] = json!(1000001);
    assert_eq!(diff::compare("s/x", &base, &count, &plain()).differences.len(), 1);
    let mut keys = base.clone();
    keys["steps"][0]["stats"][0]["future_due"]["future_due"]
        .as_object_mut()
        .unwrap()
        .remove("0");
    assert_eq!(diff::compare("s/x", &base, &keys, &plain()).differences.len(), 1);
}
