//! A conformance tree (`conformance/`): scenarios, goldens, area metadata, the
//! oracle pin and the deviations list; T0 and T1 statuses and the freshness
//! check (dsl.md D9-D13).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;
use serde_json::Value;

use crate::diff::{self, CompareOptions, Difference, pointer};
use crate::exec::live::Chain;
use crate::exec::parties::Parties;
use crate::exec::{Executor, MeguriExecutor, Needs, SegmentExecutor};
use crate::observe;
use crate::observe::overlay::{self, Overlay};
use crate::report::{Report, Tier};
use crate::scenario::{self, ActorName, LoadedScenario, Side};
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

/// `conformance/oracle.json`'s pin of the macOS native lane's lock
/// (float-determinism.md F6.1); absent until that lock exists.
#[derive(Debug, Clone, Default, Deserialize)]
struct MacosPin {
    #[serde(default)]
    macos_lock_sha256: Option<String>,
}

/// Where Meguri's executor runs: overlays apply only natively on macOS
/// (float-determinism.md F9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
}

impl Platform {
    /// The platform this build runs on.
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
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
    /// `apple_overlay: true`, only the entry `fp-libm-boundary` (D13,
    /// float-determinism.md F8.6): on macOS the scenario's overlay entries
    /// under a matching pointer put their Apple values.
    AppleOverlay(bool),
}

/// The deviation that applies overlays (float-determinism.md F8.6).
pub const FP_LIBM_BOUNDARY: &str = "fp-libm-boundary";

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

/// Reads a golden, every float exactly (`observe::parse_json`).
fn read_golden(path: &Path) -> Result<Golden, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value = observe::parse_json(&text).map_err(|e| e.to_string())?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// A scenario file in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioFile {
    pub path: PathBuf,
    /// The id the path gives: relative path without suffix.
    pub rel: String,
    pub area: String,
}

/// A scenario's status, in the order the runner decides it (D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Invalid,
    Missing,
    Stale,
    Pending,
    Live,
    Error,
    Fail,
    Pass,
}

impl Status {
    pub const ALL: [Status; 8] = [
        Status::Invalid,
        Status::Missing,
        Status::Stale,
        Status::Pending,
        Status::Live,
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
    /// It has handoff steps (D9).
    pub live: bool,
    /// The hash of the scenario text this outcome is for (D3); `None` when
    /// the file did not load.
    pub scenario_sha256: Option<String>,
    pub status: Status,
    pub message: String,
    pub repro: String,
    pub first_divergence: Option<Divergence>,
    pub differences: Vec<Difference>,
    pub missing: Option<Needs>,
    pub deviations_applied: Vec<String>,
    /// On macOS, for a scenario with an overlay: how many overlay entries
    /// its expected values took (float-determinism.md F9).
    pub overlay_applied: Option<usize>,
}

impl Outcome {
    fn new(file: &ScenarioFile, active: bool) -> Outcome {
        Outcome {
            scenario: file.rel.clone(),
            path: file.path.clone(),
            area: file.area.clone(),
            active,
            live: false,
            scenario_sha256: None,
            status: Status::Pass,
            message: String::new(),
            repro: format!("tools/mg conformance {}", file.rel),
            first_divergence: None,
            differences: Vec::new(),
            missing: None,
            deviations_applied: Vec::new(),
            overlay_applied: None,
        }
    }

    fn with(mut self, status: Status, message: impl Into<String>) -> Outcome {
        self.status = status;
        self.message = message.into();
        self
    }

    /// T0: invalid always fails; an active scenario fails unless it passes or
    /// (live scenarios, which only T1 runs) is `live`.
    pub fn fails_t0(&self) -> bool {
        self.status == Status::Invalid || (self.active && !matches!(self.status, Status::Pass | Status::Live))
    }

    /// T1: anything but a pass or a pending live scenario fails, active or
    /// not (an oracle that no longer reproduces a golden, a worker error);
    /// pending fails when active.
    pub fn fails_t1(&self) -> bool {
        match self.status {
            Status::Pass => false,
            Status::Pending => self.active,
            _ => true,
        }
    }

    pub fn fails(&self, tier: Tier) -> bool {
        match tier {
            Tier::T0 => self.fails_t0(),
            Tier::T1 => self.fails_t1(),
        }
    }

    /// The report text for this scenario (D12).
    pub fn render_text(&self) -> String {
        let active = if self.active { " [active]" } else { "" };
        let mut text = match (self.status, &self.first_divergence) {
            (Status::Fail, Some(first)) => format!(
                "{}: fail at step {}{active} ({})\n",
                self.scenario, first.step, self.message
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

/// What T1 runs scenarios with: oracle workers, Meguri for its side of live
/// chains and its actors of actor scenarios, and the handoff directory both
/// see (dsl.md D9, D10; sync-ops.md S10).
pub struct LiveRun<'a> {
    pub oracle: &'a dyn SegmentExecutor,
    pub meguri: &'a MeguriExecutor,
    /// `None`: live chains are errors (no directory to hand over through).
    pub handoff_root: Option<&'a Path>,
    /// The Meguri edge of mixed runs (sync-ops.md S8.1); `None` until the
    /// first Meguri sync adapter wires it (U105).
    pub meguri_edge: Option<&'a str>,
}

/// Options of the freshness check.
#[derive(Debug, Clone, Default)]
pub struct GoldensCheck {
    /// Goldens of a base revision, to list changed values.
    pub base: Option<PathBuf>,
    /// sha256 of the oracle's lock file, where known.
    pub oracle_lock_sha256: Option<String>,
    /// Where the oracle tree is known: `Some(Some(hash))` when its macOS lock
    /// (`macos/requirements-macos.lock`) exists, `Some(None)` when it does
    /// not; `None` when the tree is not known (float-determinism.md F7.4).
    pub oracle_macos_lock_sha256: Option<Option<String>>,
}

/// One freshness problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: String,
    pub scenario: String,
    pub message: String,
    pub repro: String,
}

/// One golden value that changed since the base.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedValue {
    pub golden: String,
    pub pointer: String,
    /// `None`: absent on that side.
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// Changed golden values in one area and observation kind.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedGroup {
    pub area: String,
    pub kind: String,
    /// In golden, then pointer order.
    pub values: Vec<ChangedValue>,
}

/// Why `Suite::oracle_sides` has no sides for a scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidesError {
    /// The scenario or its golden: missing, invalid or stale.
    Golden(String),
    /// The overlay does not parse or is stale.
    Overlay(String),
}

/// A scenario's observations on the Linux and the Apple oracle
/// (`Suite::oracle_sides`).
#[derive(Debug, Clone)]
pub struct OracleSides {
    pub scenario: LoadedScenario,
    pub linux: Value,
    pub apple: Value,
    /// The lock the Linux golden was recorded with.
    pub linux_lock_sha256: String,
    pub overlay: Option<Overlay>,
}

/// One overlay and its entries by kind (float-determinism.md F7.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySummary {
    pub scenario: String,
    pub entries: usize,
    pub day_entries: usize,
    pub memory_entries: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Freshness {
    pub scenarios: usize,
    pub problems: Vec<Problem>,
    /// Every overlay that parses, in id order.
    pub overlays: Vec<OverlaySummary>,
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
    /// `conformance/oracle.json`'s `macos_lock_sha256`, when it has one.
    pub macos_lock_sha256: Option<String>,
    pub deviations: Vec<Deviation>,
    /// Where T0 runs Meguri's executor; overlays apply on macOS only.
    pub platform: Platform,
    /// Ids of the overlays in `goldens-apple/`.
    overlays: BTreeSet<String>,
    areas: BTreeMap<String, Area>,
    files: Vec<ScenarioFile>,
    /// Canonical scenarios directory and each file's index by its path
    /// relative to it (suffix included), for `check_path`.
    canonical_scenarios: Option<PathBuf>,
    by_relative_path: BTreeMap<String, usize>,
    duplicates: BTreeSet<String>,
    goldens: BTreeSet<String>,
}

impl Suite {
    /// Reads the tree's metadata and lists its scenario and golden files.
    pub fn open(root: &Path) -> Result<Suite, SuiteError> {
        let read =
            |rel: &str| std::fs::read_to_string(root.join(rel)).map_err(|e| suite_error(format!("reading {rel}: {e}")));
        let pin_text = read("oracle.json")?;
        let pin: OraclePin = serde_json::from_str(&pin_text).map_err(|e| suite_error(format!("oracle.json: {e}")))?;
        let macos: MacosPin = serde_json::from_str(&pin_text).map_err(|e| suite_error(format!("oracle.json: {e}")))?;
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
        let mut areas: BTreeMap<String, (String, Area)> = BTreeMap::new();
        let mut files = Vec::new();
        let mut relative_paths = Vec::new();
        for path in walk(&scenarios_dir)? {
            let rel_full = relative(&scenarios_dir, &path);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name == "area.yaml" || name == "area.yml" {
                let area = read_area(&path, &rel_full)?;
                if let Some((first, _)) = areas.get(&area.area) {
                    return Err(suite_error(format!(
                        "scenarios/{rel_full}: area {:?} already has scenarios/{first}; keep one metadata file per area",
                        area.area
                    )));
                }
                areas.insert(area.area.clone(), (rel_full, area));
                continue;
            }
            let Some(suffix) = scenario::SUFFIXES.iter().find(|s| name.ends_with(*s)) else {
                continue;
            };
            let rel = rel_full[..rel_full.len() - suffix.len()].to_owned();
            let area = rel.split('/').next().unwrap_or_default().to_owned();
            files.push(ScenarioFile { path, rel, area });
            relative_paths.push(rel_full);
        }
        let mut order: Vec<usize> = (0..files.len()).collect();
        order.sort_by(|&a, &b| {
            files[a]
                .rel
                .cmp(&files[b].rel)
                .then_with(|| files[a].path.cmp(&files[b].path))
        });
        let files: Vec<ScenarioFile> = order.iter().map(|&i| files[i].clone()).collect();
        let by_relative_path = order
            .iter()
            .enumerate()
            .map(|(sorted, &i)| (relative_paths[i].clone(), sorted))
            .collect();
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
        let overlays_dir = root.join(overlay::DIR);
        let overlays = walk(&overlays_dir)?
            .iter()
            .map(|p| relative(&overlays_dir, p))
            .filter_map(|rel| rel.strip_suffix(".json").map(str::to_owned))
            .collect();

        Ok(Suite {
            root: root.to_path_buf(),
            pin,
            macos_lock_sha256: macos.macos_lock_sha256,
            deviations,
            platform: Platform::current(),
            overlays,
            areas: areas.into_values().map(|(_, a)| (a.area.clone(), a)).collect(),
            files,
            canonical_scenarios: scenarios_dir.canonicalize().ok(),
            by_relative_path,
            duplicates,
            goldens,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The same tree as T0 sees it on `platform` (tests of both lanes).
    pub fn on_platform(mut self, platform: Platform) -> Suite {
        self.platform = platform;
        self
    }

    fn overlay_path(&self, rel: &str) -> PathBuf {
        self.root.join(overlay::DIR).join(format!("{rel}.json"))
    }

    /// Ids of the overlays in `goldens-apple/`.
    pub fn overlay_ids(&self) -> impl Iterator<Item = &str> {
        self.overlays.iter().map(String::as_str)
    }

    /// The scenario's overlay checked as the runner uses it on macOS
    /// (float-determinism.md F9 step 1): `None` when it has none; a stale
    /// one makes the scenario stale, one that fails its schema or the checks
    /// of F7.4 and F8 invalid.
    fn overlay_for(
        &self,
        rel: &str,
        loaded: &LoadedScenario,
        golden: &Golden,
        outcome: &Outcome,
    ) -> Result<Option<Overlay>, Box<Outcome>> {
        if !self.overlays.contains(rel) {
            return Ok(None);
        }
        let fail = |status: Status, message: String| {
            let mut outcome = outcome.clone().with(status, message);
            outcome.repro = format!("tools/mg oracle record --platform apple {rel}");
            Box::new(outcome)
        };
        let checked = self
            .fresh_overlay(rel, loaded, golden)
            .map_err(|(status, message)| fail(status, message))?;
        let problems = overlay::check_entries(&checked, &golden.observations, &loaded.scenario);
        if let Some(first) = problems.first() {
            return Err(fail(
                Status::Invalid,
                format!(
                    "{}/{rel}.json: {} entr{} fail{} the checks of float-determinism.md F7.4 and F8; first {}: {}",
                    overlay::DIR,
                    problems.len(),
                    if problems.len() == 1 { "y" } else { "ies" },
                    if problems.len() == 1 { "s" } else { "" },
                    first.pointer,
                    first.message
                ),
            ));
        }
        Ok(Some(checked))
    }

    /// A scenario's observations on both oracles (float-determinism.md
    /// F10.1): the Linux golden's, and the Apple oracle's, which are the
    /// golden's with every overlay entry put at its pointer (F9 step 2), in
    /// the class or not. Errors when the golden is missing or stale, or the
    /// overlay does not parse or is stale.
    pub fn oracle_sides(&self, rel: &str) -> Result<OracleSides, SidesError> {
        let file = self
            .files
            .iter()
            .find(|f| f.rel == rel)
            .ok_or_else(|| SidesError::Golden(format!("no scenario {rel}")))?;
        let (loaded, golden, _) = self
            .prepare(file)
            .map_err(|o| SidesError::Golden(format!("{rel}: {}: {}", o.status.as_str(), o.message)))?;
        let mut apple = golden.observations.clone();
        let overlay = if self.overlays.contains(rel) {
            let checked = self
                .fresh_overlay(rel, &loaded, &golden)
                .map_err(|(_, message)| SidesError::Overlay(message))?;
            overlay::apply(&checked.entries, &[], &mut apple);
            Some(checked)
        } else {
            None
        };
        Ok(OracleSides {
            linux: golden.observations,
            apple,
            linux_lock_sha256: golden.oracle.lock_sha256,
            overlay,
            scenario: loaded,
        })
    }

    /// The scenario's overlay when it parses and is fresh (F7.3); otherwise
    /// the status it gives the scenario (`invalid` or `stale`) and why.
    fn fresh_overlay(&self, rel: &str, loaded: &LoadedScenario, golden: &Golden) -> Result<Overlay, (Status, String)> {
        let checked = self.read_overlay(rel).map_err(|message| (Status::Invalid, message))?;
        let observations_sha256 = overlay::observations_sha256(&golden.observations)
            .map_err(|e| (Status::Invalid, format!("goldens/{rel}.json: {e}")))?;
        match overlay::staleness(
            &checked,
            &loaded.sha256,
            &observations_sha256,
            &self.pin.anki_version,
            self.macos_lock_sha256.as_deref(),
        ) {
            Some(why) => Err((Status::Stale, format!("{}/{rel}.json is stale: {why}", overlay::DIR))),
            None => Ok(checked),
        }
    }

    fn read_overlay(&self, rel: &str) -> Result<Overlay, String> {
        let path = self.overlay_path(rel);
        std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| Overlay::parse(&text))
            .map_err(|e| format!("{}/{rel}.json is not an overlay: {e}", overlay::DIR))
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

    fn is_active(&self, rel: &str) -> bool {
        let area = rel.split('/').next().unwrap_or_default();
        self.areas
            .get(area)
            .is_some_and(|a| a.active.iter().any(|p| pointer::glob(&format!("{area}/{p}"), rel)))
    }

    fn golden_path(&self, rel: &str) -> PathBuf {
        self.root.join("goldens").join(format!("{rel}.json"))
    }

    /// The scenario's file, metadata and golden checks, before any run; the
    /// outcome so far (active, live, hash) for the run to finish.
    fn prepare(&self, file: &ScenarioFile) -> Result<(LoadedScenario, Golden, Outcome), Box<Outcome>> {
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
        let mut outcome = outcome;
        outcome.live = loaded.scenario.is_live();
        outcome.scenario_sha256 = Some(loaded.sha256.clone());
        let golden_path = self.golden_path(&file.rel);
        if !golden_path.exists() {
            let mut outcome = outcome.with(Status::Missing, "no golden");
            outcome.repro = format!("tools/mg oracle record {}", file.rel);
            return Err(Box::new(outcome));
        }
        let golden = match read_golden(&golden_path) {
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
        Ok((loaded, golden, outcome))
    }

    /// Compares observations with a golden's and turns the result into a
    /// pass or a fail; `deviations` only where Meguri's behavior is in
    /// `actual` (dsl.md D13).
    #[allow(clippy::too_many_arguments)]
    fn judge(
        &self,
        mut outcome: Outcome,
        loaded: &LoadedScenario,
        golden: &Golden,
        actual: &Value,
        deviations: bool,
        overlay: Option<&Overlay>,
        what: &str,
    ) -> Outcome {
        let none = [];
        let options = CompareOptions {
            set: &loaded.scenario.compare.set,
            deviations: if deviations { &self.deviations } else { &none },
            overlay: overlay.map_or(&[][..], |o| &o.entries),
        };
        let comparison = diff::compare(&loaded.scenario.id, &golden.observations, actual, &options);
        outcome.deviations_applied = comparison.deviations_applied;
        if overlay.is_some() {
            outcome.overlay_applied = Some(comparison.overlay_applied);
        }
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
            format!("{what}{count} difference{}{at}", if count == 1 { "" } else { "s" }),
        )
    }

    /// The T0 status of one scenario with Meguri's executor (D10).
    pub fn check(&self, file: &ScenarioFile, meguri: &dyn Executor) -> Outcome {
        guarded(file, self.is_active(&file.rel), || self.check_t0(file, meguri))
    }

    fn check_t0(&self, file: &ScenarioFile, meguri: &dyn Executor) -> Outcome {
        let (loaded, golden, outcome) = match self.prepare(file) {
            Ok(prepared) => prepared,
            Err(outcome) => return *outcome,
        };
        let scenario = &loaded.scenario;
        let needs = if scenario.has_actors() {
            scenario.needs_of_meguri_actors()
        } else if outcome.live {
            scenario.needs_on(Side::Meguri)
        } else {
            scenario.needs()
        };
        let missing = meguri.missing(&needs);
        if !missing.is_empty() {
            let mut outcome = outcome.with(Status::Pending, format!("needs {}", missing.summary()));
            outcome.missing = Some(missing);
            return outcome;
        }
        if outcome.live {
            let what = if scenario.has_actors() {
                "an actor scenario"
            } else {
                "a live chain"
            };
            let mut outcome = outcome.with(Status::Live, format!("{what}: tools/mg conformance --live runs it"));
            outcome.repro = format!("tools/mg conformance --live {}", file.rel);
            return outcome;
        }
        // F9: overlays apply only where Meguri runs natively on macOS.
        let overlay = if self.platform == Platform::MacOs {
            match self.overlay_for(&file.rel, &loaded, &golden, &outcome) {
                Ok(overlay) => overlay,
                Err(outcome) => return *outcome,
            }
        } else {
            None
        };
        match meguri.run(&loaded) {
            Ok(actual) => self.judge(outcome, &loaded, &golden, &actual, true, overlay.as_ref(), ""),
            Err(e) => outcome.with(Status::Error, e.to_string()),
        }
    }

    /// The T1 status of one scenario (D10): the oracle replays it against its
    /// golden (no deviations: they describe Meguri), and a live scenario then
    /// runs as a chain with Meguri, compared with the same golden.
    pub fn check_live(&self, file: &ScenarioFile, live: &LiveRun<'_>) -> Outcome {
        let mut outcome = guarded(file, self.is_active(&file.rel), || self.check_t1(file, live));
        outcome.repro = match outcome.status {
            Status::Missing | Status::Stale => outcome.repro,
            _ => format!("tools/mg conformance --live {}", file.rel),
        };
        outcome
    }

    fn check_t1(&self, file: &ScenarioFile, live: &LiveRun<'_>) -> Outcome {
        let (loaded, golden, outcome) = match self.prepare(file) {
            Ok(prepared) => prepared,
            Err(outcome) => return *outcome,
        };
        if loaded.scenario.has_actors() {
            return self.check_t1_actors(outcome, &loaded, &golden, live);
        }
        let drift = match live.oracle.run(&loaded) {
            Ok(actual) => self.judge(
                outcome.clone(),
                &loaded,
                &golden,
                &actual,
                false,
                None,
                "the oracle no longer reproduces its golden: ",
            ),
            Err(e) => return outcome.with(Status::Error, format!("oracle: {e}")),
        };
        if drift.status != Status::Pass || !outcome.live {
            return drift;
        }
        let missing = live.meguri.missing(&loaded.scenario.needs_on(Side::Meguri));
        if !missing.is_empty() {
            let mut outcome = outcome.with(Status::Pending, format!("needs {}", missing.summary()));
            outcome.missing = Some(missing);
            return outcome;
        }
        let Some(root) = live.handoff_root else {
            return outcome.with(
                Status::Error,
                "a live chain needs a handoff directory (--handoff-dir; tools/mg conformance --live sets it)",
            );
        };
        let chain = Chain {
            meguri: live.meguri,
            oracle: live.oracle,
            root,
        };
        match chain.run(&loaded) {
            Ok(actual) => self.judge(outcome, &loaded, &golden, &actual, true, None, "the live chain: "),
            Err(e) => outcome.with(Status::Error, format!("live chain: {e}")),
        }
    }

    /// T1 of an actor scenario (sync-ops.md S10.3), in the parties executor:
    /// the replay, the oracle playing every actor, against the golden with no
    /// deviation; then, when the scenario has Meguri actors and Meguri has
    /// what they need, the mixed run, deviations applied.
    fn check_t1_actors(
        &self,
        outcome: Outcome,
        loaded: &LoadedScenario,
        golden: &Golden,
        live: &LiveRun<'_>,
    ) -> Outcome {
        let parties = Parties {
            oracle: live.oracle,
            meguri: live.meguri,
            meguri_edge: live.meguri_edge,
        };
        let replay = match parties.replay(loaded) {
            Ok(actual) => self.judge(
                outcome.clone(),
                loaded,
                golden,
                &actual,
                false,
                None,
                "the oracle no longer reproduces its golden: ",
            ),
            Err(e) => return outcome.with(Status::Error, format!("oracle replay: {e}")),
        };
        let scenario = &loaded.scenario;
        if replay.status != Status::Pass || !scenario.actors().iter().any(ActorName::is_meguri) {
            return replay;
        }
        let missing = live.meguri.missing(&scenario.needs_of_meguri_actors());
        if !missing.is_empty() {
            let mut outcome = outcome.with(Status::Pending, format!("needs {}", missing.summary()));
            outcome.missing = Some(missing);
            return outcome;
        }
        match parties.mixed(loaded) {
            Ok(actual) => self.judge(outcome, loaded, golden, &actual, true, None, "the mixed run: "),
            Err(e) => outcome.with(Status::Error, format!("mixed run: {e}")),
        }
    }

    /// `check` for a scenario file given by path (the T0 test harness).
    pub fn check_path(&self, path: &Path, meguri: &dyn Executor) -> Outcome {
        let found = path
            .canonicalize()
            .ok()
            .zip(self.canonical_scenarios.as_deref())
            .and_then(|(path, root)| path.strip_prefix(root).ok().map(|rel| relative(Path::new(""), rel)))
            .and_then(|rel| self.by_relative_path.get(&rel))
            .map(|&i| self.files[i].clone());
        let file = found.unwrap_or_else(|| ScenarioFile {
            path: path.to_path_buf(),
            rel: path.display().to_string(),
            area: String::new(),
        });
        self.check(&file, meguri)
    }

    /// T0: every selected scenario with Meguri's executor, in parallel, in id
    /// order.
    pub fn run(&self, filters: &[String], meguri: &dyn Executor, lane: &str) -> Report {
        self.run_each(filters, Tier::T0, meguri.name(), lane, |file| self.check(file, meguri))
    }

    /// T1: every selected scenario on the oracle, and the live ones as chains.
    pub fn run_live(&self, filters: &[String], live: &LiveRun<'_>, lane: &str) -> Report {
        self.run_each(filters, Tier::T1, live.oracle.name(), lane, |file| {
            self.check_live(file, live)
        })
    }

    fn run_each(
        &self,
        filters: &[String],
        tier: Tier,
        executor: &str,
        lane: &str,
        check: impl Fn(&ScenarioFile) -> Outcome + Sync,
    ) -> Report {
        let watch = Stopwatch::start();
        let files = self.discover(filters).unwrap_or_default();
        let threads = std::thread::available_parallelism()
            .map_or(4, usize::from)
            .min(files.len().max(1));
        let next = AtomicUsize::new(0);
        let results: Vec<(usize, Outcome)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(file) = files.get(i) else { break };
                            done.push((i, check(file)));
                        }
                        done
                    })
                })
                .collect();
            // `check` catches panics, so a worker thread ends normally; were
            // one to unwind anyway, its scenarios get an error below.
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        let mut slots: Vec<Option<Outcome>> = vec![None; files.len()];
        for (i, outcome) in results {
            slots[i] = Some(outcome);
        }
        let outcomes = slots
            .into_iter()
            .zip(&files)
            .map(|(slot, file)| {
                slot.unwrap_or_else(|| {
                    Outcome::new(file, self.is_active(&file.rel))
                        .with(Status::Error, "the runner lost this scenario's result")
                })
            })
            .collect();
        Report {
            tier,
            executor: executor.to_owned(),
            lane: lane.to_owned(),
            oracle_pin: self.pin.clone(),
            outcomes,
            wall_ms: watch.elapsed_ms(),
            // F9: T0 on macOS counts the overlay entries it applied.
            overlays: tier == Tier::T0 && self.platform == Platform::MacOs,
        }
    }

    /// The freshness check (D10): goldens for every scenario, recorded for
    /// the current scenario and the pinned oracle; no orphaned goldens; area
    /// entries that match; and, with a base, the changed values.
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
            for pattern in &area.active {
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
        self.check_overlays(&mut fresh);
        if let Some(macos_lock) = &options.oracle_macos_lock_sha256
            && macos_lock.as_deref() != self.macos_lock_sha256.as_deref()
        {
            let message = match (macos_lock, &self.macos_lock_sha256) {
                (Some(lock), Some(pin)) => format!(
                    "the oracle's macOS lock (macos/requirements-macos.lock) hashes to {}, macos_lock_sha256 says {}: adopt the bump with its overlays",
                    short(lock),
                    short(pin)
                ),
                (Some(lock), None) => format!(
                    "the oracle's macOS lock (macos/requirements-macos.lock) exists (sha256 {}), but conformance/oracle.json has no macos_lock_sha256",
                    short(lock)
                ),
                (None, _) => "conformance/oracle.json has macos_lock_sha256, but the oracle has no macOS lock (macos/requirements-macos.lock)"
                    .to_owned(),
            };
            fresh.problems.push(problem(
                "pin_mismatch",
                "conformance/oracle.json",
                message,
                format!("tools/mg oracle bump {}", self.pin.anki_version),
            ));
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

    /// F7.4 on every overlay, on every platform: it parses and matches its
    /// schema, is fresh and not orphaned, and every entry resolves to its
    /// `linux` value, differs, appears once in pointer order and is in the
    /// class of F8.
    fn check_overlays(&self, fresh: &mut Freshness) {
        for rel in &self.overlays {
            let repro = format!("tools/mg oracle record --platform apple {rel}");
            let problem = |kind: &str, message: String| Problem {
                kind: kind.to_owned(),
                scenario: rel.clone(),
                message,
                repro: repro.clone(),
            };
            let checked = match self.read_overlay(rel) {
                Ok(checked) => checked,
                Err(message) => {
                    fresh.problems.push(problem("invalid_overlay", message));
                    continue;
                }
            };
            let (day_entries, memory_entries, _) = checked.counts();
            fresh.overlays.push(OverlaySummary {
                scenario: rel.clone(),
                entries: checked.entries.len(),
                day_entries,
                memory_entries,
            });
            let file = self.files.iter().find(|f| &f.rel == rel);
            let Some(file) = file.filter(|_| self.goldens.contains(rel)) else {
                fresh.problems.push(problem(
                    "orphan_overlay",
                    format!("{}/{rel}.json has no scenario or no Linux golden", overlay::DIR),
                ));
                continue;
            };
            // The scenario's own problems are reported with it; the overlay
            // of a scenario that does not load or whose golden is stale is
            // not checked further.
            let Ok((loaded, golden, outcome)) = self.prepare(file) else {
                continue;
            };
            match self.overlay_for(rel, &loaded, &golden, &outcome) {
                Ok(_) => {}
                Err(failed) if failed.status == Status::Stale => {
                    fresh.problems.push(problem("stale_overlay", failed.message));
                }
                Err(_) => {
                    let problems = overlay::check_entries(&checked, &golden.observations, &loaded.scenario);
                    let (outside, other): (Vec<_>, Vec<_>) = problems.into_iter().partition(|p| p.outside_class);
                    let list = |items: &[overlay::EntryProblem]| {
                        items
                            .iter()
                            .map(|p| format!("{}: {}", p.pointer, p.message))
                            .collect::<Vec<_>>()
                            .join("; ")
                    };
                    if !other.is_empty() {
                        fresh.problems.push(problem(
                            "overlay_entry",
                            format!("{} entries: {}", other.len(), list(&other)),
                        ));
                    }
                    if !outside.is_empty() {
                        let mut p = problem(
                            "outside_class",
                            format!(
                                "{} entr{} outside the fp-libm-boundary class (float-determinism.md F8): {}",
                                outside.len(),
                                if outside.len() == 1 { "y" } else { "ies" },
                                list(&outside)
                            ),
                        );
                        p.repro =
                            "escalate for a decision (float-determinism.md F7.2 step 7, F11); never edit an overlay"
                                .to_owned();
                        fresh.problems.push(p);
                    }
                }
            }
        }
    }

    fn changed_since(&self, base: &Path, fresh: &mut Freshness) -> Result<(), SuiteError> {
        let base_goldens: BTreeSet<String> = walk(base)?
            .iter()
            .map(|p| relative(base, p))
            .filter_map(|rel| rel.strip_suffix(".json").map(str::to_owned))
            .collect();
        let mut groups: BTreeMap<(String, String), Vec<ChangedValue>> = BTreeMap::new();
        for rel in &self.goldens {
            if !base_goldens.contains(rel) {
                fresh.added_goldens.push(rel.clone());
                continue;
            }
            let observations = |path: PathBuf| -> Value {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| observe::parse_json(&t).ok())
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
                groups.entry((area.clone(), kind)).or_default().push(ChangedValue {
                    golden: rel.clone(),
                    pointer: d.pointer,
                    before: d.expected,
                    after: d.actual,
                });
            }
        }
        fresh.removed_goldens = base_goldens.difference(&self.goldens).cloned().collect();
        fresh.changed = groups
            .into_iter()
            .map(|((area, kind), values)| ChangedGroup { area, kind, values })
            .collect();
        Ok(())
    }
}

/// Runs a check; a panic in an executor or adapter becomes this scenario's
/// `error` instead of unwinding the runner thread with every result it held.
fn guarded(file: &ScenarioFile, active: bool, check: impl FnOnce() -> Outcome) -> Outcome {
    catch_unwind(AssertUnwindSafe(check)).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic without a message".to_owned());
        Outcome::new(file, active).with(Status::Error, format!("panicked: {message}"))
    })
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
        if let Transform::AppleOverlay(on) = d.transform
            && (!on || d.id != FP_LIBM_BOUNDARY)
        {
            return Err(bad(&format!(
                "apple_overlay is true, and only in the entry {FP_LIBM_BOUNDARY} (dsl.md D13)"
            )));
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
