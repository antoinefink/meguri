use meguri_testkit::{
    exec::MeguriExecutor,
    suite::{Status, Suite},
};
use std::path::Path;

#[test]
fn every_media_golden_runs_and_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let suite = Suite::open(&root).unwrap();
    let executor = MeguriExecutor::new();
    let files = suite.discover(&["media".into()]).unwrap();
    assert!(files.len() >= 13);
    let mut failures = Vec::new();
    for file in files {
        let outcome = suite.check(&file, &executor);
        if outcome.status != Status::Pass {
            failures.push(outcome.render_text());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
