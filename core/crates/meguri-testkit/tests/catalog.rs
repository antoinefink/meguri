//! The harvest catalog and its coverage report (specs/conformance/dsl.md D14).

use std::fs;
use std::path::Path;

use meguri_testkit::catalog::{self, Catalog, LiveReport};
use meguri_testkit::exec::MeguriExecutor;
use meguri_testkit::suite::Suite;
use serde_json::json;

const HEADER: &str = "harvest_id,language,source,area,classification,reason,scenarios,status\n";
const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";
const ID: &str = "preservation/filtered-deck-sync";

fn no_live() -> LiveReport {
    LiveReport::default()
}

/// A `run --live` report (dsl.md D12) at this oracle lock: (scenario, status,
/// scenario_sha256) per scenario.
fn t1_report(lock: &str, scenarios: &[(&str, &str, &str)]) -> String {
    json!({
        "runner": "meguri-testkit", "tier": "t1", "executor": "oracle", "lane": "live-oracle",
        "oracle_pin": {"anki_version": "26.9.3", "lock_sha256": lock},
        "scenarios": scenarios.iter().map(|(id, status, sha)| json!({
            "scenario": id, "status": status, "scenario_sha256": sha, "live": true,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

/// The scenario's hash as the oracle records it in goldens.
fn sha_of(root: &Path) -> String {
    meguri_testkit::scenario::load(&root.join(format!("scenarios/{ID}.yaml")))
        .unwrap()
        .sha256
}

/// A golden for the scenario as it now is, recorded with this oracle lock.
fn golden(root: &Path, lock: &str) {
    let sha = sha_of(root);
    write(
        root,
        &format!("goldens/{ID}.json"),
        &format!(
            "{{\"scenario_id\": \"{ID}\", \"scenario_sha256\": \"{sha}\", \"tz\": \"UTC\", \
             \"oracle\": {{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{lock}\", \"driver_sha256\": \"{}\"}}, \
             \"clock\": {{\"start\": \"2030-01-15T10:00:00\", \"mode\": \"frozen\"}}, \
             \"observations\": {{\"steps\": [{{}}, {{}}, {{}}, {{}}, {{\"card\": {{\"#c1\": null}}}}]}}}}",
            "b".repeat(64)
        ),
    );
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}"),
    );
    write(root, "deviations.yaml", "deviations: []\n");
    fs::create_dir_all(root.join("goldens")).unwrap();
    // AE83: the filtered deck and its cards survive a round trip through
    // Meguri (the sync area makes the middle a sync); a live chain.
    let scenario = r#"id: preservation/filtered-deck-sync
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
setup:
  decks: [Words]
  notetypes: [{name: Pair, fields: [Front, Back], templates: [{name: F, front: "{{Front}}"}]}]
  notes: [{notetype: Pair, deck: Words, fields: [a, b]}]
steps:
  - build_filtered: {name: Filtered, search: "deck:Words"}
  - handoff: meguri
  - sync: {kind: full_upload}
  - handoff: oracle
  - observe: ["card(#c1)"]
"#;
    write(root, &format!("scenarios/{ID}.yaml"), scenario);
    golden(root, LOCK);
    dir
}

#[test]
fn a_preservation_only_row_is_counted_with_its_scenario_status() {
    let dir = tree();
    let root = dir.path();
    let csv = format!(
        "{HEADER}\
         h0123456789ab,python,tag,sched,preservation-only,\"Filtered decks are out of scope, so only their data must survive a sync\",preservation/filtered-deck-sync,done\n\
         h0123456789ac,rust,tag,desktop,not-applicable,Desktop user interface,,done\n\
         h0123456789ad,rust,tag,sched,unclassified,,,open\n\
         h0123456789ae,typescript,tag,tooling,not-applicable,Build tooling,,done\n\
         h0123456789af,rust,supplementary,sched,unclassified,,,open\n"
    );
    let catalog = Catalog::parse(&csv).unwrap();
    assert_eq!(catalog.rows.len(), 5);
    assert_eq!(
        catalog.rows[0].reason,
        "Filtered decks are out of scope, so only their data must survive a sync"
    );

    let suite = Suite::open(root).unwrap();
    let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new(), &no_live());
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let preservation = &report.by_classification["preservation-only"];
    assert_eq!(preservation.rows, 1);
    // T0 cannot run the chain, and Meguri has no handoff adapters yet.
    assert_eq!(preservation.statuses.get("pending"), Some(&1));
    assert_eq!(report.by_classification["not-applicable"].rows, 2);
    assert_eq!(report.by_classification["unclassified"].rows, 2);
    assert_eq!(
        report.by_classification["unclassified"].statuses.get("unlinked"),
        Some(&2)
    );
    assert_eq!(report.by_language["python"]["preservation-only"], 1);
    // The supplementary row is not counted at the tag.
    assert_eq!(report.by_language["rust"].values().sum::<usize>(), 2);
    assert_eq!(report.rows_at_tag, 4);
    assert_eq!(report.supplementary["rust"], 1);
    assert_eq!(report.rows.len(), 5);
    let json = report.to_json();
    assert_eq!(json["rows_total"], 5);
    assert_eq!(json["rows_at_tag"], 4);
    assert_eq!(
        json["rows"][0]["scenario_statuses"]["preservation/filtered-deck-sync"],
        "pending"
    );
    let markdown = report.render_markdown();
    assert!(markdown.contains("| preservation-only | 1 |"), "{markdown}");
    assert!(
        markdown.contains("(anki 26.9.3): 4 (python 1, rust 2, typescript 1)"),
        "{markdown}"
    );
    assert!(markdown.contains("not in the counts at the tag: 1"), "{markdown}");

    // With a T1 report, the row shows the live chain's own status (R1880).
    for t1 in ["pass", "fail"] {
        let live = LiveReport::parse(&t1_report(LOCK, &[(ID, t1, &sha_of(root))]), &suite.pin).unwrap();
        let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new(), &live);
        assert_eq!(report.by_classification["preservation-only"].statuses.get(t1), Some(&1));
    }
}

/// A T1 status stands only for the scenario text, golden and oracle pin it
/// ran with: after a scenario edit, a golden deletion or a pin change an old
/// passing report shows nothing passing.
#[test]
fn a_t1_status_counts_only_for_the_scenario_golden_and_pin_it_ran() {
    let dir = tree();
    let root = dir.path();
    let csv =
        format!("{HEADER}h0123456789ab,python,tag,sched,preservation-only,Filtered decks survive a sync,{ID},done\n");
    let catalog = Catalog::parse(&csv).unwrap();
    let row_status = |t1: &str| {
        let suite = Suite::open(root).unwrap();
        let live = LiveReport::parse(t1, &suite.pin).unwrap();
        let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new(), &live);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        report.rows[0].status.clone()
    };
    let before = sha_of(root);
    let passed = t1_report(LOCK, &[(ID, "pass", &before)]);
    assert_eq!(row_status(&passed), "pass");

    // Edited and not re-recorded: the golden is stale, and so is the row,
    // whatever the report says.
    let path = root.join(format!("scenarios/{ID}.yaml"));
    let edited = format!(
        "{}description: survives a full upload\n",
        fs::read_to_string(&path).unwrap()
    );
    write(root, &format!("scenarios/{ID}.yaml"), &edited);
    let after = sha_of(root);
    assert_ne!(after, before);
    assert_eq!(row_status(&passed), "stale");
    assert_eq!(row_status(&t1_report(LOCK, &[(ID, "pass", &after)])), "stale");

    // Re-recorded: the golden is fresh again, but the old report ran the old
    // text, so the row keeps its T0 status until T1 runs the new one.
    golden(root, LOCK);
    assert_eq!(row_status(&passed), "pending");
    assert_eq!(row_status(&t1_report(LOCK, &[(ID, "pass", &after)])), "pass");

    // No golden: missing.
    fs::remove_file(root.join(format!("goldens/{ID}.json"))).unwrap();
    assert_eq!(row_status(&t1_report(LOCK, &[(ID, "pass", &after)])), "missing");

    // Another pin: a report from the old one is refused, and one at the new
    // pin cannot pass goldens recorded with the old lock.
    golden(root, LOCK);
    let new_lock = "c".repeat(64);
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.10.0\", \"lock_sha256\": \"{new_lock}\"}}"),
    );
    let suite = Suite::open(root).unwrap();
    let err = LiveReport::parse(&t1_report(LOCK, &[(ID, "pass", &after)]), &suite.pin).unwrap_err();
    assert!(err.to_string().contains("oracle pin"), "{err}");
    let at_new_pin = t1_report(&new_lock, &[(ID, "pass", &after)]).replace("\"26.9.3\"", "\"26.10.0\"");
    assert_eq!(row_status(&at_new_pin), "stale");
}

#[test]
fn a_live_report_must_be_a_t1_report_with_known_statuses() {
    let dir = tree();
    let suite = Suite::open(dir.path()).unwrap();
    let sha = sha_of(dir.path());
    let t0 = t1_report(LOCK, &[(ID, "pass", &sha)]).replace("\"t1\"", "\"t0\"");
    assert!(
        LiveReport::parse(&t0, &suite.pin)
            .unwrap_err()
            .to_string()
            .contains("run --live")
    );
    let odd = t1_report(LOCK, &[(ID, "passed", &sha)]);
    assert!(
        LiveReport::parse(&odd, &suite.pin)
            .unwrap_err()
            .to_string()
            .contains("passed")
    );
    assert!(LiveReport::parse("{", &suite.pin).is_err());
}

#[test]
fn rows_that_break_the_catalog_rules_are_reported() {
    let dir = tree();
    let suite = Suite::open(dir.path()).unwrap();
    for (row, why) in [
        ("h0123456789ab,cobol,tag,sched,in-scope,Reason,,open", "language"),
        ("h0123456789ab,rust,main,sched,in-scope,Reason,,open", "source"),
        (
            "h0123456789ab,rust,tag,sched,in-scope-ish,Reason,,open",
            "classification",
        ),
        ("h0123456789ab,rust,tag,sched,in-scope,,,open", "reason"),
        ("h0123456789ab,rust,tag,sched,in-scope,Reason,,done", "done"),
        ("h0123456789ab,rust,tag,sched,unclassified,,,done", "unclassified"),
        ("h0123456789ab,rust,tag,sched,in-scope,Reason,,finished", "status"),
        ("x1,rust,tag,sched,in-scope,Reason,,open", "harvest_id"),
    ] {
        let err = Catalog::parse(&format!("{HEADER}{row}\n")).unwrap_err();
        assert!(err.to_string().contains(why), "{row}: {err}");
    }
    assert!(Catalog::parse("id,language\n").is_err());
    let twice = format!(
        "{HEADER}h0123456789ab,rust,tag,sched,unclassified,,,open\nh0123456789ab,rust,tag,sched,unclassified,,,open\n"
    );
    assert!(Catalog::parse(&twice).unwrap_err().to_string().contains("twice"));

    // A linked scenario must exist.
    let csv = format!("{HEADER}h0123456789ab,rust,tag,sched,in-scope,Reason,sched/nothing-here,done\n");
    let report = catalog::harvest_report(
        &Catalog::parse(&csv).unwrap(),
        &suite,
        &MeguriExecutor::new(),
        &no_live(),
    );
    assert_eq!(report.problems.len(), 1);
    assert!(report.problems[0].contains("sched/nothing-here"));
}

#[test]
fn the_committed_catalog_is_valid() {
    let conformance = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let catalog = Catalog::read(&conformance.join("catalog/harvest.csv")).unwrap();
    assert!(catalog.rows.len() > 900, "{} rows", catalog.rows.len());
    let suite = Suite::open(&conformance).unwrap();
    let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new(), &no_live());
    assert!(report.problems.is_empty(), "{:?}", report.problems);
}
