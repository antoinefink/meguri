//! The pin check against seeded changes to the real Cargo.toml, Cargo.lock and
//! pins.toml.

use std::path::PathBuf;

use layering_check::Rule;
use layering_check::pins::{self, PinReport};

fn core_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(core_dir().join(name)).unwrap_or_else(|e| panic!("reading {name}: {e}"))
}

struct Files {
    pins: String,
    manifest: String,
    lock: String,
}

impl Files {
    fn real() -> Files {
        Files {
            pins: read("pins.toml"),
            manifest: read("Cargo.toml"),
            lock: read("Cargo.lock"),
        }
    }

    fn check(&self) -> PinReport {
        pins::check(&self.pins, &self.manifest, &self.lock).expect("the files parse")
    }
}

/// Replaces `version = "<from>"` with `<to>` in the lock entry of `name`.
fn bump_lock(lock: &str, name: &str, from: &str, to: &str) -> String {
    let entry = format!("name = \"{name}\"\nversion = \"{from}\"");
    assert!(lock.contains(&entry), "Cargo.lock has no {name} {from}");
    lock.replacen(&entry, &format!("name = \"{name}\"\nversion = \"{to}\""), 1)
}

fn messages(report: &PinReport) -> String {
    report.violations.iter().map(|v| format!("{v}\n")).collect()
}

#[test]
fn the_real_files_pass_and_list_the_pending_pins() {
    let report = Files::real().check();
    assert!(report.violations.is_empty(), "{}", messages(&report));
    assert_eq!(
        report.pending,
        ["regex", "unicode-normalization", "sha1", "blake3", "difflib"]
    );
}

#[test]
fn unicase_bumped_to_2_7_0_in_the_workspace_fails_naming_unicase() {
    let mut files = Files::real();
    files.manifest = files.manifest.replace("unicase = \"=2.6.0\"", "unicase = \"=2.7.0\"");
    files.lock = bump_lock(&files.lock, "unicase", "2.6.0", "2.7.0");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 2, "{text}");
    assert!(
        report
            .violations
            .iter()
            .all(|v| v.rule == Rule::Pin && v.subject == "unicase"),
        "{text}"
    );
    assert!(text.contains("[workspace.dependencies] requires =2.7.0"), "{text}");
    assert!(text.contains("Cargo.lock has unicase 2.7.0"), "{text}");
    assert!(text.contains("pins.toml pins 2.6.0"), "{text}");
    assert!(text.contains("anki 26.09.3"), "{text}");
}

#[test]
fn a_lock_only_bump_of_the_transitive_rand_fails() {
    let mut files = Files::real();
    files.lock = bump_lock(&files.lock, "rand", "0.10.1", "0.10.3");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].subject, "rand");
    assert!(text.contains("Cargo.lock has rand 0.9.4, 0.10.3"), "{text}");
    assert!(text.contains("pins.toml pins 0.10.1 (via fsrs)"), "{text}");
}

#[test]
fn a_caret_requirement_on_a_pinned_crate_fails() {
    let mut files = Files::real();
    files.manifest = files
        .manifest
        .replace("htmlescape = \"=0.3.1\"", "htmlescape = \"0.3.1\"");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].subject, "htmlescape");
    assert!(text.contains("requires 0.3.1"), "{text}");
    assert!(text.contains("must be exactly \"=0.3.1\""), "{text}");
}

#[test]
fn a_pinned_crate_missing_from_the_workspace_dependencies_fails() {
    let mut files = Files::real();
    files.manifest = files.manifest.replace("fsrs = \"=6.6.2\"\n", "");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert!(
        text.contains("fsrs is pinned to 6.6.2 but core/Cargo.toml [workspace.dependencies] has no entry for it"),
        "{text}"
    );
}

#[test]
fn a_pinned_crate_missing_from_the_lock_fails() {
    let mut files = Files::real();
    files.lock = files
        .lock
        .replace("name = \"htmlescape\"", "name = \"htmlescape-gone\"");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert!(text.contains("Cargo.lock has no htmlescape"), "{text}");
}

#[test]
fn a_pending_pin_in_the_workspace_dependencies_fails() {
    let mut files = Files::real();
    files.manifest = files.manifest.replace(
        "[workspace.dependencies]\n",
        "[workspace.dependencies]\nregex = \"1\"\n",
    );

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].rule, Rule::PendingPin);
    assert!(text.contains("regex has no oracle version in pins.toml yet"), "{text}");
}

#[test]
fn a_workspace_crate_depending_on_a_pending_pin_fails() {
    let mut files = Files::real();
    // A member that depends on sha1 directly, however it declares it.
    files.lock = files.lock.replace(
        "name = \"meguri-template\"\nversion = \"0.0.0\"\n",
        "name = \"meguri-template\"\nversion = \"0.0.0\"\ndependencies = [\n \"sha1\",\n]\n",
    );

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].rule, Rule::PendingPin);
    assert!(text.contains("meguri-template depends on sha1"), "{text}");
}

#[test]
fn a_pin_with_neither_version_nor_pending_is_rejected() {
    let mut files = Files::real();
    files
        .pins
        .push_str("\n[[pin]]\ncrate = \"csv\"\nwhy = \"csv import\"\n");
    let err = pins::check(&files.pins, &files.manifest, &files.lock).unwrap_err();
    assert!(err.to_string().contains("csv"), "{err}");
}
