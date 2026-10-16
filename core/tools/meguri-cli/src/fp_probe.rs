//! `meguri-cli fp-probe ...`: the floating-point determinism probe
//! (specs/conformance/float-determinism.md F10). `eval` computes Meguri's
//! side on this platform with the conformance runner's own `fsrs_next` and
//! fuzz-table code (F5.5); `report` compares the four sides. `tools/mg
//! fp-probe` runs `eval` natively on macOS and in the Linux dev container,
//! then `report`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::sched::{self, FsrsModels};
use meguri_testkit::observe::{self, FSRS_NEXT_COLUMN_NAMES, FSRS_NEXT_COLUMNS, overlay};
use meguri_testkit::scenario::{
    self, FsrsNextInput, FsrsNextRequest, LogicalRef, ObservationArgs, ObservationKind, RefKind, Scenario, Step,
};
use meguri_testkit::suite::{OracleSides, SidesError, Suite};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const USAGE: &str = "usage: meguri-cli fp-probe <command> [options]

  eval --out FILE
        Meguri's side on this platform: sched/fsrs-transitions F3.2 on every
        line of corpora/fp-probe/inputs.jsonl with param-sets.json, and
        sched/fuzz Z6 on the request of scenarios/fp-probe/corpus/fuzz-table.
  report --meguri-macos FILE --meguri-linux FILE [--commit SHA]
         [--unavailable SIDE=REASON]... --out FILE
        The probe report (float-determinism.md F10.3) from the goldens, the
        overlays and the two eval files; SIDE is linux_oracle, apple_oracle,
        meguri_macos or meguri_linux.

common options:
  --conformance DIR   the conformance tree (default: the nearest conformance/ above the current directory)

exit status: 0 pass, 1 the probe fails, 2 usage or setup errors";

/// The corpus scenarios (F2.4) and the fuzz table (F4).
const CORPUS_SCENARIOS: usize = 100;
const INPUTS_PER_SCENARIO: usize = 1000;
const FUZZ_SCENARIO: &str = "fp-probe/corpus/fuzz-table";
const CORPUS: &str = "corpora/fp-probe/inputs.jsonl";
const PARAM_SETS: &str = "corpora/fp-probe/param-sets.json";
/// The text summary's differences per pair (F10.3).
const SHOWN: usize = 20;

const SIDES: [&str; 4] = ["linux_oracle", "apple_oracle", "meguri_macos", "meguri_linux"];
/// F10.2: (left, right, gate).
const PAIRS: [(&str, &str, bool); 3] = [
    ("linux_oracle", "apple_oracle", false),
    ("apple_oracle", "meguri_macos", true),
    ("linux_oracle", "meguri_linux", true),
];

fn corpus_scenario(n: usize) -> String {
    format!("fp-probe/corpus/next-states-{n:03}")
}

#[derive(Default)]
struct Options {
    conformance: Option<PathBuf>,
    out: Option<PathBuf>,
    meguri_macos: Option<PathBuf>,
    meguri_linux: Option<PathBuf>,
    commit: Option<String>,
    unavailable: BTreeMap<String, String>,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--conformance" => o.conformance = Some(value(arg)?.into()),
            "--out" => o.out = Some(value(arg)?.into()),
            "--meguri-macos" => o.meguri_macos = Some(value(arg)?.into()),
            "--meguri-linux" => o.meguri_linux = Some(value(arg)?.into()),
            "--commit" => o.commit = Some(value(arg)?),
            "--unavailable" => {
                let given = value(arg)?;
                let (side, reason) = given
                    .split_once('=')
                    .filter(|(side, _)| SIDES.contains(side))
                    .ok_or_else(|| format!("--unavailable takes SIDE=REASON with SIDE one of {}", SIDES.join(", ")))?;
                o.unavailable.insert(side.to_owned(), reason.to_owned());
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    Ok(o)
}

/// Runs an fp-probe command; returns the exit status.
pub fn main(args: &[String]) -> i32 {
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let options = match parse_options(rest) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("meguri-cli fp-probe: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let result = match command.as_str() {
        "eval" => eval(&options),
        "report" => report(&options),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(0)
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    result.unwrap_or_else(|e| {
        eprintln!("meguri-cli fp-probe {command}: {e}");
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

fn out_path(options: &Options) -> Result<&Path, String> {
    options.out.as_deref().ok_or_else(|| "--out is required".to_owned())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    observe::parse_json(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The platform name of protocol.json's hello for where this binary runs.
fn platform() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// The corpus: its digest and its inputs, in line order (F2.1).
struct Corpus {
    sha256: String,
    inputs: Vec<FsrsNextInput>,
}

fn read_corpus(root: &Path) -> Result<Corpus, String> {
    let path = root.join(CORPUS);
    let bytes = std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut inputs = Vec::new();
    // JSON lines end in LF; split on it only.
    for (k, line) in text.split('\n').enumerate().filter(|(_, l)| !l.is_empty()) {
        let value = observe::parse_json(line).map_err(|e| format!("{CORPUS} line {k}: {e}"))?;
        let input = FsrsNextInput::from_value(&value).map_err(|e| format!("{CORPUS} line {k}: {e}"))?;
        if input.id != k as u64 {
            return Err(format!("{CORPUS} line {k} has id {}", input.id));
        }
        inputs.push(input);
    }
    Ok(Corpus {
        sha256: sha256_hex(&bytes),
        inputs,
    })
}

/// The fuzz table's request and the maximum interval of its card's home
/// preset.
struct FuzzRequest {
    pairs: Vec<(u64, u64)>,
    intervals: Vec<u32>,
    maximum: u32,
}

/// The `fuzz_table` request of the fuzz-table scenario, which observes it
/// right after setup. The card's home preset is read from that setup: `#c1`
/// is the first card of the first note, in that note's deck, whose preset
/// must set `max_interval_days` (F4: 36,500).
fn fuzz_request(scenario: &Scenario) -> Result<FuzzRequest, String> {
    let bad = |why: &str| format!("{FUZZ_SCENARIO}: {why}");
    let request = match scenario.steps.first() {
        Some(Step::Observe(requests)) => requests
            .iter()
            .find(|r| r.kind == ObservationKind::FuzzTable)
            .ok_or_else(|| bad("its first step does not observe fuzz_table"))?,
        _ => return Err(bad("its first step is not an observe step")),
    };
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(bad("fuzz_table has no arguments"));
    };
    let card: LogicalRef = map["card"]
        .as_str()
        .unwrap_or_default()
        .parse()
        .map_err(|e| bad(&format!("{e}")))?;
    if card.kind != RefKind::Card || card.n != 1 {
        return Err(bad("the table's card is not #c1"));
    }
    let setup = &scenario.setup;
    let deck = setup
        .notes
        .as_ref()
        .and_then(|notes| notes.first())
        .and_then(|note| note.deck.as_deref())
        .ok_or_else(|| bad("the first note names no deck"))?;
    let preset = setup
        .decks
        .iter()
        .flatten()
        .find(|d| d.name() == deck)
        .and_then(|d| d.preset())
        .ok_or_else(|| bad(&format!("deck {deck} names no preset")))?;
    let maximum = setup
        .presets
        .iter()
        .flatten()
        .find(|p| p.name == preset)
        .and_then(|p| p.max_interval_days)
        .and_then(|m| u32::try_from(m).ok())
        .ok_or_else(|| bad(&format!("preset {preset} sets no max_interval_days")))?;
    let pairs = map["pairs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| Some((p.get(0)?.as_u64()?, p.get(1)?.as_u64()?)))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| bad("pairs are [card_id, reps]"))?;
    let intervals = map["intervals"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| i.as_u64().and_then(|i| u32::try_from(i).ok()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| bad("intervals are whole days"))?;
    Ok(FuzzRequest {
        pairs,
        intervals,
        maximum,
    })
}

fn load_scenario(root: &Path, rel: &str) -> Result<Scenario, String> {
    let base = root.join("scenarios").join(rel);
    let path = scenario::SUFFIXES
        .iter()
        .map(|suffix| PathBuf::from(format!("{}{suffix}", base.display())))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no scenario file for {rel}"))?;
    scenario::load(&path)
        .map(|loaded| loaded.scenario)
        .map_err(|e| format!("{rel}: {e}"))
}

/// `eval`: Meguri's rows for the whole corpus and its fuzz table.
fn eval(options: &Options) -> Result<i32, String> {
    let root = conformance_dir(options)?;
    let out = out_path(options)?;
    let corpus = read_corpus(&root)?;
    let param_sets =
        FsrsNextRequest::param_sets(&read_json(&root.join(PARAM_SETS))?).map_err(|e| format!("{PARAM_SETS}: {e}"))?;
    let models = FsrsModels::new(&param_sets).map_err(|e| e.to_string())?;
    let rows = corpus
        .inputs
        .iter()
        .map(|input| models.evaluate(input).and_then(|answers| sched::row(&answers)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let fuzz = fuzz_request(&load_scenario(&root, FUZZ_SCENARIO)?)?;
    let fuzz_rows = sched::fuzz_table_rows(&fuzz.pairs, &fuzz.intervals, fuzz.maximum);
    let value = json!({
        "platform": platform(),
        "corpus": {"sha256": corpus.sha256, "inputs": corpus.inputs.len()},
        "rows": rows,
        "fuzz": {"scenario": FUZZ_SCENARIO, "intervals": fuzz.intervals, "rows": fuzz_rows},
    });
    write_json(out, &value)?;
    eprintln!(
        "fp-probe eval ({}): {} inputs, {} fuzz pairs x {} intervals -> {}",
        platform(),
        corpus.inputs.len(),
        fuzz.pairs.len(),
        fuzz.intervals.len(),
        out.display()
    );
    Ok(0)
}

/// One side of the probe: rows by input id and the fuzz table's rows.
struct Side {
    rows: Vec<Value>,
    fuzz: Vec<Value>,
    info: Map<String, Value>,
}

/// The oracle sides from the goldens and overlays, and the overlay entries
/// of the corpus scenarios by (input id, column) with whether each is in the
/// class (F10.3 `in_class`).
struct OracleData {
    linux: Side,
    apple: Side,
    in_class: BTreeMap<(usize, usize), bool>,
}

fn fsrs_rows(observations: &Value, rel: &str) -> Result<Vec<Value>, String> {
    observations
        .pointer("/steps/0/fsrs_next/rows")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format!("{rel}: the golden has no /steps/0/fsrs_next/rows"))
}

fn fuzz_rows(observations: &Value) -> Result<Vec<Value>, String> {
    observations
        .pointer("/steps/0/fuzz_table/rows")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format!("{FUZZ_SCENARIO}: the golden has no /steps/0/fuzz_table/rows"))
}

/// Whether the corpus scenario holds the corpus inputs `base..` in order
/// (F2.4).
fn holds_inputs(sides: &OracleSides, corpus: &Corpus, base: usize) -> bool {
    let inputs = sides
        .scenario
        .value
        .pointer("/steps/0/observe/0/fsrs_next/inputs")
        .and_then(Value::as_array);
    inputs.is_some_and(|inputs| {
        inputs.len() == INPUTS_PER_SCENARIO
            && inputs.iter().enumerate().all(|(i, value)| {
                FsrsNextInput::from_value(value).is_ok_and(|input| corpus.inputs.get(base + i) == Some(&input))
            })
    })
}

/// The scenario's oracle sides; a problem goes to the side it concerns.
fn sides_of(
    suite: &Suite,
    rel: &str,
    linux_problems: &mut Vec<String>,
    apple_problems: &mut Vec<String>,
) -> Option<OracleSides> {
    match suite.oracle_sides(rel) {
        Ok(sides) => Some(sides),
        Err(SidesError::Overlay(e)) => {
            apple_problems.push(e);
            None
        }
        Err(SidesError::Golden(e)) => {
            linux_problems.push(e);
            None
        }
    }
}

fn oracle_data(suite: &Suite, corpus: &Corpus) -> Result<OracleData, (Option<String>, Option<String>)> {
    let mut linux_rows = Vec::with_capacity(CORPUS_SCENARIOS * INPUTS_PER_SCENARIO);
    let mut apple_rows = Vec::with_capacity(CORPUS_SCENARIOS * INPUTS_PER_SCENARIO);
    let mut in_class = BTreeMap::new();
    let (mut linux_problems, mut apple_problems) = (Vec::new(), Vec::new());
    let (mut linux_locks, mut apple_locks) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    let mut overlays = 0;
    for n in 0..CORPUS_SCENARIOS {
        let rel = corpus_scenario(n);
        let Some(sides) = sides_of(suite, &rel, &mut linux_problems, &mut apple_problems) else {
            continue;
        };
        let base = n * INPUTS_PER_SCENARIO;
        if !holds_inputs(&sides, corpus, base) {
            linux_problems.push(format!(
                "{rel} does not hold corpus inputs {base} to {} in order (regenerate the corpus and its scenarios together)",
                base + INPUTS_PER_SCENARIO - 1
            ));
            continue;
        }
        match (fsrs_rows(&sides.linux, &rel), fsrs_rows(&sides.apple, &rel)) {
            (Ok(l), Ok(a)) if l.len() == INPUTS_PER_SCENARIO && a.len() == INPUTS_PER_SCENARIO => {
                linux_rows.extend(l);
                apple_rows.extend(a);
            }
            _ => {
                linux_problems.push(format!("{rel}: the golden does not hold {INPUTS_PER_SCENARIO} rows"));
                continue;
            }
        }
        linux_locks.insert(sides.linux_lock_sha256.clone());
        if let Some(o) = &sides.overlay {
            overlays += 1;
            apple_locks.insert(o.oracle.lock_sha256.clone());
            let problems = overlay::check_entries(o, &sides.linux, &sides.scenario.scenario);
            for entry in &o.entries {
                let segments = meguri_testkit::diff::pointer::parse(&entry.pointer);
                if let [_, _, _, _, row, column] = segments.as_slice()
                    && let (Ok(row), Some(column)) = (row.parse::<usize>(), observe::fsrs_next_column(column))
                {
                    in_class.insert(
                        (base + row, column),
                        !problems.iter().any(|p| p.pointer == entry.pointer),
                    );
                }
            }
        }
    }
    let (mut linux_fuzz, mut apple_fuzz) = (Vec::new(), Vec::new());
    if let Some(sides) = sides_of(suite, FUZZ_SCENARIO, &mut linux_problems, &mut apple_problems) {
        match (fuzz_rows(&sides.linux), fuzz_rows(&sides.apple)) {
            (Ok(l), Ok(a)) => {
                linux_fuzz = l;
                apple_fuzz = a;
            }
            (Err(e), _) | (_, Err(e)) => linux_problems.push(e),
        }
        linux_locks.insert(sides.linux_lock_sha256.clone());
        if let Some(o) = &sides.overlay {
            overlays += 1;
            apple_locks.insert(o.oracle.lock_sha256.clone());
        }
    }
    let joined = |items: &[String]| (!items.is_empty()).then(|| items.join("; "));
    let (linux_error, apple_error) = (joined(&linux_problems), joined(&apple_problems));
    if linux_error.is_some() || apple_error.is_some() {
        // Without the Linux goldens neither oracle side exists.
        let apple_error = apple_error.or_else(|| {
            linux_error
                .as_ref()
                .map(|_| "the Linux goldens are not usable".to_owned())
        });
        return Err((linux_error, apple_error));
    }
    let lock = |locks: &std::collections::BTreeSet<String>, fallback: Option<&str>| match locks.len() {
        0 => json!(fallback),
        1 => json!(locks.iter().next()),
        _ => json!(locks.iter().collect::<Vec<_>>()),
    };
    let mut linux_info = Map::new();
    linux_info.insert("lock_sha256".to_owned(), lock(&linux_locks, None));
    let mut apple_info = Map::new();
    apple_info.insert(
        "lock_sha256".to_owned(),
        lock(&apple_locks, suite.macos_lock_sha256.as_deref()),
    );
    apple_info.insert("overlays".to_owned(), json!(overlays));
    Ok(OracleData {
        linux: Side {
            rows: linux_rows,
            fuzz: linux_fuzz,
            info: linux_info,
        },
        apple: Side {
            rows: apple_rows,
            fuzz: apple_fuzz,
            info: apple_info,
        },
        in_class,
    })
}

/// A Meguri side from its eval file; it must be for the current corpus.
fn meguri_side(path: Option<&Path>, corpus: &Corpus, commit: Option<&str>) -> Result<Side, String> {
    let path = path.ok_or("no eval file given")?;
    let value = read_json(path)?;
    let sha = value
        .pointer("/corpus/sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if sha != corpus.sha256 {
        return Err(format!(
            "{} evaluated another corpus (sha256 {sha}, the corpus is {})",
            path.display(),
            corpus.sha256
        ));
    }
    let rows = value["rows"].as_array().cloned().unwrap_or_default();
    if rows.len() != corpus.inputs.len() {
        return Err(format!(
            "{} holds {} rows for {} inputs",
            path.display(),
            rows.len(),
            corpus.inputs.len()
        ));
    }
    let mut info = Map::new();
    info.insert("commit".to_owned(), json!(commit));
    info.insert("platform".to_owned(), value["platform"].clone());
    Ok(Side {
        rows,
        fuzz: value
            .pointer("/fuzz/rows")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        info,
    })
}

/// Exact equality (F10.1): integers equal, `f32` values equal as `f32`.
fn same(column: usize, a: &Value, b: &Value) -> bool {
    if column.is_multiple_of(3) {
        return a.as_u64().is_some() && a.as_u64() == b.as_u64();
    }
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x as f32).to_bits() == (y as f32).to_bits(),
        _ => false,
    }
}

fn compare_pair(
    left: (&str, &Side),
    right: (&str, &Side),
    gate: bool,
    inputs: usize,
    in_class: Option<&BTreeMap<(usize, usize), bool>>,
) -> Value {
    let mut differences = Vec::new();
    let (mut days, mut memory, mut differing_inputs) = (0usize, 0usize, 0usize);
    for id in 0..inputs {
        let (l, r) = (&left.1.rows[id], &right.1.rows[id]);
        let mut differs = false;
        for column in 0..FSRS_NEXT_COLUMNS {
            let (a, b) = (&l[column], &r[column]);
            if same(column, a, b) {
                continue;
            }
            differs = true;
            if column.is_multiple_of(3) {
                days += 1;
            } else {
                memory += 1;
            }
            let mut d = json!({
                "id": id,
                "column": column,
                "name": FSRS_NEXT_COLUMN_NAMES[column],
                "left": a,
                "right": b,
            });
            if let Some(classes) = in_class {
                d["in_class"] = json!(classes.get(&(id, column)).copied().unwrap_or(false));
            }
            differences.push(d);
        }
        differing_inputs += usize::from(differs);
    }
    json!({
        "left": left.0,
        "right": right.0,
        "gate": gate,
        "inputs": inputs,
        "differing_inputs": differing_inputs,
        "differing_days": days,
        "differing_memory": memory,
        "days_rate": if inputs == 0 { 0.0 } else { days as f64 / (4 * inputs) as f64 },
        "differences": differences,
    })
}

/// `report`: the four sides, the three pairs and the fuzz table (F10).
fn report(options: &Options) -> Result<i32, String> {
    let root = conformance_dir(options)?;
    let out = out_path(options)?;
    let suite = Suite::open(&root).map_err(|e| e.to_string())?;
    let corpus = read_corpus(&root)?;
    let mut errors: BTreeMap<String, String> = options.unavailable.clone();
    let mut sides: BTreeMap<&str, Side> = BTreeMap::new();
    let mut in_class = BTreeMap::new();
    match oracle_data(&suite, &corpus) {
        Ok(data) => {
            sides.insert("linux_oracle", data.linux);
            sides.insert("apple_oracle", data.apple);
            in_class = data.in_class;
        }
        Err((linux, apple)) => {
            for (side, e) in [("linux_oracle", linux), ("apple_oracle", apple)] {
                if let Some(e) = e {
                    errors.entry(side.to_owned()).or_insert(e);
                }
            }
        }
    }
    for (name, path) in [
        ("meguri_macos", &options.meguri_macos),
        ("meguri_linux", &options.meguri_linux),
    ] {
        match meguri_side(path.as_deref(), &corpus, options.commit.as_deref()) {
            Ok(side) => {
                sides.insert(name, side);
            }
            Err(e) => {
                errors.entry(name.to_owned()).or_insert(e);
            }
        }
    }
    // A side that could not be produced takes no part in any comparison.
    for side in errors.keys() {
        sides.remove(side.as_str());
    }
    let inputs = corpus.inputs.len();
    for (name, side) in &sides {
        if side.rows.len() != inputs {
            errors.insert(
                (*name).to_owned(),
                format!("{} rows for {inputs} inputs", side.rows.len()),
            );
        }
    }
    sides.retain(|name, _| !errors.contains_key(*name));

    let mut status_fail = !errors.is_empty();
    let mut pairs = Vec::new();
    for (left, right, gate) in PAIRS {
        let pair = match (sides.get(left), sides.get(right)) {
            (Some(l), Some(r)) => {
                let classes = (!gate).then_some(&in_class);
                compare_pair((left, l), (right, r), gate, inputs, classes)
            }
            _ => json!({"left": left, "right": right, "gate": gate, "error": "a side could not be produced"}),
        };
        if gate && pair["differences"].as_array().is_some_and(|d| !d.is_empty()) {
            status_fail = true;
        }
        pairs.push(pair);
    }

    // The fuzz table: every cell on every side that exists.
    let fuzz_value = load_scenario(&root, FUZZ_SCENARIO).and_then(|s| fuzz_request(&s));
    let mut mismatches = Vec::new();
    let (mut fuzz_pairs, mut fuzz_intervals) = (0, 0);
    match fuzz_value {
        Ok(request) => {
            fuzz_pairs = request.pairs.len();
            fuzz_intervals = request.intervals.len();
            for (k, (card_id, reps)) in request.pairs.iter().enumerate() {
                for (i, interval) in request.intervals.iter().enumerate() {
                    let values: BTreeMap<&str, Value> = sides
                        .iter()
                        .map(|(name, side)| {
                            let cell = side
                                .fuzz
                                .get(k)
                                .and_then(|row| row.get(i + 2))
                                .cloned()
                                .unwrap_or(Value::Null);
                            (*name, cell)
                        })
                        .collect();
                    let first = values.values().next();
                    if values.values().any(|v| Some(v) != first || v.is_null()) {
                        mismatches
                            .push(json!({"card_id": card_id, "reps": reps, "interval": interval, "values": values}));
                    }
                }
            }
        }
        Err(e) => {
            errors.entry("fuzz".to_owned()).or_insert(e);
        }
    }
    if !mismatches.is_empty() || errors.contains_key("fuzz") {
        status_fail = true;
    }

    let mut sides_json = Map::new();
    for name in SIDES {
        let mut info = sides.get(name).map(|s| s.info.clone()).unwrap_or_default();
        if let Some(e) = errors.get(name) {
            info.insert("error".to_owned(), json!(e));
        }
        sides_json.insert(name.to_owned(), Value::Object(info));
    }
    let status = if status_fail { "fail" } else { "pass" };
    let mut fuzz_json = json!({
        "scenario": FUZZ_SCENARIO,
        "pairs": fuzz_pairs,
        "intervals": fuzz_intervals,
        "mismatches": mismatches,
    });
    if let Some(e) = errors.get("fuzz") {
        fuzz_json["error"] = json!(e);
    }
    let report = json!({
        "report_version": 1,
        "status": status,
        "corpus": {"path": format!("conformance/{CORPUS}"), "sha256": corpus.sha256, "inputs": inputs},
        "sides": sides_json,
        "pairs": pairs,
        "fuzz": fuzz_json,
    });
    write_json(out, &report)?;
    eprint!("{}", summary(&report));
    Ok(i32::from(status_fail))
}

/// F10.3's text summary: counts and rate per pair, the first 20 differences
/// of each, the fuzz result and the status.
fn summary(report: &Value) -> String {
    let mut text = String::new();
    for (name, side) in report["sides"].as_object().into_iter().flatten() {
        if let Some(e) = side.get("error") {
            let _ = writeln!(text, "{name}: not produced: {}", e.as_str().unwrap_or_default());
        }
    }
    for pair in report["pairs"].as_array().into_iter().flatten() {
        let gate = if pair["gate"].as_bool() == Some(true) {
            "gate"
        } else {
            "reported only"
        };
        if pair.get("error").is_some() {
            let _ = writeln!(
                text,
                "{} vs {} ({gate}): not compared, a side is missing",
                pair["left"].as_str().unwrap_or_default(),
                pair["right"].as_str().unwrap_or_default()
            );
            continue;
        }
        let _ = writeln!(
            text,
            "{} vs {} ({gate}): {} of {} inputs differ; {} days (rate {}), {} memory values",
            pair["left"].as_str().unwrap_or_default(),
            pair["right"].as_str().unwrap_or_default(),
            pair["differing_inputs"],
            pair["inputs"],
            pair["differing_days"],
            pair["days_rate"],
            pair["differing_memory"]
        );
        let differences = pair["differences"].as_array().cloned().unwrap_or_default();
        for d in differences.iter().take(SHOWN) {
            let class = match d.get("in_class").and_then(Value::as_bool) {
                Some(true) => " (in class)",
                Some(false) => " (outside the class)",
                None => "",
            };
            let _ = writeln!(
                text,
                "  input {} {}: {} vs {}{class}",
                d["id"],
                d["name"].as_str().unwrap_or_default(),
                d["left"],
                d["right"]
            );
        }
        if differences.len() > SHOWN {
            let _ = writeln!(text, "  (+{} more)", differences.len() - SHOWN);
        }
    }
    let fuzz = &report["fuzz"];
    let mismatches = fuzz["mismatches"].as_array().map_or(0, Vec::len);
    let _ = writeln!(
        text,
        "fuzz table: {} pairs x {} intervals, {mismatches} mismatching cells",
        fuzz["pairs"], fuzz["intervals"]
    );
    let _ = writeln!(text, "fp-probe: {}", report["status"].as_str().unwrap_or_default());
    text
}
