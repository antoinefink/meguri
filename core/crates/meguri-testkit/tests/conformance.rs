//! T0: one test per scenario file under conformance/scenarios, replayed on
//! Meguri's executor against its committed golden (specs/conformance/dsl.md
//! D10). A test fails when its scenario is invalid or is active and does not
//! pass; other statuses are printed. `tools/mg conformance` writes the full
//! report.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use meguri_testkit::exec::MeguriExecutor;
use meguri_testkit::suite::Suite;

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn suite() -> &'static (Suite, MeguriExecutor) {
    static SUITE: OnceLock<(Suite, MeguriExecutor)> = OnceLock::new();
    SUITE.get_or_init(|| {
        let suite = Suite::open(&conformance_dir()).unwrap_or_else(|e| panic!("{e}"));
        (suite, MeguriExecutor::new())
    })
}

fn scenario(path: &Path) -> datatest_stable::Result<()> {
    let (suite, executor) = suite();
    let outcome = suite.check_path(path, executor);
    if outcome.fails_t0() {
        return Err(outcome.render_text().into());
    }
    if outcome.status != meguri_testkit::suite::Status::Pass {
        eprintln!("{}", outcome.render_text());
    }
    Ok(())
}

datatest_stable::harness! {
    { test = scenario, root = "../../../conformance/scenarios", pattern = r"^(?!(?:.*/)?area\.ya?ml$).*\.(?:ya?ml|json)$" },
}
