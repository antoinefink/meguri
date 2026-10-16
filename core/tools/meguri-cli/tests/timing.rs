//! `meguri-cli timing run` through the binary (timed-operations.md T1, T2,
//! T7, T10; U18): the report on a generated collection, the defaults from
//! its manifest, `--op` and `--repeat`, the error reports and the scratch
//! directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

/// A generated collection of about 300 cards with its manifest.
fn generated(root: &Path) -> PathBuf {
    let dir = root.join("collection");
    let out = cli(&["generate", "300", "--seed", "5", "--out", path(&dir)]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    dir
}

const CATALOGUE: [&str; 22] = [
    "collection.open",
    "decks.counts",
    "queue.build",
    "review.answer",
    "search.deck",
    "search.state",
    "search.tag",
    "search.text",
    "search.regex",
    "search.prop",
    "search.rated",
    "search.combined",
    "browser.sort_field",
    "browser.sort_due",
    "stats.graphs",
    "maintenance.check_db",
    "package.apkg_export",
    "package.colpkg_export",
    "package.apkg_import",
    "sync.full_upload",
    "sync.full_download",
    "fsrs.recompute_memory_states",
];

/// K1's zone for a manifest offset, restated from the spec.
fn k1_zone(offset: i64) -> String {
    match offset / 3600 {
        0 => "UTC".to_owned(),
        h if h > 0 => format!("Etc/GMT-{h}"),
        h => format!("Etc/GMT+{}", -h),
    }
}

#[test]
fn a_run_reports_every_operation_with_the_manifests_clock_and_the_collections_counts() {
    let root = tempfile::tempdir().unwrap();
    let dir = generated(root.path());
    let manifest = read_json(&dir.join("manifest.json"));
    let before = fs::read(dir.join("collection.anki2")).unwrap();
    let out = root.path().join("out");
    let scratch = root.path().join("scratch");

    let run = cli(&[
        "timing",
        "run",
        path(&dir),
        "--out",
        path(&out),
        "--scratch",
        path(&scratch),
    ]);
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(!out.join("timing-error.json").exists());
    let text = fs::read_to_string(out.join("timing.json")).unwrap();
    assert!(
        text.starts_with("{\n  \"collection\": {"),
        "sorted keys, two-space indents"
    );
    assert!(text.ends_with("}\n"));
    let report: Value = serde_json::from_str(&text).unwrap();

    assert_eq!(report["report_version"], json!(1));
    assert_eq!(report["side"], json!("meguri"));
    let platform = if cfg!(target_os = "macos") {
        "macos-arm64"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x86_64"
    };
    assert_eq!(report["platform"], json!(platform));
    assert!(report.get("oracle").is_none());
    // Tests build the dev profile; no MEGURI_BUILD_COMMIT was given.
    assert_eq!(report["meguri"], json!({"commit": null, "profile": "dev"}));
    assert_eq!(report["protocol"], json!({"warmup": 1, "repeat": 5}));
    assert_eq!(
        report["now"],
        json!(manifest["now_ms"].as_i64().unwrap().div_euclid(1000))
    );
    assert_eq!(
        report["tz"],
        json!(k1_zone(manifest["utc_offset_secs"].as_i64().unwrap()))
    );

    let counts = &manifest["counts"];
    let collection = &report["collection"];
    for member in ["notes", "cards", "revlog", "notetypes", "decks", "presets", "tags"] {
        assert_eq!(collection[member], counts[member], "{member}");
    }
    assert_eq!(collection["graves"], json!(0));
    assert_eq!(collection.as_object().unwrap().len(), 8);

    // Every operation, in catalogue order. Meguri's engine has no entry
    // point the bridge opens collections through yet (CoreHost::open, U38),
    // so each one is unsupported: nothing ran, no samples, no check.
    let operations = report["operations"].as_array().unwrap();
    let names: Vec<&str> = operations.iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(names, CATALOGUE);
    for op in operations {
        assert_eq!(op.as_object().unwrap().len(), 2, "{op}");
        assert_eq!(op["status"], json!("unsupported"), "{op}");
    }

    assert_eq!(
        fs::read(dir.join("collection.anki2")).unwrap(),
        before,
        "<dir> is never written"
    );
    assert_eq!(
        fs::read_dir(&scratch).unwrap().count(),
        0,
        "the scratch directory is deleted"
    );
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        !names.iter().any(|n| n.ends_with("-wal") || n.ends_with("-shm")),
        "{names:?}"
    );
}

#[test]
fn op_and_repeat_choose_what_runs_and_now_and_tz_override_the_manifest() {
    let root = tempfile::tempdir().unwrap();
    let dir = generated(root.path());
    let out = root.path().join("out");
    let run = cli(&[
        "timing",
        "run",
        path(&dir),
        "--out",
        path(&out),
        "--scratch",
        path(&root.path().join("scratch")),
        "--op",
        "sync.full_upload",
        "--op",
        "collection.open",
        "--op",
        "sync.full_upload",
        "--repeat",
        "3",
        "--now",
        "2031-03-30T01:30:00+02:00",
        "--tz",
        "Europe/Paris",
    ]);
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    let report = read_json(&out.join("timing.json"));
    let names: Vec<&str> = report["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["collection.open", "sync.full_upload"]);
    assert_eq!(report["protocol"]["repeat"], json!(3));
    // 2031-03-29T23:30:00Z
    assert_eq!(report["now"], json!(1_932_593_400));
    assert_eq!(report["tz"], json!("Europe/Paris"));
}

fn error_report(out: &Path) -> Value {
    assert!(!out.join("timing.json").exists());
    read_json(&out.join("timing-error.json"))
}

#[test]
fn a_failed_run_replaces_the_previous_success_report() {
    let root = tempfile::tempdir().unwrap();
    let dir = generated(root.path());
    let out = root.path().join("out");
    let args = ["timing", "run", path(&dir), "--out", path(&out)];
    assert_eq!(cli(&args).status.code(), Some(0));
    assert!(out.join("timing.json").exists());

    let mut invalid = args.to_vec();
    invalid.extend_from_slice(&["--repeat", "0"]);
    assert_eq!(cli(&invalid).status.code(), Some(2));
    assert_eq!(error_report(&out)["kind"], json!("usage"));
}

#[test]
fn a_successful_run_replaces_the_previous_error_report() {
    let root = tempfile::tempdir().unwrap();
    let dir = generated(root.path());
    let out = root.path().join("out");
    let args = ["timing", "run", path(&dir), "--out", path(&out)];
    let mut invalid = args.to_vec();
    invalid.extend_from_slice(&["--bogus", "1"]);
    assert_eq!(cli(&invalid).status.code(), Some(2));
    assert_eq!(error_report(&out)["kind"], json!("usage"));

    assert_eq!(cli(&args).status.code(), Some(0));
    assert_eq!(read_json(&out.join("timing.json"))["side"], json!("meguri"));
    assert!(!out.join("timing-error.json").exists());
}

#[test]
fn usage_errors_exit_2_and_say_so_in_the_error_report() {
    let root = tempfile::tempdir().unwrap();
    let dir = generated(root.path());
    let out = root.path().join("out");
    let cases: [&[&str]; 6] = [
        &["--op", "collection.close"],
        &["--repeat", "0"],
        &["--now", "2030-01-15T10:00:00"],
        &["--tz", "Nowhere/Town"],
        &["--bogus", "1"],
        &["--now", "2030-01-15T10:00:00Z", "--now", "2030-01-16T10:00:00Z"],
    ];
    for extra in cases {
        let _ = fs::remove_dir_all(&out);
        let mut args = vec!["timing", "run", path(&dir), "--out", path(&out)];
        args.extend_from_slice(extra);
        let run = cli(&args);
        assert_eq!(run.status.code(), Some(2), "{extra:?}");
        assert_eq!(error_report(&out)["kind"], json!("usage"), "{extra:?}");
    }
    // Without --out there is nowhere to write: only the exit code.
    assert_eq!(cli(&["timing", "run", path(&dir)]).status.code(), Some(2));
    assert_eq!(cli(&["timing", "walk"]).status.code(), Some(2));

    // No manifest: --now and --tz are needed.
    let bare = root.path().join("bare");
    fs::create_dir_all(&bare).unwrap();
    fs::copy(dir.join("collection.anki2"), bare.join("collection.anki2")).unwrap();
    let _ = fs::remove_dir_all(&out);
    assert_eq!(
        cli(&["timing", "run", path(&bare), "--out", path(&out)]).status.code(),
        Some(2)
    );
    assert_eq!(error_report(&out)["kind"], json!("usage"));
}

#[test]
fn a_missing_collection_or_a_file_that_is_not_one_is_a_run_error() {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("out");
    let empty = root.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    let clock = ["--now", "2030-01-15T10:00:00Z", "--tz", "UTC"];
    let mut args = vec!["timing", "run", path(&empty), "--out", path(&out)];
    args.extend_from_slice(&clock);
    assert_eq!(cli(&args).status.code(), Some(1));
    assert_eq!(error_report(&out)["kind"], json!("no_collection"));

    fs::write(empty.join("collection.anki2"), b"plain text, not a collection").unwrap();
    let _ = fs::remove_dir_all(&out);
    let scratch = root.path().join("scratch");
    let mut args = vec![
        "timing",
        "run",
        path(&empty),
        "--out",
        path(&out),
        "--scratch",
        path(&scratch),
    ];
    args.extend_from_slice(&clock);
    assert_eq!(cli(&args).status.code(), Some(1));
    assert_eq!(error_report(&out)["kind"], json!("open_failed"));
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}
