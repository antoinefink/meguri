//! `meguri-cli anonymize` through the binary (U15, `specs/fixtures/anonymizer.md`
//! A2.2): a generated collection's clone, the salt file it creates and
//! reuses, and its refusals.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meguri-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn generated(dir: &Path) -> std::path::PathBuf {
    let source = dir.join("source");
    let out = cli(&[
        "generate",
        "2000",
        "--seed",
        "5",
        "--media",
        "files",
        "--out",
        path(&source),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    source
}

#[test]
fn a_generated_collection_gets_a_clone_a_report_and_a_salt() {
    let dir = tempfile::tempdir().unwrap();
    let source = generated(dir.path());
    let clone = dir.path().join("clone");
    let salt = dir.path().join("anonymize.salt");

    let out = cli(&[
        "anonymize",
        "--copy",
        path(&source),
        "--out",
        path(&clone),
        "--salt-file",
        path(&salt),
    ]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let printed = String::from_utf8(out.stdout).unwrap();
    assert_eq!(printed.trim_end(), clone.join("anonymize.json").to_str().unwrap());
    // A2.2: 64 lower-case hex digits and an LF, for the owner only.
    let contents = fs::read_to_string(&salt).unwrap();
    assert_eq!(contents.len(), 65);
    assert!(
        contents[..64]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(contents.ends_with('\n'));
    assert_eq!(fs::metadata(&salt).unwrap().permissions().mode() & 0o777, 0o600);

    let report: Value = serde_json::from_str(&fs::read_to_string(clone.join("anonymize.json")).unwrap()).unwrap();
    let manifest: Value = serde_json::from_str(&fs::read_to_string(source.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(report["notes"], manifest["counts"]["notes"]);
    assert_eq!(report["format"], 1);
    assert_eq!(report["checks"]["structure"], "pass");
    assert_eq!(report["checks"]["rows"], "pass");
    assert_eq!(report["checks"]["media"], "pass");
    assert_ne!(report["checks"]["leak"]["status"], "fail");
    assert!(report["fields"]["changed"].as_u64().unwrap() > 0);
    assert!(clone.join("collection.anki2").is_file());
    assert!(!clone.join("collection.anki2-wal").exists());

    // The same salt file again: the same clone.
    let again = dir.path().join("again");
    let out = cli(&[
        "anonymize",
        "--copy",
        path(&source),
        "--out",
        path(&again),
        "--salt-file",
        path(&salt),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(fs::read_to_string(&salt).unwrap(), contents);
    assert_eq!(
        fs::read(clone.join("collection.anki2")).unwrap(),
        fs::read(again.join("collection.anki2")).unwrap()
    );
}

#[test]
fn refusals_name_the_kind() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir(&source).unwrap();
    let salt = dir.path().join("salt");

    let out = cli(&["anonymize", "--copy", path(&source)]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("usage"), "{}", stderr(&out));

    fs::write(&salt, b"not a salt\n").unwrap();
    let clone = dir.path().join("clone");
    let out = cli(&[
        "anonymize",
        "--copy",
        path(&source),
        "--out",
        path(&clone),
        "--salt-file",
        path(&salt),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("meguri-cli anonymize: salt:"), "{}", stderr(&out));
    assert!(!clone.exists());

    fs::write(&salt, format!("{}\n", "ab".repeat(32))).unwrap();
    let out = cli(&[
        "anonymize",
        "--copy",
        path(&source),
        "--out",
        path(&clone),
        "--salt-file",
        path(&salt),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("meguri-cli anonymize: no_collection:"),
        "{}",
        stderr(&out)
    );
    assert!(!clone.exists());

    let busy = dir.path().join("busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("x"), b"x").unwrap();
    fs::write(source.join("collection.anki2"), b"x").unwrap();
    let out = cli(&[
        "anonymize",
        "--copy",
        path(&source),
        "--out",
        path(&busy),
        "--salt-file",
        path(&salt),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("meguri-cli anonymize: out_not_empty:"),
        "{}",
        stderr(&out)
    );
    assert!(busy.join("x").exists());
}
