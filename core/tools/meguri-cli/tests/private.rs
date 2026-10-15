//! `meguri-cli private` through the binary (U15): the vault's refusals, the
//! candidate strings of a generated collection and the suite's summary.
//! A successful copy needs a read-only mount, which tests cannot make; the
//! copy itself is tested in meguri-testkit (`tests/private.rs`) and through
//! `tools/mg private copy`.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn cli_env(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_meguri-cli"));
    command.args(args);
    for marker in meguri_testkit::private::vault::CI_MARKERS {
        command.env_remove(marker);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn cli(args: &[&str]) -> Output {
    cli_env(args, &[])
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn profile(dir: &Path) -> std::path::PathBuf {
    let root = dir.join("profile");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("collection.anki2"), b"collection").unwrap();
    std::fs::write(root.join("prefs21.db"), b"preferences").unwrap();
    root
}

#[test]
fn copy_refuses_a_source_that_is_not_mounted_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = profile(dir.path());
    let out = dir.path().join("copy");

    let run = cli(&["private", "copy", "--source", path(&root), "--out", path(&out)]);

    assert_eq!(run.status.code(), Some(1), "{}", stderr(&run));
    assert!(stderr(&run).contains("not on a read-only mount"), "{}", stderr(&run));
    assert!(!out.exists());
}

#[test]
fn copy_refuses_when_a_ci_marker_is_set() {
    let dir = tempfile::tempdir().unwrap();
    let root = profile(dir.path());
    let out = dir.path().join("copy");

    let run = cli_env(
        &["private", "copy", "--source", path(&root), "--out", path(&out)],
        &[("GITLAB_CI", "true")],
    );

    assert_eq!(run.status.code(), Some(1), "{}", stderr(&run));
    assert!(
        stderr(&run).contains("the CI marker GITLAB_CI is set"),
        "{}",
        stderr(&run)
    );
    assert!(!out.exists());
}

#[test]
fn tokens_lists_the_decks_tags_media_and_words_of_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    let made = cli(&["generate", "recipe", "typical", "--out", path(&copy)]);
    assert!(made.status.success(), "{}", stderr(&made));
    let out = dir.path().join("candidates.jsonl");

    let run = cli(&["private", "tokens", "--copy", path(&copy), "--out", path(&out)]);

    assert!(run.status.success(), "{}", stderr(&run));
    let counts: Value = serde_json::from_slice(&run.stdout).unwrap();
    let lines: Vec<Value> = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let has = |kind: &str, text: &str| lines.iter().any(|l| l["kind"] == kind && l["text"] == text);
    // From the recipe's own text (fixtures/recipes/typical.yaml).
    assert!(has("deck", "Languages::日本語::語彙"));
    assert!(has("deck", "語彙"));
    assert!(has("tag", "jlpt::n4"));
    assert!(has("tag", "n4"));
    assert!(has("word", "palabra"));
    assert!(lines.iter().any(|l| l["kind"] == "media"));
    for kind in ["decks", "tags", "media", "words"] {
        let n = counts[kind].as_u64().unwrap();
        let singular = &kind[..kind.len() - 1];
        let listed = lines
            .iter()
            .filter(|l| l["kind"] == singular || l["kind"] == kind)
            .count() as u64;
        assert_eq!(n, listed, "{kind}");
    }
    // stdout holds counts only.
    assert!(!String::from_utf8_lossy(&run.stdout).contains("Languages"));
}

#[test]
fn suite_writes_a_summary_of_its_checks() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    std::fs::write(copy.join("collection.anki2"), b"collection").unwrap();
    let out = dir.path().join("summary.json");

    let run = cli(&[
        "private",
        "suite",
        "--copy",
        path(&copy),
        "--out",
        path(&out),
        "--log",
        path(&dir.path().join("suite.log")),
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    // No check is registered before U249.
    assert_eq!(written, serde_json::json!({"checks": {}, "report_version": 1}));
    assert_eq!(serde_json::from_slice::<Value>(&run.stdout).unwrap(), written);
}

#[test]
fn suite_refuses_a_folder_without_a_collection() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("summary.json");
    let run = cli(&["private", "suite", "--copy", path(dir.path()), "--out", path(&out)]);
    assert_eq!(run.status.code(), Some(1));
    assert!(!out.exists());
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        &["private"][..],
        &["private", "copy", "--source", "/x"],
        &["private", "copy", "--source", "/x", "--out", "/y", "--bogus", "z"],
        &["private", "tokens", "--copy", "/x", "--copy", "/y", "--out", "/z"],
        &["private", "anonymize"],
    ] {
        assert_eq!(cli(args).status.code(), Some(2), "{args:?}");
    }
}
