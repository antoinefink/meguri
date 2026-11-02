//! `meguri-cli generate` and `meguri-cli profile-extract` through the binary
//! (U11).

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn json(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "exit {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn generate_a_size_then_reuse_it_from_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("2k");
    let first = json(&cli(&["generate", "2000", "--seed", "3", "--out", path(&out)]));
    assert_eq!(first["cached"], false);
    let manifest = &first["manifest"];
    assert_eq!(manifest["kind"], "profile");
    assert_eq!(manifest["now_ms"], 1_894_701_600_000i64);
    assert_eq!(manifest["generator_version"], 5);
    assert!(first.get("request").is_none());
    assert!(first.get("observed").is_none());
    assert_eq!(manifest["seed"], 3);
    let cards = manifest["counts"]["cards"].as_u64().unwrap();
    assert!((1900..=2100).contains(&cards), "{cards} cards");
    assert!(out.join("collection.anki2").is_file());
    assert!(out.join("collection.media").is_dir());
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap(),
        *manifest
    );

    let again = json(&cli(&[
        "generate",
        "2000",
        "--seed",
        "3",
        "--out",
        path(&out),
        "--cache",
    ]));
    assert_eq!(again["cached"], true);
    assert_eq!(again["manifest"], *manifest);
    // Another seed in the same directory is not a cache hit.
    let other = json(&cli(&[
        "generate",
        "2000",
        "--seed",
        "4",
        "--out",
        path(&out),
        "--cache",
    ]));
    assert_eq!(other["cached"], false);
    assert_ne!(other["manifest"]["dump_sha256"], manifest["dump_sha256"]);
}

#[test]
fn generate_a_worst_case_and_a_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let worst = json(&cli(&[
        "generate",
        "worst-case",
        "decks-2000",
        "--cards",
        "2500",
        "--out",
        path(&dir.path().join("w")),
    ]));
    assert_eq!(worst["manifest"]["kind"], "worst-case");
    assert_eq!(worst["manifest"]["name"], "decks-2000");
    assert_eq!(worst["manifest"]["counts"]["decks"], 2000);

    let recipe = json(&cli(&[
        "generate",
        "recipe",
        "review-ready",
        "--out",
        path(&dir.path().join("r")),
    ]));
    assert_eq!(recipe["manifest"]["kind"], "recipe");
    assert_eq!(recipe["manifest"]["counts"]["cards"], 47);
}

/// A worst case is reused only while its definition is unchanged: a stress
/// value edited in its file makes it again.
#[test]
fn an_edited_worst_case_is_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wide.json");
    let write = |decks: u64| {
        let case = serde_json::json!({
            "name": "wide",
            "description": "Many decks.",
            "cards": 300,
            "decks": decks,
        });
        std::fs::write(&file, case.to_string()).unwrap();
    };
    let out = dir.path().join("out");
    let generate = || {
        json(&cli(&[
            "generate",
            "worst-case",
            path(&file),
            "--out",
            path(&out),
            "--cache",
        ]))
    };
    write(40);
    let first = generate();
    assert_eq!(first["cached"], false);
    assert_eq!(first["manifest"]["name"], "wide");
    assert_eq!(first["manifest"]["counts"]["decks"], 40);
    assert_eq!(generate()["cached"], true, "an unchanged worst case is reused");
    write(60);
    let edited = generate();
    assert_eq!(edited["cached"], false, "an edited worst case is made again");
    assert_eq!(edited["manifest"]["counts"]["decks"], 60);
}

/// A run waits while another holds its directory's lock, and publishes
/// nothing before it gets it.
#[test]
fn a_run_waits_for_its_directory_lock() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("r");
    let lock = meguri_testkit::generate::OutputLock::acquire(&out).unwrap();
    let mut run = Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
        .args(["generate", "recipe", "review-ready", "--out", path(&out)])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // A recipe takes well under a second once the lock is free.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(run.try_wait().unwrap().is_none(), "the run waits for the lock");
    assert!(!out.exists(), "nothing is published while the lock is held");
    drop(lock);
    let summary = json(&run.wait_with_output().unwrap());
    assert_eq!(summary["manifest"]["kind"], "recipe");
    assert!(out.join("manifest.json").is_file());
}

/// The entries next to `dir` that runs into it left (`.<name>.…`), its lock
/// file aside.
fn leftovers(dir: &Path) -> Vec<String> {
    let prefix = format!(".{}.", dir.file_name().unwrap().to_string_lossy());
    std::fs::read_dir(dir.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&prefix) && !name.ends_with(".lock"))
        .collect()
}

/// Runs into one directory at the same time, each asking for another
/// collection: every run succeeds, and what is left is one run's whole
/// output (its manifest describes the collection and media beside it).
#[test]
fn concurrent_runs_into_one_directory_leave_one_whole_output() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("shared");
    for round in 0..3 {
        let seed = round.to_string();
        let runs: Vec<_> = ["1000", "1500", "2000", "2500"]
            .iter()
            .map(|size| {
                Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
                    .args([
                        "generate",
                        size,
                        "--seed",
                        &seed,
                        "--media",
                        "files",
                        "--out",
                        path(&out),
                        "--cache",
                    ])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for run in runs {
            json(&run.wait_with_output().unwrap());
        }
        let manifest = meguri_testkit::generate::Manifest::load(&out).unwrap();
        assert_eq!(
            meguri_testkit::generate::dump::logical_sha256(&out.join("collection.anki2")).unwrap(),
            manifest.dump_sha256,
            "round {round}: the manifest describes the collection beside it"
        );
        assert_eq!(
            std::fs::read_dir(out.join("collection.media")).unwrap().count() as u64,
            manifest.counts.media_files,
            "round {round}: the media beside it"
        );
        assert_eq!(leftovers(&out), Vec::<String>::new(), "round {round}");
    }
}

#[test]
fn profile_extract_writes_a_profile_from_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("src");
    json(&cli(&["generate", "recipe", "typical", "--out", path(&source)]));
    let collection = source.join("collection.anki2");
    let before = std::fs::read(&collection).unwrap();
    let target = dir.path().join("shape.json");
    let out = cli(&[
        "profile-extract",
        path(&collection),
        "--media-dir",
        path(&source.join("collection.media")),
        "--tz",
        "Europe/Paris",
        "--out",
        path(&target),
    ]);
    let summary = json(&out);
    assert_eq!(summary["out"], path(&target));
    assert_eq!(std::fs::read(&collection).unwrap(), before);
    let profile = meguri_testkit::generate::Profile::load(&target).unwrap();
    assert_eq!(
        profile.provenance,
        meguri_testkit::generate::profile::Provenance::Extracted
    );
    assert_eq!(profile.utc_offset_secs, 3600, "Europe/Paris in January");
    assert!(profile.totals.cards > 0);
}

#[test]
fn bad_arguments_are_usage_errors() {
    for args in [
        &["generate"][..],
        &["generate", "lots"],
        &["generate", "10k"],
        &["generate", "recipe", "no-such-recipe", "--out", "/tmp/x"],
        &["profile-extract", "--out", "/tmp/x"],
        // F2.6: the media folder and the zone are required.
        &["profile-extract", "/tmp/c.anki2", "--tz", "UTC", "--out", "/tmp/x"],
        &[
            "profile-extract",
            "/tmp/c.anki2",
            "--media-dir",
            "/tmp/m",
            "--out",
            "/tmp/x",
        ],
        &[
            "profile-extract",
            "/tmp/c.anki2",
            "--media-dir",
            "/tmp/m",
            "--tz",
            "Nowhere/Atlantis",
            "--out",
            "/tmp/x",
        ],
    ] {
        let out = cli(args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn cloud_small_generates_real_files_and_retains_request_and_readback_on_cache_hits() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("small");
    let first = json(&cli(&["generate", "cloud-small", "--seed", "63", "--out", path(&out)]));
    assert_eq!(first["cached"], false);
    let request = &first["request"];
    assert_eq!(request["seed"], 63);
    assert_eq!(request["now_ms"], 1_735_689_600_000i64);
    assert_eq!(request["utc_offset_secs"], 0);
    assert_eq!(request["rollover_hour"], 4);
    assert_eq!(request["media"], "files");
    assert_eq!(request["reviews_per_card"], 3);
    assert_eq!(request["review_days_before_pinned"], serde_json::json!([3, 2, 1]));
    assert_eq!(request["answer_seconds_after_day_start"], 28800);
    assert_eq!(request["answer_ordinal_seconds"], true);
    assert_eq!(first["manifest"]["kind"], "cloud-small");
    assert_eq!(
        first["observed"]["counts"],
        serde_json::json!({
            "notes": 20, "cards": 20, "revlog": 60, "decks": 2, "presets": 1,
            "notetypes": 1, "tags": 20, "media_refs": 5, "media_files": 5, "media_bytes": 51200,
        })
    );
    let manifest = meguri_testkit::generate::Manifest::load(&out).unwrap();
    assert_eq!(serde_json::to_value(manifest).unwrap(), first["manifest"]);
    assert_eq!(first["observed"]["counts"], first["manifest"]["counts"]);
    assert_eq!(first["observed"]["dump_sha256"], first["manifest"]["dump_sha256"]);
    let files: Vec<_> = std::fs::read_dir(out.join("collection.media"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 5);
    for file in &files {
        assert_eq!(std::fs::read(file).unwrap().len(), 10240);
    }
    let again = json(&cli(&[
        "generate",
        "cloud-small",
        "--seed",
        "63",
        "--out",
        path(&out),
        "--cache",
        "--cards",
        "20",
        "--media",
        "files",
        "--now",
        "2025-01-01T00:00:00Z",
    ]));
    assert_eq!(again["cached"], true);
    assert_eq!(again["request"], first["request"]);
    assert_eq!(again["observed"], first["observed"]);
    assert_eq!(again["manifest"], first["manifest"]);
    let other = json(&cli(&[
        "generate",
        "cloud-small",
        "--seed",
        "64",
        "--out",
        path(&out),
        "--cache",
    ]));
    assert_eq!(other["cached"], false);
    assert_ne!(other["manifest"]["dump_sha256"], first["manifest"]["dump_sha256"]);
    let missing = std::fs::read_dir(out.join("collection.media"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::remove_file(missing).unwrap();
    let broken = cli(&[
        "generate",
        "cloud-small",
        "--seed",
        "64",
        "--out",
        path(&out),
        "--cache",
    ]);
    assert_eq!(broken.status.code(), Some(1));
    assert!(
        broken.stdout.is_empty(),
        "a damaged cache produces no successful receipt"
    );
}

#[test]
fn cloud_small_cache_rejects_a_changed_media_byte_without_a_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("small");
    let first = json(&cli(&["generate", "cloud-small", "--seed", "63", "--out", path(&out)]));
    assert_eq!(first["cached"], false);
    let manifest = std::fs::read(out.join("manifest.json")).unwrap();
    let file = std::fs::read_dir(out.join("collection.media"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&file).unwrap();
    assert_eq!(bytes.len(), 10240);
    bytes[5120] ^= 1;
    std::fs::write(&file, &bytes).unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().len(), 10240);
    let broken = cli(&[
        "generate",
        "cloud-small",
        "--seed",
        "63",
        "--out",
        path(&out),
        "--cache",
    ]);
    assert_eq!(broken.status.code(), Some(1));
    assert!(
        broken.stdout.is_empty(),
        "a damaged cache produces no successful receipt"
    );
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(std::fs::read(out.join("manifest.json")).unwrap(), manifest);
}

#[test]
fn cloud_small_refuses_shape_clock_and_references_only_overrides_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("never-created");
    for flags in [
        vec!["--cards", "21"],
        vec!["--profile", "missing.json"],
        vec!["--now", "2030-01-15T10:00:00Z"],
        vec!["--media", "references"],
        vec!["--media", "references", "--media", "files"],
        vec!["--rollover", "3"],
        vec!["--utc-offset", "3600"],
    ] {
        let mut args = vec!["generate", "cloud-small", "--out", path(&out)];
        args.extend(flags);
        let result = cli(&args);
        assert_eq!(result.status.code(), Some(2));
        assert!(!out.exists());
    }
}
