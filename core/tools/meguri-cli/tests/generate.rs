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
        "--collection",
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
