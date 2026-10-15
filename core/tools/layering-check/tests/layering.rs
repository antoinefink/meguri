//! layering-check through its binary, against fixture workspaces that hold one
//! empty crate per crate named in the real edges.toml, plus seeded violations.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn edges_toml() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("edges.toml")
}

/// Every crate name edges.toml places, read from the real file.
fn edges_crates() -> Vec<String> {
    let text = fs::read_to_string(edges_toml()).unwrap();
    let table: toml::Table = text.parse().unwrap();
    let mut names = Vec::new();
    for row in table["row"].as_array().unwrap() {
        names.extend(
            row["crates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap().to_owned()),
        );
    }
    for group in ["any", "isolated"] {
        names.extend(
            table[group]["crates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap().to_owned()),
        );
    }
    names
}

struct Fixture {
    dir: TempDir,
}

struct Run {
    code: i32,
    violations: Vec<Value>,
    stderr: String,
}

impl Run {
    fn rules(&self) -> Vec<(String, String)> {
        self.violations
            .iter()
            .map(|v| {
                (
                    v["rule"].as_str().unwrap().to_owned(),
                    v["subject"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    fn message(&self, rule: &str, subject: &str) -> String {
        self.violations
            .iter()
            .find(|v| v["rule"] == rule && v["subject"] == subject)
            .unwrap_or_else(|| {
                panic!(
                    "no {rule} violation for {subject}: {:?} {}",
                    self.violations, self.stderr
                )
            })["message"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

impl Fixture {
    fn new() -> Fixture {
        let fixture = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        fixture.write(
            "ws/Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n",
        );
        for name in edges_crates() {
            fixture.write(&format!("ws/crates/{name}/Cargo.toml"), &manifest(&name, ""));
            fixture.write(&format!("ws/crates/{name}/src/lib.rs"), "");
        }
        fixture
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// Adds `line` to a section of a workspace crate's manifest.
    fn add(&self, krate: &str, section: &str, line: &str) {
        let path = self.path(&format!("ws/crates/{krate}/Cargo.toml"));
        let text = fs::read_to_string(&path).unwrap();
        let header = format!("[{section}]\n");
        let text = if text.contains(&header) {
            text.replacen(&header, &format!("{header}{line}\n"), 1)
        } else {
            format!("{text}\n{header}{line}\n")
        };
        fs::write(path, text).unwrap();
    }

    /// A workspace crate depends on another workspace crate.
    fn depend(&self, from: &str, to: &str) {
        self.add(from, "dependencies", &format!("{to} = {{ path = \"../{to}\" }}"));
    }

    /// A stand-in for a third-party crate, outside the workspace.
    fn vendor(&self, name: &str, deps: &str) -> PathBuf {
        self.write(&format!("vendor/{name}/Cargo.toml"), &manifest(name, deps));
        self.write(&format!("vendor/{name}/src/lib.rs"), "");
        self.path(&format!("vendor/{name}"))
    }

    fn remove(&self, krate: &str) {
        fs::remove_dir_all(self.path(&format!("ws/crates/{krate}"))).unwrap();
    }

    fn run(&self) -> Run {
        let out = Command::new(env!("CARGO_BIN_EXE_layering-check"))
            .arg("--manifest-path")
            .arg(self.path("ws/Cargo.toml"))
            .arg("--edges")
            .arg(edges_toml())
            .arg("--json")
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        Run {
            code: out.status.code().unwrap(),
            violations: stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect(),
            stderr: String::from_utf8(out.stderr).unwrap(),
        }
    }
}

fn manifest(name: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{deps}\n")
}

fn dep_on(path: &Path, name: &str) -> String {
    format!("{name} = {{ path = \"{}\" }}", path.display())
}

#[test]
fn a_workspace_with_only_allowed_edges_passes() {
    let fx = Fixture::new();
    fx.depend("meguri-model", "meguri-text");
    fx.depend("meguri-sched", "meguri-storage");
    fx.depend("meguri-engine", "meguri-sched");
    fx.depend("meguri-stats", "meguri-engine");
    fx.depend("meguri-package", "meguri-engine");
    fx.depend("meguri-host", "meguri-package");
    fx.depend("meguri-ffi", "meguri-host");
    fx.depend("meguri-testkit", "meguri-host");
    fx.depend("meguri-cli", "meguri-testkit");
    let tokio = fx.vendor("tokio", "");
    fx.add("meguri-sync", "dependencies", &dep_on(&tokio, "tokio"));

    let run = fx.run();
    assert_eq!((run.code, run.rules()), (0, vec![]), "{}", run.stderr);
}

#[test]
fn tokio_in_sched_fails_naming_the_forbidden_edge() {
    let fx = Fixture::new();
    let tokio = fx.vendor("tokio", "");
    fx.add("meguri-sched", "dependencies", &dep_on(&tokio, "tokio"));

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.rules(),
        [("restricted".to_owned(), "meguri-sched -> tokio".to_owned())]
    );
    let message = run.message("restricted", "meguri-sched -> tokio");
    assert!(message.contains("meguri-sched depends on tokio"), "{message}");
    assert!(message.contains("only in meguri-sync, meguri-servers"), "{message}");
}

#[test]
fn tokio_reached_through_a_third_party_crate_fails_with_the_path() {
    let fx = Fixture::new();
    let tokio = fx.vendor("tokio", "");
    let hyper = fx.vendor("hyper", &dep_on(&tokio, "tokio"));
    fx.add("meguri-text", "dependencies", &dep_on(&hyper, "hyper"));

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let message = run.message("restricted", "meguri-text -> tokio");
    assert!(
        message.contains("meguri-text reaches tokio through hyper -> tokio"),
        "{message}"
    );
}

#[test]
fn axum_outside_servers_and_reqwest_outside_sync_and_servers_fail() {
    let fx = Fixture::new();
    let axum = fx.vendor("axum", "");
    let reqwest = fx.vendor("reqwest", "");
    fx.add("meguri-sync", "dependencies", &dep_on(&axum, "axum"));
    fx.add("meguri-package", "dev-dependencies", &dep_on(&reqwest, "reqwest"));

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let mut rules = run.rules();
    rules.sort();
    assert_eq!(
        rules,
        [
            ("restricted".to_owned(), "meguri-package -> reqwest".to_owned()),
            ("restricted".to_owned(), "meguri-sync -> axum".to_owned()),
        ]
    );
    let message = run.message("restricted", "meguri-package -> reqwest");
    assert!(message.contains("only in meguri-sync, meguri-servers"), "{message}");
}

#[test]
fn reqwest_is_allowed_in_sync_and_servers() {
    // The sync transport (U105) and the servers' remote-media fetcher (U135,
    // fetch.rs) are separate reqwest clients.
    let fx = Fixture::new();
    let reqwest = fx.vendor("reqwest", "");
    fx.add("meguri-sync", "dependencies", &dep_on(&reqwest, "reqwest"));
    fx.add("meguri-servers", "dependencies", &dep_on(&reqwest, "reqwest"));

    let run = fx.run();
    assert_eq!((run.code, run.rules()), (0, vec![]), "{}", run.stderr);
}

#[test]
fn engine_in_sched_fails_naming_the_edges_toml_rows() {
    let fx = Fixture::new();
    fx.depend("meguri-sched", "meguri-engine");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.rules(),
        [("edge".to_owned(), "meguri-sched -> meguri-engine".to_owned())]
    );
    let message = run.message("edge", "meguri-sched -> meguri-engine");
    assert!(
        message.contains("meguri-sched (edges.toml row 6) must not depend on meguri-engine (row 7)"),
        "{message}"
    );
    assert!(message.contains("only on crates in lower rows"), "{message}");
}

#[test]
fn stats_in_package_fails_naming_the_sibling_row() {
    let fx = Fixture::new();
    fx.depend("meguri-package", "meguri-stats");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let message = run.message("edge", "meguri-package -> meguri-stats");
    assert!(
        message.contains("meguri-package (edges.toml row 8) must not depend on meguri-stats (row 8)"),
        "{message}"
    );
    assert!(message.contains("siblings"), "{message}");
}

#[test]
fn dev_and_build_dependencies_follow_the_same_table() {
    let fx = Fixture::new();
    fx.add(
        "meguri-text",
        "dev-dependencies",
        "meguri-model = { path = \"../meguri-model\" }",
    );
    fx.add(
        "meguri-model",
        "build-dependencies",
        "meguri-storage = { path = \"../meguri-storage\" }",
    );

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let mut rules = run.rules();
    rules.sort();
    assert_eq!(
        rules,
        [
            ("edge".to_owned(), "meguri-model -> meguri-storage".to_owned()),
            ("edge".to_owned(), "meguri-text -> meguri-model".to_owned()),
        ]
    );
    assert!(
        run.message("edge", "meguri-text -> meguri-model")
            .contains("dev-dependency")
    );
    assert!(
        run.message("edge", "meguri-model -> meguri-storage")
            .contains("build-dependency")
    );
}

#[test]
fn row_crates_may_use_testkit_as_a_dev_dependency() {
    let fx = Fixture::new();
    // Integration tests in a row crate's tests/ drive testkit code (U36, U91,
    // U118, U250), while testkit depends on that crate: a dev-only cycle.
    fx.depend("meguri-testkit", "meguri-engine");
    for krate in [
        "meguri-text",
        "meguri-engine",
        "meguri-search",
        "meguri-sync",
        "meguri-ffi",
    ] {
        fx.add(
            krate,
            "dev-dependencies",
            "meguri-testkit = { path = \"../meguri-testkit\" }",
        );
    }

    let run = fx.run();
    assert_eq!((run.code, run.rules()), (0, vec![]), "{}", run.stderr);
}

#[test]
fn row_crates_must_not_use_testkit_as_a_normal_or_build_dependency_nor_cli_at_all() {
    let fx = Fixture::new();
    fx.depend("meguri-text", "meguri-testkit");
    fx.add(
        "meguri-search",
        "build-dependencies",
        "meguri-testkit = { path = \"../meguri-testkit\" }",
    );
    fx.add(
        "meguri-sched",
        "dev-dependencies",
        "meguri-cli = { path = \"../meguri-cli\" }",
    );

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let mut rules = run.rules();
    rules.sort();
    assert_eq!(
        rules,
        [
            ("edge".to_owned(), "meguri-sched -> meguri-cli".to_owned()),
            ("edge".to_owned(), "meguri-search -> meguri-testkit".to_owned()),
            ("edge".to_owned(), "meguri-text -> meguri-testkit".to_owned()),
        ]
    );
    let normal = run.message("edge", "meguri-text -> meguri-testkit");
    assert!(
        normal.contains("meguri-text (edges.toml row 1) must not depend on meguri-testkit ([any] group):"),
        "{normal}"
    );
    assert!(normal.contains("only as a dev-dependency"), "{normal}");
    let build = run.message("edge", "meguri-search -> meguri-testkit");
    assert!(build.contains("as a build-dependency"), "{build}");
    let cli = run.message("edge", "meguri-sched -> meguri-cli");
    assert!(cli.contains("as a dev-dependency"), "{cli}");
    assert!(!cli.contains("only as a dev-dependency"), "{cli}");
}

#[test]
fn row_dev_dependencies_must_name_an_any_crate() {
    let text = fs::read_to_string(edges_toml()).unwrap();
    let entry = "row_dev_dependencies = [\"meguri-testkit\"]";
    assert!(text.contains(entry), "edges.toml has no {entry}");
    let text = text.replace(entry, "row_dev_dependencies = [\"meguri-text\"]");

    let err = layering_check::edges::Edges::parse(&text).unwrap_err().to_string();
    assert!(
        err.contains("row_dev_dependencies names meguri-text, which is not an [any] crate"),
        "{err}"
    );
}

#[test]
fn development_crates_may_use_any_row_crate_but_not_ffi() {
    let fx = Fixture::new();
    fx.depend("meguri-testkit", "meguri-text");
    fx.depend("meguri-cli", "meguri-servers");
    fx.depend("meguri-cli", "meguri-ffi");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.rules(),
        [("edge".to_owned(), "meguri-cli -> meguri-ffi".to_owned())]
    );
    assert!(
        run.message("edge", "meguri-cli -> meguri-ffi")
            .contains("[any] crates must not depend on meguri-ffi")
    );
}

#[test]
fn isolated_tools_neither_use_nor_serve_workspace_crates() {
    let fx = Fixture::new();
    fx.depend("layering-check", "meguri-text");
    fx.depend("meguri-cli", "layering-check");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let mut rules = run.rules();
    rules.sort();
    assert_eq!(
        rules,
        [
            ("edge".to_owned(), "layering-check -> meguri-text".to_owned()),
            ("edge".to_owned(), "meguri-cli -> layering-check".to_owned()),
        ]
    );
}

#[test]
fn a_workspace_crate_missing_from_edges_toml_fails() {
    let fx = Fixture::new();
    fx.write("ws/crates/meguri-extra/Cargo.toml", &manifest("meguri-extra", ""));
    fx.write("ws/crates/meguri-extra/src/lib.rs", "");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(run.rules(), [("unknown-crate".to_owned(), "meguri-extra".to_owned())]);
    assert!(
        run.message("unknown-crate", "meguri-extra")
            .contains("not in edges.toml")
    );
}

#[test]
fn an_edges_toml_name_that_is_not_a_workspace_crate_fails() {
    let fx = Fixture::new();
    fx.remove("meguri-search");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(run.rules(), [("stale-entry".to_owned(), "meguri-search".to_owned())]);
}

#[test]
fn a_raw_open_call_outside_host_fails_with_its_location() {
    let fx = Fixture::new();
    fx.write(
        "ws/crates/meguri-testkit/src/lib.rs",
        "//! Mentions open_raw in a comment, which is fine.\n\
         pub fn run(dir: &str) {\n    let _ = \"open_raw in a string is fine\";\n    \
         let _engine = meguri_engine::open_raw(dir);\n}\n",
    );
    fx.write(
        "ws/crates/meguri-cli/tests/flow.rs",
        "use meguri_engine::open_raw as open;\n",
    );
    fx.write(
        "ws/crates/meguri-host/src/lib.rs",
        "pub fn open(dir: &str) { meguri_engine::open_raw(dir); }\n",
    );
    fx.write("ws/crates/meguri-engine/src/lib.rs", "pub fn open_raw(_dir: &str) {}\n");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let mut rules = run.rules();
    rules.sort();
    assert_eq!(
        rules,
        [
            ("raw-open".to_owned(), "meguri-cli:tests/flow.rs:1".to_owned()),
            ("raw-open".to_owned(), "meguri-testkit:src/lib.rs:4".to_owned()),
        ]
    );
    let message = run.message("raw-open", "meguri-testkit:src/lib.rs:4");
    assert!(message.contains("open_raw"), "{message}");
    assert!(message.contains("CoreHost::open"), "{message}");
}

#[test]
fn a_raw_identifier_spelling_of_raw_open_fails() {
    let fx = Fixture::new();
    fx.write(
        "ws/crates/meguri-testkit/src/lib.rs",
        "pub fn run(dir: &str) {\n    meguri_engine::r#open_raw(dir);\n}\n",
    );
    fx.write("ws/crates/meguri-engine/src/lib.rs", "pub fn open_raw(_dir: &str) {}\n");

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.rules(),
        [("raw-open".to_owned(), "meguri-testkit:src/lib.rs:2".to_owned())]
    );
}

#[test]
fn the_storage_builder_named_in_the_engine_or_the_ffi_fails() {
    let fx = Fixture::new();
    fx.write(
        "ws/crates/meguri-engine/src/lib.rs",
        "//! CollectionBuilder in a comment is fine.\npub fn build() {\n    let _ = meguri_storage::CollectionBuilder::create;\n}\n",
    );
    fx.write(
        "ws/crates/meguri-ffi/src/lib.rs",
        "use meguri_storage::CollectionBuilder as B;\n",
    );
    fx.write(
        "ws/crates/meguri-testkit/src/lib.rs",
        "pub fn build() { let _ = meguri_storage::CollectionBuilder::create; }\n",
    );

    let run = fx.run();
    assert_eq!(run.code, 1, "{}", run.stderr);
    let mut rules = run.rules();
    rules.sort();
    assert_eq!(
        rules,
        [
            ("storage-builder".to_owned(), "meguri-engine:src/lib.rs:3".to_owned()),
            ("storage-builder".to_owned(), "meguri-ffi:src/lib.rs:1".to_owned()),
        ]
    );
    let message = run.message("storage-builder", "meguri-ffi:src/lib.rs:1");
    assert!(message.contains("CollectionBuilder"), "{message}");
}

#[test]
fn human_output_names_each_violation_and_exits_non_zero() {
    let fx = Fixture::new();
    fx.depend("meguri-sched", "meguri-engine");
    let out = Command::new(env!("CARGO_BIN_EXE_layering-check"))
        .arg("--manifest-path")
        .arg(fx.path("ws/Cargo.toml"))
        .arg("--edges")
        .arg(edges_toml())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("[edge] meguri-sched (edges.toml row 6) must not depend on meguri-engine"),
        "{stdout}"
    );
}
