//! Checks for the Meguri Rust workspace:
//! - [`layering`]: the KTD2 crate layering in `edges.toml` (allowed edges,
//!   crates restricted to a few workspace crates, the engine's raw open entry
//!   point, the storage-only builder), run by `tools/mg core-lint`;
//! - [`pins`]: the parity pins in `core/pins.toml` against `Cargo.toml` and
//!   `Cargo.lock`, run first by `tools/mg core-test` as the pin canary.

use std::fmt;

pub mod edges;
pub mod layering;
pub mod metadata;
pub mod pins;
mod raw_open;

/// The rule a [`Violation`] breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// A workspace crate depends on a workspace crate edges.toml does not allow.
    Edge,
    /// A workspace crate uses a restricted third-party crate (tokio, axum, reqwest).
    Restricted,
    /// A crate other than the engine and meguri-host names the raw open entry point.
    RawOpen,
    /// The engine or the FFI names the storage-only builder.
    StorageBuilder,
    /// A workspace crate edges.toml does not place.
    UnknownCrate,
    /// edges.toml names a crate that is not in the workspace.
    StaleEntry,
    /// A parity crate differs from its pin.
    Pin,
    /// A crate depends on a parity crate whose pin has no version yet.
    PendingPin,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Edge => "edge",
            Rule::Restricted => "restricted",
            Rule::RawOpen => "raw-open",
            Rule::StorageBuilder => "storage-builder",
            Rule::UnknownCrate => "unknown-crate",
            Rule::StaleEntry => "stale-entry",
            Rule::Pin => "pin",
            Rule::PendingPin => "pending-pin",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One broken rule. `subject` identifies it within its rule: a crate, an edge
/// such as `meguri-sched -> meguri-engine`, or a source location.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub rule: Rule,
    pub subject: String,
    pub message: String,
}

impl Violation {
    pub(crate) fn new(rule: Rule, subject: impl Into<String>, message: impl Into<String>) -> Violation {
        Violation {
            rule,
            subject: subject.into(),
            message: message.into(),
        }
    }

    /// One JSON object: `{"rule": .., "subject": .., "message": ..}`.
    pub fn to_json(&self) -> String {
        serde_json::json!({ "rule": self.rule.as_str(), "subject": self.subject, "message": self.message }).to_string()
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.rule, self.message)
    }
}

/// A check could not run: unreadable or invalid input, or cargo failed.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Error {
        Error(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
