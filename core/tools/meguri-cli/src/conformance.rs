//! `meguri-cli conformance ...`: the agent front end of the conformance runner
//! (specs/conformance/dsl.md D8-D14). `tools/mg conformance`, `goldens-check`
//! and `harvest-report` call it.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use meguri_testkit::catalog::{self, Catalog, LiveReport};
use meguri_testkit::exec::sync_guard::check_endpoint;
use meguri_testkit::exec::{Executor, MeguriExecutor, OracleExecutor};
use meguri_testkit::observe;
use meguri_testkit::scenario::{self, ObservationKind, Setup, Step};
use meguri_testkit::suite::{GoldensCheck, LiveRun, Status, Suite};
use serde_json::json;

const USAGE: &str = "usage: meguri-cli conformance <command> [options]

  run [--live] [--lane NAME] [--report FILE] [--handoff-dir DIR] [filter...]
        T0: every selected scenario on Meguri's executor against its golden.
        --live is T1: oracle workers replay every selected scenario against its
        golden, and live scenarios run as Meguri/oracle chains through the
        handoff directory (the workers must mount it).
  exec [--oracle] <scenario file>       print a run's observations (golden layout)
  diff [--live] <scenario file>         Meguri's run against the golden (or the live oracle)
  capabilities                          what Meguri's executor supports, and the language
  goldens-check [--base DIR] [--oracle-lock-sha256 HEX]
                [--oracle-macos-lock-sha256 HEX | --oracle-macos-lock-absent] [--report FILE]
        the oracle's lock files where known (the macOS one: present with its
        hash, or absent), checked against conformance/oracle.json
  harvest-report [--catalog FILE] [--live-report FILE] [--report FILE] [--markdown FILE]
        --live-report: a run --live report at the current oracle pin; its
        statuses replace `live` and `pending` for the scenario texts it ran
  to-json [--observation-json] <file>...  parsed values, one JSON line per file
        default: scenario YAML or JSON, enforcing D2's integer range
        --observation-json: golden/overlay JSON, retaining exact i64/u64 values

common options:
  --conformance DIR   the conformance tree (default: the nearest conformance/ above the current directory)
  --oracle-cmd JSON   the worker argv JSON, {tz} for zone, {worker} for trusted identity
                      (default: $MEGURI_ORACLE_WORKER)
  --meguri-edge-cmd JSON  native T1 per-session edge helper, a JSON argv array
  --jobs N            T0 scenario workers (default host parallelism), or T1 oracle workers (default 12)

exit status: 0 pass, 1 failures, 2 usage or setup errors";

/// Why Meguri's executor alone does not run an actor scenario.
const ACTOR_SCENARIO: &str =
    "an actor scenario: only the parties executor runs it, in T1 (tools/mg conformance --live; sync-ops.md S10.3)";

/// Changed values goldens-check prints per area and kind; the report has all.
const SHOWN_CHANGES: usize = 20;

struct Options {
    conformance: Option<PathBuf>,
    oracle_cmd: Option<String>,
    meguri_edge_cmd: Option<Vec<String>>,
    jobs: Option<NonZeroUsize>,
    live: bool,
    lane: Option<String>,
    report: Option<PathBuf>,
    markdown: Option<PathBuf>,
    base: Option<PathBuf>,
    catalog: Option<PathBuf>,
    live_report: Option<PathBuf>,
    handoff_dir: Option<PathBuf>,
    lock: Option<String>,
    macos_lock: Option<Option<String>>,
    observation_json: bool,
    rest: Vec<String>,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        conformance: None,
        oracle_cmd: None,
        meguri_edge_cmd: None,
        jobs: None,
        live: false,
        lane: None,
        report: None,
        markdown: None,
        base: None,
        catalog: None,
        live_report: None,
        handoff_dir: None,
        lock: None,
        macos_lock: None,
        observation_json: false,
        rest: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--conformance" => o.conformance = Some(value(arg)?.into()),
            "--oracle-cmd" => o.oracle_cmd = Some(value(arg)?),
            "--meguri-edge-cmd" => {
                let command: Vec<String> = serde_json::from_str(&value(arg)?)
                    .map_err(|_| "--meguri-edge-cmd needs a JSON array of strings".to_owned())?;
                if command.is_empty() || command.iter().any(String::is_empty) {
                    return Err("--meguri-edge-cmd needs nonempty arguments".to_owned());
                }
                o.meguri_edge_cmd = Some(command);
            }
            "--jobs" => {
                o.jobs = Some(
                    value(arg)?
                        .parse()
                        .map_err(|_| "--jobs needs a positive number".to_owned())?,
                );
            }
            "--live" | "--oracle" => o.live = true,
            "--lane" => o.lane = Some(value(arg)?),
            "--report" => o.report = Some(value(arg)?.into()),
            "--markdown" => o.markdown = Some(value(arg)?.into()),
            "--base" => o.base = Some(value(arg)?.into()),
            "--catalog" => o.catalog = Some(value(arg)?.into()),
            "--live-report" => o.live_report = Some(value(arg)?.into()),
            "--handoff-dir" => o.handoff_dir = Some(value(arg)?.into()),
            "--oracle-lock-sha256" => o.lock = Some(value(arg)?),
            "--oracle-macos-lock-sha256" => o.macos_lock = Some(Some(value(arg)?)),
            "--oracle-macos-lock-absent" => o.macos_lock = Some(None),
            "--observation-json" => o.observation_json = true,
            "--" => o.rest.extend(it.by_ref().cloned()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            other => o.rest.push(other.to_owned()),
        }
    }
    Ok(o)
}

/// Runs a conformance command; returns the exit status.
pub fn main(args: &[String]) -> i32 {
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let options = match parse_options(rest) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("meguri-cli conformance: {e}\n\n{USAGE}");
            return 2;
        }
    };
    if options.observation_json && command != "to-json" {
        eprintln!("meguri-cli conformance: --observation-json is only for to-json");
        return 2;
    }
    let result = match command.as_str() {
        "run" => run(&options),
        "exec" => exec(&options),
        "diff" => diff(&options),
        "capabilities" => capabilities(),
        "goldens-check" => goldens_check(&options),
        "harvest-report" => harvest_report(&options),
        "to-json" => to_json(&options),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(0)
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    result.unwrap_or_else(|e| {
        eprintln!("meguri-cli conformance {command}: {e}");
        2
    })
}

fn conformance_dir(options: &Options) -> Result<PathBuf, String> {
    if let Some(dir) = &options.conformance {
        return Ok(dir.clone());
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    cwd.ancestors()
        .map(|d| d.join("conformance"))
        .find(|d| d.join("oracle.json").is_file())
        .ok_or_else(|| "no conformance/oracle.json above the current directory; pass --conformance".to_owned())
}

fn open_suite(options: &Options) -> Result<Suite, String> {
    Suite::open(&conformance_dir(options)?).map_err(|e| e.to_string())
}

/// The toxiproxy API the oracle workers get for actor scenarios
/// (`ORACLE_SYNC_FAULTS`, which `tools/mg` also puts in the worker command),
/// checked before any worker starts, since a worker connects to it when a
/// session begins (sync-ops.md S8.3, R1876). Empty means none.
fn check_sync_faults() -> Result<(), String> {
    match std::env::var("ORACLE_SYNC_FAULTS") {
        Ok(api) if !api.is_empty() => check_endpoint(&api)
            .map(|_| ())
            .map_err(|refused| format!("ORACLE_SYNC_FAULTS: {refused}")),
        _ => Ok(()),
    }
}

fn oracle(options: &Options) -> Result<OracleExecutor, String> {
    check_sync_faults()?;
    let text = options
        .oracle_cmd
        .clone()
        .or_else(|| std::env::var("MEGURI_ORACLE_WORKER").ok())
        .ok_or("the oracle needs --oracle-cmd or MEGURI_ORACLE_WORKER (tools/mg conformance --live sets it)")?;
    let command: Vec<String> =
        serde_json::from_str(&text).map_err(|e| format!("the worker command must be a JSON array of strings: {e}"))?;
    if command.is_empty() {
        return Err("the worker command is empty".to_owned());
    }
    Ok(OracleExecutor::new(command, options.jobs.map_or(12, NonZeroUsize::get)))
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn run(options: &Options) -> Result<i32, String> {
    let suite = open_suite(options)?;
    let lane = options
        .lane
        .clone()
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH));
    let meguri = MeguriExecutor::fixture_harness();
    let (report, hello) = if options.live {
        let oracle = oracle(options)?.expecting(suite.pin.clone());
        let live = LiveRun {
            oracle: &oracle,
            meguri: &meguri,
            handoff_root: options.handoff_dir.as_deref(),
            meguri_edge_cmd: options.meguri_edge_cmd.as_deref(),
        };
        let report = suite.run_live(&options.rest, &live, &lane);
        (report, oracle.hello())
    } else {
        let report = match options.jobs {
            Some(jobs) => suite.run_with_jobs(&options.rest, &meguri, &lane, jobs),
            None => suite.run(&options.rest, &meguri, &lane),
        };
        (report, None)
    };
    eprint!("{}", report.render_text());
    if let Some(path) = &options.report {
        let mut json = report.to_json();
        if let Some(hello) = hello {
            json["oracle"] = json!({
                "anki_version": hello.anki_version,
                "lock_sha256": hello.lock_sha256,
                "driver_sha256": hello.driver_sha256,
            });
        }
        write_file(
            path,
            &format!("{}\n", serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?),
        )?;
    }
    if report.outcomes.is_empty() && !options.rest.is_empty() {
        eprintln!("no scenario matched {}", options.rest.join(" "));
        return Ok(1);
    }
    Ok(i32::from(!report.failures().is_empty()))
}

fn single_scenario(options: &Options) -> Result<PathBuf, String> {
    match options.rest.as_slice() {
        [path] => Ok(PathBuf::from(path)),
        _ => Err("give one scenario file".to_owned()),
    }
}

fn exec(options: &Options) -> Result<i32, String> {
    let path = single_scenario(options)?;
    let suite = open_suite(options)?;
    if suite.reference_at_path(&path).is_some() && !options.live {
        let outcome = suite.check_path(&path, &MeguriExecutor::fixture_harness());
        eprint!("{}", outcome.render_text());
        return Ok(i32::from(outcome.status != Status::Reference));
    }
    let loaded = scenario::load(&path).map_err(|e| e.to_string())?;
    if loaded.scenario.has_actors() && !options.live {
        eprintln!("{}: {}", loaded.scenario.id, ACTOR_SCENARIO);
        return Ok(1);
    }
    let result = if options.live {
        oracle(options)?.run(&loaded)
    } else {
        let executor = MeguriExecutor::fixture_harness();
        let missing = executor.missing(&loaded.scenario.needs());
        if !missing.is_empty() {
            eprintln!("{}: pending (needs {})", loaded.scenario.id, missing.summary());
            return Ok(1);
        }
        executor.run(&loaded)
    };
    match result {
        Ok(observations) => {
            print!("{}", observe::golden_text(&observations));
            Ok(0)
        }
        Err(e) => {
            eprintln!("{}: {e}", loaded.scenario.id);
            Ok(1)
        }
    }
}

fn diff(options: &Options) -> Result<i32, String> {
    let path = single_scenario(options)?;
    let suite = open_suite(options)?;
    let meguri = MeguriExecutor::fixture_harness();
    if !options.live {
        let outcome = suite.check_path(&path, &meguri);
        print!("{}", outcome.render_text());
        return Ok(i32::from(!matches!(outcome.status, Status::Pass | Status::Reference)));
    }
    if suite.reference_at_path(&path).is_some() {
        let oracle = oracle(options)?.expecting(suite.pin.clone());
        let live = LiveRun {
            oracle: &oracle,
            meguri: &meguri,
            handoff_root: options.handoff_dir.as_deref(),
            meguri_edge_cmd: options.meguri_edge_cmd.as_deref(),
        };
        let file = suite
            .discover(&[])
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|file| file.path.canonicalize().ok() == path.canonicalize().ok())
            .ok_or("reference scenario is outside the suite")?;
        let outcome = suite.check_live(&file, &live);
        print!("{}", outcome.render_text());
        return Ok(i32::from(outcome.status != Status::Reference));
    }
    // Meguri against the live oracle, no golden involved.
    let loaded = scenario::load(&path).map_err(|e| e.to_string())?;
    if loaded.scenario.has_actors() {
        println!("{}: {}", loaded.scenario.id, ACTOR_SCENARIO);
        return Ok(1);
    }
    let missing = meguri.missing(&loaded.scenario.needs());
    if !missing.is_empty() {
        println!("{}: pending (needs {})", loaded.scenario.id, missing.summary());
        return Ok(1);
    }
    let expected = oracle(options)?
        .expecting(suite.pin.clone())
        .run(&loaded)
        .map_err(|e| format!("oracle: {e}"))?;
    let actual = meguri.run(&loaded).map_err(|e| format!("meguri: {e}"))?;
    let compare = meguri_testkit::diff::CompareOptions {
        set: &loaded.scenario.compare.set,
        deviations: &suite.deviations,
        overlay: &[],
    };
    let comparison = meguri_testkit::diff::compare(&loaded.scenario.id, &expected, &actual, &compare);
    if comparison.differences.is_empty() {
        println!("{}: Meguri matches the live oracle", loaded.scenario.id);
        return Ok(0);
    }
    let step = meguri_testkit::diff::first_divergence(&comparison.differences);
    println!(
        "{}: {} differences from the live oracle, first at step {}",
        loaded.scenario.id,
        comparison.differences.len(),
        step.map_or_else(|| "?".to_owned(), |s| s.to_string())
    );
    for d in comparison.differences.iter().take(20) {
        println!(
            "  {}: expected {}, actual {}",
            d.pointer,
            meguri_testkit::diff::shown(&d.expected),
            meguri_testkit::diff::shown(&d.actual)
        );
    }
    Ok(1)
}

fn capabilities() -> Result<i32, String> {
    let executor = MeguriExecutor::new();
    let out = json!({
        "executor": executor.name(),
        "supports": executor.capabilities(),
        "language": {
            "setup": Setup::ORDER,
            "ops": Step::NAMES.iter().filter(|n| **n != "observe").copied().chain(std::iter::once("answer.filtered")).collect::<Vec<_>>(),
            "observations": ObservationKind::ALL.iter().map(|k| k.name()).collect::<Vec<_>>(),
        },
    });
    println!("{}", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?);
    Ok(0)
}

fn goldens_check(options: &Options) -> Result<i32, String> {
    let suite = open_suite(options)?;
    let check = GoldensCheck {
        base: options.base.clone(),
        oracle_lock_sha256: options.lock.clone(),
        oracle_macos_lock_sha256: options.macos_lock.clone(),
    };
    let fresh = suite.goldens_check(&check).map_err(|e| e.to_string())?;
    for p in &fresh.problems {
        eprintln!("{} {}: {}\n  fix: {}", p.kind, p.scenario, p.message, p.repro);
    }
    if !fresh.changed.is_empty() {
        eprintln!("changed golden values since the base, by area and observation kind:");
        for group in &fresh.changed {
            eprintln!("  {} {}: {}", group.area, group.kind, group.values.len());
            for v in group.values.iter().take(SHOWN_CHANGES) {
                eprintln!(
                    "    {} {}: {} -> {}",
                    v.golden,
                    v.pointer,
                    meguri_testkit::diff::shown(&v.before),
                    meguri_testkit::diff::shown(&v.after)
                );
            }
            if group.values.len() > SHOWN_CHANGES {
                eprintln!(
                    "    (+{} more; all are in the report)",
                    group.values.len() - SHOWN_CHANGES
                );
            }
        }
    }
    if !fresh.overlays.is_empty() {
        let sum = |f: fn(&meguri_testkit::suite::OverlaySummary) -> usize| fresh.overlays.iter().map(f).sum::<usize>();
        eprintln!(
            "{} overlays: {} entries ({} day, {} memory)",
            fresh.overlays.len(),
            sum(|o| o.entries),
            sum(|o| o.day_entries),
            sum(|o| o.memory_entries)
        );
    }
    eprintln!(
        "{} scenarios: {} problem{}; {} goldens changed, {} added, {} removed since the base",
        fresh.scenarios,
        fresh.problems.len(),
        if fresh.problems.len() == 1 { "" } else { "s" },
        fresh.changed_goldens.len(),
        fresh.added_goldens.len(),
        fresh.removed_goldens.len()
    );
    if let Some(path) = &options.report {
        let json = json!({
            "scenarios": fresh.scenarios,
            "ok": fresh.ok(),
            "problems": fresh.problems.iter().map(|p| json!({
                "kind": p.kind, "scenario": p.scenario, "message": p.message, "repro": p.repro,
            })).collect::<Vec<_>>(),
            "changed": fresh.changed.iter().map(|c| json!({
                "area": c.area,
                "kind": c.kind,
                "count": c.values.len(),
                "values": c.values.iter().map(|v| json!({
                    "golden": v.golden,
                    "pointer": v.pointer,
                    "before": meguri_testkit::diff::shown(&v.before),
                    "after": meguri_testkit::diff::shown(&v.after),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "overlays": fresh.overlays.iter().map(|o| json!({
                "scenario": o.scenario,
                "entries": o.entries,
                "day_entries": o.day_entries,
                "memory_entries": o.memory_entries,
            })).collect::<Vec<_>>(),
            "changed_goldens": fresh.changed_goldens,
            "added_goldens": fresh.added_goldens,
            "removed_goldens": fresh.removed_goldens,
        });
        write_file(
            path,
            &format!("{}\n", serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?),
        )?;
    }
    Ok(i32::from(!fresh.ok()))
}

fn harvest_report(options: &Options) -> Result<i32, String> {
    let suite = open_suite(options)?;
    let catalog_path = options
        .catalog
        .clone()
        .unwrap_or_else(|| suite.root().join("catalog/harvest.csv"));
    let catalog = Catalog::read(&catalog_path).map_err(|e| format!("{}: {e}", catalog_path.display()))?;
    let live = match &options.live_report {
        Some(path) => LiveReport::read(path, &suite.pin).map_err(|e| e.to_string())?,
        None => LiveReport::default(),
    };
    let report = catalog::harvest_report(&catalog, &suite, &MeguriExecutor::new(), &live);
    for p in &report.problems {
        eprintln!("problem: {p}");
    }
    for (class, summary) in &report.by_classification {
        let statuses: Vec<String> = summary.statuses.iter().map(|(s, n)| format!("{s} {n}")).collect();
        eprintln!("{class}: {} rows ({})", summary.rows, statuses.join(", "));
    }
    if let Some(path) = &options.report {
        write_file(
            path,
            &format!(
                "{}\n",
                serde_json::to_string_pretty(&report.to_json()).map_err(|e| e.to_string())?
            ),
        )?;
    }
    if let Some(path) = &options.markdown {
        write_file(path, &report.render_markdown())?;
    }
    Ok(i32::from(!report.problems.is_empty()))
}

fn to_json(options: &Options) -> Result<i32, String> {
    let mut status = 0;
    for path in &options.rest {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                if options.observation_json {
                    if !path.ends_with(".json") {
                        return Err("--observation-json requires a .json file".to_owned());
                    }
                    scenario::parse_observation_json(&text)
                } else if path.ends_with(".json") {
                    scenario::parse_json(&text)
                } else {
                    scenario::parse_yaml(&text)
                }
                .map_err(|e| e.to_string())
            });
        match parsed {
            Ok(value) => println!("{}", json!({"path": path, "value": value})),
            Err(e) => {
                println!("{}", json!({"path": path, "error": e}));
                status = 1;
            }
        }
    }
    Ok(status)
}

#[cfg(test)]
mod edge_option_tests {
    use super::parse_options;

    #[test]
    fn jobs_are_an_explicit_positive_limit_with_no_standalone_override() {
        assert!(parse_options(&[]).unwrap().jobs.is_none());
        for jobs in [1, 2, 12] {
            let options = parse_options(&["--jobs".into(), jobs.to_string()]).unwrap();
            assert_eq!(options.jobs.unwrap().get(), jobs);
        }
        for value in ["0", "-1", "two"] {
            assert!(parse_options(&["--jobs".into(), value.into()]).is_err());
        }
    }

    #[test]
    fn edge_command_is_a_literal_argv_capability() {
        let options = parse_options(&[
            "--live".into(),
            "--meguri-edge-cmd".into(),
            r#"["helper","$(touch never)","a b","{tz}"]"#.into(),
        ])
        .unwrap();
        assert_eq!(
            options.meguri_edge_cmd.unwrap(),
            ["helper", "$(touch never)", "a b", "{tz}"]
        );
        for value in ["[]", "[1]", "[\"\"]", "[\"helper\",\"\"]", "{}", "not-json"] {
            assert!(parse_options(&["--meguri-edge-cmd".into(), value.into()]).is_err());
        }
    }
}
