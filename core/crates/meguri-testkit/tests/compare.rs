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
