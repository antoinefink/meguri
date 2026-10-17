//! The protocol (T3) and the measurement (T4): for one operation, a warm-up
//! run and `repeat` timed runs, each on a fresh copy of the image, timed
//! around the operation's timed part only; the samples, their statistic and
//! the check every run must return.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::image;
use super::run::Session;
use crate::wallclock::Stopwatch;

/// An operation's result check (T6): a JSON object with exactly the members
/// its catalogue entry lists.
pub type Check = Map<String, Value>;

/// T7.2 error kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The collection lacks what the operation needs.
    NoInput,
    /// A call of the run failed.
    Failed,
    /// A run's check differed from the first run's (T3.6).
    Unstable,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NoInput => "no_input",
            ErrorKind::Failed => "failed",
            ErrorKind::Unstable => "unstable",
        }
    }
}

/// Where in a run an operation failed (T7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Setup,
    Timed,
    Check,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Setup => "setup",
            Stage::Timed => "timed",
            Stage::Check => "check",
        }
    }
}

/// Why a call of a run failed. `exception` names what failed, a type or
/// variant name such as `EngineError::Unavailable`, never a message:
/// messages may quote collection content (R1940) and are localized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunFailure {
    pub kind: ErrorKind,
    pub exception: Option<String>,
}

impl RunFailure {
    /// The collection lacks what the operation needs (T6.4: an empty queue).
    pub fn no_input() -> RunFailure {
        RunFailure {
            kind: ErrorKind::NoInput,
            exception: None,
        }
    }

    /// A call failed with the error named `exception`.
    pub fn failed(exception: impl Into<String>) -> RunFailure {
        RunFailure {
            kind: ErrorKind::Failed,
            exception: Some(exception.into()),
        }
    }
}

/// What one run of an operation works on.
#[derive(Debug)]
pub struct RunContext<'a> {
    pub session: &'a Session,
    /// The run directory (T3.2), deleted with everything in it at teardown.
    pub dir: &'a Path,
    /// The run's fresh copy of the image, `collection.anki2` in `dir`.
    pub collection: &'a Path,
}

/// One run of an operation, made by [`TimedOperation::start`] for a fresh
/// copy. Dropping it releases what it holds (its open collection).
pub trait TimedRun {
    /// T3.2 step 2: everything before the timed interval.
    fn setup(&mut self) -> Result<(), RunFailure>;
    /// T4.1: the timed part, from its first call until the side holds the
    /// complete result and any work the calls started has finished.
    fn timed(&mut self) -> Result<(), RunFailure>;
    /// T3.2 step 4: the operation's check.
    fn check(&mut self) -> Result<Check, RunFailure>;
    /// T3.2 step 5: closes what the run opened. A failure is reported at
    /// stage `check`.
    fn close(self: Box<Self>) -> Result<(), RunFailure>;
}

/// An operation a side can run.
pub trait TimedOperation {
    /// A run on `run`'s copy; nothing of the operation happens yet.
    fn start<'a>(&'a self, run: &'a RunContext<'a>) -> Box<dyn TimedRun + 'a>;
}

/// An operation's result (T7.2).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Every run returned `check`; `samples_us` are the timed runs'
    /// intervals in whole microseconds, in the order taken.
    Ok { samples_us: Vec<u64>, check: Check },
    /// The side has no implementation: nothing ran.
    Unsupported,
    Error {
        kind: ErrorKind,
        stage: Stage,
        exception: Option<String>,
    },
}

impl Outcome {
    /// T4.3 `median_ms`, in microseconds: the sample at position ⌈n/2⌉ in
    /// ascending order (the lower middle one for an even n).
    pub fn median_us(&self) -> Option<u64> {
        let Outcome::Ok { samples_us, .. } = self else {
            return None;
        };
        let mut sorted = samples_us.clone();
        sorted.sort_unstable();
        sorted.get(sorted.len().div_ceil(2).checked_sub(1)?).copied()
    }

    /// T4.3 `min_ms`, in microseconds.
    pub fn min_us(&self) -> Option<u64> {
        match self {
            Outcome::Ok { samples_us, .. } => samples_us.iter().min().copied(),
            _ => None,
        }
    }

    /// The T7.2 members other than `name`.
    pub fn to_json(&self) -> Map<String, Value> {
        let mut out = Map::new();
        match self {
            Outcome::Ok { samples_us, check } => {
                out.insert("status".into(), json!("ok"));
                out.insert(
                    "samples_ms".into(),
                    Value::Array(samples_us.iter().map(|&us| ms(us)).collect()),
                );
                let (median, min) = (self.median_us(), self.min_us());
                if let (Some(median), Some(min)) = (median, min) {
                    out.insert("statistic".into(), json!({"median_ms": ms(median), "min_ms": ms(min)}));
                }
                out.insert("check".into(), Value::Object(check.clone()));
            }
            Outcome::Unsupported => {
                out.insert("status".into(), json!("unsupported"));
            }
            Outcome::Error { kind, stage, exception } => {
                out.insert("status".into(), json!("error"));
                let mut error = Map::new();
                error.insert("kind".into(), json!(kind.as_str()));
                error.insert("stage".into(), json!(stage.as_str()));
                if let Some(exception) = exception {
                    error.insert("exception".into(), json!(exception));
                }
                out.insert("error".into(), Value::Object(error));
            }
        }
        out
    }
}

/// T4.2: a sample in milliseconds with 3 decimals, from whole microseconds.
pub fn ms(us: u64) -> Value {
    json!(us as f64 / 1000.0)
}

/// T4.2: an interval rounded to the nearest microsecond, halves up.
pub fn micros(interval: Duration) -> u64 {
    u64::try_from(interval.as_nanos().saturating_add(500) / 1000).unwrap_or(u64::MAX)
}

/// Runs `op` once as a warm-up and `repeat` times timed (T3.2), each run on
/// a fresh copy of `session`'s image. The first failing run ends the
/// operation.
pub fn time_operation(op: &dyn TimedOperation, session: &Session, repeat: u32) -> Outcome {
    let mut samples_us = Vec::new();
    let mut first: Option<Check> = None;
    for index in 0..=repeat {
        let dir = session.run_dir(index);
        let result = one_run(op, session, &dir);
        let removed = fs::remove_dir_all(&dir);
        let (interval, check) = match (result, removed) {
            (Err(error), _) => return error,
            (Ok(_), Err(_)) => return error(ErrorKind::Failed, Stage::Check, Some("io")),
            (Ok(done), Ok(())) => done,
        };
        match &first {
            None => first = Some(check),
            Some(expected) if *expected != check => return error(ErrorKind::Unstable, Stage::Check, None),
            Some(_) => {}
        }
        // Run 0 is the warm-up: measured and checked, its sample discarded.
        if index > 0 {
            samples_us.push(micros(interval));
        }
    }
    Outcome::Ok {
        samples_us,
        check: first.unwrap_or_default(),
    }
}

fn error(kind: ErrorKind, stage: Stage, exception: Option<&str>) -> Outcome {
    Outcome::Error {
        kind,
        stage,
        exception: exception.map(str::to_owned),
    }
}

/// Copy, setup, the timed interval, the check and the close of one run.
fn one_run(op: &dyn TimedOperation, session: &Session, dir: &Path) -> Result<(Duration, Check), Outcome> {
    let collection = dir.join(image::COLLECTION);
    fs::create_dir_all(dir)
        .and_then(|()| image::copy_file(session.image(), &collection))
        .map_err(|_| error(ErrorKind::Failed, Stage::Setup, Some("io")))?;
    let context = RunContext {
        session,
        dir,
        collection: &collection,
    };
    let mut run = guarded(Stage::Setup, || Ok(op.start(&context)))?;
    guarded(Stage::Setup, || run.setup())?;
    let interval = guarded(Stage::Timed, || {
        let watch = Stopwatch::start();
        run.timed()?;
        Ok(watch.elapsed())
    })?;
    let check = guarded(Stage::Check, || run.check())?;
    guarded(Stage::Check, move || run.close())?;
    Ok((interval, check))
}

/// Calls `call` at `stage`; a failure or a panic is the operation's error.
fn guarded<T>(stage: Stage, call: impl FnOnce() -> Result<T, RunFailure>) -> Result<T, Outcome> {
    match panic::catch_unwind(AssertUnwindSafe(call)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(failure)) => Err(Outcome::Error {
            kind: failure.kind,
            stage,
            exception: failure.exception,
        }),
        Err(_) => Err(error(ErrorKind::Failed, stage, Some("panic"))),
    }
}
