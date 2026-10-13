//! The harvest catalog and its coverage report (specs/conformance/dsl.md D14).

use std::fs;
use std::path::Path;

use meguri_testkit::catalog::{self, Catalog};
use meguri_testkit::exec::MeguriExecutor;
use meguri_testkit::suite::Suite;

const HEADER: &str = "harvest_id,language,area,classification,reason,scenarios,status\n";

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
    // AE83: the filtered deck and its cards survive a sync round trip.
    write(
        root,
        "scenarios/preservation/filtered-deck-sync.yaml",
        r#"id: preservation/filtered-deck-sync
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
setup:
  decks: [Words]
  notetypes: [{name: Pair, fields: [Front, Back], templates: [{name: F, front: "{{Front}}"}]}]
  notes: [{notetype: Pair, deck: Words, fields: [a, b]}]
steps:
  - build_filtered: {name: Filtered, search: "deck:Words"}
  - sync: {kind: full_upload}
  - observe: ["card(#c1)"]
"#,
    );
    write(
        root,
        "scenarios/preservation/area.yaml",
        "area: preservation\nlive: [filtered-deck-sync]\n",
    );
    dir
}

#[test]
fn a_preservation_only_row_is_counted_with_its_scenario_status() {
    let dir = tree();
    let root = dir.path();
    let csv = format!(
        "{HEADER}\
         h0123456789ab,python,sched,preservation-only,\"Filtered decks are out of scope, so only their data must survive a sync\",preservation/filtered-deck-sync,done\n\
         h0123456789ac,rust,desktop,not-applicable,Desktop user interface,,done\n\
         h0123456789ad,rust,sched,unclassified,,,open\n\
         h0123456789ae,typescript,tooling,not-applicable,Build tooling,,done\n"
    );
    let catalog = Catalog::parse(&csv).unwrap();
    assert_eq!(catalog.rows.len(), 4);
    assert_eq!(
        catalog.rows[0].reason,
        "Filtered decks are out of scope, so only their data must survive a sync"
    );

    let suite = Suite::open(root).unwrap();
    let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new());
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let preservation = &report.by_classification["preservation-only"];
    assert_eq!(preservation.rows, 1);
    assert_eq!(preservation.statuses.get("pending"), Some(&1));
    assert_eq!(report.by_classification["not-applicable"].rows, 2);
    assert_eq!(report.by_classification["unclassified"].rows, 1);
    assert_eq!(
        report.by_classification["unclassified"].statuses.get("unlinked"),
        Some(&1)
    );
    assert_eq!(report.by_language["python"]["preservation-only"], 1);
    assert_eq!(report.by_language["rust"].values().sum::<usize>(), 2);
    assert_eq!(report.rows.len(), 4);
    let json = report.to_json();
    assert_eq!(json["rows_total"], 4);
    assert_eq!(
        json["rows"][0]["scenario_statuses"]["preservation/filtered-deck-sync"],
        "pending"
    );
    let markdown = report.render_markdown();
    assert!(markdown.contains("| preservation-only | 1 |"), "{markdown}");
}

#[test]
fn rows_that_break_the_catalog_rules_are_reported() {
    let dir = tree();
    let suite = Suite::open(dir.path()).unwrap();
    for (row, why) in [
        ("h0123456789ab,cobol,sched,in-scope,Reason,,open", "language"),
        ("h0123456789ab,rust,sched,in-scope-ish,Reason,,open", "classification"),
        ("h0123456789ab,rust,sched,in-scope,,,open", "reason"),
        ("h0123456789ab,rust,sched,in-scope,Reason,,done", "done"),
        ("h0123456789ab,rust,sched,unclassified,,,done", "unclassified"),
        ("h0123456789ab,rust,sched,in-scope,Reason,,finished", "status"),
        ("x1,rust,sched,in-scope,Reason,,open", "harvest_id"),
    ] {
        let err = Catalog::parse(&format!("{HEADER}{row}\n")).unwrap_err();
        assert!(err.to_string().contains(why), "{row}: {err}");
    }
    assert!(Catalog::parse("id,language\n").is_err());
    let twice =
        format!("{HEADER}h0123456789ab,rust,sched,unclassified,,,open\nh0123456789ab,rust,sched,unclassified,,,open\n");
    assert!(Catalog::parse(&twice).unwrap_err().to_string().contains("twice"));

    // A linked scenario must exist.
    let csv = format!("{HEADER}h0123456789ab,rust,sched,in-scope,Reason,sched/nothing-here,done\n");
    let report = catalog::harvest_report(&Catalog::parse(&csv).unwrap(), &suite, &MeguriExecutor::new());
    assert_eq!(report.problems.len(), 1);
    assert!(report.problems[0].contains("sched/nothing-here"));
}

#[test]
fn the_committed_catalog_is_valid() {
    let conformance = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let catalog = Catalog::read(&conformance.join("catalog/harvest.csv")).unwrap();
    assert!(catalog.rows.len() > 900, "{} rows", catalog.rows.len());
    let suite = Suite::open(&conformance).unwrap();
    let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new());
    assert!(report.problems.is_empty(), "{:?}", report.problems);
}
