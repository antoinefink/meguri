//! Every day clock scenario (`conformance/scenarios/sched/day/`, its
//! generated `matrix/`, and `sched/unbury-on-rollover/`;
//! `specs/sched/day-clock.md` D14) played on the engine's day clock and
//! with explicit compatibility queries through the writer, and compared
//! with the oracle's golden. Automatic clock reports must agree; the deviations of
//! `conformance/deviations.yaml` applied (dsl.md D13).
//!
//! Scenarios use U28's actual bootstrap before the normal engine open.
//! U44 owns the answer scenario; it is not run here.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::day::{self, Timing};
use meguri_engine::executor::catalog;
use meguri_engine::{Collection, DayCache, DayQuery, Engine, OpSpec, Ports, day_query};
use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, StampMode};
use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::clock::ScenarioClock;
use meguri_testkit::exec::db_exec_rows;
use meguri_testkit::scenario::{self, LoadedScenario, ObservationKind, SqlParam, Step};
use meguri_testkit::suite::Suite;
use serde_json::{Map, Value, json};
use tempfile::TempDir;

/// Scenarios this file cannot play yet, with what they wait for.
const WAITING: [(&str, &str); 1] = [("sched/day/legacy-timing-after-answer", "notes and answers (U27, U44)")];

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// One scenario on one engine.
struct Player {
    scenario: LoadedScenario,
    clock: Arc<ScenarioClock>,
    _dir: TempDir,
    path: PathBuf,
    engine: Engine,
    collection: Collection,
    compatibility_day: Arc<Mutex<DayCache>>,
}

impl Player {
    fn new(scenario: LoadedScenario) -> Player {
        let clock =
            Arc::new(ScenarioClock::new(&scenario.scenario.clock.tz, scenario.scenario.clock.start.as_str()).unwrap());
        let (dir, path) = day::new_collection(clock.as_ref(), Timing::New);
        let (engine, collection) = day::open(&path, clock.clone(), Ports::default());
        Player {
            scenario,
            clock,
            _dir: dir,
            path,
            engine,
            collection,
            compatibility_day: Arc::new(Mutex::new(DayCache::default())),
        }
    }

    fn c(&self) -> &Collection {
        &self.collection
    }

    /// dsl.md D6: the scenarios' setups hold config only.
    fn setup(&self) {
        let setup = &self.scenario.scenario.setup;
        assert!(
            setup.sections().iter().all(|s| *s == "config"),
            "this test plays only setup.config"
        );
        for (key, value) in setup.config.iter().flatten() {
            self.set_config(key, value);
        }
    }

    /// Waits until every op queued so far, the checks a change set started
    /// among them, has run: a background no-op queues behind them.
    fn settle(&self) {
        self.c()
            .run(OpSpec::named("settle", OpSource::Maintenance), |_| Ok(()))
            .unwrap();
    }

    /// W4: one config key through the config API, an op of its own that
    /// keeps no undo entry.
    fn set_config(&self, key: &str, value: &Value) {
        let timing = matches!(key, "rollover" | "creationOffset");
        let key = key.to_owned();
        let value = ConfigValue::parse(value.to_string().as_bytes()).expect("a JSON value");
        self.c()
            .run(
                OpSpec::named("set_config", OpSource::UserNonUndoable).skip_undo(),
                move |ctx| {
                    ctx.scope().set_config(&key, &value)?;
                    Ok(())
                },
            )
            .unwrap();
        self.settle();
        if timing {
            self.compatibility_day.lock().unwrap().clear();
            self.compatibility_query();
        }
    }

    /// W1: one statement on the writer connection, committed at once; one
    /// that writes drops the cached day (D6).
    fn db_exec(&self, sql: &str, params: Vec<SqlValue>) -> Value {
        let statement = sql.to_owned();
        let rows = self
            .c()
            .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), move |writer| {
                Ok(writer.execute_raw(&statement, &params)?)
            })
            .wait()
            .unwrap();
        if !sql.trim_start().to_ascii_lowercase().starts_with("select") {
            self.c().drop_cached_day().unwrap();
            self.compatibility_day.lock().unwrap().clear();
        }
        json!({ "rows": db_exec_rows(&rows) })
    }

    /// sync-ops.md S6: close, then open from the file; the open runs the
    /// day clock's first check.
    fn reopen(&mut self) {
        self.compatibility_day.lock().unwrap().clear();
        self.engine.close().unwrap();
        let (engine, collection) = day::open(&self.path, self.clock.clone(), Ports::default());
        self.engine = engine;
        self.collection = collection;
    }

    /// observations.md O4 `timing`: one `checkRollover()`.
    // The protocol timing observation explicitly asks for D5/D7 compatibility
    // writes. Native automatic checks have the no-churn policy of D8.
    fn compatibility_query(&self) -> DayQuery {
        let clock = self.clock.clone();
        let cache = self.compatibility_day.clone();
        self.c()
            .run(
                OpSpec::named("compatibility_day_query", OpSource::Rollover).stamp(StampMode::Raw),
                move |ctx| {
                    let now = clock.now_ms();
                    Ok(day_query(
                        ctx.scope(),
                        &mut cache.lock().unwrap(),
                        now.div_euclid(1000),
                        clock.utc_offset_at(now),
                    )?)
                },
            )
            .unwrap()
            .value
    }

    fn timing(&self) -> Value {
        let query = self.compatibility_query();
        let report = self.c().check_rollover().unwrap();
        assert_eq!(
            (report.today, report.next_day_at),
            (query.day.today, query.day.next_day_at)
        );
        let now_ms = self.clock.now_ms();
        json!({
            "days_elapsed": report.today,
            "next_day_at": report.next_day_at,
            "now": now_ms.div_euclid(1000),
            "utc_offset_secs": self.clock.utc_offset_at(now_ms).seconds(),
        })
    }

    fn step(&mut self, step: &Step) -> Value {
        match step {
            Step::Advance(by) => {
                self.clock.advance(*by);
                json!({})
            }
            Step::SetClock(at) => {
                self.clock.set(at.as_str()).unwrap();
                json!({})
            }
            Step::SetConfig(args) => {
                assert!(!args.undoable, "the day scenarios write config without undo");
                self.set_config(&args.key, &args.value);
                json!({})
            }
            Step::DbExec(args) => {
                let params = args
                    .params
                    .iter()
                    .flatten()
                    .map(|param| match param {
                        SqlParam::Text(text) => SqlValue::Text(text.clone()),
                        SqlParam::Integer(value) => SqlValue::Integer(*value),
                        SqlParam::Real(value) => SqlValue::Real(*value),
                        SqlParam::Null => SqlValue::Null,
                    })
                    .collect();
                json!({ "db_exec": self.db_exec(&args.sql, params) })
            }
            Step::Reopen(_) => {
                self.reopen();
                json!({})
            }
            Step::Observe(requests) => {
                let mut out = Map::new();
                for request in requests {
                    assert_eq!(request.kind, ObservationKind::Timing, "this test observes timing only");
                    out.insert("timing".to_owned(), self.timing());
                }
                Value::Object(out)
            }
            other => panic!("this test does not play {}", other.name()),
        }
    }
}

/// The scenario file of `id`.
fn scenario_path(id: &str) -> PathBuf {
    let base = conformance().join("scenarios").join(id);
    ["yaml", "json"]
        .iter()
        .map(|ext| base.with_extension(ext))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("no scenario file for {id}"))
}

/// Plays `id` and compares its observations with the golden's, as T0 does
/// (dsl.md D10, observations.md O7).
fn play(id: &str) {
    let loaded = scenario::load(&scenario_path(id)).unwrap();
    let golden: Value = meguri_testkit::observe::parse_json(
        &std::fs::read_to_string(conformance().join(format!("goldens/{id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        golden["scenario_sha256"].as_str(),
        Some(loaded.sha256.as_str()),
        "{id}: the golden is fresh"
    );
    let mut player = Player::new(loaded);
    player.setup();
    let steps: Vec<Step> = player.scenario.scenario.steps.clone();
    let observed: Vec<Value> = steps.iter().map(|step| player.step(step)).collect();

    let suite = Suite::open(&conformance()).unwrap();
    let comparison = diff::compare(
        id,
        &golden["observations"],
        &json!({ "steps": observed }),
        &CompareOptions {
            set: &player.scenario.scenario.compare.set,
            deviations: &suite.deviations,
            overlay: &[],
        },
    );
    assert!(
        comparison.differences.is_empty(),
        "{id}: differences from the golden: {:#?}",
        comparison
            .differences
            .iter()
            .map(|d| (&d.pointer, &d.expected, &d.actual))
            .collect::<Vec<_>>()
    );
    // Exactly the deviations listed for the scenario applied.
    let listed: Vec<&str> = suite
        .deviations
        .iter()
        .filter(|d| d.id != "fp-libm-boundary" && d.scenarios.iter().any(|p| diff::pointer::glob(p, id)))
        .map(|d| d.id.as_str())
        .collect();
    let applied: Vec<&str> = comparison
        .deviations_applied
        .iter()
        .map(String::as_str)
        .filter(|d| *d != "fp-libm-boundary")
        .collect();
    assert_eq!(applied, listed, "{id}: the deviations that applied");
}

/// The scenario ids under `dir` (relative to `conformance/scenarios`).
fn ids_in(dir: &str) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(conformance().join("scenarios").join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_owned();
            let stem = name.strip_suffix(".yaml").or_else(|| name.strip_suffix(".json"))?;
            (stem != "area").then(|| format!("{dir}/{stem}"))
        })
        .collect();
    ids.sort();
    ids
}

macro_rules! scenarios {
    ($($name:ident => $id:literal,)*) => {
        $(
            #[test]
            fn $name() {
                play($id);
            }
        )*

        /// No day clock scenario goes unplayed: each has its test above or
        /// waits in `WAITING`.
        #[test]
        fn every_day_clock_scenario_is_played_here() {
            let played = [$($id),*];
            for dir in ["sched/day", "sched/day/matrix", "sched/unbury-on-rollover"] {
                for id in ids_in(dir) {
                    assert!(
                        played.contains(&id.as_str()) || WAITING.iter().any(|(w, _)| *w == id),
                        "{id} is not played"
                    );
                }
            }
        }
    };
}

scenarios! {
    ae84_kathmandu => "sched/day/ae84-kathmandu",
    clock_back => "sched/day/clock-back",
    config_values => "sched/day/config-values",
    created_los_angeles_studied_tokyo => "sched/day/created-los-angeles-studied-tokyo",
    created_tokyo_studied_los_angeles => "sched/day/created-tokyo-studied-los-angeles",
    creation_after_rollover_tokyo => "sched/day/creation-after-rollover-tokyo",
    creation_at_rollover_utc => "sched/day/creation-at-rollover-utc",
    creation_before_rollover => "sched/day/creation-before-rollover",
    creation_dst_fall => "sched/day/creation-dst-fall",
    creation_dst_spring => "sched/day/creation-dst-spring",
    dst_america_los_angeles => "sched/day/dst-america-los-angeles",
    dst_america_new_york => "sched/day/dst-america-new-york",
    dst_australia_lord_howe => "sched/day/dst-australia-lord-howe",
    dst_europe_paris => "sched/day/dst-europe-paris",
    dst_gap_rollover_2_europe_paris => "sched/day/dst-gap-rollover-2-europe-paris",
    dst_overlap_rollover_1_america_new_york => "sched/day/dst-overlap-rollover-1-america-new-york",
    legacy_timing => "sched/day/legacy-timing",
    legacy_timing_kept_on_reopen => "sched/day/legacy-timing-kept-on-reopen",
    offset_change_recompute => "sched/day/offset-change-recompute",
    rollover_change_recompute => "sched/day/rollover-change-recompute",
    matrix_america_los_angeles => "sched/day/matrix/america-los-angeles",
    matrix_america_new_york => "sched/day/matrix/america-new-york",
    matrix_america_st_johns => "sched/day/matrix/america-st-johns",
    matrix_asia_kathmandu => "sched/day/matrix/asia-kathmandu",
    matrix_asia_kolkata => "sched/day/matrix/asia-kolkata",
    matrix_asia_tokyo => "sched/day/matrix/asia-tokyo",
    matrix_australia_lord_howe => "sched/day/matrix/australia-lord-howe",
    matrix_europe_paris => "sched/day/matrix/europe-paris",
    matrix_pacific_chatham => "sched/day/matrix/pacific-chatham",
    matrix_pacific_kiritimati => "sched/day/matrix/pacific-kiritimati",
    matrix_utc => "sched/day/matrix/utc",
    unbury_both_kinds => "sched/unbury-on-rollover/both-kinds",
    unbury_last_unburied_window => "sched/unbury-on-rollover/last-unburied-window",
    unbury_open_next_day => "sched/unbury-on-rollover/open-next-day",
}
