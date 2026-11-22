//! `meguri-cli fp-probe` through its binary on the committed probe corpus,
//! goldens and overlays (specs/conformance/float-determinism.md F10). The
//! sides the tests build themselves come from the goldens and overlay files
//! read here, never from the probe's own code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn cli(args: &[&str]) -> Output {
    Command::new(crate::bin_exe())
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_string(value).unwrap()).unwrap();
}

/// Meguri's side on this platform, from `fp-probe eval`.
fn eval(dir: &Path) -> PathBuf {
    let out = dir.join("side.json");
    let run = cli(&[
        "fp-probe",
        "eval",
        "--conformance",
        conformance().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    out
}

/// `fp-probe report` with these sides and options; the exit code and report.
fn run_report(dir: &Path, macos: &Path, linux: &Path, extra: &[&str]) -> (i32, Value) {
    let out = dir.join("report.json");
    let root = conformance();
    let mut args = vec![
        "fp-probe",
        "report",
        "--conformance",
        root.to_str().unwrap(),
        "--meguri-macos",
        macos.to_str().unwrap(),
        "--meguri-linux",
        linux.to_str().unwrap(),
        "--commit",
        "abc123",
        "--out",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let run = cli(&args);
    assert!(out.is_file(), "{}", text(&run.stderr));
    (run.status.code().unwrap(), read(&out))
}

fn pair<'a>(report: &'a Value, left: &str, right: &str) -> &'a Value {
    report["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["left"] == left && p["right"] == right)
        .unwrap()
}

/// The oracle sides as F10.1 defines them, built here from the files: the
/// corpus goldens' rows in order, then with every overlay entry's apple
/// value put at its pointer; and the fuzz table's golden rows.
/// An overlay entry of a corpus scenario: (input id, column, linux, apple).
type Entry = (usize, usize, Value, Value);

fn oracle_files() -> (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Entry>) {
    let (mut linux, mut apple, mut entries) = (Vec::new(), Vec::new(), Vec::new());
    for n in 0..100 {
        let rel = format!("fp-probe/corpus/next-states-{n:03}");
        let golden = read(&conformance().join(format!("goldens/{rel}.json")));
        let rows = golden["observations"]["steps"][0]["fsrs_next"]["rows"]
            .as_array()
            .unwrap()
            .clone();
        let mut patched = rows.clone();
        let overlay = conformance().join(format!("goldens-apple/{rel}.json"));
        if overlay.is_file() {
            for entry in read(&overlay)["entries"].as_array().unwrap() {
                let parts: Vec<&str> = entry["pointer"].as_str().unwrap().split('/').collect();
                let (row, column): (usize, usize) = (parts[5].parse().unwrap(), parts[6].parse().unwrap());
                patched[row][column] = entry["apple"].clone();
                entries.push((n * 1000 + row, column, entry["linux"].clone(), entry["apple"].clone()));
            }
        }
        linux.extend(rows);
        apple.extend(patched);
    }
    let fuzz =
        read(&conformance().join("goldens/fp-probe/corpus/fuzz-table.json"))["observations"]["steps"][0]["fuzz_table"]
            ["rows"]
            .as_array()
            .unwrap()
            .clone();
    (linux, apple, fuzz, entries)
}

fn side_file(path: &Path, rows: &[Value], fuzz: &[Value], platform: &str) {
    let corpus = fs::read(conformance().join("corpora/fp-probe/inputs.jsonl")).unwrap();
    let sha: String = Sha256::digest(&corpus).iter().map(|b| format!("{b:02x}")).collect();
    write(
        path,
        &json!({"platform": platform, "corpus": {"sha256": sha, "inputs": rows.len()}, "rows": rows,
                "fuzz": {"scenario": "fp-probe/corpus/fuzz-table", "rows": fuzz}}),
    );
}

#[test]
fn meguri_reproduces_the_oracle_of_its_own_platform_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let side = eval(dir.path());
    let value = read(&side);
    assert_eq!(value["rows"].as_array().unwrap().len(), 100_000);
    assert_eq!(value["fuzz"]["rows"].as_array().unwrap().len(), 10_000);
    // The gate pair of this platform has no difference, and the fuzz table
    // matches the oracles. The other Meguri side is not produced here.
    let (macos, linux, pair_of_this_platform, missing) = if cfg!(target_os = "macos") {
        (
            side.clone(),
            dir.path().join("none.json"),
            ("apple_oracle", "meguri_macos"),
            "meguri_linux",
        )
    } else {
        (
            dir.path().join("none.json"),
            side.clone(),
            ("linux_oracle", "meguri_linux"),
            "meguri_macos",
        )
    };
    let (code, report) = run_report(dir.path(), &macos, &linux, &[]);
    let gate = pair(&report, pair_of_this_platform.0, pair_of_this_platform.1);
    assert_eq!(
        gate["differing_inputs"],
        0,
        "{:?}",
        gate["differences"].as_array().map(|d| &d[..d.len().min(5)])
    );
    assert_eq!(gate["inputs"], 100_000);
    assert_eq!(report["fuzz"]["mismatches"], json!([]));
    assert_eq!(report["fuzz"]["pairs"], 10_000);
    assert_eq!(report["fuzz"]["intervals"], 20);
    // A side that was not produced fails the probe, with its reason.
    assert_eq!(code, 1);
    assert_eq!(report["status"], "fail");
    assert!(
        report["sides"][missing]["error"]
            .as_str()
            .is_some_and(|e| !e.is_empty())
    );
}

#[test]
fn oracle_differences_are_listed_with_their_class_and_never_fail_the_probe() {
    let dir = tempfile::tempdir().unwrap();
    let (linux, apple, fuzz, entries) = oracle_files();
    let (macos_path, linux_path) = (dir.path().join("macos.json"), dir.path().join("linux.json"));
    side_file(&macos_path, &apple, &fuzz, "macos-arm64");
    side_file(&linux_path, &linux, &fuzz, "linux-arm64");
    let (code, report) = run_report(dir.path(), &macos_path, &linux_path, &[]);
    assert_eq!(report["status"], "pass", "{report:#}");
    assert_eq!(code, 0);
    assert_eq!(report["report_version"], 1);
    assert_eq!(report["corpus"]["inputs"], 100_000);
    assert_eq!(
        report["sides"]["meguri_macos"],
        json!({"commit": "abc123", "platform": "macos-arm64"})
    );
    let oracles = pair(&report, "linux_oracle", "apple_oracle");
    assert_eq!(oracles["gate"], false);
    let differences = oracles["differences"].as_array().unwrap();
    assert_eq!(differences.len(), entries.len());
    // Each memory difference is in the class exactly when it is within its
    // bound (F8.3), worked out here from the files: a stability (columns 1,
    // 4, 7, 10) the larger of relative 1e-4 and 1e-5 days, a difficulty
    // relative 1e-5.
    let mut beyond_relative_1e5 = 0;
    for d in differences {
        let id = d["id"].as_u64().unwrap() as usize;
        let column = d["column"].as_u64().unwrap() as usize;
        let (_, _, l, a) = entries.iter().find(|e| e.0 == id && e.1 == column).unwrap();
        assert_eq!((&d["left"], &d["right"]), (l, a));
        if !column.is_multiple_of(3) {
            let (l, a) = (l.as_f64().unwrap(), a.as_f64().unwrap());
            let m = l.abs().max(a.abs());
            let bound = if column % 3 == 1 {
                (1e-4 * m).max(1e-5)
            } else {
                1e-5 * m
            };
            assert_eq!(d["in_class"], (a - l).abs() <= bound, "{d}");
            if (a - l).abs() > 1e-5 * m {
                assert_eq!(column % 3, 1, "{d}");
                beyond_relative_1e5 += 1;
            }
        }
    }
    // F11: every difference of the committed overlays is in the class, the
    // 12 stabilities of F11.1 beyond relative 1e-5 included.
    assert!(differences.iter().all(|d| d["in_class"] == true));
    assert_eq!(beyond_relative_1e5, 12);
    let days = entries.iter().filter(|e| e.1.is_multiple_of(3)).count();
    assert_eq!(oracles["differing_memory"], entries.len() - days);
    assert_eq!(oracles["differing_days"], days);
    assert_eq!(oracles["days_rate"].as_f64(), Some(days as f64 / 400_000.0));
    for (left, right) in [("apple_oracle", "meguri_macos"), ("linux_oracle", "meguri_linux")] {
        assert_eq!(pair(&report, left, right)["differences"], json!([]));
    }
}

#[test]
fn a_gate_difference_or_a_fuzz_mismatch_fails_the_probe_and_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let (linux, apple, fuzz, _) = oracle_files();
    let (macos_path, linux_path) = (dir.path().join("macos.json"), dir.path().join("linux.json"));
    let mut wrong = apple.clone();
    let good_days = wrong[12_345][6].as_u64().unwrap();
    wrong[12_345][6] = json!(good_days + 1);
    let mut wrong_fuzz = fuzz.clone();
    let delta = wrong_fuzz[7][4].as_i64().unwrap();
    wrong_fuzz[7][4] = json!(delta + 1);
    side_file(&macos_path, &wrong, &fuzz, "macos-arm64");
    side_file(&linux_path, &linux, &wrong_fuzz, "linux-arm64");
    let (code, report) = run_report(dir.path(), &macos_path, &linux_path, &[]);
    assert_eq!((code, report["status"].as_str()), (1, Some("fail")));
    let gate = pair(&report, "apple_oracle", "meguri_macos");
    assert_eq!(
        gate["differences"],
        json!([{"id": 12_345, "column": 6, "name": "good_days", "left": good_days, "right": good_days + 1}])
    );
    assert_eq!(
        (gate["differing_inputs"].as_u64(), gate["differing_days"].as_u64()),
        (Some(1), Some(1))
    );
    let mismatches = report["fuzz"]["mismatches"].as_array().unwrap();
    assert_eq!(mismatches.len(), 1);
    let m = &mismatches[0];
    assert_eq!((&m["card_id"], &m["reps"]), (&fuzz[7][0], &fuzz[7][1]));
    // Column 4 of a row is the third interval: 3 days.
    assert_eq!(m["interval"], 3);
    assert_eq!(m["values"]["meguri_linux"], delta + 1);
    assert_eq!(m["values"]["linux_oracle"], delta);
}

#[test]
fn a_side_the_job_could_not_produce_fails_the_probe_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (linux, apple, fuzz, _) = oracle_files();
    let (macos_path, linux_path) = (dir.path().join("macos.json"), dir.path().join("linux.json"));
    side_file(&macos_path, &apple, &fuzz, "macos-arm64");
    side_file(&linux_path, &linux, &fuzz, "linux-arm64");
    let (code, report) = run_report(
        dir.path(),
        &macos_path,
        &linux_path,
        &[
            "--unavailable",
            "apple_oracle=Apple side not reproduced: 2 scenarios differ",
        ],
    );
    assert_eq!((code, report["status"].as_str()), (1, Some("fail")));
    assert_eq!(
        report["sides"]["apple_oracle"]["error"],
        "Apple side not reproduced: 2 scenarios differ"
    );
    assert!(pair(&report, "apple_oracle", "meguri_macos").get("error").is_some());
    assert_eq!(pair(&report, "linux_oracle", "meguri_linux")["differences"], json!([]));
    // A side evaluated on another corpus is not that side.
    let mut other = read(&linux_path);
    other["corpus"]["sha256"] = json!("0".repeat(64));
    write(&linux_path, &other);
    let (_, report) = run_report(dir.path(), &macos_path, &linux_path, &[]);
    assert!(
        report["sides"]["meguri_linux"]["error"]
            .as_str()
            .unwrap()
            .contains("another corpus")
    );
    // An unknown side is a usage error.
    let run = cli(&["fp-probe", "report", "--unavailable", "meguri_ios=x"]);
    assert_eq!(run.status.code(), Some(2));
}
