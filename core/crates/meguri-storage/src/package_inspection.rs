//! P12's gate on a private, extracted candidate. Never enables managed metadata.
//! The caller owns the staging directory and keeps the path exclusive throughout.
use crate::schema::{
    SCHEMA_VERSION, refuse,
    version::{self, ProbeError},
};
use crate::{OpenError, OpenOptions, ReadRows, StorageError, open_collection};
use meguri_model::{clock::Clock, rng::RandomSource};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateFacts {
    pub ver_found: i64,
    pub notes: u64,
    pub cards: u64,
    pub revlog: u64,
    pub scm: i64,
    pub modified: i64,
    pub media_count: u64,
    pub media_decoded_bytes: u64,
}

#[derive(Debug)]
pub enum CandidateError {
    Gate(OpenError),
    Storage(StorageError),
    InsufficientSpace { needed: u64, available: u64 },
    Corrupt { detail: String },
    DiskFull,
    Cancelled,
}
impl std::fmt::Display for CandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CandidateError {}
impl From<StorageError> for CandidateError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::InsufficientSpace { .. } => Self::DiskFull,
            other => Self::Storage(other),
        }
    }
}
impl From<rusqlite::Error> for CandidateError {
    fn from(error: rusqlite::Error) -> Self {
        StorageError::from(error).into()
    }
}

pub struct CandidateContext<'a> {
    pub clock: &'a dyn Clock,
    pub random: &'a dyn RandomSource,
    pub space: &'a dyn crate::space::SpaceProvider,
    pub is_cancelled: &'a (dyn Fn() -> bool + Sync),
}
fn cancelled(context: &CandidateContext<'_>) -> Result<(), CandidateError> {
    if (context.is_cancelled)() {
        Err(CandidateError::Cancelled)
    } else {
        Ok(())
    }
}
fn gate(error: OpenError) -> CandidateError {
    // OpenError predates the capacity vocabulary and retains only text for
    // SQLite failures, including those from upgrade::upgrade. Preserve its
    // typed schema refusals, and recover SQLite's stable FULL diagnostic.
    let detail = match &error {
        OpenError::Io { detail } | OpenError::UpgradeFailed { detail, .. } => Some(detail),
        _ => None,
    };
    let storage_full = std::io::Error::from(std::io::ErrorKind::StorageFull).to_string();
    if detail.is_some_and(|s| {
        s.contains("database or disk is full")
            || s.contains("No space left on device")
            || s.contains("Disk quota exceeded")
            || s.contains(&storage_full)
    }) {
        CandidateError::DiskFull
    } else {
        CandidateError::Gate(error)
    }
}

/// Bounds the probe values, runs the probe/refusals, then P10's upgrade-copy preflight,
/// the actual open gate, integrity with unicase, and the preview queries.
/// All SQLite connections close before returning. No package/R7 stamps.
pub fn inspect_candidate(path: &Path, context: &CandidateContext<'_>) -> Result<CandidateFacts, CandidateError> {
    cancelled(context)?;
    let probed = bounded_probe(path)?;
    refuse::refuse(&probed).map_err(gate)?;
    if probed.version < SCHEMA_VERSION {
        let needed = std::fs::metadata(path).map_err(StorageError::from)?.len();
        let directory = path.parent().ok_or_else(|| CandidateError::Corrupt {
            detail: "candidate directory".into(),
        })?;
        crate::space::require_space(context.space, directory, needed).map_err(|error| match error {
            StorageError::InsufficientSpace { required, available } => CandidateError::InsufficientSpace {
                needed: required,
                available,
            },
            other => other.into(),
        })?;
    }
    cancelled(context)?;
    let opened = open_collection(path, &OpenOptions::new(context.clock, context.random)).map_err(gate)?;
    cancelled(context)?;
    let facts = {
        let conn = &opened.writer().conn;
        let mut statement = conn.prepare("PRAGMA main.integrity_check")?;
        let mut rows = statement.query([])?;
        let first = rows.next()?.map(|r| r.get::<_, String>(0)).transpose()?;
        if first.as_deref() != Some("ok") || rows.next()?.is_some() {
            return Err(CandidateError::Corrupt {
                detail: "candidate integrity_check".into(),
            });
        }
        cancelled(context)?;
        let count = |table: &str| -> Result<u64, CandidateError> {
            cancelled(context)?;
            // Only storage-authored constant table names reach this query.
            let count: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
            nonnegative_count(count)
        };
        let col = opened.writer().col()?;
        CandidateFacts {
            ver_found: probed.version,
            notes: count("notes")?,
            cards: count("cards")?,
            revlog: count("revlog")?,
            scm: col.schema_modified.0,
            modified: col.modified.0,
            media_count: 0,
            media_decoded_bytes: 0,
        }
    };
    let closed = opened.close()?;
    if !closed.succeeded() {
        return Err(CandidateError::Corrupt {
            detail: "candidate checkpoint busy".into(),
        });
    }
    cancelled(context)?;
    Ok(facts)
}

fn nonnegative_count(count: i64) -> Result<u64, CandidateError> {
    u64::try_from(count).map_err(|_| CandidateError::Corrupt {
        detail: "candidate row count is negative".into(),
    })
}

/// Runs P8's metadata-only size checks before the usual version/scheduler
/// probe. The caller must keep the candidate private and stable through all
/// later gate/open operations. Suitable for a future replacement consumer.
/// Returns the detected version without applying schema/scheduler refusals or
/// upgrading. Its read-only SQLite connection closes before returning.
pub fn probe_candidate(path: &Path) -> Result<i64, CandidateError> {
    bounded_probe(path).map(|probed| probed.version)
}

fn bounded_probe(path: &Path) -> Result<version::Probed, CandidateError> {
    version::probe_candidate(path).map_err(|error| match error {
        ProbeError::Open(error) => gate(error),
        ProbeError::HotJournal => CandidateError::Corrupt {
            detail: "fresh package candidate has a hot journal".into(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_conversion_rejects_negative_and_keeps_nonnegative_range() {
        assert!(matches!(nonnegative_count(-1), Err(CandidateError::Corrupt { .. })));
        assert_eq!(nonnegative_count(0).unwrap(), 0);
        assert_eq!(nonnegative_count(i64::MAX).unwrap(), u64::try_from(i64::MAX).unwrap());
    }
}

/// Synthetic mutations of copied test candidates. Collection SQL stays in
/// storage even when a package integration test needs a malformed database.
#[cfg(feature = "test-hooks")]
pub mod testing {
    use rusqlite::{Connection, types::Value};
    use std::path::Path;

    pub enum ProbeMutation {
        MissingVersionColumn,
        MissingConfigValueColumn,
        SchedulerValue { bytes: usize, blob: bool },
        UnrelatedConfigValue { bytes: usize },
    }

    pub fn mutate_probe_candidate(path: &Path, mutation: ProbeMutation) {
        let conn = Connection::open(path).unwrap();
        crate::conn_init::probe(&conn).unwrap();
        match mutation {
            ProbeMutation::MissingVersionColumn => {
                conn.execute_batch("DROP TABLE col; CREATE TABLE col (id INTEGER)")
                    .unwrap();
            }
            ProbeMutation::MissingConfigValueColumn => {
                conn.execute_batch("DROP TABLE config; CREATE TABLE config (key TEXT)")
                    .unwrap();
            }
            ProbeMutation::SchedulerValue { bytes, blob } => {
                let version: i64 = conn.query_row("SELECT ver FROM col LIMIT 1", [], |r| r.get(0)).unwrap();
                let prefix = if version == 11 {
                    &b"{\"schedVer\":2}"[..]
                } else {
                    &b"2"[..]
                };
                let mut value = vec![b' '; bytes];
                value[..prefix.len()].copy_from_slice(prefix);
                let value = if blob {
                    Value::Blob(value)
                } else {
                    Value::Text(String::from_utf8(value).unwrap())
                };
                let sql = if version == 11 {
                    "UPDATE col SET conf = ?1"
                } else {
                    "UPDATE config SET val = ?1 WHERE key = 'schedVer'"
                };
                assert_eq!(conn.execute(sql, [value]).unwrap(), 1);
            }
            ProbeMutation::UnrelatedConfigValue { bytes } => {
                conn.execute("INSERT INTO config (key, usn, mtime_secs, val) VALUES ('inspection-test-extra', 0, 0, zeroblob(?1))", [i64::try_from(bytes).unwrap()]).unwrap();
            }
        }
    }
}
