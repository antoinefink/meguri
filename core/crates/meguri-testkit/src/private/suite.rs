//! The private suite's runner and summary (U15 plumbing for R1942, R1943).
//!
//! A [`Check`] runs on a vault copy and returns pass and fail counts. The
//! summary records only those counts per check name (R1940): a check's error
//! text may quote the collection, so it goes to the private log the caller
//! names, never into the summary. Check names are fixed identifiers
//! (lower-case letters, digits and `-`); optional numeric breakdowns use
//! fixed category identifiers too. The checks live in `meguri-cli`'s
//! `private_suite` (U63 IO round trip, then U249's remaining checks).

use std::collections::BTreeMap;
use std::path::Path;

/// Pass and fail counts of one check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckCounts {
    pub passed: u64,
    pub failed: u64,
    /// Optional numeric categories with fixed, program-defined names.
    /// IO uses ErrorKind names for strict-reader refusals.
    pub breakdown: BTreeMap<&'static str, u64>,
}

/// One check of the private suite.
pub trait Check {
    /// A fixed identifier: lower-case ASCII letters, digits and `-`.
    fn name(&self) -> &'static str;
    /// Runs on the copy in `copy` (a folder holding `collection.anki2` and
    /// `collection.media/`). An `Err` is a check that could not run; its text
    /// may quote the collection.
    fn run(&self, copy: &Path) -> Result<CheckCounts, String>;
}

/// What one check gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Counts(CheckCounts),
    /// The check could not run.
    Error,
}

/// The suite's summary: one outcome per check name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub checks: BTreeMap<&'static str, Outcome>,
}

/// The version of [`Summary::to_json`]'s layout.
pub const REPORT_VERSION: u32 = 1;

/// Whether `name` is a valid check name.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Runs `checks` in order on `copy`. `log` receives each failed check's name
/// and error text; the caller keeps it under `build/private/`.
///
/// # Panics
///
/// When a check's name or a breakdown category is invalid, or two checks
/// share a name.
pub fn run(checks: &[&dyn Check], copy: &Path, log: &mut dyn FnMut(&str, &str)) -> Summary {
    let mut summary = Summary::default();
    for check in checks {
        let name = check.name();
        assert!(valid_name(name), "check name {name:?} is not [a-z0-9-]+");
        let outcome = match check.run(copy) {
            Ok(counts) => {
                for key in counts.breakdown.keys() {
                    assert!(
                        !key.is_empty()
                            && key
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'),
                        "breakdown key {key:?} is not [a-z0-9_-]+"
                    );
                }
                Outcome::Counts(counts)
            }
            Err(text) => {
                log(name, &text);
                Outcome::Error
            }
        };
        assert!(
            summary.checks.insert(name, outcome).is_none(),
            "two checks are named {name:?}"
        );
    }
    summary
}

impl Summary {
    /// Every check ran and failed nothing. An empty suite passes.
    pub fn passed(&self) -> bool {
        self.checks
            .values()
            .all(|outcome| matches!(outcome, Outcome::Counts(c) if c.failed == 0))
    }

    /// `{"checks": {<name>: {"failed": n, "passed": n} | {"error": true}},
    /// "report_version": 1}`, keys sorted. Counts with a breakdown also
    /// carry `"breakdown": {<fixed category>: n}`.
    pub fn to_json(&self) -> serde_json::Value {
        let checks: serde_json::Map<String, serde_json::Value> = self
            .checks
            .iter()
            .map(|(name, outcome)| {
                let value = match outcome {
                    Outcome::Counts(c) => {
                        let mut value = serde_json::json!({"failed": c.failed, "passed": c.passed});
                        if !c.breakdown.is_empty() {
                            value["breakdown"] = serde_json::json!(c.breakdown);
                        }
                        value
                    }
                    Outcome::Error => serde_json::json!({"error": true}),
                };
                ((*name).to_owned(), value)
            })
            .collect();
        serde_json::json!({"checks": checks, "report_version": REPORT_VERSION})
    }
}
