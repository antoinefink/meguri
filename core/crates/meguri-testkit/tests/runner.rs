//! The T0 runner and the freshness check over a conformance tree
//! (specs/conformance/dsl.md D8-D13). Fixtures are copies of committed
//! scenarios and oracle-recorded goldens in a temporary tree.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{
    ExecError, Executor, MeguriExecutor, Needs, SegmentExecutor, SegmentOutput, SegmentRequest,
};
use meguri_testkit::scenario::{LoadedScenario, SyncSpec};
use meguri_testkit::suite::{GoldensCheck, LiveRun, Status, Suite};
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

impl SegmentExecutor for Canned {
    fn segment(&self, _scenario: &LoadedScenario, _request: &SegmentRequest) -> Result<SegmentOutput, ExecError> {
        Err(ExecError::new("unsupported_op", "canned executors run no segments"))
    }
}

/// An oracle that fails every run, as a worker that cannot start does.
struct Broken;

impl Executor for Broken {
    fn name(&self) -> &str {
        "oracle"
    }
    fn missing(&self, _needs: &Needs) -> Needs {
        Needs::default()
    }
    fn run(&self, _scenario: &LoadedScenario) -> Result<Value, ExecError> {
        Err(ExecError::new("worker_start", "no worker"))
    }
}

impl SegmentExecutor for Broken {
    fn segment(&self, _scenario: &LoadedScenario, _request: &SegmentRequest) -> Result<SegmentOutput, ExecError> {
        Err(ExecError::new("worker_start", "no worker"))
    }
}

/// T1 with this oracle and Meguri's executor, no handoff directory.
fn t1<'a>(oracle: &'a dyn SegmentExecutor, meguri: &'a MeguriExecutor) -> LiveRun<'a> {
    LiveRun {
        oracle,
        meguri,
        handoff_root: None,
        meguri_edge_cmd: None,
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
    assert!(!missing.setup.contains("notes"));
    assert!(!missing.setup.contains("decks"));
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
fn a_run_from_the_new_collection_is_pending_until_an_area_makes_it_before_the_setup() {
    use meguri_testkit::exec::Session;
    use meguri_testkit::scenario::{ObservationRequest, Setup};

    // Each adapter leaves a mark in the session, in the order they ran.
    fn make(session: &mut Session<'_>) -> Result<(), ExecError> {
        session.notes.push(1);
        Ok(())
    }
    fn decks(session: &mut Session<'_>, _: &Setup) -> Result<(), ExecError> {
        session.notes.push(2);
        Ok(())
    }
    fn marks(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
        Ok(json!(session.notes))
    }
    let dir = tree();
    let root = dir.path();
    let text = "id: smoke/fresh\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsetup: {decks: [A]}\nsteps: [{observe: [ids]}]\n";
    write(root, "scenarios/smoke/fresh.yaml", text);
    golden_for(root, "smoke/fresh", json!({"steps": [{"ids": [1, 2]}]}));
    let suite = Suite::open(root).unwrap();
    let registry = || {
        let mut registry = MeguriExecutor::area_registry(&["clock", "text"]);
        registry.setup("decks", decks);
        registry.observation("ids", marks);
        registry
    };

    // dsl.md D8: the scenario starts from a new collection, a setup
    // capability of its own.
    let report = suite.run(&[], &MeguriExecutor::with(registry()), "test");
    assert_eq!(report.outcomes[0].status, Status::Pending, "{}", report.render_text());
    let missing = report.outcomes[0].missing.clone().unwrap();
    assert_eq!(missing.setup.into_iter().collect::<Vec<_>>(), ["new_collection"]);
    assert!(missing.ops.is_empty() && missing.observations.is_empty());
    assert!(
        report
            .render_text()
            .contains("smoke/fresh: pending (needs setup: new_collection)"),
        "{}",
        report.render_text()
    );

    // Once an area makes it, it is made before the setup runs.
    let mut with_new = registry();
    with_new.new_collection(make);
    let meguri = MeguriExecutor::with(with_new);
    assert!(meguri.capabilities().setup.contains("new_collection"));
    let report = suite.run(&[], &meguri, "test");
    assert_eq!(report.outcomes[0].status, Status::Pass, "{}", report.render_text());
}

#[test]
fn search_differences_pass_only_under_a_deviation_entry() {
    let dir = tree();
    let root = dir.path();
    let scenario = "id: search/folding/umlaut\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps:\n  - observe: [{search: [{query: \"über\"}]}]\n";
    write(root, "scenarios/search/folding/umlaut.yaml", scenario);
    write(
        root,
        "scenarios/search/area.yaml",
        "area: search\nactive: [folding/*]\n",
    );
    golden_for(
        root,
        "search/folding/umlaut",
        json!({"steps": [{"search": [{"ids": ["#n1"]}]}]}),
    );
    let meguri = Canned(json!({"steps": [{"search": [{"ids": ["#n1", "#n2"]}]}]}));

    let report = Suite::open(root).unwrap().run(&[], &meguri, "test");
    assert_eq!(report.outcomes[0].status, Status::Fail);

    write(
        root,
        "deviations.yaml",
        "deviations:\n  - id: search-unicode-case-folding\n    requirement: R1036\n    summary: Unicode case folding in plain searches.\n    test: search/folding/unicode-mode\n    scenarios: [search/folding/*]\n    pointer: /steps/*/search/*/ids\n    transform: {replace: [\"#n1\", \"#n2\"]}\n",
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
        "area: smoke\nactive: [timing-tz-utc, nothing-*]\n",
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
            ("missing".to_owned(), "smoke/no-golden".to_owned()),
            ("orphan_golden".to_owned(), "smoke/gone".to_owned()),
        ]
    );
    // A live scenario needs its golden like any other: the oracle records it
    // running every segment itself.
    write(
        root,
        "scenarios/smoke/chain.yaml",
        "id: smoke/chain\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{advance: 1d}, {handoff: oracle}, {observe: [timing]}]\n",
    );
    let fresh = Suite::open(root)
        .unwrap()
        .goldens_check(&GoldensCheck::default())
        .unwrap();
    assert!(
        fresh
            .problems
            .iter()
            .any(|p| p.kind == "missing" && p.scenario == "smoke/chain")
    );
    // area.yaml has no live list: a scenario is live by its handoff steps.
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [timing-tz-utc]\nlive: [chain]\n",
    );
    assert!(Suite::open(root).unwrap_err().to_string().contains("live"));
}

#[test]
fn two_metadata_files_for_one_area_are_an_error() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-tz-utc");
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [timing-tz-utc]\n",
    );
    // An empty second file would otherwise drop the activation promise.
    write(root, "scenarios/smoke/area.yml", "area: smoke\n");
    let err = Suite::open(root).unwrap_err().to_string();
    assert!(err.contains("one metadata file per area"), "{err}");
    fs::remove_file(root.join("scenarios/smoke/area.yml")).unwrap();
    let report = Suite::open(root)
        .unwrap()
        .run(&[], &Canned(json!({"steps": []})), "test");
    assert!(report.outcomes[0].active);
}

#[test]
fn goldens_check_lists_changed_values_by_area_and_kind_against_a_base() {
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
    let good = "/steps/3/next_states/#c1/good/scheduled_days";
    // The base recorded 5 days for Good at step 3.
    let base_path = base.path().join("smoke/fsrs-new-card.json");
    let mut old = read_json(&base_path);
    *old.pointer_mut(&format!("/observations{good}")).unwrap() = json!(5);
    fs::write(&base_path, serde_json::to_string_pretty(&old).unwrap()).unwrap();
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
        .map(|c| (c.area.clone(), c.kind.clone(), c.values.len()))
        .collect();
    assert_eq!(
        changed,
        [
            ("smoke".to_owned(), "card".to_owned(), 1),
            ("smoke".to_owned(), "next_states".to_owned(), 2)
        ]
    );
    let next_states = &fresh.changed[1].values;
    let value = next_states.iter().find(|v| v.pointer == good).unwrap();
    assert_eq!(value.golden, "smoke/fsrs-new-card");
    assert_eq!(
        (value.before.clone(), value.after.clone()),
        (Some(json!(5)), Some(json!(3)))
    );
    assert_eq!(next_states[1].pointer, "/steps/6/next_states/#c1/easy/scheduled_days");
    assert_eq!(
        (next_states[1].before.clone(), next_states[1].after.clone()),
        (Some(json!(26)), Some(json!(27)))
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
    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &MeguriExecutor::new(), "test");
    let elapsed = watch.elapsed_ms();
    assert_eq!(report.outcomes.len(), 2000);
    assert!(report.failures().is_empty(), "{}", report.render_text());
    assert!(report.outcomes.iter().all(|o| o.status == Status::Pass));
    assert!(elapsed < 10_000, "T0 on 2,000 scenarios took {elapsed} ms");

    // The T0 test harness goes through check_path, one scenario file at a
    // time (tests/conformance.rs).
    let executor = MeguriExecutor::new();
    let watch = Stopwatch::start();
    for n in 0..2000 {
        let outcome = suite.check_path(&root.join(format!("scenarios/bulk/s{n}.yaml")), &executor);
        assert_eq!(
            (outcome.scenario.as_str(), outcome.status),
            (format!("bulk/s{n}").as_str(), Status::Pass)
        );
    }
    let elapsed = watch.elapsed_ms();
    assert!(elapsed < 10_000, "check_path on 2,000 scenarios took {elapsed} ms");
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

/// Meguri's executor, panicking in the middle of one scenario.
struct PanicsOn(&'static str);

impl Executor for PanicsOn {
    fn name(&self) -> &str {
        "meguri"
    }
    fn missing(&self, _needs: &Needs) -> Needs {
        Needs::default()
    }
    fn run(&self, scenario: &LoadedScenario) -> Result<Value, ExecError> {
        assert_ne!(scenario.scenario.id, self.0, "an adapter bug");
        Ok(json!({"steps": [{}]}))
    }
}

#[test]
fn a_panicking_executor_fails_its_scenario_and_loses_no_other() {
    let dir = tree();
    let root = dir.path();
    let template = "id: bulk/s{n}\nclock: {tz: Europe/Paris, start: \"2030-01-15T10:00:00\", mode: step}\nsteps: [{advance: 1d}]\n";
    for n in 0..200 {
        write(
            root,
            &format!("scenarios/bulk/s{n}.yaml"),
            &template.replace("{n}", &n.to_string()),
        );
        golden_for_bulk(root, n);
    }
    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &PanicsOn("bulk/s7"), "test");
    assert_eq!(report.outcomes.len(), 200);
    let failed = &report.outcomes.iter().find(|o| o.scenario == "bulk/s7").unwrap();
    assert_eq!(failed.status, Status::Error);
    assert!(failed.message.contains("an adapter bug"), "{}", failed.message);
    assert_eq!(report.outcomes.iter().filter(|o| o.status == Status::Pass).count(), 199);
    // Inactive: listed, T0 still passes; active: T0 fails.
    assert!(report.failures().is_empty());
    write(root, "scenarios/bulk/area.yaml", "area: bulk\nactive: [s7]\n");
    let report = Suite::open(root).unwrap().run(&[], &PanicsOn("bulk/s7"), "test");
    assert_eq!(report.failures().len(), 1);
    // The T0 test harness path reports it the same way.
    let outcome = Suite::open(root)
        .unwrap()
        .check_path(&root.join("scenarios/bulk/s7.yaml"), &PanicsOn("bulk/s7"));
    assert_eq!(outcome.status, Status::Error);
    assert!(outcome.fails_t0());
}

#[test]
fn seventeen_digit_floats_in_goldens_and_json_scenarios_are_read_exactly() {
    let dir = tree();
    let root = dir.path();
    // serde_json's default float parser reads this as 39.43013383563368.
    let exact = 39.430133835633676_f64;
    let yaml = "id: fsrs/retention\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsetup:\n  presets: [{name: Default, desired_retention: 39.430133835633676}]\n";
    write(root, "scenarios/fsrs/retention.yaml", yaml);
    let json_text = r#"{"id": "fsrs/retention-json", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "setup": {"presets": [{"name": "Default", "desired_retention": 39.430133835633676}]}}"#;
    write(root, "scenarios/fsrs/retention-json.json", json_text);
    let from_yaml = meguri_testkit::scenario::load(&root.join("scenarios/fsrs/retention.yaml")).unwrap();
    let from_json = meguri_testkit::scenario::load(&root.join("scenarios/fsrs/retention-json.json")).unwrap();
    let retention = |l: &LoadedScenario| l.value["setup"]["presets"][0]["desired_retention"].as_f64().unwrap();
    assert_eq!(retention(&from_yaml).to_bits(), exact.to_bits());
    assert_eq!(retention(&from_json).to_bits(), exact.to_bits());
    // So the JSON scenario hashes as the oracle's canon.py does (the id aside).
    let mut renamed = from_json.value.clone();
    renamed["id"] = json!("fsrs/retention");
    assert_eq!(
        meguri_testkit::scenario::scenario_sha256(&renamed).unwrap(),
        from_yaml.sha256
    );

    // A golden holding the value compares exactly with Meguri's.
    let observed = json!({"steps": [{"stats": {"retention": exact}}]});
    golden_for(root, "fsrs/retention", observed.clone());
    let text = fs::read_to_string(root.join("goldens/fsrs/retention.json")).unwrap();
    assert!(text.contains("39.430133835633676"), "{text}");
    write(root, "scenarios/fsrs/area.yaml", "area: fsrs\nactive: [retention]\n");
    fs::remove_file(root.join("scenarios/fsrs/retention-json.json")).unwrap();
    let report = Suite::open(root).unwrap().run(&[], &Canned(observed), "test");
    assert_eq!(report.outcomes[0].status, Status::Pass, "{}", report.render_text());
}

#[test]
fn deviations_apply_to_meguri_comparisons_and_not_to_oracle_replays() {
    let dir = tree();
    let root = dir.path();
    write(
        root,
        "scenarios/search/folding/umlaut.yaml",
        "id: search/folding/umlaut\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps:\n  - observe: [{search: [{query: \"über\"}]}]\n",
    );
    write(
        root,
        "scenarios/search/area.yaml",
        "area: search\nactive: [folding/*]\n",
    );
    let oracle_says = json!({"steps": [{"search": [{"ids": ["#n1"]}]}]});
    golden_for(root, "search/folding/umlaut", oracle_says.clone());
    write(
        root,
        "deviations.yaml",
        "deviations:\n  - id: search-unicode-case-folding\n    requirement: R1036\n    summary: Unicode case folding in plain searches.\n    test: search/folding/unicode-mode\n    scenarios: [search/folding/*]\n    pointer: /steps/*/search/*/ids\n    transform: {replace: [\"#n1\", \"#n2\"]}\n",
    );
    let suite = Suite::open(root).unwrap();
    // T0: Meguri's deliberate difference passes under the entry.
    let meguri_says = Canned(json!({"steps": [{"search": [{"ids": ["#n1", "#n2"]}]}]}));
    let t0 = suite.run(&[], &meguri_says, "test");
    assert_eq!(t0.outcomes[0].status, Status::Pass, "{}", t0.render_text());
    // T1: the oracle still reproduces its golden, untransformed.
    let meguri = MeguriExecutor::new();
    let oracle = Canned(oracle_says);
    let report = suite.run_live(&[], &t1(&oracle, &meguri), "test");
    assert_eq!(report.outcomes[0].status, Status::Pass, "{}", report.render_text());
    assert!(report.outcomes[0].deviations_applied.is_empty());
}

#[test]
fn t1_fails_on_any_oracle_drift_or_worker_error_active_or_not() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-tz-utc");
    copy_smoke(root, "runner-empty");
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [runner-empty]\n",
    );
    let golden = read_json(&root.join("goldens/smoke/timing-tz-utc.json"));
    let suite = Suite::open(root).unwrap();
    let meguri = MeguriExecutor::new();

    // The oracle reproduces both goldens: T1 passes.
    struct Replays(Value);
    impl Executor for Replays {
        fn name(&self) -> &str {
            "oracle"
        }
        fn missing(&self, _needs: &Needs) -> Needs {
            Needs::default()
        }
        fn run(&self, scenario: &LoadedScenario) -> Result<Value, ExecError> {
            Ok(match scenario.scenario.id.as_str() {
                "smoke/runner-empty" => json!({"steps": []}),
                _ => self.0.clone(),
            })
        }
    }
    impl SegmentExecutor for Replays {
        fn segment(&self, _: &LoadedScenario, _: &SegmentRequest) -> Result<SegmentOutput, ExecError> {
            unreachable!("no live scenario here")
        }
    }
    let faithful = Replays(golden["observations"].clone());
    let report = suite.run_live(&[], &t1(&faithful, &meguri), "test");
    assert!(report.failures().is_empty(), "{}", report.render_text());

    // An inactive scenario the oracle no longer reproduces fails T1 (T0
    // would only list it).
    let mut drifted = golden["observations"].clone();
    drifted["steps"][0]["timing"]["now"] = json!(1);
    let report = suite.run_live(&[], &t1(&Replays(drifted), &meguri), "test");
    let outcome = report
        .outcomes
        .iter()
        .find(|o| o.scenario == "smoke/timing-tz-utc")
        .unwrap();
    assert!(!outcome.active);
    assert_eq!(outcome.status, Status::Fail);
    assert!(outcome.message.contains("no longer reproduces"), "{}", outcome.message);
    assert_eq!(report.failures().len(), 1);
    assert_eq!(report.to_json()["tier"], "t1");
    assert_eq!(report.to_json()["failed"], 1);

    // Workers that cannot start fail T1 for every scenario.
    let report = suite.run_live(&[], &t1(&Broken, &meguri), "test");
    assert_eq!(report.failures().len(), 2);
    assert!(report.outcomes.iter().all(|o| o.status == Status::Error));
}

#[test]
fn an_inactive_scenario_that_breaks_the_schema_fails_t0() {
    let dir = tree();
    let root = dir.path();
    write(
        root,
        "scenarios/sched/filtered.yaml",
        "id: sched/filtered\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{build_filtered: {}}]\n",
    );
    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &MeguriExecutor::new(), "test");
    let outcome = &report.outcomes[0];
    assert!(!outcome.active);
    assert_eq!(outcome.status, Status::Invalid, "{}", report.render_text());
    assert!(
        outcome.message.contains("build_filtered needs name"),
        "{}",
        outcome.message
    );
    assert_eq!(report.failures().len(), 1);
    // The T0 test harness path.
    let outcome = suite.check_path(&root.join("scenarios/sched/filtered.yaml"), &MeguriExecutor::new());
    assert!(outcome.fails_t0());
}

/// The schema lets these members be left out, never be null.
#[test]
fn an_inactive_scenario_with_a_null_the_schema_forbids_fails_t0() {
    let dir = tree();
    let root = dir.path();
    let cases = [
        ("description", "description: null\n"),
        ("config", "setup: {config: null}\n"),
        (
            "preset-option",
            "setup: {presets: [{name: Default, new_per_day: null}]}\n",
        ),
        ("deck-preset", "setup: {decks: [{name: Words, preset: null}]}\n"),
        (
            "note-deck",
            "setup: {notes: [{notetype: Pair, deck: null, fields: [a]}]}\n",
        ),
        ("card-column", "setup: {cards: [{card: \"#c1\", due: null}]}\n"),
        ("card-data", "setup: {cards: [{card: \"#c1\", data: null}]}\n"),
        ("add-note-deck", "steps: [{add_note: {notetype: Pair, deck: null}}]\n"),
    ];
    let scenario = |name: &str, body: &str| {
        format!("id: nulls/{name}\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\n{body}")
    };
    for (name, body) in cases {
        write(root, &format!("scenarios/nulls/{name}.yaml"), &scenario(name, body));
    }
    // The schema refusal must precede capability checking or execution.
    let text = scenario("add-note-deck", cases[7].1);
    let sha = meguri_testkit::scenario::scenario_sha256(&meguri_testkit::scenario::parse_yaml(&text).unwrap()).unwrap();
    write(
        root,
        "goldens/nulls/add-note-deck.json",
        &serde_json::to_string_pretty(&json!({
            "scenario_id": "nulls/add-note-deck", "scenario_sha256": sha, "tz": "UTC",
            "oracle": {"anki_version": "26.9.3", "lock_sha256": LOCK, "driver_sha256": "a".repeat(64)},
            "clock": {"start": "2030-01-15T10:00:00", "mode": "frozen"},
            "observations": {"steps": [{"add_note": {"note": "#n1", "cards": ["#c1"]}}]},
        }))
        .unwrap(),
    );
    // Left out instead, the same members are fine (no golden: missing).
    write(
        root,
        "scenarios/nulls/left-out.yaml",
        &scenario(
            "left-out",
            "setup: {presets: [{name: Default}], decks: [{name: Words}], notes: [{notetype: Pair, fields: [a]}], cards: [{card: \"#c1\"}]}\n",
        ),
    );
    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &MeguriExecutor::new(), "test");
    assert_eq!(report.outcomes.len(), cases.len() + 1);
    for outcome in &report.outcomes {
        assert!(!outcome.active);
        let want = if outcome.scenario == "nulls/left-out" {
            Status::Missing
        } else {
            Status::Invalid
        };
        assert_eq!(outcome.status, want, "{}: {}", outcome.scenario, outcome.message);
    }
    assert_eq!(report.failures().len(), cases.len(), "{}", report.render_text());
}

#[test]
fn live_scenarios_are_pending_or_live_in_t0_and_never_run_there() {
    let dir = tree();
    let root = dir.path();
    let chain = "id: smoke/chain\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{advance: 1d}, {handoff: oracle}, {observe: [timing]}]\n";
    write(root, "scenarios/smoke/chain.yaml", chain);
    golden_for(root, "smoke/chain", json!({"steps": [{}, {}, {"timing": {}}]}));
    let suite = Suite::open(root).unwrap();
    // Without the storage area, Meguri has no handoff adapters: pending.
    let without_storage = || MeguriExecutor::area_registry(&["clock", "text"]);
    let report = suite.run(&[], &MeguriExecutor::with(without_storage()), "test");
    assert_eq!(report.outcomes[0].status, Status::Pending);
    assert!(report.outcomes[0].live);
    assert!(report.outcomes[0].missing.as_ref().unwrap().ops.contains("handoff"));
    // Its own segment needs only advance (timing is observed on the oracle).
    assert!(
        !report.outcomes[0]
            .missing
            .as_ref()
            .unwrap()
            .observations
            .contains("timing")
    );

    // Its first segment starts from the new collection (dsl.md D8, D9.2):
    // pending while no adapter makes it.
    let mut registry = without_storage();
    registry.handoff(|_, _| Ok(()), |_, _| Ok(()));
    let report = suite.run(&[], &MeguriExecutor::with(registry), "test");
    assert_eq!(report.outcomes[0].status, Status::Pending, "{}", report.render_text());
    let missing = report.outcomes[0].missing.clone().unwrap();
    assert_eq!(
        (
            missing.setup.iter().map(String::as_str).collect::<Vec<_>>(),
            missing.ops.len()
        ),
        (vec!["new_collection"], 0)
    );

    let mut registry = without_storage();
    registry.handoff(|_, _| Ok(()), |_, _| Ok(()));
    registry.new_collection(|_| Ok(()));
    let report = suite.run(&[], &MeguriExecutor::with(registry), "test");
    assert_eq!(report.outcomes[0].status, Status::Live, "{}", report.render_text());
    // With the storage area's own handoff adapters and the note type area's
    // new collection (notetype-ops.md T10.2) too.
    let report = suite.run(&[], &MeguriExecutor::new(), "test");
    assert_eq!(report.outcomes[0].status, Status::Live, "{}", report.render_text());
    assert_eq!(report.outcomes[0].repro, "tools/mg conformance --live smoke/chain");
    assert!(report.failures().is_empty());
}

#[test]
fn an_op_anki_refuses_is_recorded_and_the_run_goes_on() {
    use meguri_testkit::exec::{OpResult, Session};
    use meguri_testkit::scenario::Step;

    fn refuse(_: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
        Ok(OpResult::Rejected("no_such_note"))
    }
    fn refuse_oddly(_: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
        Ok(OpResult::Rejected("something_else"))
    }
    let dir = tree();
    let root = dir.path();
    let text = "id: notes/delete-twice\nclock: {tz: UTC, start: \"2030-01-15T10:00:00\"}\nsteps: [{delete_notes: {notes: [\"#n1\"]}}, {advance: 1d}, {delete_notes: {notes: [\"#n1\"]}}]\n";
    write(root, "scenarios/notes/delete-twice.yaml", text);
    write(
        root,
        "scenarios/notes/area.yaml",
        "area: notes\nactive: [delete-twice]\n",
    );
    let refused = json!({"delete_notes": {"error": "no_such_note"}});
    golden_for(root, "notes/delete-twice", json!({"steps": [refused, {}, refused]}));
    let suite = Suite::open(root).unwrap();

    // delete_notes touches the collection: the run starts from the new
    // collection (dsl.md D8), the note type area's (notetype-ops.md T10.2).
    let areas: Vec<_> = MeguriExecutor::AREAS
        .iter()
        .copied()
        .filter(|area| *area != "notes")
        .collect();
    let mut registry = MeguriExecutor::area_registry(&areas);
    registry.op_rejecting("delete_notes", &["no_such_note"], refuse);
    let report = suite.run(&[], &MeguriExecutor::with(registry), "test");
    assert_eq!(report.outcomes[0].status, Status::Pass, "{}", report.render_text());

    // A kind outside the op's list is the adapter's error, not a rejection.
    let mut registry = MeguriExecutor::area_registry(&areas);
    registry.op_rejecting("delete_notes", &["no_such_note"], refuse_oddly);
    let report = suite.run(&[], &MeguriExecutor::with(registry), "test");
    assert_eq!(report.outcomes[0].status, Status::Error);
    assert!(
        report.outcomes[0].message.contains("internal_error at step 0"),
        "{}",
        report.outcomes[0].message
    );
}

/// Copies a committed actor scenario under smoke/sync into the tree, with a
/// stand-in golden: T0 never compares an actor scenario's observations.
fn copy_sync_scenario(root: &Path, name: &str) {
    let rel = format!("smoke/sync/{name}");
    let from = repo_conformance().join(format!("scenarios/{rel}.yaml"));
    write(
        root,
        &format!("scenarios/{rel}.yaml"),
        &fs::read_to_string(from).unwrap(),
    );
    golden_for(root, &rel, json!({"steps": []}));
}

#[test]
fn actor_scenarios_are_live_or_pending_in_t0_and_never_run_there() {
    use meguri_testkit::exec::{OpResult, Session};
    use meguri_testkit::scenario::{ObservationRequest, Step};

    fn never_op(_: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
        Err(ExecError::new("internal_error", "T0 ran an actor scenario"))
    }
    fn never_observe(_: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
        Err(ExecError::new("internal_error", "T0 ran an actor scenario"))
    }
    fn never_dump(_: &mut Session<'_>) -> Result<Value, ExecError> {
        Err(ExecError::new("internal_error", "T0 ran an actor scenario"))
    }

    let dir = tree();
    let root = dir.path();
    copy_sync_scenario(root, "two-party");
    copy_sync_scenario(root, "meguri-downloads");
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nactive: [sync/two-party]\n",
    );
    let suite = Suite::open(root).unwrap();
    let find = |report: &meguri_testkit::report::Report, id: &str| {
        report.outcomes.iter().find(|o| o.scenario == id).cloned().unwrap()
    };

    // No Meguri actor: live, active or not (sync-ops.md S10.2).
    let report = suite.run(&[], &MeguriExecutor::new(), "test");
    let two = find(&report, "smoke/sync/two-party");
    assert_eq!(two.status, Status::Live, "{}", report.render_text());
    assert!(two.live && two.active);
    assert_eq!(two.repro, "tools/mg conformance --live smoke/sync/two-party");
    // The actual executor now supports this Meguri actor. T0 still lists it.
    let downloads = find(&report, "smoke/sync/meguri-downloads");
    assert_eq!(downloads.status, Status::Live, "{}", report.render_text());
    assert!(downloads.live);
    assert!(downloads.missing.is_none());
    assert!(report.failures().is_empty(), "{}", report.render_text());

    // An explicitly insufficient registry retains negative capability coverage.
    let registry = MeguriExecutor::area_registry(&["clock", "text", "storage", "notetypes", "cards"]);
    let report = suite.run(&[], &MeguriExecutor::with(registry), "test");
    let downloads = find(&report, "smoke/sync/meguri-downloads");
    assert_eq!(downloads.status, Status::Pending, "{}", report.render_text());
    assert!(downloads.live);
    let missing = downloads.missing.unwrap();
    assert_eq!(
        missing.ops.iter().map(String::as_str).collect::<Vec<_>>(),
        ["actor", "sync"]
    );
    assert!(missing.observations.is_empty());
    // Its new collection is the note type area's (notetype-ops.md T10.2).
    assert!(missing.setup.is_empty(), "{:?}", missing.setup);
    assert!(report.failures().is_empty(), "{}", report.render_text());

    // Once Meguri has what its actor needs, the scenario is live: T0 still
    // runs none of it (every adapter here would fail the run).
    let mut registry = MeguriExecutor::area_registry(&["clock", "text"]);
    registry.actor(never_dump);
    registry.new_collection(|_| Err(ExecError::new("internal_error", "T0 ran an actor scenario")));
    registry.op_rejecting("sync", &SyncSpec::REJECTIONS, never_op);
    registry.observation("card", never_observe);
    registry.observation("db_dump", never_observe);
    let report = suite.run(&[], &MeguriExecutor::with(registry), "test");
    let downloads = find(&report, "smoke/sync/meguri-downloads");
    assert_eq!(downloads.status, Status::Live, "{}", report.render_text());
    assert!(report.failures().is_empty(), "{}", report.render_text());

    // Meguri's executor alone refuses an actor scenario.
    let loaded = meguri_testkit::scenario::load(&root.join("scenarios/smoke/sync/two-party.yaml")).unwrap();
    let err = MeguriExecutor::new().run(&loaded).unwrap_err();
    assert_eq!(err.kind, "invalid_scenario");
    assert!(err.detail.contains("parties"), "{err}");
}

#[test]
fn every_committed_actor_scenario_loads_and_is_never_invalid_in_t0() {
    let suite = Suite::open(&repo_conformance()).unwrap();
    let report = suite.run(&["smoke/sync".to_owned()], &MeguriExecutor::new(), "test");
    assert_eq!(report.outcomes.len(), 5, "{}", report.render_text());
    for outcome in &report.outcomes {
        assert!(outcome.live, "{}", outcome.scenario);
        assert!(
            matches!(outcome.status, Status::Live | Status::Pending | Status::Missing),
            "{}",
            report.render_text()
        );
    }
    let downloads = report
        .outcomes
        .iter()
        .find(|o| o.scenario == "smoke/sync/meguri-downloads")
        .unwrap();
    assert_eq!(downloads.status, Status::Live, "{}", report.render_text());
    assert!(downloads.missing.is_none());
}

#[test]
fn explicit_filtered_answer_part_is_pending_in_preflight_while_plain_answer_runs() {
    for tagged in [false, true] {
        let dir = tree();
        let root = dir.path();
        copy_smoke(root, "fsrs-new-card");
        let path = root.join("scenarios/smoke/fsrs-new-card.yaml");
        if tagged {
            let original = fs::read_to_string(&path).unwrap();
            let mut value = meguri_testkit::scenario::parse_yaml(&original).unwrap();
            value["tags"] = json!(["needs:answer.filtered"]);
            fs::write(&path, value.to_string()).unwrap();
            // Only the temporary runner tree changes; comparison values are
            // the committed recording, not a fabricated answer expectation.
            let recorded = read_json(&root.join("goldens/smoke/fsrs-new-card.json"));
            golden_for(root, "smoke/fsrs-new-card", recorded["observations"].clone());
        }
        let suite = Suite::open(root).unwrap();
        let report = suite.run(&[], &MeguriExecutor::new(), "test");
        let outcome = &report.outcomes[0];
        if tagged {
            assert_eq!(outcome.status, Status::Pending);
            assert_eq!(
                outcome.missing.as_ref().unwrap().ops,
                ["answer.filtered".to_owned()].into()
            );
            assert_eq!(report.to_json()["totals"]["pending"], 1);
            assert_eq!(report.to_json()["totals"]["pass"], 0);
        } else {
            assert_eq!(outcome.status, Status::Pass, "{}", report.render_text());
        }
    }
}

const REFERENCE_METADATA: &str = "area: smoke\nreference:\n  timing-tz-utc:\n    reason: Recorded scope example.\n    spec: conformance/normalized-dump#N10\n";

struct NeverExecute;
impl Executor for NeverExecute {
    fn name(&self) -> &str {
        "must-not-execute"
    }
    fn missing(&self, _: &Needs) -> Needs {
        panic!("reference queried capabilities")
    }
    fn run(&self, _: &LoadedScenario) -> Result<Value, ExecError> {
        panic!("reference executed")
    }
}

#[test]
fn reference_metadata_report() {
    use meguri_testkit::suite::Platform;
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-tz-utc");
    copy_smoke(root, "runner-empty");
    write(root, "scenarios/smoke/area.yaml", REFERENCE_METADATA);
    for platform in [Platform::Linux, Platform::MacOs] {
        let suite = Suite::open(root).unwrap().on_platform(platform);
        assert!(suite.goldens_check(&GoldensCheck::default()).unwrap().ok());
        let report = suite.run(&["smoke/timing-tz-utc".to_owned()], &NeverExecute, "test");
        let outcome = &report.outcomes[0];
        assert_eq!(outcome.status, Status::Reference, "{}", report.render_text());
        assert!(!outcome.fails_t0() && !outcome.fails_t1());
        assert!(!outcome.active);
        assert_eq!(report.totals()["reference"], 1);
        assert_eq!(report.totals()["pass"], 0);
        assert_eq!(report.totals()["pending"], 0);
        let json = report.to_json();
        assert_eq!(
            json["scenarios"][0]["reference"],
            json!({
                "reason": "Recorded scope example.", "spec": "conformance/normalized-dump#N10"
            })
        );
        assert!(
            report
                .render_text()
                .contains("Recorded scope example. (conformance/normalized-dump#N10)")
        );
        // Inactive still executes and differs; only the exact declaration is exempt.
        let all = suite.run(&[], &Canned(json!({"steps": [{}]})), "test");
        assert_eq!(all.totals()["reference"], 1);
        assert_eq!(all.totals()["fail"], 1);
        assert_eq!(all.totals()["pass"], 0);
        assert!(all.outcomes.iter().all(|o| !o.active));
    }
}

#[test]
fn malformed_unknown_and_active_overlapping_reference_declarations_are_rejected() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-tz-utc");
    let value = "{reason: Scope example., spec: conformance/normalized-dump#N10}";
    let declarations = [
        format!("reference: {{unknown: {value}}}"),
        format!("active: ['timing-*']\nreference: {{timing-tz-utc: {value}}}"),
        format!("active: ['timing-??-utc']\nreference: {{timing-tz-utc: {value}}}"),
        format!("reference: {{'timing-*': {value}}}"),
        format!("reference: {{'timing-tz-utc.yaml': {value}}}"),
        format!("reference: {{'../smoke/timing-tz-utc': {value}}}"),
        "reference: null".to_owned(),
        "reference: []".to_owned(),
        "reference: {timing-tz-utc: null}".to_owned(),
        "reference: {timing-tz-utc: ['Scope example.', conformance/normalized-dump#N10]}".to_owned(),
        "reference: {timing-tz-utc: {reason: ' ', spec: conformance/normalized-dump#N10}}".to_owned(),
        "reference: {timing-tz-utc: {reason: \"a\\nb\", spec: conformance/normalized-dump#N10}}".to_owned(),
        "reference: {timing-tz-utc: {reason: Scope example., spec: R704}}".to_owned(),
        "reference: {timing-tz-utc: {reason: Scope example., spec: conformance/normalized-dump}}".to_owned(),
        "reference: {timing-tz-utc: {reason: Scope example., spec: conformance/normalized-dump#N10, extra: true}}"
            .to_owned(),
        "reference: {timing-tz-utc: {reason: Scope example.}}".to_owned(),
        "reference: {timing-tz-utc: {spec: conformance/normalized-dump#N10}}".to_owned(),
    ];
    for declaration in declarations {
        write(
            root,
            "scenarios/smoke/area.yaml",
            &format!("area: smoke\n{declaration}\n"),
        );
        assert!(Suite::open(root).is_err(), "accepted {declaration}");
    }
}

#[test]
fn reference_scope_never_hides_invalid_missing_or_stale_inputs() {
    let dir = tree();
    let root = dir.path();
    copy_smoke(root, "timing-tz-utc");
    write(root, "scenarios/smoke/area.yaml", REFERENCE_METADATA);
    let path = root.join("goldens/smoke/timing-tz-utc.json");
    let golden = fs::read_to_string(&path).unwrap();
    for (text, expected) in [
        (None, Status::Missing),
        (Some("{}".to_owned()), Status::Invalid),
        (Some(golden.replace(LOCK, &"0".repeat(64))), Status::Stale),
    ] {
        match text {
            Some(text) => fs::write(&path, text).unwrap(),
            None => fs::remove_file(&path).unwrap(),
        }
        let suite = Suite::open(root).unwrap();
        let report = suite.run(&[], &NeverExecute, "test");
        assert_eq!(report.outcomes[0].status, expected);
        assert!(report.outcomes[0].reference.is_some());
        assert_eq!(report.totals()["reference"], 0);
        assert!(!suite.goldens_check(&GoldensCheck::default()).unwrap().ok());
        let live = suite.run_live(&[], &t1(&Broken, &MeguriExecutor::new()), "test");
        assert_eq!(live.outcomes[0].status, expected);
        assert_eq!(live.failures().len(), 1);
        fs::write(&path, &golden).unwrap();
    }
    let scenario_path = root.join("scenarios/smoke/timing-tz-utc.yaml");
    let original = fs::read_to_string(&scenario_path).unwrap();
    fs::write(&scenario_path, original + "\ndescription: changed input\n").unwrap();
    let suite = Suite::open(root).unwrap();
    assert_eq!(suite.run(&[], &NeverExecute, "test").outcomes[0].status, Status::Stale);
    fs::write(&scenario_path, "id: smoke/timing-tz-utc\nsteps: []\n").unwrap();
    let suite = Suite::open(root).unwrap();
    let report = suite.run(&[], &NeverExecute, "test");
    assert_eq!(report.outcomes[0].status, Status::Invalid);
    assert_eq!(report.failures().len(), 1);
    assert!(report.outcomes[0].reference.is_some());
}

#[test]
fn reference_requires_matching_oracle_replay_with_no_deviation_or_meguri_chain() {
    let dir = tree();
    let root = dir.path();
    // A generic live reference must replay, but never require Meguri's missing handoff adapters.
    write(
        root,
        "scenarios/smoke/reference-chain.yaml",
        "id: smoke/reference-chain\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsteps: [{advance: 1d}, {handoff: meguri}, {observe: [timing]}]\n",
    );
    let observed = json!({"steps": [{}, {}, {"timing": {"now": 1}}]});
    golden_for(root, "smoke/reference-chain", observed.clone());
    write(
        root,
        "scenarios/smoke/area.yaml",
        &REFERENCE_METADATA.replace("timing-tz-utc", "reference-chain"),
    );
    let suite = Suite::open(root).unwrap();
    let meguri = MeguriExecutor::with(MeguriExecutor::area_registry(&[]));
    let faithful = Canned(observed);
    let mismatched = Canned(json!({"steps": []}));
    for (oracle, status) in [
        (&faithful as &dyn SegmentExecutor, Status::Reference),
        (&mismatched as &dyn SegmentExecutor, Status::Fail),
        (&Broken as &dyn SegmentExecutor, Status::Error),
    ] {
        let report = suite.run_live(&[], &t1(oracle, &meguri), "test");
        assert_eq!(report.outcomes[0].status, status, "{}", report.render_text());
        assert_eq!(report.totals()["pass"], 0);
        assert_eq!(report.totals()["reference"], usize::from(status == Status::Reference));
        assert_eq!(report.failures().len(), usize::from(status != Status::Reference));
        assert!(report.to_json()["scenarios"][0]["reference"].is_object());
    }
}

#[test]
fn prepared_batches_remain_pending_without_executing_any_prefix_or_mutating_inputs() {
    struct PreflightOnly(MeguriExecutor);
    impl Executor for PreflightOnly {
        fn name(&self) -> &str {
            "meguri"
        }
        fn missing(&self, needs: &Needs) -> Needs {
            self.0.missing(needs)
        }
        fn run(&self, _: &LoadedScenario) -> Result<Value, ExecError> {
            panic!("pending scenario was executed")
        }
    }
    let dir = tree();
    let root = dir.path();
    for (name, step, missing) in [
        (
            "io",
            json!({"update_notetype":{"notetype":"Pair","edits":[{"field_options":{"field":"Front","tag":null,"prevent_deletion":false}}]}}),
            "update_notetype.field_options.io",
        ),
        (
            "e8",
            json!({"export":{"format":"apkg","name":"x.apkg","limit":"collection","scheduling":false,"presets":false,"media":false,"legacy":true}}),
            "export.apkg.legacy",
        ),
        (
            "e9",
            json!({"export":{"format":"colpkg","name":"x.colpkg","media":true}}),
            "export.colpkg.media",
        ),
        (
            "observations",
            json!({"observe":[{"search_build":["whole_collection"]},{"tag_complete":{"cases":[{"input":"","limit":1}]}},{"card_stats":["#c1"]}]}),
            "card_stats",
        ),
    ] {
        let id = format!("prepared/{name}");
        let path = format!("scenarios/{id}.yaml");
        let value = json!({"id":id,"clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},
            "steps":[{"db_exec":{"sql":"UPDATE col SET mod = 7"}},step]});
        let text = serde_json::to_string(&value).unwrap();
        write(root, &path, &text);
        golden_for(root, &id, json!({"steps":[{},{}]}));
        let executor = PreflightOnly(MeguriExecutor::default());
        let before = executor.0.capabilities();
        let report = Suite::open(root)
            .unwrap()
            .run(std::slice::from_ref(&id), &executor, "prepared");
        assert_eq!(report.outcomes.len(), 1);
        let outcome = &report.outcomes[0];
        assert_eq!(outcome.status, Status::Pending);
        let needs = outcome.missing.as_ref().unwrap();
        assert!(needs.ops.contains(missing) || needs.observations.contains(missing));
        assert_eq!(before, executor.0.capabilities());
        assert_eq!(fs::read_to_string(root.join(path)).unwrap(), text);
    }
}
