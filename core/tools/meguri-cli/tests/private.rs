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
    io_collection(&copy, &[]);
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
    assert_eq!(
        written,
        serde_json::json!({
            "checks": {"io-roundtrip": {"passed": 0, "failed": 0,
                "breakdown": {"round-tripped": 0}}}, "report_version": 1
        })
    );
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

// Synthetic fields from image-occlusion-format F2, F7, F11 and F14.
fn io_collection(copy: &Path, notes: &[(i64, &str)]) {
    use meguri_engine::notetypes::stock::StockKind;
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        col::Col,
        config::ConfigEntry,
        ids::{NoteId, NotetypeId},
        note::Note,
        rng::SeededRandom,
        time::EpochMillis,
    };
    use meguri_storage::{CollectionBuilder, OpenOptions, StampMode};
    let clock = FixedClock::new(0, UtcOffset::UTC);
    let random = SeededRandom::new(63);
    let builder = CollectionBuilder::create(
        &copy.join("collection.anki2"),
        &Col {
            version: 18,
            schema_modified: EpochMillis(1),
            ..Col::default()
        },
        &[ConfigEntry {
            key: "schedVer".into(),
            value: b"2".to_vec(),
            ..ConfigEntry::default()
        }],
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
    for id in 1..=5 {
        let mut nt = StockKind::ImageOcclusion.notetype(&random);
        nt.id = NotetypeId(id);
        nt.name = format!("Synthetic type {id}");
        if id == 2 {
            // Reordered fields plus an audio field, as in Tobira notes.
            let mut extra = nt.fields[4].clone();
            extra.ord = 5;
            extra.name = "A01".into();
            nt.fields.push(extra);
            nt.fields.swap(0, 3);
        }
        if id == 3 {
            nt.config.original_stock_kind = 0;
        }
        if id == 4 {
            nt.fields.truncate(3);
        }
        if id == 5 {
            for field in &mut nt.fields {
                field.config.tag = None;
            }
        }
        for (ord, field) in nt.fields.iter_mut().enumerate() {
            field.ord = ord as u32;
        }
        scope.put_notetype(&nt).unwrap();
    }
    for (index, (ntid, field)) in notes.iter().enumerate() {
        let fields = if *ntid == 2 {
            ["back", "image", "header", field, "comments", "{{c7::audio}}"].join("\u{1f}")
        } else {
            [field, "image", "header", "back", "comments"].join("\u{1f}")
        };
        scope
            .put(&Note {
                id: NoteId(index as i64 + 1),
                notetype_id: NotetypeId(*ntid),
                fields,
                ..Note::default()
            })
            .unwrap();
    }
    scope.commit().unwrap();
    builder.finish().unwrap();
}

#[test]
fn io_roundtrip_counts_all_detected_fields_and_refusals_without_leaking_or_writing() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    io_collection(
        &copy,
        &[
            (
                1,
                "{{c1::image-occlusion:rect:left=0.3:top=0.1:fill=#ffeba2:width=0.2:height=0.08:oi=1}}<BR/>\n",
            ),
            (
                2,
                "\t{{c0::image-occlusion:text:left=.1:top=.1:text=a\\:b:c:scale=1}}<br />",
            ),
            (2, "{{c1::image-occlusion:rect:left=0:top=0:width=.001:height=.001}}"),
            (
                5,
                "{{c0::image-occlusion:text:left=.1:top=.1:text=a\\:b\\\\c:scale=1.}}",
            ),
            (1, ""),
            (1, "{{c1::image-occlusion:rect:left=abc}}"),
            (2, "{{c1::image-occlusion:rect:left=abc}}"),
            (1, "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1}}"),
            (1, "{{c1::image-occlusion:rect}}"),
            (
                2,
                "{{c0::image-occlusion:text:left=.1:top=.1:text=private\\label:scale=1}}",
            ),
            (1, "{{c1::image-occlusion:star:left=.1:top=.1}}"),
            (1, "{{c1,2::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}"),
            (
                1,
                "{{c1::image-occlusion:rect:left=.1:left=.2:top=.1:width=.1:height=.1}}",
            ),
            (
                1,
                "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1{{c2::x}}}}",
            ),
            (1, "private field content"),
            (3, "private ignored non IO"),
            (4, "private ignored three fields"),
        ],
    );
    let collection = copy.join("collection.anki2");
    let before = std::fs::read(&collection).unwrap();
    let out = dir.path().join("summary.json");
    let log = dir.path().join("suite.log");
    let run = cli(&[
        "private",
        "suite",
        "--copy",
        path(&copy),
        "--out",
        path(&out),
        "--log",
        path(&log),
    ]);
    assert_eq!(run.status.code(), Some(1), "{}", stderr(&run));
    let expected = serde_json::json!({"report_version": 1, "checks": {
        "io-roundtrip": {"passed": 5, "failed": 10, "breakdown": {
            "round-tripped": 5, "invalid_value": 2, "missing_key": 1,
            "malformed_shape": 2, "unknown_shape": 1, "multiple_numbers": 1,
            "duplicate_key": 1, "nested_cloze": 1, "extra_content": 1
        }}
    }});
    assert_eq!(serde_json::from_slice::<Value>(&run.stdout).unwrap(), expected);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&out).unwrap()).unwrap(),
        expected
    );
    assert_eq!(std::fs::read(&collection).unwrap(), before);
    assert!(!copy.join("collection.anki2-wal").exists());
    assert!(std::fs::read_to_string(log).unwrap().is_empty());
    assert!(run.stderr.is_empty(), "{}", stderr(&run));
}

#[test]
fn io_roundtrip_unreadable_collection_reports_only_check_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("collection.anki2"), b"corrupt private data").unwrap();
    let out = dir.path().join("summary.json");
    let run = cli(&["private", "suite", "--copy", path(dir.path()), "--out", path(&out)]);
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&run.stdout).unwrap(),
        serde_json::json!({"report_version": 1, "checks": {"io-roundtrip": {"error": true}}})
    );
    assert!(!stderr(&run).contains("corrupt private data"));
}

#[test]
fn io_roundtrip_reads_every_page_including_uncheckpointed_wal_and_preserves_both_files() {
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        ids::{NoteId, NotetypeId},
        note::Note,
        rng::SeededRandom,
    };
    use meguri_storage::{OpenOptions, StampMode, open_collection};
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    let field = "{{c1::image-occlusion:rect:left=0.3:top=0.1:width=0.2:height=0.08}}<br />";
    io_collection(&copy, &vec![(1, field); 5_001]);
    let clock = FixedClock::new(0, UtcOffset::UTC);
    let random = SeededRandom::new(63);
    let database = copy.join("collection.anki2");
    let collection = open_collection(&database, &OpenOptions::new(&clock, &random)).unwrap();
    let mut scope = collection.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    scope
        .put(&Note {
            id: NoteId(5_002),
            notetype_id: NotetypeId(2),
            fields: ["back", "image", "header", field, "comments", "{{c2::audio}}"].join("\u{1f}"),
            ..Note::default()
        })
        .unwrap();
    scope.commit().unwrap();
    let wal = copy.join("collection.anki2-wal");
    let before = std::fs::read(&database).unwrap();
    let wal_before = std::fs::read(&wal).unwrap();
    assert!(!wal_before.is_empty());
    let out = dir.path().join("summary.json");
    let run = cli(&["private", "suite", "--copy", path(&copy), "--out", path(&out)]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(
        serde_json::from_slice::<Value>(&run.stdout).unwrap(),
        serde_json::json!({
            "report_version": 1, "checks": {"io-roundtrip": {
                "passed": 5002, "failed": 0, "breakdown": {"round-tripped": 5002}
            }}
        })
    );
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert_eq!(std::fs::read(&wal).unwrap(), wal_before);
    collection.close().unwrap();
}
