//! A timing run (T1, T2): its operations, clock, zone and repetitions, the
//! scratch directory with the image, the collection summary, then each
//! operation through the side's implementation, or `unsupported`.

use std::cell::OnceCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use meguri_model::clock::Clock;
use serde_json::Value;
use tempfile::TempDir;

use super::catalogue::{self, Operation};
use super::image::{self, COLLECTION, ImageError, Summary};
use super::package;
use super::protocol::{Outcome, TimedOperation, time_operation};
use super::report::{self, MeguriBuild, OperationEntry, Report};
use super::stub::SyncStub;
use crate::exec::clock::ScenarioClock;
use crate::scenario::DateTimeText;

/// T1's default `--repeat`.
pub const DEFAULT_REPEAT: u32 = 5;
/// T1's largest `--repeat`.
pub const MAX_REPEAT: u32 = 100;

/// What `meguri-cli timing run` was asked (T1, T10).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimingRequest {
    /// `<dir>`: `collection.anki2`, maybe its `-wal` and `manifest.json`.
    /// Nothing in it is written.
    pub dir: PathBuf,
    /// `--op` names as given.
    pub ops: Vec<String>,
    pub now: Option<String>,
    pub tz: Option<String>,
    pub repeat: Option<String>,
    /// Where the run's scratch directory is made (T3.4: the Mac's local
    /// disk, or a tmpfs in Docker; under `build/private/` for a vault
    /// copy, T2.1).
    pub scratch_root: PathBuf,
}

/// A run that ends without a report (T8's kinds that apply to Meguri).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    Usage(String),
    NoCollection(String),
    OpenFailed(String),
    /// Any other failure of the run itself.
    Failed(String),
}

impl RunError {
    pub fn kind(&self) -> &'static str {
        match self {
            RunError::Usage(_) => "usage",
            RunError::NoCollection(_) => "no_collection",
            RunError::OpenFailed(_) => "open_failed",
            RunError::Failed(_) => "failed",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            RunError::Usage(d) | RunError::NoCollection(d) | RunError::OpenFailed(d) | RunError::Failed(d) => d,
        }
    }
}

/// T1, settled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// In catalogue order, each once.
    pub ops: Vec<Operation>,
    /// The frozen instant, epoch seconds.
    pub now: i64,
    pub tz: String,
    pub repeat: u32,
}

/// T1: the operations, the repetitions, and `now` and `tz` as given or
/// from `<dir>/manifest.json` (`collection-check.md` K1).
pub fn plan(request: &TimingRequest) -> Result<Plan, RunError> {
    let ops = catalogue::select(&request.ops).map_err(|name| RunError::Usage(format!("unknown operation {name:?}")))?;
    let repeat = match &request.repeat {
        None => DEFAULT_REPEAT,
        Some(text) => text
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=MAX_REPEAT).contains(n))
            .ok_or_else(|| RunError::Usage(format!("--repeat {text:?}: an integer from 1 to {MAX_REPEAT}")))?,
    };
    let manifest = manifest(&request.dir);
    let now = match &request.now {
        Some(text) => parse_now(text)?,
        None => manifest
            .as_ref()
            .and_then(|m| m.get("now_ms")?.as_i64())
            .map(|ms| ms.div_euclid(1000))
            .ok_or_else(|| RunError::Usage("--now is needed: <dir> has no manifest.json with now_ms".to_owned()))?,
    };
    let tz = match &request.tz {
        Some(zone) => {
            jiff::tz::db()
                .get(zone)
                .map_err(|_| RunError::Usage(format!("--tz {zone:?}: no such time zone")))?;
            zone.clone()
        }
        None => {
            let offset = manifest.as_ref().and_then(|m| m.get("utc_offset_secs")?.as_i64());
            offset.and_then(manifest_zone).ok_or_else(|| {
                RunError::Usage(match offset {
                    Some(o) => format!("--tz is needed: the manifest's utc_offset_secs {o} has no Etc zone"),
                    None => "--tz is needed: <dir> has no manifest.json with utc_offset_secs".to_owned(),
                })
            })?
        }
    };
    Ok(Plan { ops, now, tz, repeat })
}

fn manifest(dir: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(dir.join("manifest.json")).ok()?).ok()
}

/// `--now`: `clock.start` with an offset or `Z`, whole seconds (dsl.md D5).
fn parse_now(text: &str) -> Result<i64, RunError> {
    let usage = || {
        RunError::Usage(format!(
            "--now {text:?}: a date-time such as 2030-01-15T10:00:00Z, with Z or an offset"
        ))
    };
    let checked: DateTimeText = text.parse().map_err(|_| usage())?;
    if checked.as_str().len() == 19 {
        return Err(usage());
    }
    let instant: Timestamp = checked.as_str().parse().map_err(|_| usage())?;
    Ok(instant.as_second())
}

/// K1: `UTC` for 0; for a whole number of hours from -12 to +14, the `Etc`
/// zone (IANA's inverted sign: +1 h is `Etc/GMT-1`).
pub fn manifest_zone(offset_secs: i64) -> Option<String> {
    if offset_secs == 0 {
        return Some("UTC".to_owned());
    }
    if offset_secs % 3600 != 0 || !(-12 * 3600..=14 * 3600).contains(&offset_secs) {
        return None;
    }
    let hours = offset_secs / 3600;
    Some(if hours > 0 {
        format!("Etc/GMT-{hours}")
    } else {
        format!("Etc/GMT+{}", -hours)
    })
}

/// A run's scratch directory, image, clock and shared inputs. Dropping it
/// stops the stub and deletes the scratch directory with everything in it
/// (T2.1).
#[derive(Debug)]
pub struct Session {
    image: PathBuf,
    runs: PathBuf,
    now: i64,
    tz: String,
    clock: Arc<ScenarioClock>,
    package: OnceCell<PathBuf>,
    stub: OnceCell<SyncStub>,
    // Last: deleted once the stub has stopped.
    scratch: TempDir,
}

impl Session {
    /// A scratch directory in `scratch_root` and the image of `dir`'s
    /// collection in it (T2.1), with the clock frozen at `now` in `tz`.
    pub fn prepare(dir: &Path, scratch_root: &Path, now: i64, tz: &str) -> Result<Session, RunError> {
        let failed = |what: &str, e: io::Error| RunError::Failed(format!("{what}: {e}"));
        fs::create_dir_all(scratch_root).map_err(|e| failed("making the scratch root", e))?;
        let scratch = tempfile::Builder::new()
            .prefix("meguri-timing-")
            .tempdir_in(scratch_root)
            .map_err(|e| failed("making the scratch directory", e))?;
        let image = image::make_image(dir, &scratch.path().join("image")).map_err(|e| match e {
            ImageError::Io(e) => failed("copying the collection", e),
            ImageError::Unreadable(detail) => RunError::OpenFailed(detail),
        })?;
        let start = Timestamp::from_second(now)
            .map_err(|_| RunError::Usage(format!("{now} is not an instant")))?
            .to_string();
        let clock = ScenarioClock::new(tz, &start).map_err(|e| RunError::Usage(e.to_string()))?;
        Ok(Session {
            image,
            runs: scratch.path().join("runs"),
            now,
            tz: tz.to_owned(),
            clock: Arc::new(clock),
            package: OnceCell::new(),
            stub: OnceCell::new(),
            scratch,
        })
    }

    /// The frozen instant, epoch seconds.
    pub fn now(&self) -> i64 {
        self.now
    }

    pub fn tz(&self) -> &str {
        &self.tz
    }

    /// The clock frozen at `now` in `tz` for the whole run (T2.3).
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock) as Arc<dyn Clock>
    }

    /// The image, which no run ever opens: runs get copies.
    pub fn image(&self) -> &Path {
        &self.image
    }

    /// The directory of run `index` (0 is the warm-up).
    pub fn run_dir(&self, index: u32) -> PathBuf {
        self.runs.join(index.to_string())
    }

    /// T2.2: the row counts of a copy of the image.
    pub fn summary(&self) -> Result<Summary, RunError> {
        image::summarize(&self.image, &self.scratch.path().join("summary"), self.clock.as_ref())
            .map_err(RunError::OpenFailed)
    }

    /// T5.4: the import package, built from the image the first time an
    /// operation asks for it.
    pub fn package(&self) -> io::Result<&Path> {
        if let Some(path) = self.package.get() {
            return Ok(path);
        }
        let dir = self.scratch.path().join("package");
        fs::create_dir_all(&dir)?;
        let path = dir.join("import.apkg");
        package::write_import_package(&self.image, &path)?;
        Ok(self.package.get_or_init(|| path))
    }

    /// T5.5: the sync stub, started the first time an operation asks for
    /// it; its download content is the image's bytes.
    pub fn stub(&self) -> io::Result<&SyncStub> {
        if let Some(stub) = self.stub.get() {
            return Ok(stub);
        }
        let stub = SyncStub::start(&fs::read(&self.image)?)?;
        Ok(self.stub.get_or_init(|| stub))
    }
}

/// T1, T2 and T3 to T7 for one side: `side` gives each operation's
/// implementation, `None` for one it cannot run (`unsupported`, T10).
pub fn run<'s>(
    request: &TimingRequest,
    side: &dyn Fn(Operation) -> Option<&'s dyn TimedOperation>,
    build: MeguriBuild,
) -> Result<Report, RunError> {
    if !request.dir.join(COLLECTION).is_file() {
        return Err(RunError::NoCollection(format!(
            "{} does not exist",
            request.dir.join(COLLECTION).display()
        )));
    }
    let plan = plan(request)?;
    let platform =
        report::platform().ok_or_else(|| RunError::Failed("the oracle has no lane for this platform".to_owned()))?;
    let session = Session::prepare(&request.dir, &request.scratch_root, plan.now, &plan.tz)?;
    let collection = session.summary()?;
    let operations = plan
        .ops
        .iter()
        .map(|&op| OperationEntry {
            op,
            outcome: match side(op) {
                Some(implementation) => time_operation(implementation, &session, plan.repeat),
                None => Outcome::Unsupported,
            },
        })
        .collect();
    Ok(Report {
        platform,
        build,
        repeat: plan.repeat,
        now: plan.now,
        tz: plan.tz,
        collection,
        operations,
    })
}
