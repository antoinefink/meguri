//! `meguri-cli private`: the vault's copy, the output guard's candidate
//! strings and the private suite (U15, R1938–R1943). `tools/mg private` runs
//! these in a container without a network, the source mounted read-only at
//! `/vault`; see `meguri_testkit::private`.
//!
//! ```text
//! meguri-cli private copy --source <dir> --out <dir> [--access-log <file>]
//! meguri-cli private tokens --copy <dir> --out <file>
//! meguri-cli private suite --copy <dir> --out <file> [--log <file>]
//! ```
//!
//! stdout carries one JSON object of counts, sizes and hashes; nothing these
//! commands print names a deck, tag, field or media file. Exit 0 on success
//! (for `suite`: every check passed), 1 on a refusal, an error or a failed
//! check, 2 on a usage error.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use meguri_testkit::private::redact::Candidates;
use meguri_testkit::private::suite::{self, Check};
use meguri_testkit::private::vault::{FileCopy, Vault, VaultOptions};

const USAGE: &str = "usage: meguri-cli private copy --source <dir> --out <dir> [--access-log <file>]
       meguri-cli private tokens --copy <dir> --out <file>
       meguri-cli private suite --copy <dir> --out <file> [--log <file>]";

fn usage(message: &str) -> i32 {
    eprintln!("meguri-cli private: {message}");
    eprintln!("{USAGE}");
    2
}

fn fail(message: impl std::fmt::Display) -> i32 {
    eprintln!("meguri-cli private: {message}");
    1
}

/// U63's field round trip; U249 adds the remaining R1942 checks here.
fn registered_checks() -> Vec<Box<dyn Check>> {
    vec![Box::new(crate::private_suite::IoRoundtrip)]
}

/// `--name value` options; every one of `allowed` at most once.
fn options(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut out = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if !allowed.contains(&arg.as_str()) {
            return Err(format!("unknown argument {arg}"));
        }
        let Some(value) = rest.next() else {
            return Err(format!("{arg} needs a value"));
        };
        if out.insert(arg.clone(), PathBuf::from(value)).is_some() {
            return Err(format!("{arg} given twice"));
        }
    }
    Ok(out)
}

pub fn main(args: &[String]) -> i32 {
    let Some((command, rest)) = args.split_first() else {
        return usage("a command is required");
    };
    match command.as_str() {
        "copy" => copy(rest),
        "tokens" => tokens(rest),
        "suite" => run_suite(rest),
        other => usage(&format!("unknown command {other}")),
    }
}

fn file_json(copy: &FileCopy) -> serde_json::Value {
    serde_json::json!({"bytes": copy.bytes, "sha256": copy.sha256})
}

fn print(value: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(value).expect("JSON serializes"));
}

fn copy(args: &[String]) -> i32 {
    let opts = match options(args, &["--source", "--out", "--access-log"]) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    let (Some(source), Some(out)) = (opts.get("--source"), opts.get("--out")) else {
        return usage("--source and --out are required");
    };
    let report = match Vault::open(source, &VaultOptions::from_process()).and_then(|vault| vault.copy_to(out)) {
        Ok(report) => report,
        Err(e) => return fail(e),
    };
    if let Some(log) = opts.get("--access-log") {
        let mut text = report.access.join("\n");
        text.push('\n');
        if let Err(e) = fs::write(log, text) {
            return fail(format!("writing the access log failed: {e}"));
        }
    }
    print(&serde_json::json!({
        "collection": file_json(&report.collection),
        "wal": report.wal.as_ref().map(file_json),
        "media": {
            "files": report.media_files,
            "bytes": report.media_bytes,
            "skipped": report.media_skipped,
        },
        "opened": report.access.len(),
    }));
    0
}

fn tokens(args: &[String]) -> i32 {
    let opts = match options(args, &["--copy", "--out"]) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    let (Some(copy), Some(out)) = (opts.get("--copy"), opts.get("--out")) else {
        return usage("--copy and --out are required");
    };
    let found = match Candidates::extract(copy) {
        Ok(found) => found,
        Err(e) => return fail(e),
    };
    let written = fs::File::create(out).and_then(|file| {
        let mut writer = BufWriter::new(file);
        found.write_jsonl(&mut writer)?;
        writer.flush()
    });
    if let Err(e) = written {
        return fail(format!("writing the candidates failed: {e}"));
    }
    print(&serde_json::json!({
        "decks": found.decks.len(),
        "tags": found.tags.len(),
        "media": found.media.len(),
        "words": found.words.len(),
    }));
    0
}

fn run_suite(args: &[String]) -> i32 {
    let opts = match options(args, &["--copy", "--out", "--log"]) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    let (Some(copy), Some(out)) = (opts.get("--copy"), opts.get("--out")) else {
        return usage("--copy and --out are required");
    };
    let checks = registered_checks();
    let refs: Vec<&dyn Check> = checks.iter().map(Box::as_ref).collect();
    run_checks(&refs, copy, out, opts.get("--log").map(PathBuf::as_path))
}

/// Runs `checks` and writes the summary; failed checks' texts go to `log`.
fn run_checks(checks: &[&dyn Check], copy: &Path, out: &Path, log: Option<&Path>) -> i32 {
    if !copy.join(meguri_testkit::private::vault::COLLECTION).is_file() {
        return fail("the copy has no collection.anki2");
    }
    let mut errors = String::new();
    let summary = suite::run(checks, copy, &mut |name, text| {
        eprintln!("meguri-cli private: check {name} could not run (its error is in the private log)");
        errors.push_str(&format!("{name}: {text}\n"));
    });
    if let Some(log) = log
        && let Err(e) = fs::write(log, &errors)
    {
        return fail(format!("writing the private log failed: {e}"));
    }
    let json = summary.to_json();
    let mut text = serde_json::to_string_pretty(&json).expect("JSON serializes");
    text.push('\n');
    if let Err(e) = fs::write(out, &text) {
        return fail(format!("writing the summary failed: {e}"));
    }
    print(&json);
    if summary.passed() { 0 } else { 1 }
}
