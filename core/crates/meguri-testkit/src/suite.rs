//! A conformance tree (`conformance/`): scenarios, goldens, area metadata, the
//! oracle pin and the deviations list; T0 statuses and the freshness check
//! (dsl.md D9-D13).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;
use serde_json::Value;

use crate::diff::{self, CompareOptions, Difference, pointer};
use crate::exec::{Executor, Needs};
use crate::report::Report;
use crate::scenario::{self, LoadedScenario};
use crate::wallclock::Stopwatch;

#[derive(Debug, Clone)]
pub struct SuiteError(String);

impl fmt::Display for SuiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SuiteError {}

fn suite_error(message: impl Into<String>) -> SuiteError {
    SuiteError(message.into())
}

/// `conformance/oracle.json`: the current oracle pin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OraclePin {
    pub anki_version: String,
    pub lock_sha256: String,
}

/// One `deviations.yaml` entry (D13).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deviation {
    pub id: String,
    pub requirement: String,
    pub summary: String,
    pub test: String,
    pub scenarios: Vec<String>,
    pub pointer: String,
    pub transform: Transform,
}

/// Oracle-side transforms of expected values (`diff::transforms`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transform {
    /// The expected value becomes this value.
    Replace(Value),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviationsFile {
    deviations: Vec<Deviation>,
}

/// `scenarios/<area>/area.yaml` (D9).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Area {
    area: String,
    #[serde(default)]
    active: Vec<String>,
    #[serde(default)]
    live: Vec<String>,
}

/// A golden file (D11).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Golden {
    #[allow(dead_code)] // part of the format; the hash already covers the id
    scenario_id: String,
    scenario_sha256: String,
    oracle: GoldenOracle,
    #[allow(dead_code)]
    tz: String,
    #[allow(dead_code)]
    clock: Value,
    observations: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenOracle {
    #[allow(dead_code)]
    anki_version: String,
    lock_sha256: String,
    #[allow(dead_code)]
    driver_sha256: String,
}

/// A scenario file in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioFile {
    pub path: PathBuf,
    /// The id the path gives: relative path without suffix.
    pub rel: String,
    pub area: String,
}

/// A scenario's T0 status, in the order the runner decides it (D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Invalid,
    Live,
    Missing,
    Stale,
    Pending,
    Error,
    Fail,
    Pass,
}

impl Status {
    pub const ALL: [Status; 8] = [
        Status::Invalid,
        Status::Live,
        Status::Missing,
        Status::Stale,
        Status::Pending,
        Status::Error,
        Status::Fail,
        Status::Pass,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Invalid => "invalid",
            Status::Live => "live",
            Status::Missing => "missing",
            Status::Stale => "stale",
            Status::Pending => "pending",
            Status::Error => "error",
            Status::Fail => "fail",
            Status::Pass => "pass",
        }
    }
}

/// Where the first difference is.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    pub step: usize,
    pub pointer: String,
    pub expected: Value,
    pub actual: Value,
}

/// How many differences a report shows per scenario (D12).
pub const SHOWN_DIFFERENCES: usize = 20;

/// The result for one scenario (D12).
#[derive(Debug, Clone)]
pub struct Outcome {
    pub scenario: String,
    pub path: PathBuf,
    pub area: String,
    pub active: bool,
    pub status: Status,
    pub message: String,
    pub repro: String,
    pub first_divergence: Option<Divergence>,
    pub differences: Vec<Difference>,
    pub missing: Option<Needs>,
    pub deviations_applied: Vec<String>,
}

impl Outcome {
    fn new(file: &ScenarioFile, active: bool) -> Outcome {
        Outcome {
            scenario: file.rel.clone(),
            path: file.path.clone(),
            area: file.area.clone(),
            active,
            status: Status::Pass,
            message: String::new(),
            repro: format!("tools/mg conformance {}", file.rel),
            first_divergence: None,
            differences: Vec::new(),
            missing: None,
            deviations_applied: Vec::new(),
        }
    }

    fn with(mut self, status: Status, message: impl Into<String>) -> Outcome {
        self.status = status;
        self.message = message.into();
        self
    }

    /// Invalid always fails T0; anything but a pass fails it when active.
    pub fn fails_t0(&self) -> bool {
        self.status == Status::Invalid || (self.active && self.status != Status::Pass)
    }

    /// The report text for this scenario (D12).
    pub fn render_text(&self) -> String {
        let active = if self.active { " [active]" } else { "" };
        let mut text = match (self.status, &self.first_divergence) {
            (Status::Fail, Some(first)) => format!(
                "{}: fail at step {}{active} ({} difference{})\n",
                self.scenario,
                first.step,
                self.differences.len(),
                if self.differences.len() == 1 { "" } else { "s" }
            ),
            (Status::Pending, _) => format!("{}: pending ({}){active}\n", self.scenario, self.message),
            (Status::Pass, _) => format!("{}: pass{active}\n", self.scenario),
            (status, _) => format!("{}: {}{active}: {}\n", self.scenario, status.as_str(), self.message),
        };
        for d in self.differences.iter().take(SHOWN_DIFFERENCES) {
            text.push_str(&format!(
                "  {}: expected {}, actual {}\n",
                d.pointer,
                diff::shown(&d.expected),
                diff::shown(&d.actual)
            ));
        }
        if self.differences.len() > SHOWN_DIFFERENCES {
            text.push_str(&format!(
                "  (+{} more differences)\n",
                self.differences.len() - SHOWN_DIFFERENCES
            ));
        }
        if !self.deviations_applied.is_empty() {
            text.push_str(&format!(
                "  deviations applied: {}\n",
                self.deviations_applied.join(", ")
            ));
        }
        if self.status != Status::Pass {
            text.push_str(&format!("  repro: {}\n", self.repro));
        }
        text
    }
}

/// Options of the freshness check.
#[derive(Debug, Clone, Default)]
pub struct GoldensCheck {
    /// Goldens of a base revision, to list changed values.
    pub base: Option<PathBuf>,
    /// sha256 of the oracle's lock file, where known.
    pub oracle_lock_sha256: Option<String>,
}

/// One freshness problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: String,
    pub scenario: String,
    pub message: String,
    pub repro: String,
}

/// Changed golden values in one area and observation kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedGroup {
    pub area: String,
    pub kind: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Freshness {
    pub scenarios: usize,
    pub problems: Vec<Problem>,
    /// Against the base: changed values by area and observation kind.
    pub changed: Vec<ChangedGroup>,
    pub changed_goldens: Vec<String>,
    pub added_goldens: Vec<String>,
    pub removed_goldens: Vec<String>,
}

impl Freshness {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Suite {
    root: PathBuf,
    pub pin: OraclePin,
    pub deviations: Vec<Deviation>,
    areas: BTreeMap<String, Area>,
    files: Vec<ScenarioFile>,
    duplicates: BTreeSet<String>,
    goldens: BTreeSet<String>,
}

impl Suite {
    /// Reads the tree's metadata and lists its scenario and golden files.
    pub fn open(root: &Path) -> Result<Suite, SuiteError> {
        let read =
            |rel: &str| std::fs::read_to_string(root.join(rel)).map_err(|e| suite_error(format!("reading {rel}: {e}")));
        let pin: OraclePin =
            serde_json::from_str(&read("oracle.json")?).map_err(|e| suite_error(format!("oracle.json: {e}")))?;
        let deviations = if root.join("deviations.yaml").exists() {
            let value = scenario::parse_yaml(&read("deviations.yaml")?)
                .map_err(|e| suite_error(format!("deviations.yaml: {e}")))?;
            let file: DeviationsFile =
                serde_json::from_value(value).map_err(|e| suite_error(format!("deviations.yaml: {e}")))?;
            check_deviations(&file.deviations)?;
            file.deviations
        } else {
            Vec::new()
        };

        let scenarios_dir = root.join("scenarios");
        let mut areas = BTreeMap::new();
        let mut files = Vec::new();
        for path in walk(&scenarios_dir)? {
            let rel_full = relative(&scenarios_dir, &path);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name == "area.yaml" || name == "area.yml" {
                let area = read_area(&path, &rel_full)?;
                areas.insert(area.area.clone(), area);
                continue;
            }
            let Some(suffix) = scenario::SUFFIXES.iter().find(|s| name.ends_with(*s)) else {
                continue;
            };
            let rel = rel_full[..rel_full.len() - suffix.len()].to_owned();
            let area = rel.split('/').next().unwrap_or_default().to_owned();
            files.push(ScenarioFile { path, rel, area });
        }
        files.sort_by(|a, b| a.rel.cmp(&b.rel).then_with(|| a.path.cmp(&b.path)));
        let duplicates = files
            .windows(2)
            .filter(|w| w[0].rel == w[1].rel)
            .map(|w| w[0].rel.clone())
            .collect::<BTreeSet<_>>();

        let goldens_dir = root.join("goldens");
        let goldens = walk(&goldens_dir)?
            .iter()
            .map(|p| relative(&goldens_dir, p))
            .filter_map(|rel| rel.strip_suffix(".json").map(str::to_owned))
            .collect();

        Ok(Suite {
            root: root.to_path_buf(),
            pin,
            deviations,
            areas,
            files,
            duplicates,
            goldens,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Scenario files matching any filter (all without filters): an id, a
    /// directory prefix, or a pattern (`*` also matches `/`), with or
    /// without suffix.
    pub fn discover(&self, filters: &[String]) -> Result<Vec<ScenarioFile>, SuiteError> {
        Ok(self
            .files
            .iter()
            .filter(|f| filters.is_empty() || filters.iter().any(|x| selects(x, f)))
            .cloned()
            .collect())
    }

    /// The area's patterns for `rel`, as full scenario id patterns.
    fn area_entries(&self, rel: &str, pick: fn(&Area) -> &Vec<String>) -> Vec<String> {
        let area = rel.split('/').next().unwrap_or_default();
        self.areas
            .get(area)
            .map(|a| pick(a).iter().map(|p| format!("{area}/{p}")).collect())
            .unwrap_or_default()
    }

    fn is_active(&self, rel: &str) -> bool {
        self.area_entries(rel, |a| &a.active)
            .iter()
            .any(|p| pointer::glob(p, rel))
    }

    fn is_live(&self, rel: &str) -> bool {
        self.area_entries(rel, |a| &a.live)
            .iter()
            .any(|p| pointer::glob(p, rel))
    }

    fn golden_path(&self, rel: &str) -> PathBuf {
        self.root.join("goldens").join(format!("{rel}.json"))
    }

    /// The scenario's file, metadata and golden checks, before any run.
    fn prepare(&self, file: &ScenarioFile) -> Result<(LoadedScenario, Option<Golden>), Box<Outcome>> {
        let active = self.is_active(&file.rel);
        let outcome = Outcome::new(file, active);
        if self.duplicates.contains(&file.rel) {
            return Err(Box::new(outcome.with(
                Status::Invalid,
                "several files share this id, so they would share one golden",
            )));
        }
        let loaded = match scenario::load(&file.path).and_then(|l| scenario::check_id(&l, &file.rel).map(|()| l)) {
            Ok(loaded) => loaded,
            Err(e) => return Err(Box::new(outcome.with(Status::Invalid, e.to_string()))),
        };
        let live = self.is_live(&file.rel);
        if live && active {
            return Err(Box::new(
                outcome.with(Status::Invalid, "area.yaml lists it as both active and live"),
            ));
        }
        if live {
            return Ok((loaded, None));
        }
        let golden_path = self.golden_path(&file.rel);
        if !golden_path.exists() {
            let mut outcome = outcome.with(Status::Missing, "no golden");
            outcome.repro = format!("tools/mg oracle record {}", file.rel);
            return Err(Box::new(outcome));
        }
        let golden: Golden = match std::fs::read_to_string(&golden_path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(golden) => golden,
            Err(e) => {
                return Err(Box::new(outcome.with(
                    Status::Invalid,
                    format!("goldens/{}.json is not a golden: {e}", file.rel),
                )));
            }
        };
        if golden.scenario_sha256 != loaded.sha256 {
            let mut outcome = outcome.with(
                Status::Stale,
                "the scenario changed since its golden was recorded (scenario_sha256 differs)",
            );
            outcome.repro = format!("tools/mg oracle record {}", file.rel);
            return Err(Box::new(outcome));
        }
        if golden.oracle.lock_sha256 != self.pin.lock_sha256 {
            let message = format!(
                "the golden was recorded with oracle lock {}, the pin is {} (conformance/oracle.json)",
                short(&golden.oracle.lock_sha256),
                short(&self.pin.lock_sha256)
            );
            let mut outcome = outcome.with(Status::Stale, message);
            outcome.repro = format!("tools/mg oracle bump {}", self.pin.anki_version);
            return Err(Box::new(outcome));
        }
        Ok((loaded, Some(golden)))
    }

    /// The T0 status of one scenario with this executor (D10).
    pub fn check(&self, file: &ScenarioFile, executor: &dyn Executor) -> Outcome {
        let (loaded, golden) = match self.prepare(file) {
            Ok(prepared) => prepared,
            Err(outcome) => return *outcome,
        };
        let outcome = Outcome::new(file, self.is_active(&file.rel));
        let missing = executor.missing(&loaded.scenario.needs());
        if !missing.is_empty() {
            let mut outcome = outcome.with(Status::Pending, format!("needs {}", missing.summary()));
            outcome.missing = Some(missing);
            return outcome;
        }
        let Some(golden) = golden else {
            return outcome.with(Status::Live, "T1 only: tools/mg conformance --live runs it");
        };
        let actual = match executor.run(&loaded) {
            Ok(actual) => actual,
            Err(e) => return outcome.with(Status::Error, e.to_string()),
        };
        let options = CompareOptions {
            set: &loaded.scenario.compare.set,
            deviations: &self.deviations,
        };
        let comparison = diff::compare(&file.rel, &golden.observations, &actual, &options);
        let mut outcome = outcome;
        outcome.deviations_applied = comparison.deviations_applied;
        if comparison.differences.is_empty() {
            return outcome;
        }
        let step = diff::first_divergence(&comparison.differences);
        let first = comparison
            .differences
            .iter()
            .find(|d| {
                step.is_none_or(|s| {
                    d.pointer == format!("/steps/{s}") || d.pointer.starts_with(&format!("/steps/{s}/"))
                })
            })
            .unwrap_or(&comparison.differences[0]);
        outcome.first_divergence = Some(Divergence {
            step: step.unwrap_or(0),
            pointer: first.pointer.clone(),
            expected: diff::shown(&first.expected),
            actual: diff::shown(&first.actual),
        });
        let count = comparison.differences.len();
        outcome.differences = comparison.differences;
        let at = step.map(|s| format!(" from step {s}")).unwrap_or_default();
        outcome.with(
            Status::Fail,
            format!("{count} difference{}{at}", if count == 1 { "" } else { "s" }),
        )
    }

    /// `check` for a scenario file given by path.
    pub fn check_path(&self, path: &Path, executor: &dyn Executor) -> Outcome {
        let wanted = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let file = self
            .files
            .iter()
            .find(|f| f.path.canonicalize().unwrap_or_else(|_| f.path.clone()) == wanted)
            .cloned()
            .unwrap_or_else(|| ScenarioFile {
                path: path.to_path_buf(),
                rel: path.display().to_string(),
                area: String::new(),
            });
        self.check(&file, executor)
    }

    /// Checks every selected scenario, in parallel, in id order.
    pub fn run(&self, filters: &[String], executor: &dyn Executor, lane: &str) -> Report {
        let watch = Stopwatch::start();
        let files = self.discover(filters).unwrap_or_default();
        let threads = std::thread::available_parallelism()
            .map_or(4, usize::from)
            .min(files.len().max(1));
        let next = AtomicUsize::new(0);
        let mut slots: Vec<Option<Outcome>> = vec![None; files.len()];
        let results: Vec<Vec<(usize, Outcome)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(file) = files.get(i) else { break };
                            done.push((i, self.check(file, executor)));
                        }
                        done
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
        });
        for (i, outcome) in results.into_iter().flatten() {
            slots[i] = Some(outcome);
        }
        Report {
            executor: executor.name().to_owned(),
            lane: lane.to_owned(),
            oracle_pin: self.pin.clone(),
            outcomes: slots.into_iter().flatten().collect(),
            wall_ms: watch.elapsed_ms(),
        }
    }

    /// The freshness check (D10): goldens for every scenario that needs one,
    /// recorded for the current scenario and the pinned oracle; no orphaned
    /// goldens; area entries that match; and, with a base, the changed values.
    pub fn goldens_check(&self, options: &GoldensCheck) -> Result<Freshness, SuiteError> {
        let mut fresh = Freshness {
            scenarios: self.files.len(),
            ..Freshness::default()
        };
        let problem = |kind: &str, scenario: &str, message: String, repro: String| Problem {
            kind: kind.to_owned(),
            scenario: scenario.to_owned(),
            message,
            repro,
        };
        let mut reported_duplicates = BTreeSet::new();
        for file in &self.files {
            match self.prepare(file).map_err(|o| *o) {
                Ok(_) => {}
                Err(outcome) => {
                    let kind = match outcome.status {
                        Status::Invalid if self.duplicates.contains(&file.rel) => {
                            if !reported_duplicates.insert(file.rel.clone()) {
                                continue;
                            }
                            "duplicate"
                        }
                        Status::Invalid => "invalid",
                        Status::Missing => "missing",
                        Status::Stale if outcome.repro.starts_with("tools/mg oracle bump") => "stale_oracle",
                        Status::Stale => "stale_scenario",
                        _ => "invalid",
                    };
                    fresh
                        .problems
                        .push(problem(kind, &file.rel, outcome.message, outcome.repro));
                }
            }
        }
        let scenario_ids: BTreeSet<&str> = self.files.iter().map(|f| f.rel.as_str()).collect();
        for rel in &self.goldens {
            if !scenario_ids.contains(rel.as_str()) {
                fresh.problems.push(problem(
                    "orphan_golden",
                    rel,
                    format!("goldens/{rel}.json has no scenario"),
                    format!("git rm conformance/goldens/{rel}.json, or restore its scenario"),
                ));
            }
        }
        for (name, area) in &self.areas {
            for pattern in area.active.iter().chain(&area.live) {
                let full = format!("{name}/{pattern}");
                if !self.files.iter().any(|f| pointer::glob(&full, &f.rel)) {
                    fresh.problems.push(problem(
                        "area_pattern",
                        &full,
                        format!("scenarios/{name}/area.yaml lists {pattern:?}, which matches no scenario"),
                        "tools/mg goldens-check".to_owned(),
                    ));
                }
            }
        }
        if let Some(lock) = &options.oracle_lock_sha256
            && lock != &self.pin.lock_sha256
        {
            fresh.problems.push(problem(
                "pin_mismatch",
                "conformance/oracle.json",
                format!(
                    "the oracle's lock file hashes to {}, the pin says {}: adopt the bump with its goldens",
                    short(lock),
                    short(&self.pin.lock_sha256)
                ),
                format!("tools/mg oracle bump {}", self.pin.anki_version),
            ));
        }
        if let Some(base) = &options.base {
            self.changed_since(base, &mut fresh)?;
        }
        Ok(fresh)
    }

    fn changed_since(&self, base: &Path, fresh: &mut Freshness) -> Result<(), SuiteError> {
        let base_goldens: BTreeSet<String> = walk(base)?
            .iter()
            .map(|p| relative(base, p))
            .filter_map(|rel| rel.strip_suffix(".json").map(str::to_owned))
            .collect();
        let mut groups: BTreeMap<(String, String), usize> = BTreeMap::new();
        for rel in &self.goldens {
            if !base_goldens.contains(rel) {
                fresh.added_goldens.push(rel.clone());
                continue;
            }
            let observations = |path: PathBuf| -> Value {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .and_then(|g| g.get("observations").cloned())
                    .unwrap_or(Value::Null)
            };
            let old = observations(base.join(format!("{rel}.json")));
            let new = observations(self.golden_path(rel));
            let differences = diff::exact_differences(&old, &new);
            if differences.is_empty() {
                continue;
            }
            fresh.changed_goldens.push(rel.clone());
            let area = rel.split('/').next().unwrap_or_default().to_owned();
            for d in differences {
                let segments = pointer::parse(&d.pointer);
                let kind = match segments.as_slice() {
                    [steps, _, kind, ..] if steps == "steps" => kind.clone(),
                    _ => "(structure)".to_owned(),
                };
                *groups.entry((area.clone(), kind)).or_default() += 1;
            }
        }
        fresh.removed_goldens = base_goldens.difference(&self.goldens).cloned().collect();
        fresh.changed = groups
            .into_iter()
            .map(|((area, kind), count)| ChangedGroup { area, kind, count })
            .collect();
        Ok(())
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn selects(filter: &str, file: &ScenarioFile) -> bool {
    let mut f = filter.trim_matches('/');
    for suffix in scenario::SUFFIXES {
        f = f.strip_suffix(suffix).unwrap_or(f);
    }
    let rel = file.rel.as_str();
    let full = file.path.file_name().and_then(|n| n.to_str()).map(|n| {
        let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if dir.is_empty() {
            n.to_owned()
        } else {
            format!("{dir}/{n}")
        }
    });
    rel == f
        || rel.starts_with(&format!("{f}/"))
        || pointer::glob(f, rel)
        || full.is_some_and(|full| pointer::glob(filter.trim_matches('/'), &full))
}

fn read_area(path: &Path, rel: &str) -> Result<Area, SuiteError> {
    let text = std::fs::read_to_string(path).map_err(|e| suite_error(format!("reading {rel}: {e}")))?;
    let value = scenario::parse_yaml(&text).map_err(|e| suite_error(format!("scenarios/{rel}: {e}")))?;
    let area: Area = serde_json::from_value(value).map_err(|e| suite_error(format!("scenarios/{rel}: {e}")))?;
    let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    if dir != area.area {
        return Err(suite_error(format!(
            "scenarios/{rel}: area {:?} must name its directory {dir:?} (area files live directly in an area directory)",
            area.area
        )));
    }
    if let Some(both) = area.active.iter().find(|p| area.live.contains(p)) {
        return Err(suite_error(format!(
            "scenarios/{rel}: {both:?} is listed as both active and live"
        )));
    }
    Ok(area)
}

fn check_deviations(deviations: &[Deviation]) -> Result<(), SuiteError> {
    let mut ids = BTreeSet::new();
    for d in deviations {
        let bad = |why: &str| suite_error(format!("deviations.yaml: entry {:?}: {why}", d.id));
        if !ids.insert(d.id.as_str()) {
            return Err(bad("the id is used twice"));
        }
        if !(d.requirement.starts_with('R')
            && d.requirement.len() > 1
            && d.requirement[1..].bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(bad("requirement must look like R1036"));
        }
        if !d.pointer.starts_with('/') {
            return Err(bad("pointer must start with /"));
        }
        if d.test.trim().is_empty() || d.scenarios.is_empty() {
            return Err(bad(
                "it needs a test asserting Meguri's behavior and at least one scenario pattern",
            ));
        }
    }
    Ok(())
}

/// Every file under `dir`, recursively; nothing when `dir` does not exist.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, SuiteError> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| suite_error(format!("reading {}: {e}", d.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| suite_error(format!("reading {}: {e}", d.display())))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| suite_error(format!("reading {}: {e}", path.display())))?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && !entry.file_name().to_string_lossy().starts_with('.') {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `path` relative to `dir`, with `/` separators.
fn relative(dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(dir).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
