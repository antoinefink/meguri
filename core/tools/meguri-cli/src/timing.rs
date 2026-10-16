//! `meguri-cli timing run`: the Meguri side of the timed operations
//! (`specs/conformance/timed-operations.md` T10, U18). It takes the same
//! collection directory and options as `tools/mg oracle time`, follows T2 to
//! T6 through Meguri's engine and writes the same report (T7) with `side`
//! `meguri`. `tools/mg perf-ratio` compares it with the oracle's.
//!
//! ```text
//! meguri-cli timing run <dir> --out <dir> [--scratch <dir>] [--op <name>]... [--now <instant>] [--tz <zone>] [--repeat <n>]
//! ```
//!
//! `--out` (made when missing) gets `timing.json`, or `timing-error.json`
//! (`{kind, detail}`; kinds `usage`, `no_collection`, `open_failed`,
//! `failed`) when the run cannot report. A run first removes both files, so
//! `--out` never holds an earlier run's report beside its own. Copies go to
//! a directory made in `--scratch` (default: the system's temporary
//! directory) and deleted at the end: a tmpfs in Docker, the Mac's local
//! disk natively, under `build/private/` for a vault copy (T2.1, T3.4).
//! Exit 0 with a report, 1 with an error report (or when an earlier report
//! cannot be removed), 2 on a usage error.
//!
//! The report's `meguri.commit` is `MEGURI_BUILD_COMMIT` as the build saw
//! it (40 hex digits; `null` otherwise) and `meguri.profile` the Cargo
//! profile the build used (`build.rs`). Comparisons need `release` (T3.5).
//! The build carries meguri-testkit's test hooks (unarmed fault points),
//! which only add work to Meguri's side.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::timing::{self, MeguriBuild, Operation, RunError, TimedOperation, TimingRequest};
use serde_json::json;

const USAGE: &str = "usage: meguri-cli timing run <dir> --out <dir> [--scratch <dir>] [--op <name>]... \
                     [--now <instant>] [--tz <zone>] [--repeat <n>]";

/// The operations Meguri times, each through the entry point the app's
/// bridge calls (T4.1). Every one starts from an open collection, and the
/// bridge opens collections through `CoreHost::open` (U38), which does not
/// exist yet: until it does, the table is empty and every operation is
/// `unsupported`. An area unit adds its operation here once that entry
/// point and the operation's own exist.
static IMPLEMENTED: &[(Operation, &(dyn TimedOperation + Sync))] = &[];

fn implementation(op: Operation) -> Option<&'static dyn TimedOperation> {
    IMPLEMENTED
        .iter()
        .find(|(implemented, _)| *implemented == op)
        .map(|&(_, run)| run as &dyn TimedOperation)
}

fn build() -> MeguriBuild {
    let commit = option_env!("MEGURI_BUILD_COMMIT")
        .filter(|c| c.len() == 40 && c.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .map(str::to_owned);
    MeguriBuild {
        commit,
        profile: env!("MEGURI_CLI_PROFILE").to_owned(),
    }
}

pub fn main(args: &[String]) -> i32 {
    match args.split_first() {
        Some((command, rest)) if command == "run" => run(rest),
        Some((command, _)) => usage(None, &format!("unknown command {command}")),
        None => usage(None, "a command is required"),
    }
}

fn usage(out: Option<&Path>, message: &str) -> i32 {
    eprintln!("meguri-cli timing: {message}");
    eprintln!("{USAGE}");
    if let Some(out) = out {
        write_error(out, "usage", message);
    }
    2
}

fn write_error(out: &Path, kind: &str, detail: &str) {
    let text = meguri_testkit::observe::golden_text(&json!({"kind": kind, "detail": detail}));
    if let Err(e) = fs::create_dir_all(out).and_then(|()| fs::write(out.join("timing-error.json"), text)) {
        eprintln!("meguri-cli timing: writing timing-error.json failed: {e}");
    }
}

fn run(args: &[String]) -> i32 {
    let mut request = TimingRequest {
        scratch_root: std::env::temp_dir(),
        ..TimingRequest::default()
    };
    let mut dir: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut problem: Option<String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if !arg.starts_with("--") {
            if dir.is_some() {
                problem.get_or_insert(format!("one collection directory, not also {arg}"));
            }
            dir = Some(PathBuf::from(arg));
            continue;
        }
        let Some(value) = rest.next() else {
            problem.get_or_insert(format!("{arg} needs a value"));
            continue;
        };
        let once = |slot: &mut Option<String>, problem: &mut Option<String>| {
            if slot.replace(value.clone()).is_some() {
                problem.get_or_insert(format!("{arg} given twice"));
            }
        };
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value)),
            "--scratch" => request.scratch_root = PathBuf::from(value),
            "--op" => request.ops.push(value.clone()),
            "--now" => once(&mut request.now, &mut problem),
            "--tz" => once(&mut request.tz, &mut problem),
            "--repeat" => once(&mut request.repeat, &mut problem),
            other => {
                problem.get_or_insert(format!("unknown option {other}"));
            }
        }
    }
    let Some(out) = out else {
        return usage(None, "--out is required");
    };
    for name in ["timing.json", "timing-error.json"] {
        if let Err(e) = fs::remove_file(out.join(name))
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("meguri-cli timing: removing {name} failed: {e}");
            return 1;
        }
    }
    if let Some(problem) = problem {
        return usage(Some(&out), &problem);
    }
    let Some(dir) = dir else {
        return usage(Some(&out), "a collection directory is required");
    };
    request.dir = dir;

    match timing::run::run(&request, &implementation, build()) {
        Ok(report) => {
            let written = fs::create_dir_all(&out).and_then(|()| fs::write(out.join("timing.json"), report.text()));
            match written {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("meguri-cli timing: writing timing.json failed: {e}");
                    1
                }
            }
        }
        Err(RunError::Usage(message)) => usage(Some(&out), &message),
        Err(error) => {
            eprintln!("meguri-cli timing: {}: {}", error.kind(), error.detail());
            write_error(&out, error.kind(), error.detail());
            1
        }
    }
}
