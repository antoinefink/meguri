//! The harvest catalog and its coverage report (specs/conformance/dsl.md D14).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use meguri_testkit::catalog::{self, Catalog};
use meguri_testkit::exec::MeguriExecutor;
use meguri_testkit::suite::{Status, Suite};

const HEADER: &str = "harvest_id,language,source,area,classification,reason,scenarios,status\n";

fn no_live() -> BTreeMap<String, Status> {
    BTreeMap::new()
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
        "{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7\"}",
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
    write(root, "scenarios/preservation/filtered-deck-sync.yaml", scenario);
    let sha = meguri_testkit::scenario::load(&root.join("scenarios/preservation/filtered-deck-sync.yaml"))
        .unwrap()
        .sha256;
    write(
        root,
        "goldens/preservation/filtered-deck-sync.json",
        &format!(
            "{{\"scenario_id\": \"preservation/filtered-deck-sync\", \"scenario_sha256\": \"{sha}\", \"tz\": \"UTC\", \
             \"oracle\": {{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7\", \"driver_sha256\": \"{}\"}}, \
             \"clock\": {{\"start\": \"2030-01-15T10:00:00\", \"mode\": \"frozen\"}}, \
             \"observations\": {{\"steps\": [{{}}, {{}}, {{}}, {{}}, {{\"card\": {{\"#c1\": null}}}}]}}}}",
            "b".repeat(64)
        ),
    );
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
    for (t1, shown) in [(Status::Pass, "pass"), (Status::Fail, "fail")] {
        let live = BTreeMap::from([("preservation/filtered-deck-sync".to_owned(), t1)]);
        let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new(), &live);
        assert_eq!(
            report.by_classification["preservation-only"].statuses.get(shown),
            Some(&1)
        );
    }
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
