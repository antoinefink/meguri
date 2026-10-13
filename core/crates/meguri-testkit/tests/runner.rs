//! The T0 runner and the freshness check over a conformance tree
//! (specs/conformance/dsl.md D8-D13). Fixtures are copies of committed
//! scenarios and oracle-recorded goldens in a temporary tree.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor, Needs};
use meguri_testkit::scenario::LoadedScenario;
use meguri_testkit::suite::{GoldensCheck, Status, Suite};
use meguri_testkit::wallclock::Stopwatch;
use serde_json::{Value, json};

const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";

fn repo_conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// A conformance tree with the current pin, no deviations and no scenarios.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("scenarios")).unwrap();
    fs::create_dir_all(root.join("goldens")).unwrap();
    fs::write(
        root.join("oracle.json"),
        format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}\n"),
    )
    .unwrap();
    fs::write(root.join("deviations.yaml"), "deviations: []\n").unwrap();
    dir
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Copies a committed smoke scenario and its golden into the tree.
fn copy_smoke(root: &Path, name: &str) {
    for (dir, suffix) in [("scenarios", "yaml"), ("goldens", "json")] {
        let from = repo_conformance().join(format!("{dir}/smoke/{name}.{suffix}"));
        write(
            root,
            &format!("{dir}/smoke/{name}.{suffix}"),
            &fs::read_to_string(from).unwrap(),
        );
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// An executor that can do everything and answers with fixed observations.
struct Canned(Value);

impl Executor for Canned {
    fn name(&self) -> &str {
        "canned"
    }
    fn missing(&self, _needs: &Needs) -> Needs {
        Needs::default()
    }
    fn run(&self, _scenario: &LoadedScenario) -> Result<Value, ExecError> {
        Ok(self.0.clone())
    }
}

#[test]
fn an_interval_mismatch_is_reported_with_step_pointer_values_and_repro() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "fsrs-new-card");
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [fsrs-new-card]\n",
    );
    // The oracle said 5 days for Good at step 3; Meguri says 4.
    let golden_path = root.join("goldens/smoke/fsrs-new-card.json");
    let mut golden = read_json(&golden_path);
    golden["observations"]["steps"][3]["next_states"]["#c1"]["good"]["scheduled_days"] = json!(5);
    fs::write(&golden_path, serde_json::to_string_pretty(&golden).unwrap()).unwrap();
    let mut meguri = golden["observations"].clone();
    meguri["steps"][3]["next_states"]["#c1"]["good"]["scheduled_days"] = json!(4);

    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &Canned(meguri), "test");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.scenario, "smoke/fsrs-new-card");
    assert_eq!(outcome.status, Status::Fail);
    assert!(outcome.fails_t0());
    let first = outcome.first_divergence.as_ref().unwrap();
    assert_eq!(first.step, 3);
    assert_eq!(first.pointer, "/steps/3/next_states/#c1/good/scheduled_days");
    assert_eq!(first.expected, json!(5));
    assert_eq!(first.actual, json!(4));
    assert_eq!(outcome.repro, "tools/mg conformance smoke/fsrs-new-card");

    let text = report.render_text();
    for needle in [
        "smoke/fsrs-new-card: fail at step 3",
        "/steps/3/next_states/#c1/good/scheduled_days: expected 5, actual 4",
        "repro: tools/mg conformance smoke/fsrs-new-card",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    let json = report.to_json();
    assert_eq!(
        json["scenarios"][0]["first_divergence"]["pointer"],
        "/steps/3/next_states/#c1/good/scheduled_days"
    );
    assert_eq!(json["failed"], 1);
}

#[test]
fn editing_a_scenario_without_re_recording_makes_its_golden_stale() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-advance");
    copy_smoke(root, "timing-tz-utc");
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [timing-advance]\n",
    );
    let suite = Suite::open(root).unwrap();
    assert!(suite.goldens_check(&GoldensCheck::default()).unwrap().ok());

    // Comments and layout are not edits.
    let path = root.join("scenarios/smoke/timing-advance.yaml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!(
            "# reworded\n{}",
            original.replace("advance: 20m", "advance: '20m'  # twenty")
        ),
    )
    .unwrap();
    assert!(
        Suite::open(root)
            .unwrap()
            .goldens_check(&GoldensCheck::default())
            .unwrap()
            .ok()
    );

    for name in ["timing-advance", "timing-tz-utc"] {
        let path = root.join(format!("scenarios/smoke/{name}.yaml"));
        let text = fs::read_to_string(&path).unwrap();
        let edited = text.replacen("observe: [timing]", "observe: [timing, ids]", 1);
        assert_ne!(edited, text);
        fs::write(&path, edited).unwrap();
    }
    let suite = Suite::open(root).unwrap();
    let fresh = suite.goldens_check(&GoldensCheck::default()).unwrap();
    assert!(!fresh.ok());
    let stale: Vec<&str> = fresh
        .problems
        .iter()
        .filter(|p| p.kind == "stale_scenario")
        .map(|p| p.scenario.as_str())
        .collect();
    assert_eq!(stale, ["smoke/timing-advance", "smoke/timing-tz-utc"]);
    assert!(
        fresh.problems[0]
            .repro
            .contains("tools/mg oracle record smoke/timing-advance")
    );

    // T0 fails for the active one only; the other is listed as stale.
    let report = suite.run(&[], &Canned(json!({"steps": []})), "test");
    let status = |id: &str| report.outcomes.iter().find(|o| o.scenario == id).unwrap();
    assert_eq!(status("smoke/timing-advance").status, Status::Stale);
    assert!(status("smoke/timing-advance").fails_t0());
    assert_eq!(status("smoke/timing-tz-utc").status, Status::Stale);
    assert!(!status("smoke/timing-tz-utc").fails_t0());
    assert_eq!(report.failures().len(), 1);
}

#[test]
fn a_golden_from_an_older_oracle_lock_is_stale_with_the_bump_command() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "ids-frozen-collision");
    let golden_path = root.join("goldens/smoke/ids-frozen-collision.json");
    let mut golden = read_json(&golden_path);
    golden["oracle"]["lock_sha256"] = json!("0".repeat(64));
    fs::write(&golden_path, serde_json::to_string_pretty(&golden).unwrap()).unwrap();

    let suite = Suite::open(root).unwrap();
    let fresh = suite.goldens_check(&GoldensCheck::default()).unwrap();
    let problem = &fresh.problems[0];
    assert_eq!(
        (problem.scenario.as_str(), problem.kind.as_str()),
        ("smoke/ids-frozen-collision", "stale_oracle")
    );
    assert!(
        problem.repro.contains("tools/mg oracle bump 26.9.3"),
        "{}",
        problem.repro
    );
    let report = suite.run(&[], &Canned(json!({})), "test");
    assert_eq!(report.outcomes[0].status, Status::Stale);
    assert!(report.outcomes[0].repro.contains("tools/mg oracle bump 26.9.3"));

    // The pin itself must match the oracle's lock file where it is known.
    let check = GoldensCheck {
        oracle_lock_sha256: Some("1".repeat(64)),
        ..GoldensCheck::default()
    };
    assert!(
        suite
            .goldens_check(&check)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.kind == "pin_mismatch")
    );
}

const FILTERED: &str = r#"id: preservation/filtered-rebuild
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
setup:
  decks: [Words]
  notetypes: [{name: Pair, fields: [Front, Back], templates: [{name: F, front: "{{Front}}"}]}]
  notes: [{notetype: Pair, deck: Words, fields: [a, b]}]
steps:
  - build_filtered: {name: Filtered, search: "deck:Words"}
  - observe: ["card(#c1)"]
"#;

fn golden_for(root: &Path, rel: &str, observations: Value) {
    // A golden in the oracle's format for a scenario with no oracle support yet.
    let loaded = meguri_testkit::scenario::load(&root.join(format!("scenarios/{rel}.yaml"))).unwrap();
    let golden = json!({
        "scenario_id": rel, "scenario_sha256": loaded.sha256, "tz": "UTC",
        "oracle": {"anki_version": "26.9.3", "lock_sha256": LOCK, "driver_sha256": "a".repeat(64)},
        "clock": {"start": "2030-01-15T10:00:00", "mode": "frozen"},
        "observations": observations,
    });
    write(
        root,
        &format!("goldens/{rel}.json"),
        &serde_json::to_string_pretty(&golden).unwrap(),
    );
}

#[test]
fn a_scenario_with_an_op_meguri_lacks_is_pending_and_t0_still_passes() {
    let dir = tree();
    let root = dir.path();
    write(root, "scenarios/preservation/filtered-rebuild.yaml", FILTERED);
    golden_for(
        root,
        "preservation/filtered-rebuild",
        json!({"steps": [{"build_filtered": {"deck": "Filtered", "cards": 1}}, {}]}),
    );
    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &MeguriExecutor::new(), "test");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Pending);
    let missing = outcome.missing.as_ref().unwrap();
    assert!(missing.ops.contains("build_filtered"));
    assert!(missing.setup.contains("notes"));
    assert!(report.failures().is_empty());
    assert!(
        report
            .render_text()
            .contains("preservation/filtered-rebuild: pending (needs ops: build_filtered")
    );
    assert_eq!(report.to_json()["totals"]["pending"], 1);

    // Once an area marks it active, pending fails T0.
    write(
        root,
        "scenarios/preservation/area.yaml",
        "area: preservation\nactive: [filtered-*]\n",
    );
    let report = Suite::open(root).unwrap().run(&[], &MeguriExecutor::new(), "test");
    assert_eq!(report.failures().len(), 1);
}

#[test]
fn search_differences_pass_only_under_a_deviation_entry() {
    let dir = tree();
    let root = dir.path();
    let scenario = "id: search/folding/umlaut\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps:\n  - observe: [{search: {query: \"über\"}}]\n";
    write(root, "scenarios/search/folding/umlaut.yaml", scenario);
    write(
        root,
        "scenarios/search/area.yaml",
        "area: search\nactive: [folding/*]\n",
    );
    golden_for(
        root,
        "search/folding/umlaut",
        json!({"steps": [{"search": {"ids": ["#n1"]}}]}),
    );
    let meguri = Canned(json!({"steps": [{"search": {"ids": ["#n1", "#n2"]}}]}));

    let report = Suite::open(root).unwrap().run(&[], &meguri, "test");
    assert_eq!(report.outcomes[0].status, Status::Fail);

    write(
        root,
        "deviations.yaml",
        "deviations:\n  - id: search-unicode-case-folding\n    requirement: R1036\n    summary: Unicode case folding in plain searches.\n    test: search/folding/unicode-mode\n    scenarios: [search/folding/*]\n    pointer: /steps/*/search/ids\n    transform: {replace: [\"#n1\", \"#n2\"]}\n",
    );
    let report = Suite::open(root).unwrap().run(&[], &meguri, "test");
    assert_eq!(report.outcomes[0].status, Status::Pass);
    assert_eq!(report.outcomes[0].deviations_applied, ["search-unicode-case-folding"]);
}

#[test]
fn missing_orphaned_and_misplaced_files_fail_the_freshness_check() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-tz-utc");
    write(
        root,
        "scenarios/smoke/no-golden.yaml",
        "id: smoke/no-golden\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\n",
    );
    write(
        root,
        "scenarios/smoke/wrong-id.yaml",
        "id: smoke/right-id\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\n",
    );
    write(
        root,
        "scenarios/smoke/twice.yaml",
        "id: smoke/twice\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\n",
    );
    write(
        root,
        "scenarios/smoke/twice.json",
        "{\"id\": \"smoke/twice\", \"clock\": {\"tz\": \"UTC\", \"start\": \"2030-01-15T10:00:00\"}}",
    );
    write(root, "goldens/smoke/gone.json", "{}");
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [timing-tz-utc, nothing-*]\nlive: [no-golden]\n",
    );
    let fresh = Suite::open(root)
        .unwrap()
        .goldens_check(&GoldensCheck::default())
        .unwrap();
    let mut kinds: Vec<(String, String)> = fresh
        .problems
        .iter()
        .map(|p| (p.kind.clone(), p.scenario.clone()))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        [
            ("area_pattern".to_owned(), "smoke/nothing-*".to_owned()),
            ("duplicate".to_owned(), "smoke/twice".to_owned()),
            ("invalid".to_owned(), "smoke/wrong-id".to_owned()),
            ("orphan_golden".to_owned(), "smoke/gone".to_owned()),
        ]
    );
    // A live scenario needs no golden; without the live entry it would be missing.
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [timing-tz-utc]\n",
    );
    let fresh = Suite::open(root)
        .unwrap()
        .goldens_check(&GoldensCheck::default())
        .unwrap();
    assert!(
        fresh
            .problems
            .iter()
            .any(|p| p.kind == "missing" && p.scenario == "smoke/no-golden")
    );
}

#[test]
fn goldens_check_groups_changed_values_by_area_and_kind_against_a_base() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "fsrs-new-card");
    copy_smoke(root, "timing-advance");
    let base = tempfile::tempdir().unwrap();
    for name in ["fsrs-new-card", "timing-advance"] {
        let from = root.join(format!("goldens/smoke/{name}.json"));
        write(
            base.path(),
            &format!("smoke/{name}.json"),
            &fs::read_to_string(from).unwrap(),
        );
    }
    // A re-recorded golden whose values moved (as after an oracle bump).
    let path = root.join("goldens/smoke/fsrs-new-card.json");
    let mut golden = read_json(&path);
    golden["observations"]["steps"][3]["card"]["#c1"]["due"] = json!(1);
    golden["observations"]["steps"][3]["next_states"]["#c1"]["good"]["scheduled_days"] = json!(3);
    golden["observations"]["steps"][6]["next_states"]["#c1"]["easy"]["scheduled_days"] = json!(27);
    fs::write(&path, serde_json::to_string_pretty(&golden).unwrap()).unwrap();

    let check = GoldensCheck {
        base: Some(base.path().to_path_buf()),
        ..GoldensCheck::default()
    };
    let fresh = Suite::open(root).unwrap().goldens_check(&check).unwrap();
    assert!(fresh.ok());
    let changed: Vec<(String, String, usize)> = fresh
        .changed
        .iter()
        .map(|c| (c.area.clone(), c.kind.clone(), c.count))
        .collect();
    assert_eq!(
        changed,
        [
            ("smoke".to_owned(), "card".to_owned(), 1),
            ("smoke".to_owned(), "next_states".to_owned(), 2)
        ]
    );
    assert_eq!(fresh.changed_goldens, ["smoke/fsrs-new-card"]);
}

#[test]
fn the_meguri_executor_passes_an_empty_scenario_and_moves_its_clock() {
    let dir = tree();
    let root = dir.path();
    let text = fs::read_to_string(repo_conformance().join("scenarios/smoke/runner-empty.yaml")).unwrap();
    write(root, "scenarios/smoke/runner-empty.yaml", &text);
    golden_for(root, "smoke/runner-empty", json!({"steps": []}));
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [runner-empty]\n",
    );
    let report = Suite::open(root).unwrap().run(&[], &MeguriExecutor::new(), "test");
    assert_eq!(report.outcomes[0].status, Status::Pass, "{}", report.render_text());

    let clocky = "id: smoke/clocky\nclock: {tz: Europe/Paris, start: \"2030-01-15T10:00:00\"}\nsteps:\n  - advance: 20m\n  - set_clock: \"2030-03-31T03:00:00\"\n";
    write(root, "scenarios/smoke/clocky.yaml", clocky);
    golden_for(root, "smoke/clocky", json!({"steps": [{}, {}]}));
    let report = Suite::open(root)
        .unwrap()
        .run(&["smoke/clocky".to_owned()], &MeguriExecutor::new(), "test");
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(report.outcomes[0].status, Status::Pass, "{}", report.render_text());
    // A wall time in the spring-forward gap is an executor error at that step.
    fs::write(
        root.join("scenarios/smoke/clocky.yaml"),
        clocky.replace("03:00:00", "02:30:00"),
    )
    .unwrap();
    golden_for(root, "smoke/clocky", json!({"steps": [{}, {}]}));
    let report = Suite::open(root)
        .unwrap()
        .run(&["smoke/clocky".to_owned()], &MeguriExecutor::new(), "test");
    assert_eq!(report.outcomes[0].status, Status::Error, "{}", report.render_text());
    assert!(
        report.outcomes[0].message.contains("step 1"),
        "{}",
        report.outcomes[0].message
    );
}

#[test]
fn two_thousand_trivial_scenarios_run_in_under_ten_seconds() {
    let dir = tree();
    let root = dir.path();
    let template = "id: bulk/s{n}\nclock: {tz: Europe/Paris, start: \"2030-01-15T10:00:00\", mode: step}\nsteps: [{advance: 1d}]\n";
    for n in 0..2000 {
        write(
            root,
            &format!("scenarios/bulk/s{n}.yaml"),
            &template.replace("{n}", &n.to_string()),
        );
    }
    // Goldens as the oracle would write them for these scenarios.
    for n in 0..2000 {
        golden_for_bulk(root, n);
    }
    write(root, "scenarios/bulk/area.yaml", "area: bulk\nactive: [\"*\"]\n");

    let watch = Stopwatch::start();
    let report = Suite::open(root).unwrap().run(&[], &MeguriExecutor::new(), "test");
    let elapsed = watch.elapsed_ms();
    assert_eq!(report.outcomes.len(), 2000);
    assert!(report.failures().is_empty(), "{}", report.render_text());
    assert!(report.outcomes.iter().all(|o| o.status == Status::Pass));
    assert!(elapsed < 10_000, "T0 on 2,000 scenarios took {elapsed} ms");
}

fn golden_for_bulk(root: &Path, n: usize) {
    let rel = format!("bulk/s{n}");
    let loaded = meguri_testkit::scenario::load(&root.join(format!("scenarios/{rel}.yaml"))).unwrap();
    let golden = json!({
        "scenario_id": rel, "scenario_sha256": loaded.sha256, "tz": "Europe/Paris",
        "oracle": {"anki_version": "26.9.3", "lock_sha256": LOCK, "driver_sha256": "a".repeat(64)},
        "clock": {"start": "2030-01-15T10:00:00", "mode": "step"},
        "observations": {"steps": [{}]},
    });
    write(
        root,
        &format!("goldens/{rel}.json"),
        &serde_json::to_string(&golden).unwrap(),
    );
}

#[test]
fn filters_select_by_id_directory_or_pattern() {
    let dir = tree();
    let root = dir.path();
    for name in ["timing-advance", "timing-tz-utc", "ids-frozen-collision"] {
        copy_smoke(root, name);
    }
    let suite = Suite::open(root).unwrap();
    let ids = |filters: &[&str]| -> Vec<String> {
        let filters: Vec<String> = filters.iter().map(|s| (*s).to_owned()).collect();
        suite.discover(&filters).unwrap().into_iter().map(|f| f.rel).collect()
    };
    assert_eq!(ids(&[]).len(), 3);
    assert_eq!(ids(&["smoke"]).len(), 3);
    assert_eq!(
        ids(&["smoke/timing-*"]),
        ["smoke/timing-advance", "smoke/timing-tz-utc"]
    );
    assert_eq!(
        ids(&["smoke/ids-frozen-collision.yaml"]),
        ["smoke/ids-frozen-collision"]
    );
    assert!(ids(&["sched"]).is_empty());
}
