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

/// Every scenario file under `dir`, subdirectories included, as (path, id
/// relative to conformance/scenarios).
fn scenario_files(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if path.is_dir() {
            scenario_files(&path, &format!("{prefix}/{name}"), out);
        } else if !name.starts_with("area.") && name.ends_with(".yaml") {
            out.push((path, format!("{prefix}/{}", name.trim_end_matches(".yaml"))));
        }
    }
}

#[test]
fn every_smoke_scenario_hashes_like_the_oracle_recorded_it() {
    let mut files = Vec::new();
    scenario_files(&conformance_dir().join("scenarios/smoke"), "smoke", &mut files);
    assert!(
        files.iter().any(|(_, id)| id.starts_with("smoke/sync/")),
        "the walk reaches subdirectories"
    );
    let mut checked = 0;
    for (path, id) in files {
        // Every smoke scenario loads, subdirectories included.
        let loaded = scenario::load(&path).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(loaded.scenario.id, id);
        // Scenarios the oracle has recorded hash as it did.
        let golden = conformance_dir().join("goldens").join(format!("{id}.json"));
        if id == "smoke/runner-empty" || !golden.exists() {
            continue;
        }
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
    // i128::MIN must return the range error without overflowing in abs().
    for integer in [
        "-170141183460469231731687303715884105728",
        "170141183460469231731687303715884105727",
        "-170141183460469231731687303715884105729",
    ] {
        let err = scenario::parse_yaml(&format!("a: {integer}\n")).unwrap_err();
        assert!(
            err.to_string().contains(&format!("integer {integer} is outside ±2^53")),
            "{err}"
        );
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
compare: {set: ["/steps/*/search/*/ids"]}
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
  - export: {format: apkg, name: out.apkg, limit: collection, scheduling: true, presets: true, media: false}
  - check_db: {}
  - db_exec: {sql: "update cards set due = 1"}
  - reopen: {}
  - handoff: oracle
  - observe:
      - ids
      - "card(#c1, #c2)"
      - timing
      - {next_states: ["#c1"]}
      - {fuzz_table: {card: "#c1", pairs: [[1, 0]], intervals: [3]}}
      - {search: [{query: "deck:A"}]}
      - {canonical_search: ["deck:A"]}
      - queue
      - deck_tree
      - render(#c1)
      - db_dump
      - {db_dump: {tables: [notes, cards]}}
      - {stats: [{scope: collection, history: year, parts: [context]}]}
      - digest
"##;
    let value = scenario::parse_yaml(text).unwrap();
    let parsed = scenario::Scenario::from_value(&value).unwrap();
    assert_eq!(parsed.steps.len(), 20);
    assert!(matches!(parsed.steps[0], Step::Advance(d) if d.seconds() == 93_600));
    let needs = parsed.needs();
    assert_eq!(
        needs.setup.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "cards",
            "config",
            "decks",
            "new_collection",
            "notes",
            "notetypes",
            "presets",
            "revlog"
        ]
    );
    assert!(needs.ops.contains("build_filtered") && needs.ops.contains("reopen"));
    assert!(!needs.ops.contains("observe"));
    assert!(matches!(parsed.steps[17], Step::Reopen(_)));
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
        // Actor ops and the server need actors (sync-ops.md S1).
        "steps: [{sync: {kind: login}}]",
        "steps: [{actor: client_a}]",
        "steps: [{net_fault: {kind: clear}}]",
        "steps: [{converge: {}}]",
        "server: {clock_offset_secs: 301}",
        "steps: [{reopen: {now: true}}]",
        "steps: [{reopen: null}]",
    ] {
        let text = format!("id: area/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{bad}\n");
        let value = scenario::parse_yaml(&text).unwrap();
        assert!(scenario::Scenario::from_value(&value).is_err(), "accepted {bad}");
    }
}

/// The schema's required draft-op arguments (scenario.schema.json
/// `$defs/step`), read from the schema itself.
fn schema_draft_requirements() -> Vec<(String, Vec<String>, Vec<String>)> {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(conformance_dir().join("schema/scenario.schema.json")).unwrap())
            .unwrap();
    let mut out = Vec::new();
    for (op, def) in schema["$defs"]["step"]["properties"].as_object().unwrap() {
        // A fixed form may leave the other forms draft (`import.colpkg`).
        let mut def = def;
        while let Some(fallback) = def.get("else") {
            def = fallback;
        }
        let parts: Vec<&Value> = match def.get("allOf") {
            Some(all) => all.as_array().unwrap().iter().collect(),
            None if def.get("$ref") == Some(&json!("#/$defs/draft_args")) => vec![def],
            None => continue,
        };
        if !parts
            .iter()
            .any(|p| p.get("$ref") == Some(&json!("#/$defs/draft_args")))
        {
            continue;
        }
        let required: Vec<String> = parts
            .iter()
            .filter_map(|p| p.get("required"))
            .flat_map(|r| r.as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_owned()))
            .collect();
        let typed: Vec<String> = parts
            .iter()
            .filter_map(|p| p.get("properties"))
            .flat_map(|props| props.as_object().unwrap().keys().cloned())
            .collect();
        out.push((op.clone(), required, typed));
    }
    out
}

/// A valid argument mapping for each draft op.
fn valid_draft_args(op: &str) -> Value {
    match op {
        "reposition" => json!({"cards": ["#c1"], "start": 5}),
        "update_preset" => json!({"name": "Default"}),
        "build_filtered" => json!({"name": "F", "search": "deck:A"}),
        "import" => json!({"fixture": "package/x.apkg"}),
        "export" => {
            json!({"format": "apkg", "name": "out.apkg", "limit": "collection", "scheduling": true, "presets": true, "media": false})
        }
        other => panic!("no valid example for draft op {other}: add one with the schema change"),
    }
}

fn model_accepts(step: Value) -> Result<(), String> {
    let value = json!({"id": "area/x", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": [step]});
    scenario::Scenario::from_value(&value)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn observation_text_rejects_empty_list_entries_at_the_format_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.yaml");
    for request in [
        "card(#c1,,#c2)",
        "card(#c1,)",
        "card(,#c1)",
        "card(,)",
        "card(#c1, ,#c2)",
        "card(#c1, \t)",
        "card( \t,#c1)",
        "card( , \t )",
        "ids(#n1,,#c2)",
        "timing(,)",
        "queue(,)",
    ] {
        assert!(
            model_accepts(json!({"observe": [request]})).is_err(),
            "accepted {request:?}"
        );
        let quoted = serde_json::to_string(request).unwrap();
        let text = format!(
            "id: area/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{observe: [{quoted}]}}]\n"
        );
        fs::write(&path, text).unwrap();
        // D10: loading fails before a golden or an executor is consulted.
        assert!(scenario::load(&path).is_err(), "loaded {request:?}");
    }
}

#[test]
fn observation_text_preserves_empty_arguments_refs_and_structured_syntax() {
    use meguri_testkit::scenario::{ObservationArgs, ObservationKind};

    let refs = ObservationArgs::Refs(vec!["#c2".parse().unwrap(), "#c1".parse().unwrap()]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.json");
    for (request, kind, args) in [
        (json!("card"), ObservationKind::Card, ObservationArgs::None),
        (json!("card()"), ObservationKind::Card, ObservationArgs::None),
        (json!("card( \t )"), ObservationKind::Card, ObservationArgs::None),
        (json!("ids( )"), ObservationKind::Ids, ObservationArgs::None),
        (json!("timing()"), ObservationKind::Timing, ObservationArgs::None),
        (json!("queue( \t)"), ObservationKind::Queue, ObservationArgs::None),
        (json!(" \tcard( #c2 ,\t#c1 ) \t"), ObservationKind::Card, refs.clone()),
        (json!({"card": ["#c2", "#c1"]}), ObservationKind::Card, refs),
        (json!("deck_counts"), ObservationKind::DeckCounts, ObservationArgs::None),
        (json!("congrats"), ObservationKind::Congrats, ObservationArgs::None),
        (
            json!({"search": [{"query": "deck:A"}]}),
            ObservationKind::Search,
            ObservationArgs::List(vec![json!({"query": "deck:A"})]),
        ),
    ] {
        let value = json!({"id": "area/x", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": [{"observe": [request.clone()]}]});
        let parsed = scenario::Scenario::from_value(&value).unwrap_or_else(|e| panic!("{request}: {e}"));
        let Step::Observe(requests) = &parsed.steps[0] else {
            panic!("not an observe step")
        };
        assert_eq!(requests[0].kind, kind, "{request}");
        assert_eq!(requests[0].args, args, "{request}");
        fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
        let loaded = scenario::load(&path).unwrap_or_else(|e| panic!("{request}: {e}"));
        let Step::Observe(loaded_requests) = &loaded.scenario.steps[0] else {
            panic!("not an observe step")
        };
        assert_eq!(loaded_requests, requests, "{request}");
    }
    // Required mapping/list arguments still go through checked-kind validation.
    for request in ["text()", "text( \t )", "search()", "search( \t )"] {
        assert!(
            model_accepts(json!({"observe": [request]})).is_err(),
            "accepted {request:?}"
        );
    }
}

#[test]
fn the_model_requires_what_the_schema_requires_of_draft_ops() {
    let from_schema = schema_draft_requirements();
    // Five draft ops: `sync` left them when sync-ops.md fixed its shape,
    // `edit_note`, `delete_notes` and `db_exec` with record-ops.md W5,
    // `check_db` with collection-check.md K10, and `bury`, `suspend`,
    // `forget` and `set_due` with sched-ops.md J2-J7.
    assert!(from_schema.len() >= 5, "{from_schema:?}");
    for exact in [
        "edit_note",
        "delete_notes",
        "db_exec",
        "set_config",
        "check_db",
        "bury",
        "suspend",
        "forget",
        "set_due",
    ] {
        assert!(
            !from_schema.iter().any(|(op, _, _)| op == exact),
            "{exact} is not a draft op"
        );
    }
    assert!(!from_schema.iter().any(|(op, _, _)| op == "sync"), "{from_schema:?}");
    // K10: `check_db` takes exactly `{}` (the schema's maxProperties 0).
    assert_eq!(model_accepts(json!({"check_db": {}})), Ok(()));
    assert!(model_accepts(json!({"check_db": {"repair": true}})).is_err());
    assert!(!scenario::draft_requirements().iter().any(|(op, _)| *op == "sync"));
    let in_model: Vec<(String, Vec<String>)> = scenario::draft_requirements()
        .into_iter()
        .map(|(op, keys)| (op.to_owned(), keys.into_iter().map(str::to_owned).collect()))
        .collect();
    for (op, required, typed) in &from_schema {
        let model = in_model.iter().find(|(o, _)| o == op);
        assert_eq!(model.map(|(_, r)| r), Some(required), "required arguments of {op}");
        let valid = valid_draft_args(op);
        assert_eq!(model_accepts(json!({op.clone(): valid.clone()})), Ok(()), "{op}");
        for key in required {
            let mut without = valid.clone();
            without.as_object_mut().unwrap().remove(key);
            assert!(
                model_accepts(json!({op.clone(): without})).is_err(),
                "{op} without {key}"
            );
        }
        // Every argument the schema types refuses a boolean.
        for key in typed {
            let mut wrong = valid.clone();
            wrong[key.as_str()] = json!(true);
            assert!(model_accepts(json!({op.clone(): wrong})).is_err(), "{op}.{key}: true");
        }
    }
}

/// The ops of deck-ops.md K2-K10.
const DECK_OPS: [&str; 11] = [
    "add_deck",
    "rename_deck",
    "reparent_decks",
    "delete_decks",
    "set_deck_collapsed",
    "set_current_deck",
    "add_preset",
    "clone_preset",
    "rename_preset",
    "remove_preset",
    "set_deck_preset",
];

/// A valid argument mapping for each deck op, every typed argument given.
fn valid_deck_args(op: &str) -> Value {
    match op {
        "add_deck" => json!({"name": "A::B"}),
        "rename_deck" => json!({"deck": "A", "name": "B"}),
        "reparent_decks" => json!({"decks": ["A", "B"], "parent": ""}),
        "delete_decks" => json!({"decks": ["A"]}),
        "set_deck_collapsed" => json!({"decks": ["A"], "scope": "browser", "collapsed": true}),
        "set_current_deck" => json!({"deck": "A"}),
        "add_preset" => json!({"name": "P"}),
        "clone_preset" | "rename_preset" => json!({"preset": "P", "name": "Q"}),
        "remove_preset" => json!({"preset": "P", "fallback": "Default"}),
        "set_deck_preset" => json!({"deck": "A", "preset": "P", "subtree": true}),
        other => panic!("no valid example for deck op {other}"),
    }
}

#[test]
fn the_model_reads_the_deck_ops_as_the_schema_types_them() {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(conformance_dir().join("schema/scenario.schema.json")).unwrap())
            .unwrap();
    let steps = schema["$defs"]["step"]["properties"].as_object().unwrap();
    // Every op the schema cites deck-ops.md for, and no other.
    let mut cited: Vec<&str> = steps
        .iter()
        .filter(|(_, def)| {
            def["description"]
                .as_str()
                .is_some_and(|d| d.starts_with("deck-ops.md"))
        })
        .map(|(op, _)| op.as_str())
        .collect();
    cited.sort_unstable();
    let mut ours = DECK_OPS.to_vec();
    ours.sort_unstable();
    assert_eq!(cited, ours);
    for op in DECK_OPS {
        assert!(Step::NAMES.contains(&op), "{op} is not in Step::NAMES");
        let def = &steps[op];
        let valid = valid_deck_args(op);
        assert_eq!(model_accepts(json!({op: valid.clone()})), Ok(()), "{op}");
        // A mapping, never the list of its values that serde's derive reads.
        let as_list: Vec<Value> = valid.as_object().unwrap().values().cloned().collect();
        assert!(model_accepts(json!({op: as_list})).is_err(), "{op} as a list");
        assert!(model_accepts(json!({op: null})).is_err(), "{op}: null");
        let mut extra = valid.clone();
        extra["colour"] = json!("red");
        assert!(model_accepts(json!({op: extra})).is_err(), "{op} with an unknown key");
        for key in def["required"].as_array().unwrap() {
            let mut without = valid.clone();
            without.as_object_mut().unwrap().remove(key.as_str().unwrap());
            assert!(model_accepts(json!({op: without})).is_err(), "{op} without {key}");
        }
        // No deck-op argument is nullable or a mapping.
        for key in def["properties"].as_object().unwrap().keys() {
            for wrong_value in [Value::Null, json!({"a": 1})] {
                let mut wrong = valid.clone();
                wrong[key.as_str()] = wrong_value.clone();
                assert!(model_accepts(json!({op: wrong})).is_err(), "{op}.{key}: {wrong_value}");
            }
        }
    }
}

#[test]
fn deck_ops_read_their_arguments_and_load_time_rules() {
    use meguri_testkit::scenario::CollapseScope;

    let text = r##"
id: decks/all-ops
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
setup: {decks: [A, "A::B"], presets: [{name: P}]}
steps:
  - add_deck: {name: " C :: D "}
  - rename_deck: {deck: "A::B", name: B}
  - reparent_decks: {decks: [B, C], parent: ""}
  - delete_decks: {decks: ["C::D"]}
  - set_deck_collapsed: {decks: [A, B], scope: study, collapsed: false}
  - set_current_deck: {deck: A}
  - add_preset: {name: Q}
  - clone_preset: {preset: Q, name: R}
  - rename_preset: {preset: R, name: S}
  - remove_preset: {preset: S, fallback: Default}
  - set_deck_preset: {deck: A, preset: P}
  - set_deck_preset: {deck: A, preset: P, subtree: true}
  - observe: [decks, deck_tree, {decks: null}]
"##;
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    // K1: typed names are passed as given.
    assert!(matches!(&parsed.steps[0], Step::AddDeck(a) if a.name == " C :: D "));
    assert!(matches!(&parsed.steps[1], Step::RenameDeck(a) if a.deck == "A::B" && a.name == "B"));
    assert!(matches!(&parsed.steps[2], Step::ReparentDecks(a) if a.decks == ["B", "C"] && a.parent.is_empty()));
    assert!(matches!(&parsed.steps[3], Step::DeleteDecks(a) if a.decks == ["C::D"]));
    assert!(matches!(
        &parsed.steps[4],
        Step::SetDeckCollapsed(a) if a.decks == ["A", "B"] && a.scope == CollapseScope::Study && !a.collapsed
    ));
    assert!(matches!(&parsed.steps[5], Step::SetCurrentDeck(a) if a.deck == "A"));
    assert!(matches!(&parsed.steps[6], Step::AddPreset(a) if a.name == "Q"));
    assert!(matches!(&parsed.steps[7], Step::ClonePreset(a) if a.preset == "Q" && a.name == "R"));
    assert!(matches!(&parsed.steps[8], Step::RenamePreset(a) if a.preset == "R" && a.name == "S"));
    assert!(matches!(&parsed.steps[9], Step::RemovePreset(a) if a.preset == "S" && a.fallback == "Default"));
    // K10: subtree is false when missing.
    assert!(matches!(&parsed.steps[10], Step::SetDeckPreset(a) if a.deck == "A" && a.preset == "P" && !a.subtree));
    assert!(matches!(&parsed.steps[11], Step::SetDeckPreset(a) if a.subtree));
    let needs = parsed.needs();
    let mut ops = DECK_OPS.to_vec();
    ops.sort_unstable();
    assert_eq!(needs.ops.iter().map(String::as_str).collect::<Vec<_>>(), ops);
    assert_eq!(
        needs.observations.iter().map(String::as_str).collect::<Vec<_>>(),
        ["deck_tree", "decks"]
    );

    for (bad, why) in [
        // The schema's minLength and minItems.
        ("rename_deck: {deck: \"\", name: B}", "deck name"),
        ("set_current_deck: {deck: \"\"}", "deck name"),
        ("set_deck_preset: {deck: \"\", preset: P}", "deck name"),
        ("delete_decks: {decks: []}", "at least one deck"),
        ("reparent_decks: {decks: [], parent: A}", "at least one deck"),
        (
            "set_deck_collapsed: {decks: [], scope: study, collapsed: true}",
            "at least one deck",
        ),
        ("delete_decks: {decks: [A, \"\"]}", "deck name"),
        ("reparent_decks: {decks: [\"\"], parent: A}", "deck name"),
        (
            "set_deck_collapsed: {decks: [\"\"], scope: browser, collapsed: true}",
            "deck name",
        ),
        (
            "set_deck_collapsed: {decks: [A], scope: both, collapsed: true}",
            "unknown variant",
        ),
        (
            "set_deck_collapsed: {decks: [A], scope: study, collapsed: \"yes\"}",
            "invalid type",
        ),
        // K9: the fallback is another preset.
        ("remove_preset: {preset: P, fallback: P}", "differ"),
        ("observe: [\"decks(#c1)\"]", "does not take"),
        ("observe: [{decks: {}}]", "does not take"),
        ("observe: [{deck_tree: {}}]", "does not take"),
    ] {
        let text = format!("id: decks/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{{bad}}}]\n");
        let err = scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
            .expect_err(bad)
            .to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

#[test]
fn the_model_enforces_the_schema_rules_beyond_types() {
    for (bad, why) in [
        ("tags: [a, a]", "appears twice"),
        ("compare: {set: [steps/ids]}", "pointer pattern"),
        ("setup: {decks: [\"\"]}", "deck name"),
        ("setup: {presets: [{name: \"\"}]}", "preset name"),
        (
            "setup: {notetypes: [{name: Q, fields: [], templates: [{name: C}]}]}",
            "field list",
        ),
        (
            "setup: {notetypes: [{name: Q, fields: [F], templates: []}]}",
            "template list",
        ),
        ("steps: [{observe: [{card: null}]}]", "cannot take null"),
        ("steps: [{observe: [{card: []}]}]", "at least one reference"),
        (
            "steps: [{observe: [{fuzz_table: {card: \"#c1\", pairs: [[1]], intervals: [3]}}]}]",
            "pairs",
        ),
        (
            "steps: [{observe: [{fuzz_table: {card: \"#c1\", pairs: [], intervals: []}}]}]",
            "intervals",
        ),
        ("steps: [{observe: [{net_probe: {host: h}}]}]", "net_probe"),
        (
            "steps: [{observe: [{search: [{query: q, kind: decks}]}]}]",
            "cards or notes",
        ),
        // search-ops.md S1, S2: non-empty lists of requests and of texts.
        ("steps: [{observe: [{search: []}]}]", "at least one request"),
        ("steps: [{observe: [{search: {query: q}}]}]", "does not take"),
        ("steps: [{observe: [search]}]", "does not take"),
        ("steps: [{observe: [{search: [{query: 1}]}]}]", "query string"),
        (
            "steps: [{observe: [{search: [{query: q, order: id}]}]}]",
            "observation search: order is a mapping",
        ),
        ("steps: [{observe: [{search: [q]}]}]", "{query, kind?, order?} mapping"),
        (
            "steps: [{observe: [{canonical_search: []}]}]",
            "at least one search text",
        ),
        (
            "steps: [{observe: [{canonical_search: [q, 1]}]}]",
            "every item is a string",
        ),
        ("steps: [{observe: [canonical_search]}]", "does not take"),
        // observations.md O2: a kind whose value is a list, once per step.
        (
            "steps: [{observe: [{search: [{query: a}]}, {search: [{query: b}]}]}]",
            "search is requested more than once",
        ),
        (
            "steps: [{observe: [{canonical_search: [a]}, {canonical_search: [b]}]}]",
            "canonical_search is requested more than once",
        ),
        ("steps: [{observe: [{queue: {deck: 1}}]}]", "does not take"),
        ("steps: [{observe: [{queue: {deck: A}}]}]", "does not take"),
        ("steps: [{observe: [{deck_counts: {}}]}]", "does not take"),
        ("steps: [{observe: [{congrats: []}]}]", "reference"),
        ("steps: [{handoff: elsewhere}]", "unknown variant"),
        ("steps: [{handoff: oracle}, {handoff: oracle}]", "alternate"),
        (
            "steps: [{legacy_update: {deck: A, notetype: B, patch: {}}}]",
            "exactly one",
        ),
        ("steps: [{legacy_update: {patch: {}}}]", "exactly one"),
        ("steps: [{legacy_update: {deck: \"\", patch: {}}}]", "non-empty name"),
        ("steps: [{legacy_update: {deck: A}}]", "patch"),
        ("steps: [{legacy_update: {deck: A, patch: 5}}]", "invalid type"),
        ("steps: [{legacy_update: {deck: A, patch: {}, x: 1}}]", "unknown field"),
        ("steps: [{observe: [{legacy_json: {}}]}]", "legacy_json"),
        ("steps: [{observe: [{legacy_json: {decks: []}}]}]", "legacy_json"),
        ("steps: [{observe: [{legacy_json: {decks: [\"\"]}}]}]", "legacy_json"),
        ("steps: [{observe: [{legacy_json: {tags: [a]}}]}]", "legacy_json"),
        ("steps: [{observe: [legacy_json]}]", "does not take"),
        ("steps: [{observe: [{blob_decode: {}}]}]", "blob_decode"),
        ("steps: [{observe: [{blob_decode: {presets: []}}]}]", "blob_decode"),
        (
            "steps: [{observe: [{blob_decode: {notetypes: [\"\"]}}]}]",
            "blob_decode",
        ),
        ("steps: [{observe: [{blob_decode: {cards: [a]}}]}]", "blob_decode"),
        ("steps: [{observe: [{blob_decode: null}]}]", "cannot take null"),
        ("steps: [{observe: [blob_decode]}]", "does not take"),
        // model-ops.md L6: the legacy form of export is exact.
        (
            "steps: [{export: {format: colpkg, name: x.apkg, legacy: true}}]",
            "legacy export",
        ),
        (
            "steps: [{export: {format: apkg, name: x.apkg, legacy: false}}]",
            "legacy export",
        ),
        (
            "steps: [{export: {format: apkg, name: x.apkg, legacy: true, media: true}}]",
            "legacy export",
        ),
        (
            "steps: [{export: {format: apkg, name: x.colpkg, legacy: true}}]",
            "package file name",
        ),
        (
            "steps: [{export: {format: apkg, name: ../x.apkg, legacy: true}}]",
            "package file name",
        ),
        (
            "steps: [{export: {format: apkg, name: _x.apkg, legacy: true}}]",
            "package file name",
        ),
        (
            "steps: [{export: {format: apkg, name: a b.apkg, legacy: true}}]",
            "package file name",
        ),
        (
            "steps: [{export: {format: apkg, name: .apkg, legacy: true}}]",
            "package file name",
        ),
        // model-ops.md L7.
        ("steps: [{observe: [{package_col: {name: x.apkg}}]}]", "package_col"),
        ("steps: [{observe: [{package_col: {columns: [ver]}}]}]", "package_col"),
        (
            "steps: [{observe: [{package_col: {name: x.apkg, columns: []}}]}]",
            "package_col",
        ),
        (
            "steps: [{observe: [{package_col: {name: x.apkg, columns: [ver, ver]}}]}]",
            "listed twice",
        ),
        (
            "steps: [{observe: [{package_col: {name: x.apkg, columns: [cards]}}]}]",
            "package_col",
        ),
        (
            "steps: [{observe: [{package_col: {name: x.apkg, columns: ver}}]}]",
            "package_col",
        ),
        (
            "steps: [{observe: [{package_col: {name: x.zip, columns: [ver]}}]}]",
            "package file name",
        ),
        (
            "steps: [{observe: [{package_col: {name: x.apkg, columns: [ver], at: 1}}]}]",
            "package_col",
        ),
        ("steps: [{observe: [{package_col: null}]}]", "cannot take null"),
        ("steps: [{observe: [package_col]}]", "does not take"),
    ] {
        let text = format!("id: area/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{bad}\n");
        let value = scenario::parse_yaml(&text).unwrap();
        let err = scenario::Scenario::from_value(&value).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
    // Null where the schema allows it, and an empty id list (every id).
    let text = "id: area/ok\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{observe: [{timing: null}, {queue: null}, {ids: []}]}]\n";
    assert!(scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).is_ok());
    // model-ops.md L1 and L2.
    let text = "id: area/ok\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{legacy_update: {preset: P, patch: {a: null, new: {b: [1]}}}}, {observe: [{legacy_json: {decks: [\"A::B\"], presets: [P]}}]}]\n";
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    let needs = parsed.needs();
    assert!(needs.ops.contains("legacy_update") && needs.observations.contains("legacy_json"));
    // model-ops.md L4.
    let text = "id: area/ok\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{observe: [{blob_decode: {notetypes: [Pair], decks: [\"A::B\"], presets: [P]}}]}]\n";
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    assert!(parsed.needs().observations.contains("blob_decode"));
    // model-ops.md L6 and L7; other export forms stay draft and open.
    let text = "id: area/ok\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{export: {format: apkg, name: Note_types-2.v1.apkg, legacy: true}}, {export: {format: apkg, name: out.apkg, limit: collection, scheduling: false, presets: true, media: true}}, {observe: [{package_col: {name: Note_types-2.v1.apkg, columns: [ver, conf, models, decks, dconf, tags]}}]}]\n";
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    let needs = parsed.needs();
    assert!(needs.ops.contains("export") && needs.observations.contains("package_col"));
}

/// deck-options-ops.md E1, E2: the op and the observation, with
/// scenario.schema.json's shapes.
#[test]
fn deck_options_ops_follow_the_schema() {
    use meguri_testkit::scenario::{IgnoredBeforeCountRequest, LeechAction, ObservationArgs, ObservationKind};

    let parse = |steps: &str| {
        let text = format!("id: sched/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: {steps}\n");
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    let parsed = parse(
        "[{save_deck_options: {deck: Words}}, \
         {save_deck_options: {deck: Misc, preset: Lang, deck_desired_retention: 0.75}}, \
         {save_deck_options: {deck: Phrases, deck_desired_retention: null, options: {desired_retention: 0.85, \
           fsrs_params: [0.2, 1], learn_steps: [1m, 1d], leech_action: tag_only, historical_retention: 0.8, \
           ignore_reviews_before: \"2025-10-03\"}}}, \
         {observe: [{ignored_before_count: {preset: Default, dates: [\"2026-01-01\", \"\", \" 2026-1-3\"]}}]}]",
    )
    .unwrap();
    let saves: Vec<_> = parsed
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::SaveDeckOptions(args) => Some(args),
            _ => None,
        })
        .collect();
    assert_eq!(saves.len(), 3);
    assert_eq!(
        (
            saves[0].deck.as_str(),
            saves[0].preset.as_deref(),
            saves[0].deck_desired_retention
        ),
        ("Words", None, None)
    );
    assert!(saves[0].options.is_none());
    assert_eq!(
        (saves[1].preset.as_deref(), saves[1].deck_desired_retention),
        (Some("Lang"), Some(Some(0.75)))
    );
    assert_eq!(saves[2].deck_desired_retention, Some(None));
    let options = saves[2].options.as_ref().unwrap();
    assert_eq!(options.desired_retention, Some(0.85));
    assert_eq!(options.fsrs_params.as_deref(), Some(&[0.2, 1.0][..]));
    assert_eq!(
        options
            .learn_steps
            .as_ref()
            .unwrap()
            .iter()
            .map(|d| d.seconds())
            .collect::<Vec<_>>(),
        [60, 86_400]
    );
    assert_eq!(options.leech_action, Some(LeechAction::TagOnly));
    assert_eq!(options.historical_retention, Some(0.8));
    assert_eq!(options.ignore_reviews_before.as_deref(), Some("2025-10-03"));
    assert_eq!(options.new_per_day, None);
    let Step::Observe(requests) = &parsed.steps[3] else {
        panic!("not an observe step")
    };
    assert_eq!(requests[0].kind, ObservationKind::IgnoredBeforeCount);
    let ObservationArgs::Mapping(map) = &requests[0].args else {
        panic!("not a mapping")
    };
    assert_eq!(
        IgnoredBeforeCountRequest::from_mapping(map).unwrap(),
        IgnoredBeforeCountRequest {
            preset: "Default".to_owned(),
            dates: vec!["2026-01-01".to_owned(), String::new(), " 2026-1-3".to_owned()],
        }
    );
    let needs = parsed.needs();
    assert!(needs.ops.contains("save_deck_options"));
    assert!(needs.observations.contains("ignored_before_count"));
    for (bad, why) in [
        ("[{save_deck_options: {}}]", "deck"),
        ("[{save_deck_options: {deck: \"\"}}]", "non-empty deck"),
        ("[{save_deck_options: {deck: A, preset: \"\"}}]", "non-empty name"),
        ("[{save_deck_options: {deck: A, preset: null}}]", "invalid type"),
        (
            "[{save_deck_options: {deck: A, options: null}}]",
            "an op argument takes a mapping, found null",
        ),
        (
            "[{save_deck_options: {deck: A, options: 7}}]",
            "an op argument takes a mapping, found 7",
        ),
        ("[{save_deck_options: {deck: A, colour: red}}]", "unknown field"),
        ("[{save_deck_options: {deck: A, options: {name: P}}}]", "unknown field"),
        (
            "[{save_deck_options: {deck: A, options: {new_per_day: \"7\"}}}]",
            "invalid type",
        ),
        (
            "[{save_deck_options: {deck: A, options: {ignore_reviews_before: 20251003}}}]",
            "invalid type",
        ),
        (
            "[{save_deck_options: {deck: A, deck_desired_retention: \"0.8\"}}]",
            "invalid type",
        ),
        (
            "[{save_deck_options: A}]",
            "an op argument takes a mapping, found \"A\"",
        ),
        ("[{observe: [ignored_before_count]}]", "does not take"),
        (
            "[{observe: [{ignored_before_count: {preset: P}}]}]",
            "takes {preset, dates}",
        ),
        (
            "[{observe: [{ignored_before_count: {preset: P, dates: []}}]}]",
            "non-empty list",
        ),
        (
            "[{observe: [{ignored_before_count: {preset: P, dates: [1]}}]}]",
            "non-empty list",
        ),
        (
            "[{observe: [{ignored_before_count: {preset: \"\", dates: [x]}}]}]",
            "non-empty preset",
        ),
        (
            "[{observe: [{ignored_before_count: {preset: P, dates: [x], deck: A}}]}]",
            "takes {preset, dates}",
        ),
        (
            "[{observe: [{ignored_before_count: {preset: P, dates: [x]}}, {ignored_before_count: {preset: Q, dates: [y]}}]}]",
            "more than once",
        ),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

/// undo-ops.md V1-V3: the ops and the observation, with
/// scenario.schema.json's shapes.
#[test]
fn undo_ops_follow_the_schema() {
    let parse = |steps: &str| {
        let text = format!("id: undo/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: {steps}\n");
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    let parsed = parse(
        "[{undo: {}}, {redo: {}}, {remove_config: {key: k}}, {set_config: {key: k, value: 1, undoable: true}}, \
         {set_config: {key: k, value: 1, undoable: false}}, {set_config: {key: k, value: 1}}, \
         {observe: [undo_status, {undo_status: null}]}]",
    )
    .unwrap();
    let undoable: Vec<bool> = parsed
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::SetConfig(args) => Some(args.undoable),
            _ => None,
        })
        .collect();
    assert_eq!(undoable, [true, false, false]);
    assert!(matches!(&parsed.steps[2], Step::RemoveConfig(args) if args.key == "k"));
    let needs = parsed.needs();
    for op in ["undo", "redo", "remove_config", "set_config"] {
        assert!(needs.ops.contains(op), "{op}");
    }
    assert!(needs.observations.contains("undo_status"));
    for (bad, why) in [
        ("[{undo: {steps: 1}}]", "unknown key"),
        ("[{undo: null}]", "takes a mapping"),
        ("[{redo: []}]", "takes a mapping"),
        ("[{remove_config: {}}]", "key"),
        ("[{remove_config: {key: \"\"}}]", "non-empty key"),
        ("[{remove_config: {key: k, undoable: true}}]", "unknown field"),
        ("[{remove_config: k}]", "invalid type"),
        ("[{set_config: {key: k, value: 1, undoable: null}}]", "invalid type"),
        ("[{set_config: {key: k, value: 1, undoable: 1}}]", "invalid type"),
        ("[{set_config: {key: k, value: 1, undoable: \"true\"}}]", "invalid type"),
        ("[{observe: [\"undo_status(#c1)\"]}]", "does not take"),
        ("[{observe: [{undo_status: {}}]}]", "undo_status"),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

/// tag-ops.md G1-G9: the ops and the observation, with
/// scenario.schema.json's shapes.
#[test]
fn tag_ops_follow_the_schema() {
    let parse = |steps: &str| {
        let text = format!("id: tags/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: {steps}\n");
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    let parsed = parse(
        "[{add_note_tags: {notes: [\"#n1\", \"#n2\"], tags: \"a b\"}}, {remove_note_tags: {notes: [\"#n1\"], tags: \"\"}}, \
         {delete_tags: {tags: \"x y\"}}, {rename_tag: {from: \"C++\", to: \"a b\"}}, \
         {reparent_tags: {tags: [a, \"b::c\"], parent: \"\"}}, {clear_unused_tags: {}}, \
         {set_tag_collapsed: {tags: [a], collapsed: false}}, {set_flag: {cards: [\"#c1\"], flag: 8}}, \
         {observe: [tag_tree, {tag_tree: null}]}]",
    )
    .unwrap();
    assert!(matches!(&parsed.steps[0], Step::AddNoteTags(args) if args.notes.len() == 2 && args.tags == "a b"));
    assert!(matches!(&parsed.steps[3], Step::RenameTag(args) if args.from == "C++" && args.to == "a b"));
    assert!(matches!(&parsed.steps[4], Step::ReparentTags(args) if args.parent.is_empty() && args.tags.len() == 2));
    assert!(matches!(&parsed.steps[6], Step::SetTagCollapsed(args) if !args.collapsed));
    assert!(matches!(&parsed.steps[7], Step::SetFlag(args) if args.flag == 8));
    let needs = parsed.needs();
    for op in [
        "add_note_tags",
        "remove_note_tags",
        "delete_tags",
        "rename_tag",
        "reparent_tags",
        "clear_unused_tags",
        "set_tag_collapsed",
        "set_flag",
    ] {
        assert!(needs.ops.contains(op), "{op}");
        assert!(Step::NAMES.contains(&op), "{op}");
    }
    assert!(needs.observations.contains("tag_tree"));
    assert!(parsed.starts_from_new_collection(), "tag_tree reads the collection");
    for (bad, why) in [
        ("[{add_note_tags: {notes: [], tags: a}}]", "at least one note"),
        (
            "[{remove_note_tags: {notes: [\"#c1\"], tags: a}}]",
            "not a note reference",
        ),
        ("[{add_note_tags: {notes: [\"#n1\"]}}]", "tags"),
        ("[{add_note_tags: {notes: [\"#n1\"], tags: 1}}]", "invalid type"),
        ("[{delete_tags: {tags: [a]}}]", "invalid type"),
        ("[{rename_tag: {from: \"\", to: b}}]", "non-empty from"),
        ("[{rename_tag: {from: \"a b\", to: b}}]", "non-empty from"),
        ("[{rename_tag: {from: \"a\u{3000}b\", to: b}}]", "non-empty from"),
        ("[{rename_tag: {from: a}}]", "to"),
        ("[{reparent_tags: {tags: [], parent: p}}]", "at least one tag"),
        ("[{reparent_tags: {tags: [\"\"], parent: p}}]", "non-empty tag names"),
        ("[{reparent_tags: {tags: [a]}}]", "parent"),
        ("[{clear_unused_tags: {all: true}}]", "unknown key"),
        ("[{set_tag_collapsed: {tags: [], collapsed: true}}]", "at least one tag"),
        ("[{set_tag_collapsed: {tags: [a], collapsed: 1}}]", "invalid type"),
        ("[{set_flag: {cards: [], flag: 1}}]", "at least one card"),
        ("[{set_flag: {cards: [\"#c1\"], flag: -1}}]", "invalid value"),
        ("[{set_flag: {cards: [\"#n1\"], flag: 1}}]", "not a card reference"),
        ("[{observe: [{tag_tree: {}}]}]", "tag_tree"),
        ("[{observe: [\"tag_tree(#c1)\"]}]", "does not take"),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

/// browser-config-ops.md B1, B2: the op with scenario.schema.json's key
/// list and value shapes, and the observation.
#[test]
fn browser_config_ops_follow_the_schema() {
    let parse = |steps: &str| {
        let text = format!("id: browser/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: {steps}\n");
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    let parsed = parse(
        "[{browser_config_set: {key: activeCols, value: [question, \"\", addonX]}}, \
         {browser_config_set: {key: sortType, value: myAddonColumn}}, \
         {browser_config_set: {key: collapseTags, value: true}}, \
         {browser_config_set: {key: savedFilters, value: {a: \"is:due\"}}}, \
         {browser_config_set: {key: flagLabels, value: {}}}, \
         {observe: [browser_config, {browser_config: null}]}]",
    )
    .unwrap();
    assert!(
        matches!(&parsed.steps[0], Step::BrowserConfigSet(args) if args.key == "activeCols"
        && args.value == serde_json::json!(["question", "", "addonX"]))
    );
    let needs = parsed.needs();
    assert!(needs.ops.contains("browser_config_set"));
    assert!(Step::NAMES.contains(&"browser_config_set"));
    assert!(needs.observations.contains("browser_config"));
    assert!(
        parsed.starts_from_new_collection(),
        "browser_config reads the collection"
    );
    for (bad, why) in [
        (
            "[{browser_config_set: {key: normalize_note_text, value: false}}]",
            "22 browser config keys",
        ),
        (
            "[{browser_config_set: {key: curDeck, value: 1}}]",
            "22 browser config keys",
        ),
        (
            "[{browser_config_set: {key: ActiveCols, value: []}}]",
            "22 browser config keys",
        ),
        (
            "[{browser_config_set: {key: collapseTags, value: 1}}]",
            "takes a boolean",
        ),
        (
            "[{browser_config_set: {key: setDueBrowser, value: 5}}]",
            "takes a string",
        ),
        ("[{browser_config_set: {key: sortType, value: null}}]", "takes a string"),
        (
            "[{browser_config_set: {key: activeCols, value: [noteFld, 5]}}]",
            "takes an array of strings",
        ),
        (
            "[{browser_config_set: {key: activeCols, value: noteFld}}]",
            "takes an array of strings",
        ),
        (
            "[{browser_config_set: {key: savedFilters, value: {a: 3}}}]",
            "takes an object of strings",
        ),
        (
            "[{browser_config_set: {key: flagLabels, value: []}}]",
            "takes an object of strings",
        ),
        ("[{browser_config_set: {key: flagLabels}}]", "value"),
        (
            "[{browser_config_set: {key: flagLabels, value: {}, undoable: true}}]",
            "unknown field",
        ),
        ("[{observe: [{browser_config: {}}]}]", "browser_config"),
        ("[{observe: [\"browser_config(#c1)\"]}]", "does not take"),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

#[test]
fn text_requests_follow_the_schema() {
    let parse = |request: &str| {
        let text = format!(
            "id: text/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{observe: [{request}]}}]\n"
        );
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    for (bad, why) in [
        ("text", "does not take these arguments"),
        ("{text: null}", "cannot take null"),
        ("{text: {fns: [checksum]}}", "exactly {fns, inputs}"),
        (
            "{text: {fns: [checksum], inputs: [], extra: 1}}",
            "exactly {fns, inputs}",
        ),
        ("{text: {fns: [], inputs: []}}", "at least one function"),
        ("{text: {fns: checksum, inputs: []}}", "list of function names"),
        ("{text: {fns: [strip_html], inputs: []}}", "not a text function"),
        ("{text: {fns: [checksum, checksum], inputs: []}}", "appears twice"),
        ("{text: {fns: [checksum], inputs: x}}", "inputs is a list"),
        ("{text: {fns: [checksum], inputs: [[a, b]]}}", "inputs are strings"),
        ("{text: {fns: [checksum], inputs: [1]}}", "inputs are strings"),
        (
            "{text: {fns: [unicase_compare, checksum], inputs: []}}",
            "shares a request with no other function",
        ),
        ("{text: {fns: [unicase_compare], inputs: [a]}}", "pairs of strings"),
        ("{text: {fns: [unicase_compare], inputs: [[a]]}}", "pairs of strings"),
        (
            "{text: {fns: [unicase_compare], inputs: [[a, b, c]]}}",
            "pairs of strings",
        ),
        ("{text: {fns: [unicase_compare], inputs: [[a, 1]]}}", "pairs of strings"),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
    for good in [
        "{text: {fns: [checksum, sort_field], inputs: [a, \"\"]}}",
        "{text: {fns: [media_refs], inputs: []}}",
        "{text: {fns: [unicase_compare], inputs: [[a, B], [\"\", \"\"]]}}",
    ] {
        let parsed = parse(good).unwrap_or_else(|e| panic!("{good}: {e}"));
        assert!(parsed.needs().observations.contains("text"));
    }
}

/// note-ops.md N4: the request shape of scenario.schema.json's
/// `fields_check_request`, read into the typed request both executors use.
#[test]
fn fields_check_requests_follow_the_schema() {
    use meguri_testkit::scenario::{Fields, FieldsCheckCase, FieldsCheckRequest, ObservationArgs, ObservationKind};

    let parse = |request: &str| {
        let text = format!(
            "id: notes/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{observe: [{request}]}}]\n"
        );
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    let parsed = parse(
        "{fields_check: {cases: [{notetype: Std, fields: [x, \"\"]}, {notetype: Std, fields: {Back: y}}, \
         {note: \"#n2\"}, {note: \"#n1\", fields: [a]}, {note: \"#n1\", fields: {Back: b}}]}}",
    )
    .unwrap();
    assert!(parsed.needs().observations.contains("fields_check"));
    let Step::Observe(requests) = &parsed.steps[0] else {
        panic!("not an observe step")
    };
    assert_eq!(requests[0].kind, ObservationKind::FieldsCheck);
    let ObservationArgs::Mapping(map) = &requests[0].args else {
        panic!("not a mapping")
    };
    let request = FieldsCheckRequest::from_mapping(map).unwrap();
    let note = |n: u32| scenario::NoteRef(format!("#n{n}").parse().unwrap());
    let map_of = |name: &str, value: &str| Fields::Map([(name.to_owned(), value.to_owned())].into());
    assert_eq!(
        request.cases,
        [
            FieldsCheckCase::NewNote {
                notetype: "Std".to_owned(),
                fields: Fields::List(vec!["x".to_owned(), String::new()]),
            },
            FieldsCheckCase::NewNote {
                notetype: "Std".to_owned(),
                fields: map_of("Back", "y"),
            },
            FieldsCheckCase::StoredNote {
                note: note(2),
                fields: None,
            },
            FieldsCheckCase::StoredNote {
                note: note(1),
                fields: Some(Fields::List(vec!["a".to_owned()])),
            },
            FieldsCheckCase::StoredNote {
                note: note(1),
                fields: Some(map_of("Back", "b")),
            },
        ]
    );
    for (bad, why) in [
        ("fields_check", "does not take these arguments"),
        ("{fields_check: null}", "cannot take null"),
        ("{fields_check: [\"#n1\"]}", "does not take these arguments"),
        ("{fields_check: {}}", "cases is a non-empty list"),
        ("{fields_check: {cases: []}}", "cases is a non-empty list"),
        (
            "{fields_check: {cases: {notetype: Std, fields: [x]}}}",
            "cases is a non-empty list",
        ),
        (
            "{fields_check: {cases: [{notetype: Std, fields: [x]}], extra: 1}}",
            "takes {cases}",
        ),
        ("{fields_check: {cases: [Std]}}", "must be a mapping"),
        ("{fields_check: {cases: [{notetype: Std}]}}", "needs fields"),
        ("{fields_check: {cases: [{fields: [x]}]}}", "names a notetype or a note"),
        (
            "{fields_check: {cases: [{notetype: \"\", fields: [x]}]}}",
            "non-empty note type name",
        ),
        (
            "{fields_check: {cases: [{notetype: 1, fields: [x]}]}}",
            "non-empty note type name",
        ),
        (
            "{fields_check: {cases: [{notetype: Std, fields: [x], tags: []}]}}",
            "not \"tags\"",
        ),
        (
            "{fields_check: {cases: [{notetype: Std, note: \"#n1\", fields: [x]}]}}",
            "not \"notetype\"",
        ),
        ("{fields_check: {cases: [{note: \"#c1\"}]}}", "not a note reference"),
        ("{fields_check: {cases: [{note: \"#n0\"}]}}", "#n0"),
        ("{fields_check: {cases: [{note: 1}]}}", "#nN reference"),
        ("{fields_check: {cases: [{note: \"#n1\", deck: A}]}}", "not \"deck\""),
        (
            "{fields_check: {cases: [{notetype: Std, fields: [1]}]}}",
            "field values are strings",
        ),
        (
            "{fields_check: {cases: [{notetype: Std, fields: [null]}]}}",
            "field values are strings",
        ),
        (
            "{fields_check: {cases: [{notetype: Std, fields: {Front: true}}]}}",
            "field values are strings",
        ),
        (
            "{fields_check: {cases: [{notetype: Std, fields: x}]}}",
            "list of values or a mapping",
        ),
        (
            "{fields_check: {cases: [{note: \"#n1\", fields: null}]}}",
            "list of values or a mapping",
        ),
        (
            "{fields_check: {cases: [{note: \"#n1\"}]}}, {fields_check: {cases: [{note: \"#n2\"}]}}",
            "more than once in one step",
        ),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
    // The two scenarios of specs/notes/field-check.md K6 load.
    for id in ["notes/fields-check/states", "notes/fields-check/duplicates"] {
        let loaded = scenario::load(&conformance_dir().join(format!("scenarios/{id}.yaml")))
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(loaded.scenario.needs().observations.contains("fields_check"), "{id}");
    }
}

#[test]
fn the_text_functions_are_the_schemas() {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string(conformance_dir().join("schema/scenario.schema.json")).unwrap())
            .unwrap();
    let request = schema["$defs"]["observation_request"]["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|branch| branch.get("properties"))
        .expect("observation request has an object alternative");
    let from_schema: Vec<&str> = request["text"]["properties"]["fns"]["items"]["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("no text fns enum in {request}"))
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let from_model: Vec<&str> = scenario::TextFunction::ALL.iter().map(|f| f.name()).collect();
    assert_eq!(from_model, from_schema);
    let kinds: Vec<&str> = request.as_object().unwrap().keys().map(String::as_str).collect();
    for kind in scenario::ObservationKind::ALL {
        assert!(kinds.contains(&kind.name()), "{} is not in the schema", kind.name());
    }
}

#[test]
fn handoffs_split_a_live_scenario_into_alternating_segments() {
    use meguri_testkit::scenario::{Segment, Side};

    let text = r##"
id: preservation/filtered-round-trip
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
setup:
  decks: [Words]
  notetypes: [{name: Pair, fields: [Front, Back], templates: [{name: F, front: "{{Front}}"}]}]
  notes: [{notetype: Pair, deck: Words, fields: [a, b]}]
steps:
  - build_filtered: {name: Filtered, search: "deck:Words"}
  - handoff: meguri
  - edit_note: {note: "#n1", fields: [c]}
  - observe: ["card(#c1)"]
  - handoff: oracle
  - observe: ["card(#c1)", deck_tree]
"##;
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    assert!(parsed.is_live());
    assert_eq!(
        parsed.segments(),
        [
            Segment {
                side: Side::Oracle,
                steps: 0..1,
                handoff: Some(1)
            },
            Segment {
                side: Side::Meguri,
                steps: 2..4,
                handoff: Some(4)
            },
            Segment {
                side: Side::Oracle,
                steps: 5..6,
                handoff: None
            },
        ]
    );
    // Meguri runs no setup and no filtered-deck op here: only its segment.
    let meguri = parsed.needs_on(Side::Meguri);
    assert!(meguri.setup.is_empty());
    assert_eq!(
        meguri.ops.iter().map(String::as_str).collect::<Vec<_>>(),
        ["edit_note", "handoff"]
    );
    assert_eq!(
        meguri.observations.iter().map(String::as_str).collect::<Vec<_>>(),
        ["card"]
    );
    assert!(parsed.needs().ops.contains("build_filtered"));

    let plain =
        scenario::parse_yaml("id: a/b\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{advance: 1d}]\n")
            .unwrap();
    let plain = scenario::Scenario::from_value(&plain).unwrap();
    assert!(!plain.is_live());
    assert_eq!(plain.segments().len(), 1);
}

const ACTORS: &str = r##"
id: sync/all-actor-ops
clock: {tz: UTC, start: "2030-01-15T10:00:00", mode: step}
actors: [client_a, meguri, client_c]
server: {clock_offset_secs: -301}
setup:
  decks: [Words]
steps:
  - advance: 1m
  - sync: {kind: login}
  - sync: {kind: normal}
  - sync: {kind: full_upload}
  - actor: meguri
  - sync: {kind: login}
  - sync: {kind: full_download}
  - net_fault: {kind: reset, direction: to_client}
  - actor: client_c
  - sync: {kind: login}
  - net_fault: {kind: reset, direction: to_server}
  - net_fault: {kind: clear}
  - reopen: {}
  - observe: [{db_dump: {tables: [col, config]}}]
  - converge: {}
"##;

fn actor_scenario(text: &str) -> Result<scenario::Scenario, String> {
    scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).map_err(|e| e.to_string())
}

fn review_session_scenario(steps: Value, actors: Option<Value>) -> Result<scenario::Scenario, scenario::FormatError> {
    let mut value = json!({
        "id": "sync/review-session",
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": steps,
    });
    if let Some(actors) = actors {
        value["actors"] = actors;
    }
    scenario::Scenario::from_value(&value)
}

#[test]
fn review_session_markers_are_local_and_supported_by_the_real_executor() {
    use meguri_testkit::exec::{Executor, MeguriExecutor};
    use meguri_testkit::scenario::ReviewSession;

    let parsed = actor_scenario(
        "id: sync/review-session\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\n\
         steps: [{review_session: begin}, {review_session: end}, {review_session: begin}]\n",
    )
    .unwrap();
    assert!(matches!(parsed.steps[0], Step::ReviewSession(ReviewSession::Begin)));
    assert!(matches!(parsed.steps[1], Step::ReviewSession(ReviewSession::End)));
    assert!(!parsed.steps[0].is_global());
    assert!(Step::NAMES.contains(&"review_session"));
    assert!(parsed.needs().ops.contains("review_session"));
    let executor = MeguriExecutor::new();
    assert!(!executor.missing(&parsed.needs()).ops.contains("review_session"));

    let actors = Some(json!(["client_a", "meguri"]));
    let oracle_only = review_session_scenario(json!([{"review_session": "begin"}]), actors.clone()).unwrap();
    assert!(!oracle_only.needs_of_meguri_actors().ops.contains("review_session"));
    let mixed = review_session_scenario(
        json!([
            {"review_session": "begin"}, {"actor": "meguri"},
            {"review_session": "begin"}, {"actor": "client_a"},
            {"review_session": "end"}, {"actor": "meguri"},
            {"review_session": "end"}, {"review_session": "begin"},
        ]),
        actors,
    )
    .unwrap();
    assert!(mixed.needs_of_meguri_actors().ops.contains("review_session"));
    assert!(
        !executor
            .missing(&mixed.needs_of_meguri_actors())
            .ops
            .contains("review_session")
    );
    assert_eq!(mixed.step_actors()[0].unwrap().as_str(), "client_a");
    assert_eq!(mixed.step_actors()[2].unwrap().as_str(), "meguri");
}

#[test]
fn review_session_requires_exact_strings_and_alternating_markers() {
    for actors in [None, Some(json!(["meguri"]))] {
        for bad in [
            Value::Null,
            json!(true),
            json!(1),
            json!(1.5),
            json!([]),
            json!(["begin"]),
            json!({}),
            json!({"begin": null}),
            json!({"end": null}),
            json!({"kind": "begin"}),
            json!(""),
            json!("Begin"),
            json!("pause"),
        ] {
            assert!(review_session_scenario(json!([{"review_session": bad}]), actors.clone()).is_err());
        }
        for steps in [
            json!([{"review_session": "end"}]),
            json!([{"review_session": "begin"}, {"review_session": "begin"}]),
            json!([{"review_session": "begin"}, {"review_session": "end"}, {"review_session": "end"}]),
        ] {
            let err = review_session_scenario(steps, actors.clone()).unwrap_err().to_string();
            assert!(err.contains("sync-ops.md S13"), "{err}");
        }
        for steps in [
            json!([{"review_session": "begin"}]), // An open EOF is valid.
            json!([{"review_session": "begin"}, {"review_session": "end"}]),
        ] {
            assert!(review_session_scenario(steps, actors.clone()).is_ok());
        }
        for step in [
            json!({"review_session": "begin", "reopen": {}}),
            json!({"review_session": "begin", "extra": true}),
        ] {
            assert!(review_session_scenario(json!([step]), actors.clone()).is_err());
        }
    }
    // Switching actors neither opens nor ends the other's session.
    for steps in [
        json!([{"review_session": "begin"}, {"actor": "meguri"}, {"review_session": "end"}]),
        json!([{"review_session": "begin"}, {"actor": "meguri"}, {"review_session": "begin"},
               {"actor": "client_a"}, {"review_session": "begin"}]),
    ] {
        assert!(review_session_scenario(steps, Some(json!(["client_a", "meguri"]))).is_err());
    }
    // Existing actor and non-actor rules still apply.
    assert!(review_session_scenario(json!([{"review_session": "begin"}, {"actor": "meguri"}]), None).is_err());
    assert!(
        review_session_scenario(
            json!([{"review_session": "begin"}, {"actor": "client_b"}]),
            Some(json!(["meguri"]))
        )
        .is_err()
    );
    assert!(
        review_session_scenario(
            json!([{"review_session": "begin"}, {"sync": {"kind": "normal"}}]),
            Some(json!(["meguri"]))
        )
        .unwrap_err()
        .to_string()
        .contains("login")
    );
}

#[test]
fn open_review_sessions_guard_only_the_current_collections_closing_steps() {
    for close in [
        json!({"reopen": {}}),
        json!({"sync": {"kind": "full_upload"}}),
        json!({"sync": {"kind": "full_download"}}),
    ] {
        let actors = Some(json!(["meguri", "client_a"]));
        let err = review_session_scenario(
            json!([{"sync": {"kind": "login"}}, {"review_session": "begin"}, close.clone()]),
            actors.clone(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("step 2") && err.contains("sync-ops.md S13"), "{err}");
        assert!(
            review_session_scenario(
                json!([{"sync": {"kind": "login"}}, {"review_session": "begin"},
                       {"review_session": "end"}, close.clone()]),
                actors.clone(),
            )
            .is_ok()
        );
        assert!(
            review_session_scenario(
                json!([{"review_session": "begin"}, {"actor": "client_a"},
                       {"sync": {"kind": "login"}}, close.clone()]),
                actors.clone(),
            )
            .is_ok()
        );
        assert!(
            review_session_scenario(
                json!([{"sync": {"kind": "login"}}, {"review_session": "begin"}, {"actor": "client_a"},
                       {"actor": "meguri"}, close]),
                actors,
            )
            .is_err()
        );
    }
    assert!(review_session_scenario(json!([{"review_session": "begin"}, {"reopen": {}}]), None).is_err());
    assert!(
        review_session_scenario(
            json!([{"review_session": "begin"}, {"review_session": "end"}, {"reopen": {}}]),
            None,
        )
        .is_ok()
    );
    assert!(
        review_session_scenario(
            json!([{"review_session": "begin"}, {"sync": {"kind": "login"}},
                   {"sync": {"kind": "normal"}}, {"sync": {"kind": "status"}}, {"advance": "1m"}]),
            Some(json!(["meguri"])),
        )
        .is_ok()
    );
}

#[test]
fn actor_scenarios_read_their_actors_server_and_ops() {
    use meguri_testkit::scenario::{Direction, NetFault, SyncKind};

    let parsed = actor_scenario(ACTORS).unwrap();
    let names: Vec<&str> = parsed.actors().iter().map(|a| a.as_str()).collect();
    assert_eq!(names, ["client_a", "meguri", "client_c"]);
    assert_eq!(
        parsed.actors().iter().map(|a| a.is_meguri()).collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(parsed.server.as_ref().map(|s| s.clock_offset_secs), Some(-301));
    // Actor scenarios are live (dsl.md D9.3): T0 never runs them.
    assert!(parsed.has_actors() && parsed.is_live());
    assert!(matches!(&parsed.steps[1], Step::Sync(s) if s.kind == SyncKind::Login));
    assert!(matches!(&parsed.steps[3], Step::Sync(s) if s.kind == SyncKind::FullUpload));
    assert!(matches!(&parsed.steps[4], Step::Actor(a) if a.as_str() == "meguri"));
    assert!(matches!(
        &parsed.steps[7],
        Step::NetFault(NetFault::Reset(Direction::ToClient))
    ));
    assert!(matches!(
        &parsed.steps[10],
        Step::NetFault(NetFault::Reset(Direction::ToServer))
    ));
    assert!(matches!(&parsed.steps[11], Step::NetFault(NetFault::Clear)));
    assert!(matches!(parsed.steps[12], Step::Reopen(_)));
    assert!(matches!(parsed.steps[14], Step::Converge(_)));
    // Who runs each step: the first actor until an actor step, global steps
    // nobody in particular (sync-ops.md S2.3).
    let by: Vec<Option<&str>> = parsed.step_actors().iter().map(|a| a.map(|a| a.as_str())).collect();
    assert_eq!(
        by,
        [
            None,
            Some("client_a"),
            Some("client_a"),
            Some("client_a"),
            None,
            Some("meguri"),
            Some("meguri"),
            None,
            None,
            Some("client_c"),
            None,
            None,
            Some("client_c"),
            Some("client_c"),
            None,
        ]
    );
    // A server without an offset runs on the scenario clock.
    let plain = actor_scenario(&ACTORS.replace("server: {clock_offset_secs: -301}\n", "server: {}\n")).unwrap();
    assert_eq!(plain.server.as_ref().map(|s| s.clock_offset_secs), Some(0));
    // The language lists the new ops.
    for op in ["actor", "sync", "net_fault", "converge", "reopen"] {
        assert!(Step::NAMES.contains(&op), "{op}");
    }
}

#[test]
fn actor_scenarios_are_checked_when_they_load() {
    let base = |actors: &str, steps: &str| {
        format!("id: sync/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nactors: {actors}\nsteps: {steps}\n")
    };
    for (text, why) in [
        (base("[]", "[]"), "actors"),
        (base("[client_a, client_a]", "[]"), "twice"),
        (base("[meguri_1]", "[]"), "meguri_1"),
        (base("[meguri_02]", "[]"), "meguri_02"),
        (base("[client_aa]", "[]"), "client_aa"),
        (base("[client_A]", "[]"), "client_A"),
        (base("[anki]", "[]"), "anki"),
        // S2.3: an actor step names a declared actor.
        (base("[client_a]", "[{actor: client_b}]"), "client_b"),
        // S4.2: each actor's first sync is a login, in step order.
        (base("[client_a]", "[{sync: {kind: normal}}]"), "login"),
        (
            base(
                "[client_a, client_c]",
                "[{sync: {kind: login}}, {actor: client_c}, {sync: {kind: full_download}}]",
            ),
            "client_c",
        ),
        (base("[client_a]", "[{sync: {kind: upload}}]"), "upload"),
        (base("[client_a]", "[{sync: {kind: login, client: a}}]"), "client"),
        (base("[client_a]", "[{sync: {}}]"), "kind"),
        (base("[client_a]", "[{net_fault: {kind: reset}}]"), "direction"),
        (
            base("[client_a]", "[{net_fault: {kind: reset, direction: sideways}}]"),
            "sideways",
        ),
        (
            base("[client_a]", "[{net_fault: {kind: clear, direction: to_client}}]"),
            "direction",
        ),
        (base("[client_a]", "[{net_fault: {kind: drop}}]"), "drop"),
        (base("[client_a]", "[{converge: {all: true}}]"), "all"),
        (base("[client_a]", "[{handoff: oracle}]"), "handoff"),
        (
            format!("{}server: {{clock_offset_secs: 86401}}\n", base("[client_a]", "[]")),
            "86401",
        ),
        (
            format!("{}server: {{clock_offset_secs: 1.5}}\n", base("[client_a]", "[]")),
            "clock_offset_secs",
        ),
        (format!("{}server: {{skew: 1}}\n", base("[client_a]", "[]")), "skew"),
        (format!("{}server: null\n", base("[client_a]", "[]")), "server"),
        // O8: the tables a db_dump lists.
        (
            base("[client_a]", "[{observe: [{db_dump: {tables: [cards, cards]}}]}]"),
            "twice",
        ),
        (base("[client_a]", "[{observe: [{db_dump: {tables: []}}]}]"), "tables"),
        (
            base("[client_a]", "[{observe: [{db_dump: {tables: [sqlite_master]}}]}]"),
            "sqlite_master",
        ),
        (
            base("[client_a]", "[{observe: [{db_dump: {tables: [cards], raw: 1}}]}]"),
            "raw",
        ),
    ] {
        let err = actor_scenario(&text).expect_err(&text);
        assert!(err.contains(why), "{text}: {err}");
    }
    // A Meguri actor's name: meguri, or meguri_<n> with n >= 2.
    for actors in ["[meguri]", "[meguri_2, client_z]", "[meguri_10]"] {
        assert!(actor_scenario(&base(actors, "[]")).is_ok(), "{actors}");
    }
}

#[test]
fn a_login_says_how_it_derives_its_credentials() {
    use meguri_testkit::scenario::{Credentials, SyncKind};

    // sync-ops.md S4.5: after a login, a second sync step with these
    // arguments; `account` is the default.
    let second = |args: &str| {
        actor_scenario(&format!(
            "id: sync/creds\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nactors: [meguri]\n\
             steps: [{{sync: {{kind: login}}}}, {{sync: {args}}}]\n"
        ))
    };
    for (args, kind, credentials) in [
        ("{kind: login}", SyncKind::Login, Credentials::Account),
        (
            "{kind: login, credentials: account}",
            SyncKind::Login,
            Credentials::Account,
        ),
        (
            "{kind: login, credentials: wrong_password}",
            SyncKind::Login,
            Credentials::WrongPassword,
        ),
        ("{kind: normal}", SyncKind::Normal, Credentials::Account),
        // S4.1: the sync status the client's indicator asks for (U106).
        ("{kind: status}", SyncKind::Status, Credentials::Account),
    ] {
        let parsed = second(args).unwrap_or_else(|e| panic!("{args}: {e}"));
        let Step::Sync(spec) = &parsed.steps[1] else {
            panic!("{args}: not a sync step")
        };
        assert_eq!((spec.kind, spec.credentials), (kind, credentials), "{args}");
    }
    for (args, why) in [
        ("{kind: normal, credentials: account}", "credentials"),
        ("{kind: status, credentials: account}", "credentials"),
        ("{kind: full_upload, credentials: wrong_password}", "credentials"),
        ("{kind: login, credentials: other}", "other"),
        ("{kind: login, credentials: null}", "credentials"),
        ("{kind: login, credentials: 1}", "credentials"),
        ("{kind: login, credentials: [account]}", "credentials"),
        ("{kind: login, password: secret}", "password"),
    ] {
        let err = second(args).expect_err(args);
        assert!(err.contains(why), "{args}: {err}");
    }

    // The committed scenario parses with the wrong password on its first login.
    let loaded = scenario::load(&conformance_dir().join("scenarios/sync/transport/login-wrong-password.yaml")).unwrap();
    assert!(matches!(
        &loaded.scenario.steps[1],
        Step::Sync(s) if s.kind == SyncKind::Login && s.credentials == Credentials::WrongPassword
    ));

    // The committed status scenario parses, its fourth step a status.
    let loaded = scenario::load(&conformance_dir().join("scenarios/sync/meta/status.yaml")).unwrap();
    assert!(matches!(&loaded.scenario.steps[3], Step::Sync(s) if s.kind == SyncKind::Status));
}

#[test]
fn the_wrong_password_is_the_accounts_password_followed_by_wrong() {
    use meguri_testkit::scenario::{Credentials, SyncSpec};

    // sync-ops.md S4.5: every executor derives it the same way.
    assert_eq!(Credentials::Account.password("meguri-test-a"), "meguri-test-a");
    assert_eq!(
        Credentials::WrongPassword.password("meguri-test-a"),
        "meguri-test-a-wrong"
    );
    // S4.4: the closed list of rejections, auth_failed among them.
    assert_eq!(
        SyncSpec::REJECTIONS,
        [
            "auth_failed",
            "clock_incorrect",
            "network",
            "sanity_check_failed",
            "db_check_required"
        ]
    );
}

#[test]
fn meguri_actors_need_what_they_run_and_nothing_else() {
    let path = conformance_dir().join("scenarios/smoke/sync/meguri-downloads.yaml");
    let loaded = scenario::load(&path).unwrap();
    // sync-ops.md S10.2: `actor`; the new collection it starts from (S2.2,
    // dsl.md D8) but no setup section (client_a runs the setup); the ops and
    // observations of meguri's steps; advance (the scenario uses it); db_dump
    // for converge. client_a's full_upload is the oracle's business.
    let needs = loaded.scenario.needs_of_meguri_actors();
    assert_eq!(
        needs.ops.iter().map(String::as_str).collect::<Vec<_>>(),
        ["actor", "advance", "sync"]
    );
    assert_eq!(
        needs.observations.iter().map(String::as_str).collect::<Vec<_>>(),
        ["card", "db_dump"]
    );
    assert_eq!(
        needs.setup.iter().map(String::as_str).collect::<Vec<_>>(),
        ["new_collection"]
    );

    // Without a Meguri actor, nothing.
    let two = scenario::load(&conformance_dir().join("scenarios/smoke/sync/two-party.yaml")).unwrap();
    assert!(two.scenario.needs_of_meguri_actors().is_empty());

    // A Meguri first actor runs the setup; steps of oracle actors and global
    // steps other than the clock add nothing.
    let text = r##"
id: sync/meguri-first
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
actors: [meguri, client_b]
setup: {decks: [Words], notetypes: [{name: P, fields: [F], templates: [{name: T}]}]}
steps:
  - sync: {kind: login}
  - actor: client_b
  - add_note: {notetype: P, fields: [x]}
  - answer: {card: "#c1", rating: good}
  - net_fault: {kind: clear}
  - set_clock: "2030-01-16T00:00:00"
  - actor: meguri
  - reopen: {}
"##;
    let needs = actor_scenario(text).unwrap().needs_of_meguri_actors();
    assert_eq!(
        needs.setup.iter().map(String::as_str).collect::<Vec<_>>(),
        ["decks", "new_collection", "notetypes"]
    );
    assert_eq!(
        needs.ops.iter().map(String::as_str).collect::<Vec<_>>(),
        ["actor", "reopen", "set_clock", "sync"]
    );
    assert!(needs.observations.is_empty());
}

#[test]
fn a_scenario_starts_from_the_new_collection_unless_it_opens_a_fixture_or_reads_none() {
    // dsl.md D8: the new collection is the setup capability `new_collection`.
    let load = |setup: &str, steps: &str| {
        let text = format!("id: smoke/d8\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{setup}steps:\n{steps}");
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap()).unwrap()
    };
    let needs_it = |setup: &str, steps: &str| {
        let parsed = load(setup, steps);
        let needs = parsed.needs().setup.contains(scenario::NEW_COLLECTION);
        assert_eq!(needs, parsed.starts_from_new_collection(), "{steps}");
        needs
    };
    let template = |case: &str| {
        format!(
            "  - observe:\n      - template:\n          notetype: {{name: P, fields: [Front, Back]}}\n          cases: [{case}]\n"
        )
    };
    let new_note = r#"{front: "{{Front}}", back: "{{Back}}", fields: [a, b]}"#;
    // No setup and only steps that touch no collection: nothing.
    for steps in [
        " []\n".to_owned(),
        "  - advance: 1d\n  - set_clock: \"2030-03-31T03:00:00\"\n".to_owned(),
        "  - observe: [{text: {fns: [strip_markup], inputs: [\"<b>a</b>\"]}}]\n".to_owned(),
        "  - observe: [{cloze: {fns: [numbers], inputs: [\"{{c1::a}}\"]}}]\n".to_owned(),
        "  - observe: [{fsrs_next: {param_sets: {default: []}, inputs: [{id: 0, card_id: 1, reps: 1, s: 1.0, d: 5.0, elapsed_days: 1, scheduled_days: 1, dr: 0.9, params: default, max_ivl: 36500}]}}]\n".to_owned(),
        "  - observe: [{canonical_search: [\"deck:a\", \"a or\"]}]\n".to_owned(),
        template(new_note),
    ] {
        assert!(!needs_it("", &steps), "{steps}");
    }
    // A step that reads or writes a collection, or any setup section.
    for steps in [
        "  - observe: [timing]\n".to_owned(),
        "  - db_exec: {sql: \"SELECT 1\"}\n".to_owned(),
        "  - set_config: {key: k, value: 1}\n".to_owned(),
        "  - advance: 1d\n  - observe: [undo_status]\n".to_owned(),
        template(&format!(
            r##"{new_note}, {{card: "#c1", front: "{{{{Front}}}}", back: "x"}}"##
        )),
    ] {
        assert!(needs_it("", &steps), "{steps}");
    }
    assert!(needs_it("setup: {decks: [A]}\n", " []\n"));
    // A scenario that opens a fixture first starts from that fixture.
    assert!(!needs_it(
        "",
        "  - open: {fixture: storage/schema-18.anki2}\n  - db_exec: {sql: \"SELECT 1\"}\n"
    ));

    // A live scenario: the side that runs the first segment (D9.2).
    let chain = load("", "  - advance: 1d\n  - handoff: oracle\n  - observe: [timing]\n");
    assert!(
        chain
            .needs_on(scenario::Side::Meguri)
            .setup
            .contains(scenario::NEW_COLLECTION)
    );
    assert!(
        !chain
            .needs_on(scenario::Side::Oracle)
            .setup
            .contains(scenario::NEW_COLLECTION)
    );
    let opened = load(
        "",
        "  - open: {fixture: storage/schema-18.anki2}\n  - handoff: oracle\n  - observe: [timing]\n",
    );
    assert!(
        !opened
            .needs_on(scenario::Side::Meguri)
            .setup
            .contains(scenario::NEW_COLLECTION)
    );
}

#[test]
fn each_meguri_actor_gets_its_own_seed() {
    use meguri_testkit::exec::actor_seed;

    let loaded = scenario::load(&conformance_dir().join("scenarios/smoke/sync/meguri-downloads.yaml")).unwrap();
    // sync-ops.md S2.5: the first eight bytes of SHA-256("<hash>/<actor>").
    let digest = sha2_hex(&format!("{}/meguri", loaded.sha256));
    let expected = u64::from_str_radix(&digest[..16], 16).unwrap();
    assert_eq!(actor_seed(&loaded, "meguri"), expected);
    assert_ne!(actor_seed(&loaded, "meguri"), actor_seed(&loaded, "meguri_2"));
}

/// SHA-256 of a text, lower-case hex, through the shell's sha256 tool: an
/// implementation other than the one under test.
fn sha2_hex(text: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let (program, args): (&str, &[&str]) = if Command::new("sha256sum").arg("--version").output().is_ok() {
        ("sha256sum", &[])
    } else {
        ("shasum", &["-a", "256"])
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn open_scenarios_are_checked_when_they_load() {
    let load = |body: &str| {
        let text = format!("id: storage/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{body}");
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap()).map_err(|e| e.to_string())
    };
    let ok = load(
        "steps:\n  - open: {fixture: storage/schema-11.anki2}\n  - open: {fixture: a/b-c_d.e.anki2}\n  - observe: [schema_objects, {schema_objects: null}]\n  - handoff: oracle\n",
    )
    .unwrap();
    assert!(ok.opens_fixtures());
    assert!(
        matches!(&ok.steps[1], Step::Open(scenario::OpenSpec::Fixture(path)) if path.as_str() == "a/b-c_d.e.anki2")
    );
    let needs = ok.needs();
    assert!(needs.ops.contains("open") && needs.observations.contains("schema_objects"));

    for bad in [
        // P1.3: no setup, open first, no actors.
        "setup: {decks: [A]}\nsteps: [{open: {fixture: storage/schema-18.anki2}}]",
        "steps: [{advance: 1d}, {open: {fixture: storage/schema-18.anki2}}]",
        "actors: [client_a]\nsteps: [{open: {fixture: storage/schema-18.anki2}}]",
        // The fixture path stays below conformance/fixtures/.
        "steps: [{open: {fixture: ../storage/schema-18.anki2}}]",
        "steps: [{open: {fixture: /storage/schema-18.anki2}}]",
        "steps: [{open: {fixture: storage//schema-18.anki2}}]",
        "steps: [{open: {fixture: storage/Schema-18.anki2}}]",
        "steps: [{open: {fixture: storage/schema-18.apkg}}]",
        "steps: [{open: {fixture: storage/schema-18.anki2, mode: ro}}]",
        "steps: [{open: {}}]",
        "steps: [{open: {fixture: 1}}]",
        "steps: [{open: storage/schema-18.anki2}]",
        // schema_objects takes no arguments.
        "steps: [{open: {fixture: s.anki2}}, {observe: [\"schema_objects(#c1)\"]}]",
        "steps: [{open: {fixture: s.anki2}}, {observe: [{schema_objects: {}}]}]",
    ] {
        assert!(load(bad).is_err(), "accepted {bad}");
    }
}

/// The ops of notetype-ops.md T2-T6.
const NOTETYPE_OPS: [&str; 5] = [
    "add_notetype",
    "add_stock_notetype",
    "clone_notetype",
    "update_notetype",
    "ensure_io_notetype",
];

/// A valid argument mapping for each note type op, every typed argument
/// given.
fn valid_notetype_args(op: &str) -> Value {
    match op {
        "add_notetype" => json!({
            "name": "Pair",
            "kind": "cloze",
            "fields": ["Text"],
            "templates": [{"name": "Cloze", "front": "{{cloze:Text}}", "back": ""}],
            "css": ".card {}"
        }),
        "add_stock_notetype" => json!({"kind": "basic_typing", "name": "Typing"}),
        "clone_notetype" => json!({"notetype": "Pair", "name": "Pair 2"}),
        "update_notetype" => json!({"notetype": "Pair", "edits": [{"css": ".card {}"}], "skip_checks": true}),
        "ensure_io_notetype" => json!({}),
        other => panic!("no valid example for note type op {other}"),
    }
}

/// A valid example of each note type edit (notetype-ops.md T5), every typed
/// member given.
fn valid_notetype_edit(key: &str) -> Value {
    match key {
        "rename" => json!("Q"),
        "css" => json!(".card {}"),
        "sort_field" => json!("Back"),
        "add_field" => json!("Extra"),
        "remove_field" => json!("Back"),
        "rename_field" => json!({"from": "Back", "to": "Answer"}),
        "move_field" => json!({"field": "Back", "to": 0}),
        "field_options" => json!({
            "field": "Back",
            "sticky": true,
            "rtl": false,
            "font_name": "Arial",
            "font_size": 20,
            "description": "d",
            "plain_text": true,
            "collapsed": false,
            "exclude_from_search": true
        }),
        "add_template" => json!({"name": "T", "front": "{{Front}}", "back": "{{Back}}"}),
        "remove_template" => json!("T"),
        "rename_template" => json!({"from": "T", "to": "U"}),
        "move_template" => json!({"template": "T", "to": 1}),
        "template" => json!({
            "name": "T",
            "front": "f",
            "back": "b",
            "browser_front": "bf",
            "browser_back": "bb",
            "browser_font_name": "Arial",
            "browser_font_size": 12,
            "deck": "Default"
        }),
        other => panic!("no valid example for note type edit {other}"),
    }
}

fn scenario_schema() -> Value {
    serde_json::from_str(&fs::read_to_string(conformance_dir().join("schema/scenario.schema.json")).unwrap()).unwrap()
}

/// A schema definition with its `$ref` (`#/$defs/<name>`) followed.
fn resolved<'a>(schema: &'a Value, def: &'a Value) -> &'a Value {
    match def["$ref"].as_str() {
        Some(target) => &schema["$defs"][target.strip_prefix("#/$defs/").unwrap()],
        None => def,
    }
}

#[test]
fn the_model_reads_the_notetype_ops_as_the_schema_types_them() {
    let schema = scenario_schema();
    let steps = schema["$defs"]["step"]["properties"].as_object().unwrap();
    // Every op the schema cites notetype-ops.md for, and no other.
    let mut cited: Vec<&str> = steps
        .iter()
        .filter(|(_, def)| {
            def["description"]
                .as_str()
                .is_some_and(|d| d.starts_with("notetype-ops.md"))
        })
        .map(|(op, _)| op.as_str())
        .collect();
    cited.sort_unstable();
    let mut ours = NOTETYPE_OPS.to_vec();
    ours.sort_unstable();
    assert_eq!(cited, ours);
    for op in NOTETYPE_OPS {
        assert!(Step::NAMES.contains(&op), "{op} is not in Step::NAMES");
        let def = resolved(&schema, &steps[op]);
        let valid = valid_notetype_args(op);
        assert_eq!(model_accepts(json!({op: valid.clone()})), Ok(()), "{op}");
        // A mapping, never the list of its values that serde's derive reads.
        let as_list: Vec<Value> = valid.as_object().unwrap().values().cloned().collect();
        assert!(model_accepts(json!({op: as_list})).is_err(), "{op} as a list");
        assert!(model_accepts(json!({op: null})).is_err(), "{op}: null");
        let mut extra = valid.clone();
        extra["colour"] = json!("red");
        assert!(model_accepts(json!({op: extra})).is_err(), "{op} with an unknown key");
        for key in def["required"].as_array().into_iter().flatten() {
            let mut without = valid.clone();
            without.as_object_mut().unwrap().remove(key.as_str().unwrap());
            assert!(model_accepts(json!({op: without})).is_err(), "{op} without {key}");
        }
        // No argument of these ops is nullable or a mapping.
        for key in def["properties"].as_object().into_iter().flatten().map(|(k, _)| k) {
            for wrong_value in [Value::Null, json!({"a": 1})] {
                let mut wrong = valid.clone();
                wrong[key.as_str()] = wrong_value.clone();
                assert!(model_accepts(json!({op: wrong})).is_err(), "{op}.{key}: {wrong_value}");
            }
        }
    }
}

#[test]
fn the_model_reads_the_notetype_edits_as_the_schema_types_them() {
    let schema = scenario_schema();
    let edit_defs = schema["$defs"]["notetype_edit"]["properties"].as_object().unwrap();
    assert_eq!(edit_defs.len(), 13, "T5 has 13 edits");
    let update = |edits: Value| model_accepts(json!({"update_notetype": {"notetype": "Pair", "edits": edits}}));
    assert_eq!(update(json!([])), Ok(()));
    for (key, def) in edit_defs {
        let def = resolved(&schema, def);
        let valid = valid_notetype_edit(key);
        assert_eq!(update(json!([{key.clone(): valid.clone()}])), Ok(()), "{key}");
        // One key per edit, and only the edits of T5.
        let other = if key == "css" { "rename" } else { "css" };
        assert!(
            update(json!([{key.clone(): valid.clone(), other: ""}])).is_err(),
            "{key} and {other}"
        );
        assert!(update(json!([key])).is_err(), "{key} as a string");
        let Some(members) = valid.as_object() else {
            assert!(update(json!([{key.clone(): {"a": 1}}])).is_err(), "{key}: a mapping");
            assert!(update(json!([{key.clone(): null}])).is_err(), "{key}: null");
            continue;
        };
        let as_list: Vec<Value> = members.values().cloned().collect();
        assert!(update(json!([{key.clone(): as_list}])).is_err(), "{key} as a list");
        let mut extra = valid.clone();
        extra["colour"] = json!("red");
        assert!(
            update(json!([{key.clone(): extra}])).is_err(),
            "{key} with an unknown key"
        );
        for required in def["required"].as_array().unwrap() {
            let mut without = valid.clone();
            without.as_object_mut().unwrap().remove(required.as_str().unwrap());
            assert!(
                update(json!([{key.clone(): without}])).is_err(),
                "{key} without {required}"
            );
        }
        for (member, member_def) in def["properties"].as_object().unwrap() {
            let nullable = member_def["type"]
                .as_array()
                .is_some_and(|t| t.contains(&json!("null")))
                || member_def["oneOf"]
                    .as_array()
                    .is_some_and(|branches| branches.iter().any(|branch| branch["type"] == "null"));
            let mut wrongs = vec![json!({"a": 1}), json!(-1)];
            if !nullable {
                wrongs.push(Value::Null);
            }
            for wrong_value in wrongs {
                let mut wrong = valid.clone();
                wrong[member.as_str()] = wrong_value.clone();
                assert!(
                    update(json!([{key.clone(): wrong}])).is_err(),
                    "{key}.{member}: {wrong_value}"
                );
            }
        }
    }
    assert!(update(json!([{"rename_note": "Q"}])).is_err());
    assert!(update(json!([{}])).is_err());
}

#[test]
fn notetype_ops_read_their_arguments_and_load_time_rules() {
    use meguri_testkit::scenario::{NotetypeEdit, NotetypeKind, StockKind};

    let text = r##"
id: notetypes/all-ops
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
steps:
  - add_notetype: {name: Pair, fields: [Front, Back], templates: [{name: Card 1, front: "{{Front}}"}]}
  - add_stock_notetype: {kind: image_occlusion}
  - add_stock_notetype: {kind: basic_and_reversed, name: Both}
  - clone_notetype: {notetype: Pair}
  - update_notetype:
      notetype: Pair
      edits:
        - {rename_field: {from: Back, to: Answer}}
        - {template: {name: Card 1, deck: null}}
        - {template: {name: Card 1, deck: Default, browser_front: "{{Front}}"}}
        - {template: {name: Card 1}}
        - {move_field: {field: Answer, to: 0}}
  - update_notetype: {notetype: Pair, edits: [], skip_checks: true}
  - ensure_io_notetype: {}
  - observe: [notetype_structure, {notetype_structure: null}, {notetype_structure: {notetypes: [Pair, Both]}}]
"##;
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    assert!(matches!(
        &parsed.steps[0],
        Step::AddNotetype(spec) if spec.name == "Pair" && spec.kind == NotetypeKind::Standard
            && spec.fields == ["Front", "Back"] && spec.css.is_empty()
            && spec.templates.len() == 1 && spec.templates[0].back.is_empty()
    ));
    assert!(
        matches!(&parsed.steps[1], Step::AddStockNotetype(a) if a.kind == StockKind::ImageOcclusion && a.name.is_none())
    );
    assert!(matches!(
        &parsed.steps[2],
        Step::AddStockNotetype(a) if a.kind == StockKind::BasicAndReversed && a.name.as_deref() == Some("Both")
    ));
    assert!(matches!(&parsed.steps[3], Step::CloneNotetype(a) if a.notetype == "Pair" && a.name.is_none()));
    let Step::UpdateNotetype(update) = &parsed.steps[4] else {
        panic!("{:?}", parsed.steps[4]);
    };
    assert_eq!(update.notetype, "Pair");
    assert!(!update.skip_checks);
    assert!(matches!(&update.edits[0], NotetypeEdit::RenameField(p) if p.from == "Back" && p.to == "Answer"));
    // The deck override: null clears it, a name sets it, missing keeps it.
    assert!(matches!(&update.edits[1], NotetypeEdit::Template(t) if t.deck == Some(None) && t.front.is_none()));
    assert!(matches!(
        &update.edits[2],
        NotetypeEdit::Template(t) if t.deck == Some(Some("Default".to_owned())) && t.browser_front.as_deref() == Some("{{Front}}")
    ));
    assert!(matches!(&update.edits[3], NotetypeEdit::Template(t) if t.deck.is_none()));
    assert!(matches!(&update.edits[4], NotetypeEdit::MoveField(m) if m.field == "Answer" && m.to == 0));
    assert!(matches!(&parsed.steps[5], Step::UpdateNotetype(u) if u.skip_checks && u.edits.is_empty()));
    assert!(matches!(&parsed.steps[6], Step::EnsureIoNotetype(_)));
    assert!(parsed.needs().observations.contains("notetype_structure"));

    for (bad, why) in [
        // The schema's minLength and minItems, as for setup.notetypes.
        (
            "add_notetype: {name: \"\", fields: [F], templates: [{name: T}]}",
            "note type name",
        ),
        (
            "add_notetype: {name: Q, fields: [], templates: [{name: T}]}",
            "field list",
        ),
        ("add_notetype: {name: Q, fields: [F], templates: []}", "template list"),
        (
            "add_notetype: {name: Q, kind: io, fields: [F], templates: [{name: T}]}",
            "unknown variant",
        ),
        ("add_stock_notetype: {kind: basic, name: \"\"}", "note type name"),
        ("add_stock_notetype: {kind: basic_reversed}", "unknown variant"),
        ("clone_notetype: {notetype: \"\"}", "note type name"),
        ("clone_notetype: {notetype: Pair, name: \"\"}", "note type name"),
        ("update_notetype: {notetype: \"\", edits: []}", "note type name"),
        (
            "update_notetype: {notetype: Pair, edits: [{move_field: {field: F, to: 1.5}}]}",
            "invalid type",
        ),
        ("ensure_io_notetype: {name: X}", "unknown key"),
        ("observe: [\"notetype_structure(#c1)\"]", "does not take"),
        ("observe: [{notetype_structure: {}}]", "notetypes"),
        ("observe: [{notetype_structure: {notetypes: []}}]", "notetypes"),
        ("observe: [{notetype_structure: {notetypes: [\"\"]}}]", "notetypes"),
        ("observe: [{notetype_structure: {notetypes: Pair}}]", "notetypes"),
        (
            "observe: [{notetype_structure: {notetypes: [Pair], kind: x}}]",
            "notetypes",
        ),
    ] {
        let text =
            format!("id: notetypes/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{{bad}}}]\n");
        let err = scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
            .expect_err(bad)
            .to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

#[test]
fn update_notetype_needs_a_capability_per_kind_of_edit() {
    let needs_of = |edits: &str| {
        let text = format!(
            "id: notetypes/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps:\n  - update_notetype: {{notetype: Pair, edits: {edits}, skip_checks: true}}\n"
        );
        let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap()).unwrap();
        parsed.needs().ops.into_iter().collect::<Vec<_>>()
    };
    // notetype-ops.md T10.1: the op, and one capability per kind of edit;
    // skip_checks and an empty list add nothing.
    assert_eq!(needs_of("[]"), ["update_notetype"]);
    assert_eq!(
        needs_of("[{css: a}, {css: b}, {rename: Q}]"),
        ["update_notetype", "update_notetype.css", "update_notetype.rename"]
    );
    // A template edit that sets front or back needs template_text instead.
    assert_eq!(
        needs_of("[{template: {name: T, browser_front: x, deck: null}}]"),
        ["update_notetype", "update_notetype.template"]
    );
    assert_eq!(
        needs_of("[{template: {name: T, front: x}}]"),
        ["update_notetype", "update_notetype.template_text"]
    );
    assert_eq!(
        needs_of("[{template: {name: T, back: x}}, {template: {name: T}}]"),
        [
            "update_notetype",
            "update_notetype.template",
            "update_notetype.template_text"
        ]
    );
    // Every other edit needs its own key.
    let schema = scenario_schema();
    for key in schema["$defs"]["notetype_edit"]["properties"]
        .as_object()
        .unwrap()
        .keys()
    {
        if key == "template" {
            continue;
        }
        let edit = serde_json::to_string(&json!([{key.clone(): valid_notetype_edit(key)}])).unwrap();
        assert_eq!(
            needs_of(&edit),
            ["update_notetype".to_owned(), format!("update_notetype.{key}")],
            "{key}"
        );
    }
    // The same names in a live chain's and a Meguri actor's needs.
    let text = "id: notetypes/live\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps:\n  - handoff: meguri\n  - update_notetype: {notetype: Pair, edits: [{add_field: F}]}\n";
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    assert!(
        parsed
            .needs_on(scenario::Side::Meguri)
            .ops
            .contains("update_notetype.add_field")
    );
}

/// render-ops.md Q4.1: `render_status` takes card references or none, and
/// reads the collection.
#[test]
fn render_status_requests_follow_the_schema() {
    let parse = |request: &str| {
        let text = format!(
            "id: notetypes/x\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{observe: [{request}]}}]\n"
        );
        scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
    };
    for good in [
        "render_status",
        "\"render_status(#c1, #c3)\"",
        "{render_status: [\"#c2\"]}",
        "\"render_status(#c1)\", {render_status: [\"#c2\"]}",
    ] {
        let parsed = parse(good).unwrap_or_else(|e| panic!("{good}: {e}"));
        assert!(parsed.needs().observations.contains("render_status"), "{good}");
        assert!(parsed.starts_from_new_collection(), "{good}");
    }
    for (bad, why) in [
        ("\"render_status(#n1)\"", "does not take these arguments"),
        ("{render_status: [\"#n1\"]}", "does not take these arguments"),
        ("{render_status: {cards: [\"#c1\"]}}", "does not take these arguments"),
        ("{render_status: []}", "at least one reference"),
        ("{render_status: null}", "cannot take null"),
    ] {
        let err = parse(bad).expect_err(bad).to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

/// records.md N12: in notetypes/records/stock-render-handoff Meguri only makes
/// its new collection and hands it over; the oracle adds and renders.
#[test]
fn stock_render_handoff_needs_only_the_new_collection_and_handoff_on_meguri() {
    let path = conformance_dir().join("scenarios/notetypes/records/stock-render-handoff.yaml");
    let text = fs::read_to_string(path).unwrap();
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap()).unwrap();
    let meguri = parsed.needs_on(scenario::Side::Meguri);
    assert_eq!(meguri.setup.iter().collect::<Vec<_>>(), [scenario::NEW_COLLECTION]);
    assert_eq!(meguri.ops.iter().collect::<Vec<_>>(), ["handoff"]);
    assert!(meguri.observations.is_empty(), "{}", meguri.summary());
    let oracle = parsed.needs_on(scenario::Side::Oracle);
    assert!(oracle.observations.contains("render_status"), "{}", oracle.summary());
}

#[test]
fn queue_count_requests_follow_the_frozen_no_argument_schema() {
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(conformance_dir().join("schema/scenario.schema.json")).unwrap())
            .unwrap();
    let observation = &schema["$defs"]["observation_request"];
    // Locate the mapping alternative by its properties, keeping this test
    // tied to the checked-in schema instead of a second invented shape.
    let alternatives = observation["anyOf"].as_array().unwrap();
    for name in ["queue", "deck_counts", "congrats"] {
        let mapping = alternatives
            .iter()
            .find_map(|v| v.get("properties").and_then(|p| p.get(name)))
            .expect("schema mapping");
        assert_eq!(mapping["type"], json!("null"));
        for request in [json!(name), json!({name:null})] {
            let value = json!({"id":"sched/requests","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"observe":[request]}]});
            assert!(meguri_testkit::scenario::Scenario::from_value(&value).is_ok());
        }
        for arg in [json!({}), json!({"deck":"A"}), json!(["#c1"])] {
            let value = json!({"id":"sched/requests","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"observe":[{name:arg}]}]});
            assert!(meguri_testkit::scenario::Scenario::from_value(&value).is_err());
        }
    }
}

#[test]
fn declared_filtered_answer_needs_follow_answer_ownership_without_runtime_inference() {
    use meguri_testkit::exec::{Executor, MeguriExecutor};
    use scenario::Side;
    let plain = "id: capability/answer\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsteps: [{answer: {card: '#c1', rating: good}}]\n";
    let parse = |text: &str| scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    let untagged = parse(plain);
    assert!(!untagged.needs().ops.contains("answer.filtered"));
    assert!(MeguriExecutor::new().missing(&untagged.needs()).is_empty());
    let tagged = parse(&format!("tags: ['needs:answer.filtered']\n{plain}"));
    assert!(tagged.needs().ops.contains("answer.filtered"));
    assert_eq!(
        MeguriExecutor::new().missing(&tagged.needs()).ops,
        ["answer.filtered".to_owned()].into()
    );
    let metadata = parse(&format!("tags: ['unrelated:answer.filtered']\n{plain}"));
    assert_eq!(metadata.needs(), untagged.needs());
    let no_answer = parse(&format!(
        "tags: ['needs:answer.filtered']\n{}",
        plain.replace("{answer: {card: '#c1', rating: good}}", "{advance: 1s}")
    ));
    assert!(!no_answer.needs().ops.contains("answer.filtered"));

    let live = parse(
        "id: capability/live\ntags: ['needs:answer.filtered']\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsteps:\n  - answer: {card: '#c1', rating: good}\n  - handoff: meguri\n  - advance: 1s\n",
    );
    assert!(live.needs_on(Side::Oracle).ops.contains("answer.filtered"));
    assert!(!live.needs_on(Side::Meguri).ops.contains("answer.filtered"));
    let mut reversed = live.clone();
    reversed.steps[1] = Step::Handoff(Side::Oracle);
    assert!(reversed.needs_on(Side::Meguri).ops.contains("answer.filtered"));
    assert!(!reversed.needs_on(Side::Oracle).ops.contains("answer.filtered"));

    let actors = actor_scenario("id: capability/actors\ntags: ['needs:answer.filtered']\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nactors: [client_a, meguri]\nsteps:\n  - answer: {card: '#c1', rating: good}\n  - actor: meguri\n  - advance: 1s\n").unwrap();
    assert!(!actors.needs_of_meguri_actors().ops.contains("answer.filtered"));
    let mut owned = actors.clone();
    owned.steps.push(actors.steps[0].clone());
    assert!(owned.needs_of_meguri_actors().ops.contains("answer.filtered"));
    let existing = scenario::load(&conformance_dir().join("scenarios/sched/compat/enable-fsrs-live.yaml")).unwrap();
    assert!(!existing.scenario.needs_on(Side::Meguri).ops.contains("answer"));
    assert!(!existing.scenario.needs_on(Side::Meguri).ops.contains("answer.filtered"));
}

/// The ops of sched-ops.md J2-J9.
const SCHED_OPS: [&str; 8] = [
    "bury",
    "suspend",
    "unsuspend",
    "unbury_deck",
    "set_due",
    "forget",
    "grade_now",
    "empty_filtered",
];

/// A valid argument mapping for each scheduling op, every typed argument
/// given.
fn valid_sched_args(op: &str) -> Value {
    match op {
        "bury" | "suspend" => json!({"notes": ["#n2", "#n1"]}),
        "unsuspend" => json!({"cards": ["#c1", "#c2"]}),
        "unbury_deck" => json!({"deck": "A::B", "mode": "siblings"}),
        "set_due" => json!({"cards": ["#c2", "#c1"], "days": "3-1!", "context": "browser"}),
        "forget" => {
            json!({"cards": ["#c2", "#c1"], "restore_position": true, "reset_counts": false, "context": "reviewer"})
        }
        "grade_now" => json!({"cards": ["#c2", "#c1"], "rating": "hard"}),
        "empty_filtered" => json!({"deck": "F"}),
        other => panic!("no valid example for scheduling op {other}"),
    }
}

#[test]
fn the_model_reads_the_sched_ops_as_the_schema_types_them() {
    let schema = scenario_schema();
    let steps = schema["$defs"]["step"]["properties"].as_object().unwrap();
    // Every op the schema cites sched-ops.md for, and no other.
    let mut cited: Vec<&str> = steps
        .iter()
        .filter(|(_, def)| {
            def["description"]
                .as_str()
                .is_some_and(|d| d.contains("sched-ops.md J"))
        })
        .map(|(op, _)| op.as_str())
        .collect();
    cited.sort_unstable();
    let mut ours = SCHED_OPS.to_vec();
    ours.sort_unstable();
    assert_eq!(cited, ours);
    for op in SCHED_OPS {
        assert!(Step::NAMES.contains(&op), "{op} is not in Step::NAMES");
        let valid = valid_sched_args(op);
        assert_eq!(model_accepts(json!({op: valid.clone()})), Ok(()), "{op}");
        // A mapping, never the list of its values that serde's derive reads.
        let as_list: Vec<Value> = valid.as_object().unwrap().values().cloned().collect();
        assert!(model_accepts(json!({op: as_list})).is_err(), "{op} as a list");
        assert!(model_accepts(json!({op: null})).is_err(), "{op}: null");
        assert!(model_accepts(json!({op: {}})).is_err(), "{op}: {{}}");
        let mut extra = valid.clone();
        extra["colour"] = json!("red");
        assert!(model_accepts(json!({op: extra})).is_err(), "{op} with an unknown key");
        let def = resolved(&schema, &steps[op]);
        let (required, properties) = match def.get("oneOf") {
            // J2, J3: one form per member of cards_or_notes.
            Some(forms) => {
                assert_eq!(forms.as_array().unwrap().len(), 2, "{op}");
                assert_eq!(model_accepts(json!({op: {"cards": ["#c1"]}})), Ok(()), "{op} {{cards}}");
                assert!(
                    model_accepts(json!({op: {"cards": ["#c1"], "notes": ["#n1"]}})).is_err(),
                    "{op} with cards and notes"
                );
                (Vec::new(), vec!["notes".to_owned()])
            }
            None => (
                def["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap().to_owned())
                    .collect(),
                def["properties"].as_object().unwrap().keys().cloned().collect(),
            ),
        };
        for key in &required {
            let mut without = valid.clone();
            without.as_object_mut().unwrap().remove(key);
            assert!(model_accepts(json!({op: without})).is_err(), "{op} without {key}");
        }
        // No scheduling-op argument is nullable or a mapping.
        for key in &properties {
            for wrong_value in [Value::Null, json!({"a": 1})] {
                let mut wrong = valid.clone();
                wrong[key.as_str()] = wrong_value.clone();
                assert!(model_accepts(json!({op: wrong})).is_err(), "{op}.{key}: {wrong_value}");
            }
        }
    }
}

#[test]
fn sched_ops_read_their_arguments_and_load_time_rules() {
    use meguri_testkit::scenario::{CardsOrNotes, Rating, SchedContext, UnburyMode};

    let text = r##"
id: sched/all-ops
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
steps:
  - bury: {cards: ["#c2", "#c1"]}
  - bury: {notes: ["#n1"]}
  - suspend: {cards: ["#c1"]}
  - suspend: {notes: ["#n2", "#n1"]}
  - unsuspend: {cards: ["#c1"]}
  - unbury_deck: {deck: "A::B", mode: manual}
  - set_due: {cards: ["#c1"], days: " 3"}
  - set_due: {cards: ["#c2", "#c1"], days: "", context: reviewer}
  - forget: {cards: ["#c2", "#c1"]}
  - forget: {cards: ["#c1"], restore_position: true, reset_counts: true, context: browser}
  - grade_now: {cards: ["#c2", "#c1"], rating: again}
  - empty_filtered: {deck: F}
"##;
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    let refs = |cards: &[meguri_testkit::scenario::CardRef]| cards.iter().map(|c| c.0.to_string()).collect::<Vec<_>>();
    // J1: the listed order is kept.
    assert!(matches!(&parsed.steps[0], Step::Bury(CardsOrNotes::Cards(c)) if refs(c) == ["#c2", "#c1"]));
    assert!(matches!(&parsed.steps[1], Step::Bury(CardsOrNotes::Notes(n)) if n.len() == 1));
    assert!(matches!(&parsed.steps[2], Step::Suspend(CardsOrNotes::Cards(_))));
    assert!(matches!(&parsed.steps[3], Step::Suspend(CardsOrNotes::Notes(n)) if n[0].0.to_string() == "#n2"));
    assert!(matches!(&parsed.steps[4], Step::Unsuspend(a) if refs(&a.cards) == ["#c1"]));
    assert!(matches!(&parsed.steps[5], Step::UnburyDeck(a) if a.deck == "A::B" && a.mode == UnburyMode::Manual));
    // J6: the text as typed, a refused one included; no context when missing.
    assert!(matches!(&parsed.steps[6], Step::SetDue(a) if a.days == " 3" && a.context.is_none()));
    assert!(matches!(
        &parsed.steps[7],
        Step::SetDue(a) if a.days.is_empty() && a.context == Some(SchedContext::Reviewer) && refs(&a.cards) == ["#c2", "#c1"]
    ));
    // J7: both choices false and no context when missing.
    assert!(matches!(
        &parsed.steps[8],
        Step::Forget(a) if !a.restore_position && !a.reset_counts && a.context.is_none() && refs(&a.cards) == ["#c2", "#c1"]
    ));
    assert!(matches!(
        &parsed.steps[9],
        Step::Forget(a) if a.restore_position && a.reset_counts && a.context == Some(SchedContext::Browser)
    ));
    assert!(
        matches!(&parsed.steps[10], Step::GradeNow(a) if a.rating == Rating::Again && refs(&a.cards) == ["#c2", "#c1"])
    );
    assert!(matches!(&parsed.steps[11], Step::EmptyFiltered(a) if a.deck == "F"));
    let needs = parsed.needs();
    let mut ops = SCHED_OPS.to_vec();
    ops.sort_unstable();
    assert_eq!(needs.ops.iter().map(String::as_str).collect::<Vec<_>>(), ops);

    for (bad, why) in [
        // The schema's minItems and uniqueItems (J1).
        ("bury: {cards: []}", "at least one"),
        ("bury: {notes: []}", "at least one"),
        ("suspend: {cards: [\"#c1\", \"#c2\", \"#c1\"]}", "lists #c1 twice"),
        ("suspend: {notes: [\"#n1\", \"#n1\"]}", "lists #n1 twice"),
        ("unsuspend: {cards: []}", "at least one"),
        ("set_due: {cards: [\"#c1\", \"#c1\"], days: \"1\"}", "twice"),
        ("forget: {cards: [\"#c3\", \"#c3\"]}", "twice"),
        ("grade_now: {cards: [], rating: good}", "at least one"),
        // J2, J3: exactly one of cards and notes, of their kinds.
        ("bury: {cards: [\"#c1\"], notes: [\"#n1\"]}", "single key"),
        ("suspend: {notes: [\"#c1\"]}", "not a note reference"),
        ("bury: {cards: [\"#n1\"]}", "not a card reference"),
        ("bury: {decks: [A]}", "unknown variant"),
        // Non-empty deck names (deck-ops.md K1).
        ("unbury_deck: {deck: \"\", mode: all}", "deck name"),
        ("empty_filtered: {deck: \"\"}", "deck name"),
        ("unbury_deck: {deck: A, mode: user}", "unknown variant"),
        ("set_due: {cards: [\"#c1\"], days: 3}", "invalid type"),
        (
            "set_due: {cards: [\"#c1\"], days: \"3\", context: editor}",
            "unknown variant",
        ),
        ("forget: {cards: [\"#c1\"], reset_counts: 1}", "invalid type"),
        ("grade_now: {cards: [\"#c1\"], rating: 3}", "invalid type"),
    ] {
        let text = format!("id: sched/bad\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps: [{{{bad}}}]\n");
        let err = scenario::Scenario::from_value(&scenario::parse_yaml(&text).unwrap())
            .expect_err(bad)
            .to_string();
        assert!(err.contains(why), "{bad}: {err}");
    }
}

#[test]
fn prepared_package_contracts_load_with_their_recorded_hashes() {
    let mut files = Vec::new();
    scenario_files(&conformance_dir().join("scenarios/package"), "package", &mut files);
    assert_eq!(files.len(), 29 + 16 + 14 + 2 + 9 + 14);
    for (path, id) in files {
        let loaded = scenario::load(&path).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(loaded.scenario.id, id);
        assert_eq!(loaded.sha256, golden_hash(&id), "{id}");
    }
}

#[test]
fn package_forms_enforce_sources_names_and_capability_parts() {
    let with_steps = |steps: Value| {
        scenario::Scenario::from_value(&json!({
            "id": "area/package", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
            "steps": steps
        }))
    };
    let open = json!({"open": {"fixture": "storage/schema-18.anki2"}});
    for source in [
        json!({"name": "A-1.colpkg"}),
        json!({"fixture": "package/backup/standard.colpkg"}),
    ] {
        let mut args = source;
        args["format"] = json!("colpkg");
        let parsed = with_steps(json!([open, {"import": args.clone()}])).unwrap();
        assert!(parsed.needs().ops.contains("import.colpkg"));
        let typed = scenario::CollectionPackageImportArgs::from_mapping(args.as_object().unwrap()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), args);
    }
    let export = json!({"format": "colpkg", "name": "A-1.colpkg"});
    let typed = scenario::CollectionPackageExportArgs::from_mapping(export.as_object().unwrap()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), export);
    let parsed = with_steps(json!([{"export": export}])).unwrap();
    assert!(parsed.needs().ops.contains("export.colpkg"));
    for args in [
        json!({"format": "colpkg"}),
        json!({"format": "colpkg", "name": "x.colpkg", "fixture": "package/x.colpkg"}),
        json!({"format": "colpkg", "name": null}),
        json!({"format": "colpkg", "name": "x.apkg"}),
        json!({"format": "colpkg", "fixture": "../x.colpkg"}),
        json!({"format": "colpkg", "fixture": "package/X.colpkg"}),
        json!({"format": "colpkg", "name": "../x.colpkg"}),
        json!({"format": "colpkg", "name": "x.colpkg", "media": true}),
    ] {
        assert!(
            with_steps(json!([open, {"import": args.clone()}])).is_err(),
            "import {args}"
        );
    }
    for args in [
        json!({"format": "colpkg", "name": "x.apkg"}),
        json!({"format": "colpkg", "name": "_x.colpkg"}),
    ] {
        assert!(model_accepts(json!({"export": args})).is_err());
    }
    let import = json!({"import": {"format": "colpkg", "name": "x.colpkg"}});
    assert!(with_steps(json!([import])).is_err());
    assert!(with_steps(json!([open, {"add_note": {"notetype": "Pair"}}, import])).is_err());
    assert!(with_steps(json!([open, import, {"observe": ["card(#c1)"]}])).is_err());
    let mut setup = json!({"id": "area/package", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [open, import]});
    setup["setup"] = json!({});
    assert!(scenario::Scenario::from_value(&setup).is_err());
    // Unspecified import forms keep draft rules; E7 export has an exact shape.
    assert!(model_accepts(json!({"import": {"fixture": "draft.apkg", "future": true}})).is_ok());
    assert!(model_accepts(json!({"export": {"format": "apkg", "name": "draft.apkg", "future": true}})).is_err());
}

#[test]
fn backup_contracts_preserve_defaults_and_reject_malformed_inputs() {
    let value = json!({"id": "area/backup", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [{"force_backup": {}}, {"backup": {"force": false}}]});
    let parsed = scenario::Scenario::from_value(&value).unwrap();
    let Step::ForceBackup(force) = &parsed.steps[0] else {
        panic!("force_backup")
    };
    assert_eq!(force.copy_as, None);
    assert_eq!(serde_json::to_value(force).unwrap(), json!({}));
    let Step::Backup(backup) = &parsed.steps[1] else {
        panic!("backup")
    };
    assert!(!backup.force);
    assert_eq!(serde_json::to_value(backup).unwrap(), json!({"force": false}));
    for step in [
        json!({"force_backup": {"copy_as": "copy.colpkg"}}),
        json!({"add_backup_files": {"names": ["foreign file", "é", "a\\b"]}}),
        json!({"add_backup_files": {"series": {"from": "2030-01-01T00:00:00", "until": "2030-01-01T05:33:19", "every": "1s"}}}),
        json!({"save_backup_settings": {"daily": 0, "weekly": 4294967295_u64, "monthly": 1, "minimum_interval_mins": 30}}),
    ] {
        assert!(model_accepts(step.clone()).is_ok(), "{step}");
    }
    for step in [
        json!({"backup": {}}),
        json!({"backup": {"force": null}}),
        json!({"backup": {"force": 0}}),
        json!({"backup": [false]}),
        json!({"backup": {"force": true, "future": true}}),
        json!({"force_backup": {"copy_as": null}}),
        json!({"force_backup": []}),
        json!({"force_backup": {"copy_as": "x.apkg"}}),
        json!({"force_backup": {"copy_as": "x.colpkg", "future": true}}),
        json!({"save_backup_settings": {"daily": 0, "weekly": 0, "monthly": 0}}),
        json!({"save_backup_settings": {"daily": -1, "weekly": 0, "monthly": 0, "minimum_interval_mins": 0}}),
        json!({"save_backup_settings": {"daily": 4294967296_u64, "weekly": 0, "monthly": 0, "minimum_interval_mins": 0}}),
        json!({"save_backup_settings": [0, 0, 0, 0]}),
        json!({"add_backup_files": {}}),
        json!({"add_backup_files": {"names": []}}),
        json!({"add_backup_files": {"names": ["x", "x"]}}),
        json!({"add_backup_files": {"names": [".meguri-work"]}}),
        json!({"add_backup_files": {"names": ["."]}}),
        json!({"add_backup_files": {"names": [".."]}}),
        json!({"add_backup_files": {"names": ["a/b"]}}),
        json!({"add_backup_files": {"names": ["a\u{0}b"]}}),
        json!({"add_backup_files": {"names": ["é".repeat(128)]}}),
        json!({"add_backup_files": {"names": ["x"], "series": {"from": "2030-01-01T00:00:00", "until": "2030-01-01T00:00:00", "every": "1s"}}}),
    ] {
        assert!(model_accepts(step.clone()).is_err(), "accepted {step}");
    }
    for (from, until, every) in [
        ("2030-01-01T00:00:00", "2030-01-01T05:33:20", "1s"), // 20,001 files
        ("2030-01-02T00:00:00", "2030-01-01T00:00:00", "1h"),
        ("2030-01-01T00:00:00", "2030-01-01T00:00:00", "0s"),
        ("2030-02-30T00:00:00", "2030-03-01T00:00:00", "1h"),
        ("2030-01-01T00:00:00Z", "2030-01-02T00:00:00", "1h"),
    ] {
        assert!(
            model_accepts(json!({"add_backup_files": {"series": {"from": from, "until": until, "every": every}}}))
                .is_err()
        );
    }
    let mut twice = value;
    twice["steps"] = json!([{"force_backup": {}}, {"backup": {"force": true}}, {"force_backup": {}}]);
    assert!(scenario::Scenario::from_value(&twice).is_err());
    for reset in [json!({"reopen": {}}), json!({"handoff": "oracle"})] {
        twice["steps"] = json!([{"force_backup": {}}, reset, {"force_backup": {}}]);
        assert!(scenario::Scenario::from_value(&twice).is_ok());
    }
}

#[test]
fn package_observation_requests_follow_the_committed_schema() {
    let schema: Value = meguri_testkit::observe::parse_json(
        &fs::read_to_string(conformance_dir().join("schema/scenario.schema.json")).unwrap(),
    )
    .unwrap();
    let kinds = schema["$defs"]["observation_request"]["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|v| v.get("properties"))
        .unwrap();
    for kind in ["package_listing", "backup_files", "backup_settings"] {
        assert!(!kinds[kind].is_null());
        assert!(scenario::ObservationKind::from_name(kind).is_some());
    }
    for request in [
        json!("backup_files"),
        json!({"backup_files": null}),
        json!("backup_settings"),
        json!({"backup_settings": null}),
        json!({"package_listing": {"name": "A.apkg"}}),
        json!({"package_listing": {"fixture": "package/backup/standard.colpkg"}}),
    ] {
        assert!(
            model_accepts(json!({"observe": [request.clone()]})).is_ok(),
            "{request}"
        );
    }
    for request in [
        json!("package_listing"),
        json!({"package_listing": null}),
        json!({"package_listing": {}}),
        json!({"package_listing": {"name": null}}),
        json!({"package_listing": {"name": "../x.apkg"}}),
        json!({"package_listing": {"fixture": "package/X.colpkg"}}),
        json!({"package_listing": {"fixture": "package/x.zip"}}),
        json!({"package_listing": {"name": "x.apkg", "fixture": "package/x.apkg"}}),
        json!({"package_listing": {"name": "x.apkg", "future": true}}),
        json!({"backup_files": {}}),
        json!({"backup_settings": {}}),
    ] {
        assert!(
            model_accepts(json!({"observe": [request.clone()]})).is_err(),
            "accepted {request}"
        );
    }
    let request = json!({"package_listing": {"name": "x.apkg"}});
    assert!(model_accepts(json!({"observe": [request, request]})).is_err());
}

#[test]
fn optimize_is_a_typed_empty_mapping_operation() {
    let value = json!({"id": "maintenance/typed-optimize", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [{"optimize": {}}]});
    let parsed = scenario::Scenario::from_value(&value).unwrap();
    assert!(matches!(parsed.steps[0], Step::Optimize(_)));
    assert_eq!(parsed.steps[0].name(), "optimize");
    assert!(Step::NAMES.contains(&"optimize"));
    for args in [json!({"future": true}), json!([]), json!(null), json!(true)] {
        let mut invalid = value.clone();
        invalid["steps"] = json!([{"optimize": args}]);
        assert!(scenario::Scenario::from_value(&invalid).is_err());
    }
}

// Prepared held vocabulary: recognition only, with no area activation.
fn held_steps(steps: Value) -> Result<scenario::Scenario, scenario::FormatError> {
    scenario::Scenario::from_value(&json!({
        "id": "area/intake", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": steps,
    }))
}

fn held_import() -> Value {
    json!({"format": "apkg", "name": "Export-1.apkg", "merge_notetypes": false,
        "update_notes": "if_newer", "update_notetypes": "always",
        "with_scheduling": true, "with_deck_configs": false})
}

fn held_export() -> Value {
    json!({"format": "apkg", "name": "Export-1.apkg", "limit": "collection",
        "scheduling": true, "presets": false, "media": true})
}

#[test]
fn held_step_vocabulary_is_strict_and_remains_unregistered() {
    let examples = [
        ("remove_notetype", json!({"notetype": "Pair"})),
        ("restore_notetype", json!({"notetype": "Pair", "kind": "basic"})),
        ("change_notetype_info", json!({"notes": ["#n1"], "notetype": "Pair"})),
        (
            "change_notetype",
            json!({"fields": [0, null, 0], "templates": [1, null, 0]}),
        ),
        ("empty_cards", json!({})),
        ("delete_empty_cards", json!({"keep_notes": false})),
        ("schema11_copy", json!({"name": "copy-1.anki2"})),
        ("check_media", json!({})),
        ("delete_unused_media", json!({})),
        ("tag_missing_media", json!({})),
    ];
    let capabilities = meguri_testkit::exec::MeguriExecutor::default().capabilities();
    assert_eq!(Step::NAMES.len(), 87);
    assert_eq!(Step::NAMES.iter().collect::<std::collections::BTreeSet<_>>().len(), 87);
    for (op, args) in examples {
        let prefix = match op {
            "change_notetype" => json!({"change_notetype_info": {"notes": ["#n1"], "notetype": "Pair"}}),
            "delete_empty_cards" => json!({"empty_cards": {}}),
            "delete_unused_media" | "tag_missing_media" => json!({"check_media": {}}),
            _ => json!({"advance": "1s"}),
        };
        let parsed = held_steps(json!([prefix, {op: args.clone()}])).unwrap();
        assert_eq!(parsed.steps[1].name(), op);
        assert!(parsed.needs().ops.contains(op));
        assert!(!capabilities.ops.contains(op), "{op} is recognition only");
        assert!(held_steps(json!([prefix, {op: null}])).is_err(), "{op} null");
        assert!(held_steps(json!([prefix, {op: []}])).is_err(), "{op} list");
        let mut extra = args.clone();
        extra["unknown"] = json!(true);
        assert!(held_steps(json!([prefix, {op: extra}])).is_err(), "{op} unknown key");
    }
    for kind in [
        "basic",
        "basic_and_reversed",
        "basic_optional_reversed",
        "basic_typing",
        "cloze",
        "image_occlusion",
    ] {
        assert!(model_accepts(json!({"restore_notetype": {"notetype": "Pair", "kind": kind}})).is_ok());
    }
    assert!(model_accepts(json!({"restore_notetype": {"notetype": "Pair"}})).is_ok());
    for step in [
        json!({"remove_notetype": {"notetype": ""}}),
        json!({"remove_notetype": {}}),
        json!({"restore_notetype": {"notetype": "Pair", "kind": "unknown"}}),
        json!({"restore_notetype": {"notetype": "Pair", "kind": null}}),
        json!({"change_notetype_info": {"notes": [], "notetype": "Pair"}}),
        json!({"change_notetype_info": {"notes": ["#c1"], "notetype": "Pair"}}),
        json!({"change_notetype_info": {"notes": ["#n1"], "notetype": ""}}),
        json!({"delete_empty_cards": {}}),
        json!({"delete_empty_cards": {"keep_notes": 0}}),
    ] {
        assert!(model_accepts(step.clone()).is_err(), "{step}");
    }
    let info = json!({"change_notetype_info": {"notes": ["#n1"], "notetype": "Pair"}});
    assert!(held_steps(json!([info, {"change_notetype": {}}])).is_ok());
    for map in [
        json!({"fields": null}),
        json!({"fields": [-1]}),
        json!({"fields": [1.0]}),
        json!({"templates": [1, 1]}),
        json!({"templates": [true]}),
        json!({"fields": {"0": 1}}),
    ] {
        assert!(
            held_steps(json!([info, {"change_notetype": map.clone()}])).is_err(),
            "{map}"
        );
    }
}

#[test]
fn held_collection_scans_and_selections_have_segment_and_actor_chronology() {
    let info = json!({"change_notetype_info": {"notes": ["#n1"], "notetype": "Pair"}});
    for (scan, action) in [
        (info, json!({"change_notetype": {}})),
        (
            json!({"empty_cards": {}}),
            json!({"delete_empty_cards": {"keep_notes": true}}),
        ),
    ] {
        assert!(held_steps(json!([action])).is_err());
        assert!(held_steps(json!([scan, action, action])).is_ok());
        for side in ["meguri", "oracle"] {
            assert!(held_steps(json!([scan, {"handoff": side}, action])).is_err());
        }
        let actors = json!({"id": "area/actors", "actors": ["client_a", "meguri"],
            "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
            "steps": [scan, {"actor": "meguri"}, action]});
        assert!(scenario::Scenario::from_value(&actors).is_err());
        let mut back = actors;
        back["steps"] = json!([scan, {"actor": "meguri"}, {"actor": "client_a"}, action]);
        assert!(scenario::Scenario::from_value(&back).is_ok());
    }
}

#[test]
fn held_check_media_actions_never_reuse_an_invalidated_catalog() {
    let scan = json!({"check_media": {}});
    let tag = json!({"tag_missing_media": {}});
    let delete = json!({"delete_unused_media": {}});
    for action in [&tag, &delete] {
        assert!(held_steps(json!([action])).is_err());
        assert!(
            held_steps(json!([scan, {"observe": ["media_dir"]}, {"advance": "1s"},
            {"set_clock": "2030-01-16T00:00:00"}, tag, tag, action]))
            .is_ok()
        );
        for invalidating in [
            json!({"media_outside": [{"name": "x", "text": "x"}]}),
            json!({"db_exec": {"sql": "SELECT 1"}}),
            json!({"reopen": {}}),
            json!({"undo": {}}),
            json!({"delete_unused_media": {}}),
            json!({"set_config": {"key": "x", "value": 1}}),
            json!({"import": held_import()}),
        ] {
            assert!(
                held_steps(json!([scan, invalidating, action])).is_err(),
                "{invalidating}"
            );
            assert!(
                held_steps(json!([scan, invalidating, scan, action])).is_ok(),
                "reset after {invalidating}"
            );
        }
        for side in ["meguri", "oracle"] {
            assert!(held_steps(json!([scan, {"handoff": side}, action])).is_err());
            assert!(held_steps(json!([scan, {"handoff": side}, scan, action])).is_ok());
        }
        let mut actors = json!({"id": "area/media-actors", "actors": ["client_a", "meguri"],
            "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
            "steps": [scan, {"actor": "meguri"}, action]});
        assert!(scenario::Scenario::from_value(&actors).is_err());
        actors["steps"] = json!([scan, {"actor": "meguri"}, scan, action]);
        assert!(scenario::Scenario::from_value(&actors).is_ok());
    }
}

#[test]
fn held_session_files_require_a_fixture_first_and_safe_single_names() {
    let fixture = json!({"open": {"fixture": "storage/schema-11.anki2"}});
    let file = json!({"open": {"file": "copy.anki2"}});
    assert!(held_steps(json!([file])).is_err());
    assert!(
        scenario::Scenario::from_value(&json!({"id": "area/open", "setup": {},
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": [fixture]}))
        .is_err()
    );
    // Absence of a produced file is a runtime unknown_ref, not a load error.
    assert!(held_steps(json!([fixture, file])).is_ok());
    assert!(
        held_steps(json!([fixture, {"schema11_copy": {"name": "copy.anki2"}},
        {"handoff": "oracle"}, file]))
        .is_ok()
    );
    for name in [
        "collection.anki2",
        "Collection.anki2",
        "../copy.anki2",
        "a/copy.anki2",
        "_copy.anki2",
        ".anki2",
        "copy.apkg",
        "",
        "a\\copy.anki2",
    ] {
        assert!(
            model_accepts(json!({"schema11_copy": {"name": name}})).is_err(),
            "{name}"
        );
        assert!(
            held_steps(json!([fixture, {"open": {"file": name}}])).is_err(),
            "{name}"
        );
    }
    for args in [
        json!({}),
        json!({"file": null}),
        json!({"fixture": "a.anki2", "file": "copy.anki2"}),
        json!({"file": "copy.anki2", "unknown": true}),
    ] {
        assert!(held_steps(json!([fixture, {"open": args}])).is_err());
    }
}

#[test]
fn held_observation_shapes_and_pure_formatting_are_recognized() {
    use scenario::{NEW_COLLECTION, ObservationKind as K};
    let requests = [
        json!({"browser_rows": {"columns": ["noteFld", "cardDue"], "rows": ["#c1", "#c2"]}}),
        json!({"notetype_templates": {"notetypes": ["Pair"]}}),
        json!("studied_today_raw"),
        json!("studied_today_text"),
        json!({"studied_today_format": {"inputs": [[0, -9007199254740991_i64], [4294967295_u64, 9007199254740991_i64]]}}),
        json!({"today_stats": ["collection", {"deck": "Default"}, {"search": ""}, {"search": "  "}]}),
        json!({"file_col": {"name": "copy.anki2", "columns": ["crt", "mod", "scm", "ver", "dty", "usn", "ls", "conf", "models", "decks", "dconf", "tags"]}}),
        json!({"file_objects": {"name": "copy.anki2"}}),
        json!({"file_query": {"name": "copy.anki2", "sql": "SELECT 1"}}),
        json!({"package_media": {"name": "Export-1.apkg"}}),
        json!({"package_db_dump": {"fixture": "package/standard.colpkg", "tables": ["notes", "cards"], "raw": true}}),
    ];
    let capabilities = meguri_testkit::exec::MeguriExecutor::default().capabilities();
    assert_eq!(K::ALL.len(), 64);
    for request in &requests {
        let parsed = held_steps(json!([{"observe": [request]}])).unwrap();
        let Step::Observe(observe) = &parsed.steps[0] else {
            unreachable!()
        };
        let kind = observe[0].kind;
        assert_eq!(K::from_name(kind.name()), Some(kind));
        assert!(!capabilities.observations.contains(kind.name()));
        assert_eq!(
            parsed.needs().setup.contains(NEW_COLLECTION),
            kind != K::StudiedTodayFormat
        );
        assert!(
            held_steps(json!([{"observe": [request, request]}])).is_err()
                || matches!(kind, K::StudiedTodayRaw | K::StudiedTodayText)
        );
        if let Some(map) = request.as_object() {
            let (name, args) = map.iter().next().unwrap();
            if let Some(args) = args.as_object() {
                let mut extra = args.clone();
                extra.insert("unknown".to_owned(), json!(true));
                assert!(model_accepts(json!({"observe": [{name: extra}]})).is_err(), "{name}");
                for key in args.keys().filter(|k| !matches!(k.as_str(), "raw" | "tables")) {
                    let mut missing = args.clone();
                    missing.remove(key);
                    assert!(
                        model_accepts(json!({"observe": [{name: missing}]})).is_err(),
                        "{name}.{key}"
                    );
                }
            }
        }
    }
    // The pure exception applies to H6 only, even though all three names concern today.
    let pure = held_steps(json!([{"advance": "1s"}, {"observe": [requests[4]]}])).unwrap();
    assert!(!pure.starts_from_new_collection());
    assert!(
        held_steps(json!([{"observe": ["studied_today_raw"]}]))
            .unwrap()
            .starts_from_new_collection()
    );
}

#[test]
fn held_observations_reject_bad_members_bounds_refs_and_browse_modes() {
    for request in [
        json!({"browser_rows": {"columns": [], "rows": ["#c1"]}}),
        json!({"browser_rows": {"columns": ["deck", "deck"], "rows": ["#c1"]}}),
        json!({"browser_rows": {"columns": ["unknown"], "rows": ["#c1"]}}),
        json!({"browser_rows": {"columns": ["deck"], "rows": []}}),
        json!({"browser_rows": {"columns": ["deck"], "rows": ["#c1", "#c1"]}}),
        json!({"browser_rows": {"columns": ["deck"], "rows": ["#n1", "#c1"]}}),
        json!({"browser_rows": {"columns": ["deck"], "rows": ["#c0"]}}),
        json!({"notetype_templates": {"notetypes": [""]}}),
        json!({"notetype_templates": {"notetypes": []}}),
        json!({"studied_today_raw": {}}),
        json!({"studied_today_text": {}}),
        json!("studied_today_raw()"),
        json!("studied_today_text()"),
        json!({"studied_today_format": {"inputs": []}}),
        json!({"studied_today_format": {"inputs": [[0]]}}),
        json!({"studied_today_format": {"inputs": [[0, 1, 2]]}}),
        json!({"studied_today_format": {"inputs": [[-1, 0]]}}),
        json!({"studied_today_format": {"inputs": [[4294967296_u64, 0]]}}),
        json!({"studied_today_format": {"inputs": [[1.0, 0]]}}),
        json!({"studied_today_format": {"inputs": [[1, 0.0]]}}),
        json!({"studied_today_format": {"inputs": [[1, 9007199254740992_i64]]}}),
        json!({"today_stats": []}),
        json!({"today_stats": ["deck"]}),
        json!({"today_stats": [{"deck": ""}]}),
        json!({"today_stats": [{"search": null}]}),
        json!({"today_stats": [{"search": "", "deck": "Default"}]}),
        json!({"file_col": {"name": "copy.anki2", "columns": ["ver", "ver"]}}),
        json!({"file_col": {"name": "copy.anki2", "columns": ["unknown"]}}),
        json!({"file_objects": {"name": "collection.anki2"}}),
        json!({"file_query": {"name": "copy.anki2", "sql": ""}}),
        json!({"package_media": {"fixture": "../x.apkg"}}),
        json!({"package_media": {"name": "x.apkg", "fixture": "package/x.apkg"}}),
        json!({"package_media": {"name": "x.apkg", "tables": ["notes"]}}),
        json!({"package_db_dump": {"name": "x.apkg", "tables": []}}),
        json!({"package_db_dump": {"name": "x.apkg", "tables": ["notes", "notes"]}}),
        json!({"package_db_dump": {"name": "x.apkg", "tables": ["unknown"]}}),
        json!({"package_db_dump": {"name": "x.apkg", "raw": null}}),
        json!({"package_db_dump": {"name": "x.apkg", "raw": 1}}),
    ] {
        assert!(
            model_accepts(json!({"observe": [request.clone()]})).is_err(),
            "{request}"
        );
    }
    let note_rows = json!({"observe": [{"browser_rows": {"columns": ["deck"], "rows": ["#n1"]}}]});
    assert!(held_steps(json!([note_rows])).is_err());
    assert!(
        held_steps(json!([{"set_config": {"key": "browserTableShowNotesMode", "value": true}}, note_rows])).is_ok()
    );
    let card_rows = json!({"observe": [{"browser_rows": {"columns": ["deck"], "rows": ["#c1"]}}]});
    assert!(
        held_steps(json!([{"set_config": {"key": "browserTableShowNotesMode", "value": true}}, card_rows])).is_err()
    );
}

#[test]
fn held_browser_rows_follow_browser_config_mode_switches() {
    for (notes, row, wrong) in [(true, "#n1", "#c1"), (false, "#c1", "#n1")] {
        let before = json!({"browser_config_set": {"key": "browserTableShowNotesMode", "value": !notes}});
        let switch = json!({"browser_config_set": {"key": "browserTableShowNotesMode", "value": notes}});
        let rows = |row| json!({"observe": [{"browser_rows": {"columns": ["deck"], "rows": [row]}}]});
        assert!(held_steps(json!([before, rows(wrong), switch, rows(row)])).is_ok());
        let error = held_steps(json!([before, switch, rows(wrong)])).unwrap_err();
        assert!(error.to_string().contains("references disagree with Browse mode"));
        // An unrelated browser write keeps the known mode; raw SQL makes it unknown.
        let unrelated = json!({"browser_config_set": {"key": "previewBothSides", "value": !notes}});
        assert!(held_steps(json!([switch, unrelated, rows(wrong)])).is_err());
        assert!(held_steps(json!([switch, {"db_exec": {"sql": "SELECT 1"}}, rows(wrong)])).is_ok());
    }
    // The mode accepts only true or false; each other JSON type is rejected.
    for invalid_mode in [json!(null), json!(0), json!(1), json!("false"), json!([]), json!({})] {
        assert!(
            held_steps(json!([{"browser_config_set": {"key": "browserTableShowNotesMode", "value": invalid_mode}}]))
                .is_err()
        );
    }
}

#[test]
fn held_browser_config_mode_writes_belong_to_the_current_actor() {
    for (writer, other) in [("client_a", "meguri"), ("meguri", "client_a")] {
        for (notes, row, wrong) in [(true, "#n1", "#c1"), (false, "#c1", "#n1")] {
            let switch = json!({"browser_config_set": {"key": "browserTableShowNotesMode", "value": notes}});
            let rows = |row| json!({"observe": [{"browser_rows": {"columns": ["deck"], "rows": [row]}}]});
            let mut value = json!({"id": "area/browser-mode-actors", "actors": ["client_a", "meguri"],
                "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
                "steps": []});
            let prefix = json!([
                {"set_config": {"key": "browserTableShowNotesMode", "value": !notes}},
                {"actor": "meguri"},
                {"set_config": {"key": "browserTableShowNotesMode", "value": !notes}},
                {"actor": writer}, switch
            ]);
            for (tail, accepted) in [
                (json!([rows(row)]), true),
                (json!([rows(wrong)]), false),
                (
                    json!([{"actor": other}, rows(wrong), {"actor": writer}, rows(row)]),
                    true,
                ),
                (json!([{"actor": other}, rows(row)]), false),
            ] {
                let mut steps = prefix.as_array().unwrap().clone();
                steps.extend(tail.as_array().unwrap().iter().cloned());
                value["steps"] = json!(steps);
                assert_eq!(
                    scenario::Scenario::from_value(&value).is_ok(),
                    accepted,
                    "writer={writer}, notes={notes}, tail={tail}"
                );
            }
        }
    }
}

#[test]
fn held_search_orders_are_optional_strict_and_preserved() {
    for order in [
        json!({"column": "cardMod"}),
        json!({"column": "unknown", "reverse": true}),
        json!({"column": "", "reverse": false}),
    ] {
        let request = json!({"query": "", "kind": "notes", "order": order});
        let parsed = scenario::SearchRequest::from_list(&[request]).unwrap();
        assert_eq!(
            parsed[0].order.as_ref().unwrap().column,
            order["column"].as_str().unwrap()
        );
        assert_eq!(
            parsed[0].order.as_ref().unwrap().reverse,
            order.get("reverse").and_then(Value::as_bool)
        );
    }
    assert!(
        scenario::SearchRequest::from_list(&[json!({"query": ""})]).unwrap()[0]
            .order
            .is_none()
    );
    for order in [
        json!(null),
        json!([]),
        json!({}),
        json!({"column": 1}),
        json!({"column": "deck", "reverse": null}),
        json!({"column": "deck", "reverse": 1}),
        json!({"column": "deck", "unknown": false}),
    ] {
        assert!(model_accepts(json!({"observe": [{"search": [{"query": "", "order": order}]}]})).is_err());
    }
}

#[test]
fn held_apkg_sources_options_limits_and_finer_needs_are_exact() {
    let import = held_import();
    let export = held_export();
    let parsed = held_steps(json!([{"import": import}, {"export": export}])).unwrap();
    assert!(parsed.needs().ops.contains("import.apkg"));
    assert!(parsed.needs().ops.contains("export.apkg"));
    let capabilities = meguri_testkit::exec::MeguriExecutor::default().capabilities();
    assert!(!capabilities.ops.contains("import.apkg"));
    assert!(!capabilities.ops.contains("export.apkg"));
    for (op, args) in [("import", &import), ("export", &export)] {
        for key in args.as_object().unwrap().keys() {
            let mut missing = args.clone();
            missing.as_object_mut().unwrap().remove(key);
            // The format-free import form intentionally retains its draft contract.
            if op == "import" && key == "format" {
                continue;
            }
            assert!(model_accepts(json!({op: missing})).is_err(), "{op}.{key}");
        }
        let mut extra = args.clone();
        extra["unknown"] = json!(true);
        assert!(model_accepts(json!({op: extra})).is_err());
    }
    for policy in ["if_newer", "always", "never"] {
        let mut args = import.clone();
        args["update_notes"] = json!(policy);
        args["update_notetypes"] = json!(policy);
        assert!(model_accepts(json!({"import": args})).is_ok());
    }
    for key in [
        "merge_notetypes",
        "with_scheduling",
        "with_deck_configs",
        "update_notes",
        "update_notetypes",
    ] {
        for value in [json!(null), json!(1), json!("unknown")] {
            let mut args = import.clone();
            args[key] = value;
            assert!(model_accepts(json!({"import": args})).is_err(), "{key}");
        }
    }
    let mut fixture_import = import.clone();
    fixture_import.as_object_mut().unwrap().remove("name");
    fixture_import["fixture"] = json!("package/apkg/x.apkg");
    assert!(model_accepts(json!({"import": fixture_import})).is_ok());
    for source in ["../x.apkg", "x.colpkg", "_x.apkg", "a/x.apkg"] {
        let mut args = import.clone();
        args["name"] = json!(source);
        assert!(model_accepts(json!({"import": args})).is_err());
    }
    let mut both = import.clone();
    both["fixture"] = json!("package/x.apkg");
    assert!(model_accepts(json!({"import": both})).is_err());
    for limit in [
        json!("collection"),
        json!({"deck": "Default"}),
        json!({"notes": ["#n1"]}),
        json!({"cards": ["#c1"]}),
    ] {
        let mut args = export.clone();
        args["limit"] = limit;
        assert!(model_accepts(json!({"export": args})).is_ok());
    }
    for limit in [
        json!("unknown"),
        json!({"deck": ""}),
        json!({"notes": []}),
        json!({"notes": ["#c1"]}),
        json!({"cards": ["#n1"]}),
        json!({"deck": "Default", "cards": ["#c1"]}),
    ] {
        let mut args = export.clone();
        args["limit"] = limit;
        assert!(model_accepts(json!({"export": args})).is_err());
    }
    for flag in ["scheduling", "presets", "media"] {
        let mut args = export.clone();
        args[flag] = json!(1);
        assert!(model_accepts(json!({"export": args})).is_err());
    }
    // Session ownership: only the side/actor actually importing needs the finer form.
    let chain = held_steps(json!([{"handoff": "oracle"}, {"import": import}])).unwrap();
    assert!(!chain.needs_on(scenario::Side::Meguri).ops.contains("import.apkg"));
    assert!(chain.needs_on(scenario::Side::Oracle).ops.contains("import.apkg"));
    let actors = scenario::Scenario::from_value(&json!({"id": "area/actors", "actors": ["client_a", "meguri"],
        "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [{"import": import}, {"actor": "meguri"}, {"export": export}]}))
    .unwrap();
    assert!(!actors.needs_of_meguri_actors().ops.contains("import.apkg"));
    assert!(actors.needs_of_meguri_actors().ops.contains("export.apkg"));
}

#[test]
fn held_media_case_variants_are_opt_in_but_nfc_duplicates_never_are() {
    for outside in [false, true] {
        let files = json!([{"name": "A.jpg", "text": "A"}, {"name": "a.jpg", "text": "a"}]);
        let mut value =
            json!({"id": "area/media", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": []});
        if outside {
            value["steps"] = json!([{"media_outside": files}]);
        } else {
            value["setup"] = json!({"media_files": files});
        }
        assert!(scenario::Scenario::from_value(&value).is_err());
        value["tags"] = json!(["media-case-variants"]);
        let parsed = scenario::Scenario::from_value(&value).unwrap();
        assert!(parsed.needs().setup.contains("media_files.case_variants"));
        assert!(
            parsed
                .needs_on(scenario::Side::Oracle)
                .setup
                .contains("media_files.case_variants")
        );
        assert!(
            !meguri_testkit::exec::MeguriExecutor::default()
                .capabilities()
                .setup
                .contains("media_files.case_variants")
        );
        let equal = json!([{"name": "é.jpg", "text": "x"}, {"name": "e\u{301}.jpg", "text": "y"}]);
        if outside {
            value["steps"] = json!([{"media_outside": equal}]);
        } else {
            value["setup"] = json!({"media_files": equal});
        }
        assert!(scenario::Scenario::from_value(&value).is_err());
    }
}

#[test]
fn held_raw_requests_and_numeric_observation_values_retain_their_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("intake.json");
    let value = json!({"id": "area/intake", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "steps": [{"import": held_import()}, {"export": held_export()},
            {"observe": [{"studied_today_format": {"inputs": [[4294967295_u64, -9007199254740991_i64]]}},
                {"search": [{"query": "", "order": {"column": "cardMod", "reverse": false}}]}]}]});
    fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
    let loaded = scenario::load(&path).unwrap();
    assert_eq!(loaded.value, value); // CLI to-json emits this raw value.
    assert_eq!(scenario::jcs(&loaded.value).unwrap(), scenario::jcs(&value).unwrap());
    for (index, raw) in [(0, held_import()), (1, held_export())] {
        assert_eq!(
            loaded.scenario.steps[index].draft_args().unwrap().0,
            *raw.as_object().unwrap()
        );
    }
    let exact = meguri_testkit::observe::parse_json(
        r#"{"conf":{"u":18446744073709551615,"i":1,"f":1.0},"mod":9223372036854775807}"#,
    )
    .unwrap();
    assert_eq!(exact["conf"]["u"].as_u64(), Some(u64::MAX));
    assert_eq!(exact["mod"].as_i64(), Some(i64::MAX));
    assert!(exact["conf"]["i"].as_number().unwrap().is_u64());
    assert!(exact["conf"]["f"].as_number().unwrap().is_f64());
    assert_eq!(
        serde_json::to_string(&exact).unwrap(),
        r#"{"conf":{"f":1.0,"i":1,"u":18446744073709551615},"mod":9223372036854775807}"#
    );
    for text in [
        r#"{"open":{"fixture":"a.anki2","fixture":"b.anki2"}}"#,
        r#"{"import":{"format":"apkg","format":"colpkg"}}"#,
        r#"{"observe":[{"file_col":{"name":"a.anki2","name":"b.anki2"}}]}"#,
    ] {
        assert!(scenario::parse_json(text).is_err());
    }
}
