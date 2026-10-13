//! layering-check: enforces edges.toml on a Cargo workspace (`tools/mg core-lint`).
//!
//! usage: layering-check [--manifest-path <Cargo.toml>] [--edges <edges.toml>] [--json]
//!
//! Prints one violation per line (one JSON object per line with `--json`) and
//! exits 0 when there is none, 1 when there are violations, 2 when the check
//! cannot run. The default edges.toml is the one next to this tool in the
//! workspace.

use std::path::PathBuf;
use std::process::ExitCode;

use layering_check::{Error, edges::Edges, layering, metadata};

const USAGE: &str = "usage: layering-check [--manifest-path <Cargo.toml>] [--edges <edges.toml>] [--json]";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("layering-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, Error> {
    let mut manifest = PathBuf::from("Cargo.toml");
    let mut edges_path = None;
    let mut json = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--manifest-path") => manifest = args.next().map(PathBuf::from).ok_or_else(|| Error::new(USAGE))?,
            Some("--edges") => edges_path = Some(args.next().map(PathBuf::from).ok_or_else(|| Error::new(USAGE))?),
            Some("--json") => json = true,
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return Ok(true);
            }
            _ => {
                return Err(Error::new(format!(
                    "unexpected argument {}\n{USAGE}",
                    arg.to_string_lossy()
                )));
            }
        }
    }

    let meta = metadata::load(&manifest)?;
    let edges_path = edges_path.unwrap_or_else(|| meta.workspace_root.join("tools/layering-check/edges.toml"));
    let edges = Edges::load(&edges_path)?;
    let violations = layering::check(&meta, &edges)?;
    for v in &violations {
        if json {
            println!("{}", v.to_json());
        } else {
            println!("{v}");
        }
    }
    if violations.is_empty() {
        eprintln!(
            "layering-check: {} workspace crates follow {}",
            meta.workspace_members.len(),
            edges_path.display()
        );
    } else {
        eprintln!(
            "layering-check: {} violation(s) of {}",
            violations.len(),
            edges_path.display()
        );
    }
    Ok(violations.is_empty())
}
