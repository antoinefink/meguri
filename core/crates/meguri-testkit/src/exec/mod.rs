//! Executors run scenarios and return their observations (dsl.md D8). The
//! oracle executor drives oracle workers; Meguri's runs the core in-process
//! through per-area adapters registered in a capabilities registry.

pub mod clock;
mod meguri;
mod oracle;

use std::fmt;

use serde_json::Value;

use crate::scenario::LoadedScenario;
pub use crate::scenario::Needs;
pub use meguri::{MeguriExecutor, Registry, Session};
pub use oracle::{Hello, OracleExecutor};

/// A failed run: a protocol error kind (`unknown_ref`, `invalid_scenario`,
/// `unsupported_op`, `anki_error`, `worker_died`, ...), the step when known and
/// text for people, which is never compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecError {
    pub kind: String,
    pub step: Option<usize>,
    pub detail: String,
}

impl ExecError {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>) -> ExecError {
        ExecError {
            kind: kind.into(),
            step: None,
            detail: detail.into(),
        }
    }

    pub fn at_step(mut self, step: usize) -> ExecError {
        self.step.get_or_insert(step);
        self
    }
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(step) = self.step {
            write!(f, " at step {step}")?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for ExecError {}

/// Something that runs scenarios.
pub trait Executor: Sync {
    /// `meguri` or `oracle`.
    fn name(&self) -> &str;

    /// What of `needs` this executor cannot do; empty when it can run it.
    fn missing(&self, needs: &Needs) -> Needs;

    /// Runs the scenario on a fresh collection: `{"steps": [...]}`.
    fn run(&self, scenario: &LoadedScenario) -> Result<Value, ExecError>;
}
