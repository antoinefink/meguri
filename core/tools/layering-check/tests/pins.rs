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

/// Adds `entry` (`name` or `name version`) to the lock dependencies of the
/// workspace crate `member`, as cargo records a direct dependency.
fn add_lock_dependency(lock: &str, member: &str, entry: &str) -> String {
    let head = format!("name = \"{member}\"\nversion = \"0.0.0\"\n");
    assert!(lock.contains(&head), "Cargo.lock has no {member}");
    let with_list = format!("{head}dependencies = [\n");
    if lock.contains(&with_list) {
        lock.replacen(&with_list, &format!("{with_list} \"{entry}\",\n"), 1)
    } else {
        lock.replacen(&head, &format!("{with_list} \"{entry}\",\n]\n"), 1)
    }
}

/// Adds a crates.io package to the lock.
fn add_lock_package(lock: &str, name: &str, version: &str) -> String {
    format!(
        "{lock}\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
    )
}

/// Adds a `pending = true` pin for `name` to pins.toml.
fn add_pending_pin(pins: &str, name: &str) -> String {
    format!("{pins}\n[[pin]]\ncrate = \"{name}\"\npending = true\nwhy = \"a seeded pending pin\"\n")
}

fn messages(report: &PinReport) -> String {
    report.violations.iter().map(|v| format!("{v}\n")).collect()
}

/// A pin failure must identify the crate on every violation and explain all
/// the required parts of the workspace, lockfile or member dependency drift.
fn assert_pin_failure(report: &PinReport, subject: &str, count: usize, fragments: &[&str]) {
    let text = messages(report);
    assert_eq!(report.violations.len(), count, "{text}");
    for violation in &report.violations {
        assert_eq!(
            (violation.rule, violation.subject.as_str()),
            (Rule::Pin, subject),
            "unexpected violation identity: {violation:?}\n{text}"
        );
    }
    for &fragment in fragments {
        assert!(
            text.contains(fragment),
            "missing diagnostic fragment {fragment:?}:\n{text}"
        );
    }
}

#[test]
fn the_real_files_pass_with_no_pending_pins() {
    let report = Files::real().check();
    assert!(report.violations.is_empty(), "{}", messages(&report));
    assert!(report.pending.is_empty(), "{:?}", report.pending);
}

#[test]
fn unicase_bumped_to_2_7_0_in_the_workspace_fails_naming_unicase() {
    let mut files = Files::real();
    files.manifest = files.manifest.replace("unicase = \"=2.6.0\"", "unicase = \"=2.7.0\"");
    files.lock = bump_lock(&files.lock, "unicase", "2.6.0", "2.7.0");

    assert_pin_failure(
        &files.check(),
        "unicase",
        3,
        &[
            "[workspace.dependencies] requires =2.7.0",
            "Cargo.lock has unicase 2.7.0",
            "meguri-text depends on unicase 2.7.0",
            "pins.toml pins 2.6.0",
            "anki 26.09.3",
        ],
    );
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
fn a_lock_only_bump_of_the_regex_case_folding_tables_fails() {
    // A bare `cargo update` moves regex-syntax under the =-pinned regex.
    let mut files = Files::real();
    files.lock = bump_lock(&files.lock, "regex-syntax", "0.8.6", "0.8.11");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].subject, "regex-syntax");
    assert!(text.contains("Cargo.lock has regex-syntax 0.8.11"), "{text}");
    assert!(text.contains("pins.toml pins 0.8.6 (via regex)"), "{text}");
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
fn a_workspace_crate_using_the_rand_that_fsrs_brings_fails() {
    let mut files = Files::real();
    // meguri-search declares `rand = "0.10"` in its own manifest; cargo
    // resolves it to the rand 0.10.1 already in the graph for fsrs.
    files.lock = add_lock_dependency(&files.lock, "meguri-search", "rand 0.10.1");

    assert_pin_failure(
        &files.check(),
        "rand",
        1,
        &[
            "meguri-search depends on rand 0.10.1",
            "pins.toml pins 0.9.4 from the anki 26.09.3 lockfile",
            "rand 0.10.1 only as the version fsrs brings",
            "rand.workspace = true",
        ],
    );
}

#[test]
fn a_renamed_workspace_dependency_on_the_fsrs_rand_fails() {
    let mut files = Files::real();
    files.manifest = files.manifest.replace(
        "rand = \"=0.9.4\"\n",
        "rand = \"=0.9.4\"\nrand010 = { package = \"rand\", version = \"=0.10.1\" }\n",
    );
    files.lock = add_lock_dependency(&files.lock, "meguri-sched", "rand 0.10.1");

    assert_pin_failure(
        &files.check(),
        "rand",
        2,
        &[
            "rand (as rand010): core/Cargo.toml [workspace.dependencies] requires =0.10.1",
            "must be exactly \"=0.9.4\"",
            "meguri-sched depends on rand 0.10.1",
        ],
    );
}

#[test]
fn a_second_unpinned_version_of_a_parity_crate_in_a_workspace_crate_fails() {
    let mut files = Files::real();
    // meguri-search declares `unicase = "=1.4.2"` itself. The lock then holds
    // both versions, so cargo names the version in every reference.
    files.lock = files.lock.replace(" \"unicase\",\n", " \"unicase 2.6.0\",\n");
    files.lock = add_lock_package(&files.lock, "unicase", "1.4.2");
    files.lock = add_lock_dependency(&files.lock, "meguri-search", "unicase 1.4.2");

    assert_pin_failure(
        &files.check(),
        "unicase",
        1,
        &[
            "meguri-search depends on unicase 1.4.2",
            "pins.toml pins 2.6.0",
            "anki 26.09.3",
        ],
    );
}

#[test]
fn a_pending_pin_in_the_workspace_dependencies_fails() {
    let mut files = Files::real();
    files.pins = add_pending_pin(&files.pins, "csv");
    files.manifest = files
        .manifest
        .replace("[workspace.dependencies]\n", "[workspace.dependencies]\ncsv = \"1\"\n");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].rule, Rule::PendingPin);
    assert!(text.contains("csv has no oracle version in pins.toml yet"), "{text}");
    assert_eq!(report.pending, ["csv"]);
}

#[test]
fn a_workspace_crate_depending_on_a_pending_pin_fails() {
    let mut files = Files::real();
    files.pins = add_pending_pin(&files.pins, "csv");
    // A member that depends on csv directly, however it declares it.
    files.lock = add_lock_dependency(&files.lock, "meguri-template", "csv");

    let report = files.check();
    let text = messages(&report);
    assert_eq!(report.violations.len(), 1, "{text}");
    assert_eq!(report.violations[0].rule, Rule::PendingPin);
    assert!(text.contains("meguri-template depends on csv"), "{text}");
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
