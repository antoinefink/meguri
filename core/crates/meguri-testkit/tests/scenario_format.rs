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
  - reopen: {}
  - handoff: oracle
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
      - {db_dump: {tables: [notes, cards]}}
      - {stats: {}}
      - digest
"##;
    let value = scenario::parse_yaml(text).unwrap();
    let parsed = scenario::Scenario::from_value(&value).unwrap();
    assert_eq!(parsed.steps.len(), 20);
    assert!(matches!(parsed.steps[0], Step::Advance(d) if d.seconds() == 93_600));
    let needs = parsed.needs();
    assert_eq!(
        needs.setup.iter().map(String::as_str).collect::<Vec<_>>(),
        ["cards", "config", "decks", "notes", "notetypes", "presets", "revlog"]
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
        "bury" | "suspend" | "forget" => json!({"cards": ["#c1"]}),
        "set_due" => json!({"cards": ["#c1"], "days": "1-7"}),
        "reposition" => json!({"cards": ["#c1"], "start": 5}),
        "update_preset" => json!({"name": "Default"}),
        "build_filtered" => json!({"name": "F", "search": "deck:A"}),
        "import" => json!({"fixture": "package/x.apkg"}),
        "export" => json!({"format": "apkg", "name": "out.apkg"}),
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
fn the_model_requires_what_the_schema_requires_of_draft_ops() {
    let from_schema = schema_draft_requirements();
    // Nine draft ops: `sync` left them when sync-ops.md fixed its shape,
    // `edit_note`, `delete_notes` and `db_exec` with record-ops.md W5, and
    // `check_db` with collection-check.md K10.
    assert!(from_schema.len() >= 9, "{from_schema:?}");
    for exact in ["edit_note", "delete_notes", "db_exec", "set_config", "check_db"] {
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
            "steps: [{observe: [{search: {query: q, kind: decks}}]}]",
            "cards or notes",
        ),
        ("steps: [{observe: [{queue: {deck: 1}}]}]", "deck"),
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
    let text = "id: area/ok\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{export: {format: apkg, name: Note_types-2.v1.apkg, legacy: true}}, {export: {format: colpkg, name: out.colpkg, media: true}}, {observe: [{package_col: {name: Note_types-2.v1.apkg, columns: [ver, conf, models, decks, dconf, tags]}}]}]\n";
    let parsed = scenario::Scenario::from_value(&scenario::parse_yaml(text).unwrap()).unwrap();
    let needs = parsed.needs();
    assert!(needs.ops.contains("export") && needs.observations.contains("package_col"));
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
    let request = &schema["$defs"]["observation_request"]["anyOf"][1]["properties"];
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
            base("[client_a]", "[{observe: [{db_dump: {tables: [cards], raw: true}}]}]"),
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
fn meguri_actors_need_what_they_run_and_nothing_else() {
    let path = conformance_dir().join("scenarios/smoke/sync/meguri-downloads.yaml");
    let loaded = scenario::load(&path).unwrap();
    // sync-ops.md S10.2: `actor`; no setup (client_a runs it); the ops and
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
    assert!(needs.setup.is_empty());

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
        ["decks", "notetypes"]
    );
    assert_eq!(
        needs.ops.iter().map(String::as_str).collect::<Vec<_>>(),
        ["actor", "reopen", "set_clock", "sync"]
    );
    assert!(needs.observations.is_empty());
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
    assert!(matches!(&ok.steps[1], Step::Open(spec) if spec.fixture.as_str() == "a/b-c_d.e.anki2"));
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
