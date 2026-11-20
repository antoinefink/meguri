//! `meguri-cli conformance` through its binary (specs/conformance/dsl.md D8,
//! D12), on the committed conformance tree.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn capabilities_list_what_meguri_runs_and_the_whole_language() {
    let out = cli(&["conformance", "capabilities"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["supports"]["ops"],
        serde_json::json!([
            "actor",
            "add_backup_files",
            "add_deck",
            "add_media",
            "add_note",
            "add_note_tags",
            "add_notetype",
            "add_preset",
            "add_stock_notetype",
            "adopt",
            "advance",
            "answer",
            "backup",
            "browser_config_set",
            "bury",
            "clear_unused_tags",
            "clone_notetype",
            "clone_preset",
            "db_exec",
            "delete_decks",
            "delete_notes",
            "delete_tags",
            "edit_note",
            "empty_filtered",
            "empty_media_trash",
            "enable_fsrs",
            "enable_v3",
            "ensure_io_notetype",
            "export",
            "export.colpkg",
            "force_backup",
            "forget",
            "grade_now",
            "handoff",
            "import",
            "import.colpkg",
            "legacy_update",
            "media_outside",
            "media_reset",
            "open",
            "optimize",
            "redo",
            "remove_config",
            "remove_note_tags",
            "remove_preset",
            "rename_deck",
            "rename_preset",
            "rename_tag",
            "reopen",
            "reparent_decks",
            "reparent_tags",
            "restore_media_trash",
            "review_session",
            "save_backup_settings",
            "save_deck_options",
            "set_clock",
            "set_config",
            "set_current_deck",
            "set_deck_collapsed",
            "set_deck_preset",
            "set_due",
            "set_flag",
            "set_scheduling_prefs",
            "set_tag_collapsed",
            "suspend",
            "sync",
            "trash_media",
            "unbury_deck",
            "undo",
            "unsuspend",
            "update_notetype",
            "update_notetype.add_field",
            "update_notetype.add_template",
            "update_notetype.css",
            "update_notetype.field_options",
            "update_notetype.move_field",
            "update_notetype.move_template",
            "update_notetype.remove_field",
            "update_notetype.remove_template",
            "update_notetype.rename",
            "update_notetype.rename_field",
            "update_notetype.rename_template",
            "update_notetype.sort_field",
            "update_notetype.template",
            "update_notetype.template_text",
        ])
    );
    assert!(
        json["language"]["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op == "answer.filtered")
    );
    assert!(
        !json["supports"]["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op == "answer.filtered")
    );
    // dsl.md D8: U28 bootstrap declares the new_collection capability.
    assert_eq!(
        json["supports"]["setup"],
        serde_json::json!([
            "config",
            "decks",
            "media_files",
            "new_collection",
            "notes",
            "notetypes",
            "presets",
        ])
    );
    assert_eq!(
        json["supports"]["observations"],
        serde_json::json!([
            "answer_labels",
            "backup_files",
            "backup_settings",
            "blob_decode",
            "browser_config",
            "canonical_search",
            "card",
            "card_excerpts",
            "card_extractors",
            "cloze",
            "congrats",
            "db_dump",
            "deck_counts",
            "deck_tree",
            "decks",
            "fields_check",
            "fsrs_next",
            "fuzz_table",
            "ids",
            "ignored_before_count",
            "image_occlusion",
            "legacy_json",
            "media_dir",
            "media_pending",
            "media_quick_check",
            "media_trash",
            "next_states",
            "notetype_structure",
            "notetype_templates",
            "package_listing",
            "queue",
            "render",
            "render_status",
            "review_html",
            "revlog",
            "schema_objects",
            "search",
            "tag_tree",
            "template",
            "template.card",
            "text",
            "timing",
            "type_answer",
            "undo_status",
        ])
    );
    // Prepared batch3/4 language must not advertise execution support.
    assert_eq!(json["language"]["observations"].as_array().unwrap().len(), 64);
    for kind in [
        "add_defaults",
        "notetype_last_deck",
        "search_build",
        "search_join",
        "search_replace",
        "tag_complete",
        "card_stats",
        "stats",
    ] {
        assert!(
            json["language"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == kind),
            "{kind}"
        );
        assert!(
            !json["supports"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == kind),
            "{kind}"
        );
    }
    for part in [
        "update_notetype.field_options.io",
        "export.apkg",
        "export.apkg_legacy",
        "export.apkg.legacy",
        "export.colpkg.media",
        "export.colpkg.legacy",
    ] {
        assert!(
            !json["supports"]["ops"].as_array().unwrap().iter().any(|v| v == part),
            "{part}"
        );
    }
    // dsl.md D7.2/D8: the current step vocabulary excludes observe and adds
    // the declared answer.filtered part, including parsing-only operations.
    assert_eq!(json["language"]["ops"].as_array().unwrap().len(), 87);
    for (op, supported) in [("media_reset", true), ("media_sync", false)] {
        assert!(
            json["language"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            "{op}"
        );
        assert_eq!(
            json["supports"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            supported,
            "{op}"
        );
    }
    for op in [
        "add_backup_files",
        "force_backup",
        "optimize",
        "save_backup_settings",
        "backup",
        "export",
        "import",
    ] {
        assert!(
            json["language"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            "{op}"
        );
        assert!(
            json["supports"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            "{op}"
        );
    }
    // Form capabilities are supported parts, not standalone step vocabulary.
    for op in ["export.colpkg", "import.colpkg"] {
        assert!(
            json["supports"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            "{op}"
        );
        assert!(
            !json["language"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            "{op}"
        );
    }
    for (kind, supported) in [
        ("backup_files", true),
        ("backup_settings", true),
        ("package_listing", true),
        ("package_col", false),
    ] {
        assert!(
            json["language"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k == kind),
            "{kind}"
        );
        assert_eq!(
            json["supports"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k == kind),
            supported,
            "{kind}"
        );
    }
    for op in [
        "actor",
        "sync",
        "net_fault",
        "converge",
        "reopen",
        "open",
        "adopt",
        "set_config",
        "add_deck",
        "set_deck_preset",
        "save_deck_options",
        "set_scheduling_prefs",
        "enable_fsrs",
        "enable_v3",
        "add_notetype",
        "update_notetype",
        "undo",
        "redo",
        "remove_config",
        "rename_tag",
        "set_flag",
        "browser_config_set",
        "unsuspend",
        "unbury_deck",
        "grade_now",
        "empty_filtered",
    ] {
        assert!(
            json["language"]["ops"].as_array().unwrap().iter().any(|o| o == op),
            "{op}"
        );
    }
    assert!(
        json["language"]["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "handoff")
    );
    assert!(
        json["language"]["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "legacy_update")
    );
    assert!(
        json["language"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "next_states")
    );
    assert!(
        json["language"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "legacy_json")
    );
    // undo-ops.md V2.
    assert!(
        json["language"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "undo_status")
    );
    // observations.md O4: these kinds belong to the language; U36 runs
    // blob_decode, while package_col remains owned by package export.
    for kind in ["blob_decode", "package_col", "ignored_before_count"] {
        assert!(
            json["language"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k == kind),
            "{kind}"
        );
    }
    for kind in ["media_quick_check", "review_html", "type_answer"] {
        assert!(
            json["language"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k == kind),
            "{kind}"
        );
    }
}

#[test]
fn exec_prints_observations_and_reports_pending_scenarios() {
    let empty = conformance().join("scenarios/smoke/runner-empty.yaml");
    let out = cli(&["conformance", "exec", empty.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "{\n  \"steps\": []\n}\n");

    let fsrs = conformance().join("scenarios/smoke/fsrs-new-card.yaml");
    let out = cli(&["conformance", "exec", fsrs.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let actual: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        actual["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.get("card").is_some())
    );
    assert!(
        actual["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.get("revlog").is_some())
    );
}

#[test]
fn exec_forced_backup_uses_the_fixture_harness_and_preserves_exact_output() {
    let root = conformance();
    let path = root.join("scenarios/package/backups/foreign-files.yaml");
    let out = cli(&["conformance", "exec", path.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(out.stderr.is_empty(), "{}", text(&out.stderr));
    let golden: Value =
        serde_json::from_slice(&std::fs::read(root.join("goldens/package/backups/foreign-files.json")).unwrap())
            .unwrap();
    assert_eq!(
        text(&out.stdout),
        meguri_testkit::observe::golden_text(&golden["observations"])
    );
}

#[test]
fn diff_and_run_compare_with_the_goldens() {
    let dir = conformance();
    let empty = dir.join("scenarios/smoke/runner-empty.yaml");
    let out = cli(&[
        "conformance",
        "diff",
        "--conformance",
        dir.to_str().unwrap(),
        empty.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", text(&out.stdout));
    assert_eq!(text(&out.stdout), "smoke/runner-empty: pass [active]\n");

    let report = tempfile::tempdir().unwrap();
    let path = report.path().join("t0.json");
    let out = cli(&[
        "conformance",
        "run",
        "--conformance",
        dir.to_str().unwrap(),
        "--report",
        path.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let json: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["tier"], "t0");
    assert_eq!(json["failed"], 0);
    let scenarios = json["scenarios"].as_array().unwrap();
    assert!(scenarios.len() >= 8);
    let passing: Vec<&str> = scenarios
        .iter()
        .filter(|s| s["status"] == "pass")
        .map(|s| s["scenario"].as_str().unwrap())
        .collect();
    assert_eq!(json["totals"]["pass"], passing.len());
    assert_eq!(json["totals"]["reference"], 2);
    let references: Vec<&Value> = scenarios.iter().filter(|s| s["status"] == "reference").collect();
    assert_eq!(
        references
            .iter()
            .map(|s| s["scenario"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["preservation/recipe/kitchen-sink", "preservation/review/answers"]
    );
    for reference in references {
        assert_eq!(reference["active"], false);
        assert_eq!(reference["fails"], false);
        assert!(!reference["reference"]["reason"].as_str().unwrap().is_empty());
        assert_eq!(reference["reference"]["spec"], "conformance/normalized-dump#N10");
        assert!(text(&out.stderr).contains(reference["reference"]["reason"].as_str().unwrap()));
    }
    // The scenarios Meguri has adapters for: the empty runner and the text
    // area (meguri-text, U20), all of whose 53 scenarios are active.
    assert!(passing.contains(&"smoke/runner-empty"), "{passing:?}");
    assert!(
        passing.iter().filter(|id| id.starts_with("text/")).count() >= 53,
        "{passing:?}"
    );

    // A filter that matches nothing is a failure, not a silent pass.
    let out = cli(&[
        "conformance",
        "run",
        "--conformance",
        dir.to_str().unwrap(),
        "sched/nothing",
    ]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn to_json_prints_parsed_values_and_flags_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("a.yaml");
    let bad = dir.path().join("b.yaml");
    std::fs::write(&good, "a: 017\nb: yes\n").unwrap();
    std::fs::write(&bad, "a: &x 1\nb: *x\n").unwrap();
    let out = cli(&["conformance", "to-json", good.to_str().unwrap(), bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let lines: Vec<Value> = text(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["value"], serde_json::json!({"a": 17, "b": "yes"}));
    assert!(lines[1]["error"].as_str().unwrap().contains("alias"));
}

#[test]
fn to_json_observation_mode_preserves_exact_integer_values_and_float_kind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("observation.json");
    std::fs::write(
        &path,
        r#"{"nested":[-9223372036854775808,12345678901234567890,18446744073709551615,1.0]}"#,
    )
    .unwrap();
    let filename = path.to_str().unwrap();
    let strict = cli(&["conformance", "to-json", filename]);
    assert_eq!(strict.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&strict.stdout).unwrap();
    assert_eq!(error["path"], filename);
    assert!(error["error"].as_str().unwrap().contains("outside ±2^53"));

    let out = cli(&["conformance", "to-json", "--observation-json", "--", filename]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout).lines().count(), 1);
    let parsed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(parsed["path"], filename);
    let values = &parsed["value"]["nested"];
    assert_eq!(values[0].as_i64(), Some(i64::MIN));
    assert_eq!(values[1].as_u64(), Some(12_345_678_901_234_567_890));
    assert_eq!(values[2].as_u64(), Some(u64::MAX));
    assert!(values[3].is_f64());
    assert_eq!(values[3].as_f64(), Some(1.0));
    assert!(
        text(&out.stdout).contains(r#""nested":[-9223372036854775808,12345678901234567890,18446744073709551615,1.0]"#)
    );

    std::fs::write(
        &path,
        r#"{"nested":[0,1,-0,1.0,1e0,1E0,-0.0,18446744073709551616.0,-9223372036854775809.0,1.5e300,1.2345678901234567]}"#,
    )
    .unwrap();
    let ordinary = cli(&["conformance", "to-json", filename]);
    let observation = cli(&["conformance", "to-json", "--observation-json", filename]);
    assert_eq!(ordinary.status.code(), Some(0));
    assert_eq!(observation.status.code(), Some(0));
    assert_eq!(ordinary.stdout, observation.stdout);
    let parsed = meguri_testkit::observe::parse_json(&text(&observation.stdout)).unwrap();
    let values = parsed["value"]["nested"].as_array().unwrap();
    assert_eq!(values.len(), 11);
    for (value, expected) in values[..2].iter().zip([0, 1]) {
        assert_eq!(value.as_i64(), Some(expected));
        assert!(!value.is_f64());
    }
    for (value, expected) in values[2..].iter().zip([
        "-0",
        "1.0",
        "1e0",
        "1E0",
        "-0.0",
        "18446744073709551616.0",
        "-9223372036854775809.0",
        "1.5e300",
        "1.2345678901234567",
    ]) {
        assert!(value.is_f64());
        if matches!(expected, "-0" | "-0.0") {
            assert!(value.as_f64().unwrap().is_sign_negative());
        }
        assert_eq!(
            value.as_f64().unwrap().to_bits(),
            expected.parse::<f64>().unwrap().to_bits()
        );
    }
    assert_eq!(
        cli(&["conformance", "capabilities", "--observation-json"])
            .status
            .code(),
        Some(2)
    );
    let yaml = dir.path().join("observation.yaml");
    std::fs::write(&yaml, "value: 1\n").unwrap();
    let out = cli(&["conformance", "to-json", "--observation-json", yaml.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("requires a .json file"));
}

#[test]
fn to_json_observation_mode_keeps_outside_64_bit_integer_float_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outside64.json");
    for number in ["18446744073709551616", "-9223372036854775809"] {
        std::fs::write(&path, format!(r#"{{"nested":[{{"value":{number}}}]}}"#)).unwrap();
        let filename = path.to_str().unwrap();
        let authored = cli(&["conformance", "to-json", filename]);
        assert_eq!(authored.status.code(), Some(1), "{number}");
        let item: Value = serde_json::from_slice(&authored.stdout).unwrap();
        assert_eq!(item["path"], filename);
        assert!(item["error"].as_str().unwrap().contains("outside ±2^53"));
        assert!(item.get("value").is_none());

        let observation = cli(&["conformance", "to-json", "--observation-json", filename]);
        assert_eq!(observation.status.code(), Some(0), "{}", text(&observation.stderr));
        let item = meguri_testkit::observe::parse_json(&text(&observation.stdout)).unwrap();
        assert_eq!(item["path"], filename);
        let value = &item["value"]["nested"][0]["value"];
        assert!(value.is_f64(), "{number}");
        assert_eq!(
            value.as_f64().unwrap().to_bits(),
            number.parse::<f64>().unwrap().to_bits()
        );
        assert!(item.get("error").is_none());
    }
}

#[test]
fn to_json_both_modes_reject_invalid_json_and_continue_reporting_each_path() {
    let dir = tempfile::tempdir().unwrap();
    let documents = [
        r#"{"nested":{"a":1,"a":2}}"#,
        r#"{"nested":{"a":1,"\u0061":2}}"#,
        r#"{"a": [1,]}"#,
        r#"{"a": NaN}"#,
        r#"{"a": Infinity}"#,
        r#"{"a": -Infinity}"#,
        r#"{"a": 1e999}"#,
        r#"{"a": 1} {}"#,
        r#"{"a":1,"f":1.0}"#,
    ];
    let paths: Vec<_> = documents
        .iter()
        .enumerate()
        .map(|(index, document)| {
            let path = dir.path().join(format!("{index}.json"));
            std::fs::write(&path, document).unwrap();
            path.to_str().unwrap().to_owned()
        })
        .collect();
    for observations in [false, true] {
        let mut args = vec!["conformance", "to-json"];
        if observations {
            args.push("--observation-json");
        }
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
        let out = cli(&args);
        assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
        let items: Vec<Value> = text(&out.stdout)
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(items.len(), documents.len());
        for (index, item) in items.iter().enumerate() {
            assert_eq!(item["path"], paths[index]);
            if index + 1 == documents.len() {
                assert_eq!(item["value"], serde_json::json!({"a": 1, "f": 1.0}));
                assert!(item["value"]["f"].is_f64());
            } else {
                assert!(item["error"].is_string(), "{item}");
                assert!(item.get("value").is_none());
                if index < 2 {
                    assert!(item["error"].as_str().unwrap().contains("duplicate key"));
                }
            }
        }
    }
}

#[test]
fn schema_checker_uses_observation_conversion_and_keeps_authored_d2_limits() {
    use serde_json::json;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("conformance");
    for folder in [
        "schema",
        "scenarios/example",
        "goldens/example",
        "goldens-apple/example",
    ] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    for name in ["scenario", "observation", "golden", "area", "deviations", "overlay"] {
        std::fs::copy(
            conformance().join(format!("schema/{name}.schema.json")),
            root.join(format!("schema/{name}.schema.json")),
        )
        .unwrap();
    }
    let hash = "0".repeat(64);
    let numbers = json!({"min": i64::MIN, "wide": 12_345_678_901_234_567_890_u64,
        "max": u64::MAX, "float": 1.0});
    let golden = json!({"scenario_id": "example/wide", "scenario_sha256": hash,
        "oracle": {"anki_version": "fixture", "lock_sha256": hash, "driver_sha256": hash},
        "tz": "UTC", "clock": {"start": "2030-01-15T10:00:00", "mode": "frozen"},
        "observations": {"steps": [{"file_col": {"conf": numbers}}]}});
    let overlay = json!({"scenario_id": "example/wide", "scenario_sha256": hash,
        "linux_observations_sha256": hash,
        "oracle": {"anki_version": "fixture", "lock_sha256": hash, "driver_sha256": hash,
            "platform": "macos-arm64"},
        "entries": [
            {"pointer": "/steps/0/file_col/conf/min", "linux": i64::MIN, "apple": i64::MIN},
            {"pointer": "/steps/0/file_col/conf/wide", "linux": numbers["wide"], "apple": numbers["wide"]},
            {"pointer": "/steps/0/file_col/conf/max", "linux": u64::MAX, "apple": u64::MAX},
            {"pointer": "/steps/0/file_col/conf/float", "linux": 1.0, "apple": 1.0}]});
    for (folder, value) in [("goldens", &golden), ("goldens-apple", &overlay)] {
        std::fs::write(root.join(format!("{folder}/example/wide.json")), value.to_string()).unwrap();
    }
    let scenario = json!({"id": "example/safe", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
        "setup": {"config": {"nested": [9_007_199_254_740_992_i64, -9_007_199_254_740_992_i64]}}});
    std::fs::write(root.join("scenarios/example/safe.json"), scenario.to_string()).unwrap();
    let yaml = "id: example/safe\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsetup: {config: {nested: [9007199254740992, -9007199254740992]}}\n";
    std::fs::write(root.join("scenarios/example/safe.yaml"), yaml).unwrap();
    let report = dir.path().join("report.json");
    let check = |jobs: &str| {
        let out = Command::new("python3")
            .args(["-B", "-I"])
            .arg(conformance().join("../tools/conformance/check_schemas.py"))
            .arg("--cli")
            .arg(env!("CARGO_BIN_EXE_meguri-cli"))
            .arg("--conformance")
            .arg(&root)
            .arg("--report")
            .arg(&report)
            .args(["--jobs", jobs])
            .output()
            .unwrap();
        let parsed: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
        (out, parsed)
    };
    let (out, parsed) = check("1");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(parsed, json!({"files": 4, "errors": []}));

    let mut expected_errors = Vec::new();
    for number in ["9007199254740993", "-9007199254740993"] {
        for suffix in ["json", "yaml"] {
            let rel = format!("scenarios/example/{number}.{suffix}");
            let document = if suffix == "json" {
                let mut value = scenario.clone();
                value["setup"]["config"]["nested"] = json!([{"value": number.parse::<i64>().unwrap()}]);
                value.to_string()
            } else {
                yaml.replace(
                    "[9007199254740992, -9007199254740992]",
                    &format!("[{{value: {number}}}]"),
                )
            };
            let path = root.join(&rel);
            std::fs::write(&path, &document).unwrap();
            let error = meguri_testkit::scenario::load(&path).unwrap_err().to_string();
            assert!(error.contains("outside ±2^53"), "{rel}: {error}");
            if suffix == "json" {
                assert!(meguri_testkit::scenario::parse_json(&document).is_err());
            }
            expected_errors.push((rel, "outside ±2^53"));
        }
    }
    for number in [
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
    ] {
        let rel = format!("scenarios/example/{number}.json");
        let document = scenario.to_string().replace(
            "[9007199254740992,-9007199254740992]",
            &format!("[{{\"value\":{number}}}]"),
        );
        assert!(document.contains(&format!("\"value\":{number}")));
        let path = root.join(&rel);
        std::fs::write(&path, &document).unwrap();
        let error = meguri_testkit::scenario::load(&path).unwrap_err().to_string();
        assert!(meguri_testkit::scenario::parse_json(&document).is_err());
        let out = cli(&["conformance", "to-json", path.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(1), "{number}");
        assert_eq!(text(&out.stdout).lines().count(), 1);
        let item: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(item["path"], path.to_str().unwrap());
        assert!(item["error"].is_string(), "{number}: {item}");
        assert!(item.get("value").is_none(), "{number}: {item}");
        assert!(error.contains("outside ±2^53"), "{number}: {error}");
        assert!(item["error"].as_str().unwrap().contains("outside ±2^53"));
        expected_errors.push((rel, "outside ±2^53"));
    }
    for folder in ["goldens", "goldens-apple"] {
        for (name, document, why) in [
            ("duplicate", r#"{"nested":{"a":1,"a":2}}"#, "duplicate key"),
            ("decoded", r#"{"nested":{"a":1,"\u0061":2}}"#, "duplicate key"),
            ("malformed", r#"{"a":[1,]}"#, "JSON:"),
            ("nonfinite", r#"{"a":1e999}"#, "JSON:"),
            ("trailing", r#"{} {}"#, "JSON:"),
            ("shape", "{}", "required"),
        ] {
            let rel = format!("{folder}/example/{name}.json");
            std::fs::write(root.join(&rel), document).unwrap();
            expected_errors.push((rel, why));
        }
    }
    expected_errors.sort_by(|a, b| a.0.cmp(&b.0));
    let (out, parsed) = check("2");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert_eq!(parsed["files"], 24);
    // Shape failures can report multiple required fields; every bad input
    // must still appear, and every diagnostic must keep its original path.
    let errors = parsed["errors"].as_array().unwrap();
    let mut actual_files: Vec<_> = errors.iter().map(|error| error["file"].as_str().unwrap()).collect();
    actual_files.dedup();
    assert_eq!(
        actual_files,
        expected_errors
            .iter()
            .map(|(file, _)| file.as_str())
            .collect::<Vec<_>>()
    );
    for (file, why) in expected_errors {
        assert!(
            errors
                .iter()
                .any(|error| error["file"] == file && error["message"].as_str().unwrap().contains(why)),
            "{file}: {parsed}"
        );
    }
}

#[test]
fn to_json_reads_quoted_keys_and_flow_mappings_as_the_same_members() {
    // tools/mg finds actor scenarios by the top-level `actors` member that
    // to-json prints (sync-ops.md S2.1), so every spelling of the key must
    // come out as that member.
    let dir = tempfile::tempdir().unwrap();
    let texts = [
        "id: smoke/sync/x\nactors: [client_a, client_c]\n",
        "id: smoke/sync/x\n'actors': [client_a, client_c]\n",
        "id: smoke/sync/x\n\"actors\": [client_a, client_c]\n",
        "{id: smoke/sync/x, actors: [client_a, client_c]}\n",
        "{\"id\": \"smoke/sync/x\", 'actors': [client_a, client_c]}\n",
    ];
    let paths: Vec<String> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let path = dir.path().join(format!("s{i}.yaml"));
            std::fs::write(&path, text).unwrap();
            path.to_str().unwrap().to_owned()
        })
        .collect();
    let mut args = vec!["conformance", "to-json"];
    args.extend(paths.iter().map(String::as_str));
    let out = cli(&args);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let values: Vec<Value> = text(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["value"].clone())
        .collect();
    assert_eq!(values.len(), texts.len());
    for value in values {
        assert_eq!(
            value,
            serde_json::json!({"id": "smoke/sync/x", "actors": ["client_a", "client_c"]})
        );
    }
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(cli(&["conformance", "frobnicate"]).status.code(), Some(2));
    assert_eq!(cli(&["conformance", "run", "--jobs"]).status.code(), Some(2));
    assert_eq!(cli(&["conformance", "run", "--jobs", "0"]).status.code(), Some(2));
    assert_eq!(cli(&[]).status.code(), Some(2));
}

#[test]
fn t0_accepts_explicit_jobs_and_preserves_the_selected_report() {
    let dir = tree();
    let report = dir.path().join("report.json");
    for jobs in ["1", "2"] {
        let out = cli(&[
            "conformance",
            "run",
            "--conformance",
            dir.path().to_str().unwrap(),
            "--jobs",
            jobs,
            "--report",
            report.to_str().unwrap(),
            "smoke/timing-tz-utc",
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        let value: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
        assert_eq!(value["tier"], "t0");
        assert_eq!(value["scenarios"].as_array().unwrap().len(), 1);
        assert_eq!(value["scenarios"][0]["scenario"], "smoke/timing-tz-utc");
        assert_eq!(value["scenarios"][0]["status"], "pass");
    }
}

const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";

/// A temporary tree with the committed timing-tz-utc scenario and golden,
/// inactive.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, from) in [
        ("oracle.json", "oracle.json"),
        ("deviations.yaml", "deviations.yaml"),
        (
            "scenarios/smoke/timing-tz-utc.yaml",
            "scenarios/smoke/timing-tz-utc.yaml",
        ),
        ("goldens/smoke/timing-tz-utc.json", "goldens/smoke/timing-tz-utc.json"),
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(conformance().join(from), path).unwrap();
    }
    dir
}

/// A stand-in oracle worker that answers every run with `observations`.
fn worker(dir: &Path, observations: &str, lock: &str) -> String {
    let script = dir.join("worker.sh");
    std::fs::write(
        &script,
        format!(
            r#"while IFS= read -r line; do
  case "$line" in
    *'"cmd":"hello"'*) printf '{{"ok":true,"protocol":1,"anki_version":"26.9.3","lock_sha256":"{lock}","driver_sha256":"%s","tz":"%s"}}\n' "$(printf 'b%.0s' $(seq 64))" "$TZ" ;;
    *) printf '%s\n' '{{"ok":true,"scenario_id":"x","elapsed_ms":1,"observations":{observations}}}' ;;
  esac
done
"#
        ),
    )
    .unwrap();
    serde_json::to_string(&["env", "TZ={tz}", "sh", script.to_str().unwrap()]).unwrap()
}

fn run_live(root: &Path, cmd: &str) -> (Option<i32>, Value) {
    let report = root.join("t1.json");
    let out = cli(&[
        "conformance",
        "run",
        "--live",
        "--conformance",
        root.to_str().unwrap(),
        "--oracle-cmd",
        cmd,
        "--report",
        report.to_str().unwrap(),
    ]);
    let json = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    (out.status.code(), json)
}

#[test]
fn run_live_fails_on_oracle_drift_or_errors_in_scenarios_that_are_not_active() {
    let dir = tree();
    let root = dir.path();
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("goldens/smoke/timing-tz-utc.json")).unwrap()).unwrap();
    let recorded = serde_json::to_string(&golden["observations"]).unwrap();

    let (code, json) = run_live(root, &worker(root, &recorded, LOCK));
    assert_eq!(code, Some(0), "{json}");
    assert_eq!(
        (json["tier"].as_str(), json["scenarios"][0]["active"].as_bool()),
        (Some("t1"), Some(false))
    );
    assert_eq!(json["oracle"]["lock_sha256"], LOCK);
    // Each result names the scenario text it ran (harvest-report checks it).
    assert_eq!(json["scenarios"][0]["scenario_sha256"], golden["scenario_sha256"]);

    // The oracle no longer reproduces the golden of an inactive scenario.
    let drifted = recorded.replace("\"now\":", "\"now\":1,\"was\":");
    assert_ne!(drifted, recorded);
    let (code, json) = run_live(root, &worker(root, &drifted, LOCK));
    assert_eq!(code, Some(1), "{json}");
    assert_eq!(json["scenarios"][0]["status"], "fail");
    assert_eq!(json["scenarios"][0]["fails"], true);

    // Workers of another oracle than the pin: every scenario is an error.
    let (code, json) = run_live(root, &worker(root, &recorded, &"0".repeat(64)));
    assert_eq!(code, Some(1), "{json}");
    assert_eq!(json["scenarios"][0]["status"], "error");
    assert!(
        json["scenarios"][0]["message"]
            .as_str()
            .unwrap()
            .contains("oracle_mismatch")
    );
}

#[test]
fn run_fails_on_an_inactive_scenario_that_breaks_the_schema() {
    let dir = tree();
    let root = dir.path();
    let path = root.join("scenarios/sched/filtered.yaml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "id: sched/filtered\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{build_filtered: {}}]\n",
    )
    .unwrap();
    let out = cli(&["conformance", "run", "--conformance", root.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("sched/filtered: invalid"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn goldens_check_lists_each_changed_value_with_before_and_after() {
    let dir = tree();
    let root = dir.path();
    let base = tempfile::tempdir().unwrap();
    let golden_path = root.join("goldens/smoke/timing-tz-utc.json");
    let mut golden: Value = serde_json::from_str(&std::fs::read_to_string(&golden_path).unwrap()).unwrap();
    std::fs::create_dir_all(base.path().join("smoke")).unwrap();
    std::fs::write(base.path().join("smoke/timing-tz-utc.json"), golden.to_string()).unwrap();
    let before = golden["observations"]["steps"][0]["timing"]["now"].clone();
    golden["observations"]["steps"][0]["timing"]["now"] = serde_json::json!(7);
    std::fs::write(&golden_path, golden.to_string()).unwrap();
    let report = root.join("fresh.json");
    let out = cli(&[
        "conformance",
        "goldens-check",
        "--conformance",
        root.to_str().unwrap(),
        "--base",
        base.path().to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains(&format!("smoke/timing-tz-utc /steps/0/timing/now: {before} -> 7")),
        "{stderr}"
    );
    let json: Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let value = &json["changed"][0]["values"][0];
    assert_eq!(
        (value["before"].clone(), value["after"].clone()),
        (before, serde_json::json!(7))
    );
    assert_eq!(json["changed"][0]["count"], 1);
}

#[test]
fn harvest_report_refuses_a_live_report_from_another_oracle_pin() {
    let dir = tree();
    let root = dir.path();
    std::fs::create_dir_all(root.join("catalog")).unwrap();
    std::fs::write(
        root.join("catalog/harvest.csv"),
        "harvest_id,language,source,area,classification,reason,scenarios,status\n\
         h0123456789ab,rust,tag,smoke,in-scope,The day boundary in UTC,smoke/timing-tz-utc,done\n",
    )
    .unwrap();
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("goldens/smoke/timing-tz-utc.json")).unwrap()).unwrap();
    let report = |lock: &str| {
        let path = root.join(format!("t1-{}.json", &lock[..8]));
        let json = serde_json::json!({
            "tier": "t1", "oracle_pin": {"anki_version": "26.9.3", "lock_sha256": lock},
            "scenarios": [{"scenario": "smoke/timing-tz-utc", "status": "pass", "scenario_sha256": golden["scenario_sha256"]}],
        });
        std::fs::write(&path, json.to_string()).unwrap();
        cli(&[
            "conformance",
            "harvest-report",
            "--conformance",
            root.to_str().unwrap(),
            "--live-report",
            path.to_str().unwrap(),
        ])
    };
    let out = report(&"0".repeat(64));
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("oracle pin"), "{}", text(&out.stderr));
    let out = report(LOCK);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("in-scope: 1 rows"), "{}", text(&out.stderr));
}

#[test]
fn run_live_checks_the_fault_proxy_api_before_any_worker_starts() {
    use meguri_testkit::exec::sync_guard::ANKIWEB_HOST;

    let dir = tree();
    let root = dir.path();
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("goldens/smoke/timing-tz-utc.json")).unwrap()).unwrap();
    let recorded = serde_json::to_string(&golden["observations"]).unwrap();
    // A worker that leaves a mark when it starts.
    let started = root.join("worker-started");
    let script = root.join("marking.sh");
    std::fs::write(
        &script,
        format!(
            "touch '{}'\nexec sh '{}'\n",
            started.display(),
            root.join("worker.sh").display()
        ),
    )
    .unwrap();
    let _ = worker(root, &recorded, LOCK);
    let cmd = serde_json::to_string(&["env", "TZ={tz}", "sh", script.to_str().unwrap()]).unwrap();
    let run = |api: &str| {
        Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
            .args([
                "conformance",
                "run",
                "--live",
                "--conformance",
                root.to_str().unwrap(),
                "--oracle-cmd",
                &cmd,
            ])
            .env("ORACLE_SYNC_FAULTS", api)
            .output()
            .unwrap()
    };

    // sync-ops.md S8.3: an API the guard refuses stops the run first.
    for api in [format!("api.{ANKIWEB_HOST}:8474"), "203.0.113.5:8474".to_owned()] {
        let out = run(&api);
        assert_eq!(out.status.code(), Some(2), "{api}: {}", text(&out.stderr));
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains("ORACLE_SYNC_FAULTS") && stderr.contains("refused"),
            "{stderr}"
        );
        assert!(!started.exists(), "{api}: a worker started");
    }
    // The compose service passes, and empty means no fault proxy.
    for api in ["sync-faults:8474", ""] {
        let out = run(api);
        assert_eq!(out.status.code(), Some(0), "{api:?}: {}", text(&out.stderr));
        assert!(started.exists());
    }
}

#[test]
fn exec_sends_actor_scenarios_to_the_parties_executor() {
    let downloads = conformance().join("scenarios/smoke/sync/meguri-downloads.yaml");
    let out = cli(&["conformance", "exec", downloads.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("smoke/sync/meguri-downloads: an actor scenario"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn explicit_reference_selection_never_runs_meguri_and_worker_failures_still_fail() {
    let dir = tree();
    let root = dir.path();
    std::fs::write(
        root.join("scenarios/smoke/area.yaml"),
        "area: smoke\nreference:\n  timing-tz-utc:\n    reason: Scope example.\n    spec: conformance/dsl#D9.4\n",
    )
    .unwrap();
    let path = root.join("scenarios/smoke/timing-tz-utc.yaml");
    for command in ["exec", "diff"] {
        let out = cli(&[
            "conformance",
            command,
            "--conformance",
            root.to_str().unwrap(),
            path.to_str().unwrap(),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        let report = text(&out.stdout) + &text(&out.stderr);
        assert!(report.contains("reference"), "{report}");
        assert!(report.contains("Scope example. (conformance/dsl#D9.4)"));
        assert!(!report.contains("timing-tz-utc: pass"));
    }
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("goldens/smoke/timing-tz-utc.json")).unwrap()).unwrap();
    let observed = golden["observations"].to_string();
    for (observations, lock, expected, code) in [
        (observed.as_str(), LOCK.to_owned(), "reference", 0),
        ("{\"steps\":[]}", LOCK.to_owned(), "fail", 1),
        (observed.as_str(), "0".repeat(64), "error", 1),
    ] {
        let cmd = worker(root, observations, &lock);
        let (actual_code, json) = run_live(root, &cmd);
        assert_eq!(actual_code, Some(code), "{json}");
        assert_eq!(json["scenarios"][0]["status"], expected);
        assert_eq!(json["totals"]["pass"], 0);
        assert_eq!(
            json["scenarios"][0]["reference"],
            serde_json::json!({
                "reason": "Scope example.", "spec": "conformance/dsl#D9.4"
            })
        );
    }
}
