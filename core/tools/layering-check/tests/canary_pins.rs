//! Pin canary (R2027, AE82 ordering). `tools/mg core-test` runs every
//! `canary_*` test binary before the other tests and stops when one fails, so
//! a parity crate that drifted from the oracle's lockfile is the first failure.

use std::path::PathBuf;

use layering_check::pins;

#[test]
fn parity_pins_match_the_oracle_lockfile() {
    let core = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = pins::check_workspace(&core).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        report.violations.is_empty(),
        "parity pins drifted from core/pins.toml:\n{}",
        report.render()
    );
}
