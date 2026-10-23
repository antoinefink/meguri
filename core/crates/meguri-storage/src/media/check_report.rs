use super::{MediaError, MediaStore, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// T6 wall-clock instants are milliseconds supplied by the store's Clock.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCheckReport {
    pub started_at: i64,
    pub finished_at: i64,
    pub completed: bool,
    pub listed: bool,
    pub seen: u64,
    pub hashed: u64,
    pub added: u64,
    pub changed: u64,
    pub removed: u64,
    pub renamed: u64,
    pub skipped: u64,
}
impl MediaCheckReport {
    pub(super) fn started(now: i64) -> Self {
        Self {
            started_at: now,
            finished_at: now,
            ..Self::default()
        }
    }
}
impl MediaStore {
    pub(super) fn finish_check_report(&self, report: &mut MediaCheckReport) -> Result<()> {
        report.finished_at = self.inner.clock.now_ms();
        let value = serde_json::to_string(report).map_err(|e| MediaError::Database(e.to_string()))?;
        self.db()?.execute(
            "INSERT INTO meta VALUES('last_check_report',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![value],
        )?;
        Ok(())
    }
    pub fn last_check_report(&self) -> Result<Option<MediaCheckReport>> {
        let value: Option<String> = self
            .db()?
            .query_row("SELECT value FROM meta WHERE key='last_check_report'", [], |r| r.get(0))
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s).map_err(|e| MediaError::Database(e.to_string())))
            .transpose()
    }
    /// Full-precision directory marker, for diagnostics; never a transfer grant.
    pub fn change_marker(&self) -> Result<Option<String>> {
        super::journal_db::marker(&*self.db()?)
    }
}
