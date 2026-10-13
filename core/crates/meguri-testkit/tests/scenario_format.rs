//! Scenario parsing and hashing (specs/conformance/dsl.md D1-D3).
//!
//! Expected hashes come from the oracle side: every committed golden records
//! the scenario_sha256 the oracle harness computed for its scenario.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::scenario::{self, Step};
use serde_json::{Value, json};

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn golden_hash(id: &str) -> String {
    let path = conformance_dir().join("goldens").join(format!("{id}.json"));
    let golden: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    golden["scenario_sha256"].as_str().unwrap().to_owned()
}

fn hash_of_yaml(text: &str) -> String {
    scenario::scenario_sha256(&scenario::parse_yaml(text).unwrap()).unwrap()
}

#[test]
fn every_smoke_scenario_hashes_like_the_oracle_recorded_it() {
    let mut checked = 0;
    for entry in fs::read_dir(conformance_dir().join("scenarios/smoke")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if name.starts_with("area.") || name == "runner-empty.yaml" {
            continue;
        }
        let loaded = scenario::load(&path).unwrap();
        let id = format!("smoke/{}", name.trim_end_matches(".yaml"));
        assert_eq!(loaded.scenario.id, id);
        assert_eq!(loaded.sha256, golden_hash(&id), "{id}");
        checked += 1;
    }
    assert!(checked >= 7, "only {checked} smoke scenarios checked");
}

#[test]
fn comments_key_order_and_layout_do_not_change_the_hash() {
    let original = fs::read_to_string(conformance_dir().join("scenarios/smoke/fsrs-new-card.yaml")).unwrap();
    // The same scenario: other comments, keys reordered, block style instead
    // of flow style, other quoting.
    let rewritten = r##"
# A different comment.
steps:
  - observe: [ids, "next_states(#c1)"]
  - answer: {rating: good, card: "#c1", time_ms: 4000}
  - advance: '10m'
  - observe: ["next_states(#c1)", "card(#c1)"]
  - answer: {card: "#c1", time_ms: 3000, rating: good}
  - advance: 3d   # three days
  - observe: ["next_states(#c1)", "card(#c1)"]
  - answer: {card: "#c1", rating: again, time_ms: 9000}
  - observe: ["next_states(#c1)", "card(#c1)", "revlog(#c1)", timing]
setup:
  notes:
    - notetype: Pair
      deck: Words
      fields:
        - cat
        - gato
  notetypes:
    - templates: [{front: "{{Front}}", name: Forward, back: "{{Back}}"}]
      name: Pair
      fields: [Front, Back]
  decks: [{preset: Daily, name: Words}]
  presets:
    - {name: Daily, learn_steps: [1m, 10m], relearn_steps: [10m], max_interval_days: 36500, desired_retention: 0.90}
  config: {loadBalancerEnabled: false, fsrs: true}
clock:
  mode: step
  start: "2030-01-15T10:00:00"
  tz: Europe/Paris
id: smoke/fsrs-new-card
"##;
    assert_eq!(hash_of_yaml(rewritten), hash_of_yaml(&original));
    assert_eq!(hash_of_yaml(rewritten), golden_hash("smoke/fsrs-new-card"));
    // JSON text of the same value hashes the same.
    let as_json = serde_json::to_string(&scenario::parse_yaml(&original).unwrap()).unwrap();
    assert_eq!(
        scenario::scenario_sha256(&scenario::parse_json(&as_json).unwrap()).unwrap(),
        golden_hash("smoke/fsrs-new-card")
    );
}

#[test]
fn changing_one_value_changes_the_hash() {
    let original = fs::read_to_string(conformance_dir().join("scenarios/smoke/timing-advance.yaml")).unwrap();
    let edited = original.replace("advance: 20m", "advance: 21m");
    assert_ne!(edited, original);
    assert_ne!(hash_of_yaml(&edited), hash_of_yaml(&original));
}

#[test]
fn plain_scalars_resolve_with_the_yaml_1_2_core_schema() {
    let cases: &[(&str, Value)] = &[
        ("yes", json!("yes")),
        ("on", json!("on")),
        ("017", json!(17)),
        ("0o17", json!(15)),
        ("0x1F", json!(31)),
        ("+5", json!(5)),
        ("-12", json!(-12)),
        ("1_000", json!("1_000")),
        ("0b101", json!("0b101")),
        ("2030-01-15", json!("2030-01-15")),
        ("1.0", json!(1.0)),
        ("1.", json!(1.0)),
        (".5", json!(0.5)),
        ("1e3", json!(1000.0)),
        ("-2.5E-1", json!(-0.25)),
        ("12e", json!("12e")),
        ("True", json!(true)),
        ("FALSE", json!(false)),
        ("tRUE", json!("tRUE")),
        ("~", Value::Null),
        ("null", Value::Null),
        ("Null", Value::Null),
        ("NULL", Value::Null),
        ("nULL", json!("nULL")),
        ("", Value::Null),
        ("'017'", json!("017")),
        ("\"true\"", json!("true")),
        ("9007199254740992", json!(9_007_199_254_740_992_i64)),
    ];
    for (text, expected) in cases {
        let value = scenario::parse_yaml(&format!("a: {text}\n")).unwrap();
        assert_eq!(&value["a"], expected, "a: {text}");
        // Integers stay integers and floats stay floats.
        assert_eq!(value["a"].is_f64(), expected.is_f64(), "a: {text}");
    }
}

#[test]
fn the_json_compatible_subset_is_enforced() {
    let rejected = [
        "a: &x 1\nb: *x\n",
        "<<: {a: 1}\nb: 2\n",
        "a: !!str true\n",
        "a: !foo bar\n",
        "a: ! bar\n",
        "a: 1\na: 2\n",
        "1: x\n",
        "true: x\n",
        "? [1]\n: x\n",
        "a: .inf\n",
        "a: -.Inf\n",
        "a: .nan\n",
        "a: 9007199254740993\n",
        "a: 1e999\n",
        "a: 1\n---\nb: 2\n",
    ];
    for text in rejected {
        assert!(scenario::parse_yaml(text).is_err(), "accepted {text:?}");
    }
    // An anchor alone is allowed and changes nothing; quoted keys are strings.
    assert_eq!(
        scenario::parse_yaml("a: &x 1\n'1': y\n").unwrap(),
        json!({"a": 1, "1": "y"})
    );
    assert!(scenario::parse_json(r#"{"a": 1, "a": 2}"#).is_err());
}

#[test]
fn jcs_writes_numbers_and_strings_as_rfc_8785() {
    let value = json!({
        "b": [1.0, 1e21, 1e-7, 0.000001, 123.456, -0.0, 100, 1.5e300],
        "a": "é\u{1}\"\\\n\u{2028}",
        "\u{20ac}": null,
        "\u{1f600}": true
    });
    assert_eq!(
        scenario::jcs(&value).unwrap(),
        "{\"a\":\"é\\u0001\\\"\\\\\\n\u{2028}\",\"b\":[1,1e+21,1e-7,0.000001,123.456,0,100,1.5e+300],\
         \"\u{20ac}\":null,\"\u{1f600}\":true}"
    );
}

#[test]
fn ids_must_match_the_file_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.yaml");
    fs::write(
        &path,
        "id: smoke/other\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\n",
    )
    .unwrap();
    let loaded = scenario::load(&path).unwrap();
    let err = scenario::check_id(&loaded, "smoke/x").unwrap_err();
    assert!(err.to_string().contains("smoke/other"), "{err}");
    assert!(scenario::check_id(&loaded, "smoke/other").is_ok());
}

#[test]
fn the_typed_model_reads_every_v0_op_and_rejects_unknown_ones() {
    let text = r##"
id: area/all-ops
clock: {tz: UTC, start: "2030-01-15T10:00:00Z", mode: frozen}
compare: {set: ["/steps/*/search/ids"]}
setup:
  config: {rollover: 4}
  presets: [{name: Default, learn_steps: [30s, 1d], leech_action: tag_only}]
  decks: [A, {name: "A::B", preset: Default}]
  notetypes: [{name: Q, kind: cloze, fields: [Text], templates: [{name: C}], css: ""}]
  notes: [{notetype: Q, deck: A, fields: {Text: "{{c1::x}}"}, tags: [t]}]
  cards: [{card: "#c1", due: 3}]
  revlog: [{card: "#c1", id: 1, ease: 3, ivl: 1, lastIvl: 0, factor: 2500, time: 10, type: 1}]
steps:
  - advance: 1d2h
  - set_clock: "2030-01-16T00:00:00+05:45"
  - add_note: {notetype: Q, fields: ["y"]}
  - answer: {card: "#c1", rating: hard}
  - edit_note: {note: "#n1", fields: [z]}
  - delete_notes: {notes: ["#n2"]}
  - bury: {cards: ["#c1"]}
  - suspend: {cards: ["#c1"]}
  - forget: {cards: ["#c1"], reset_counts: true}
  - set_due: {cards: ["#c1"], days: "1-7"}
  - reposition: {cards: ["#c1"], start: 5}
  - update_preset: {name: Default, new_per_day: 5}
  - build_filtered: {name: F, search: "deck:A"}
  - import: {fixture: package/x.apkg}
  - export: {format: apkg, name: out.apkg}
  - check_db: {}
  - db_exec: {sql: "update cards set due = 1"}
  - sync: {kind: normal}
  - observe:
      - ids
      - "card(#c1, #c2)"
      - timing
      - {next_states: ["#c1"]}
      - {fuzz_table: {card: "#c1", pairs: [[1, 0]], intervals: [3]}}
      - {search: {query: "deck:A"}}
      - queue
      - deck_tree
      - render(#c1)
      - db_dump
      - {stats: {}}
      - digest
"##;
    let value = scenario::parse_yaml(text).unwrap();
    let parsed = scenario::Scenario::from_value(&value).unwrap();
    assert_eq!(parsed.steps.len(), 19);
    assert!(matches!(parsed.steps[0], Step::Advance(d) if d.seconds() == 93_600));
    let needs = parsed.needs();
    assert_eq!(
        needs.setup.iter().map(String::as_str).collect::<Vec<_>>(),
        ["cards", "config", "decks", "notes", "notetypes", "presets", "revlog"]
    );
    assert!(needs.ops.contains("build_filtered") && needs.ops.contains("sync"));
    assert!(!needs.ops.contains("observe"));
    assert!(needs.observations.contains("fuzz_table") && needs.observations.contains("digest"));

    for bad in [
        "steps: [{teleport: 1d}]",
        "steps: [{advance: 1d, set_clock: \"2030-01-15T10:00:00\"}]",
        "steps: [{advance: 1w}]",
        "steps: [{observe: [weather]}]",
        "steps: [{observe: [\"timing(#c1)\"]}]",
        "steps: [{observe: [\"card(#n1)\"]}]",
        "steps: [{answer: {card: \"#c1\", rating: great}}]",
        "steps: [{answer: {card: \"#c0\", rating: good}}]",
        "steps: [{add_note: {notetype: Q, fields: [1]}}]",
        "colour: red",
        "setup: {cards: [{card: \"#c1\", colour: 1}]}",
    ] {
        let text = format!("id: area/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{bad}\n");
        let value = scenario::parse_yaml(&text).unwrap();
        assert!(scenario::Scenario::from_value(&value).is_err(), "accepted {bad}");
    }
}
