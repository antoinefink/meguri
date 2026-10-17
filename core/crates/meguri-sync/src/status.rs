//! The `meta` status check, the sync status and the meta probe
//! (specs/sync/meta-status.md; R1513-R1518, R1559): what a sync job sends
//! first, what it refuses (`cont` false, a clock more than 300 s off), how it
//! compares the two collections, and the status the app's indicator shows,
//! answered offline when the local stamps decide, from an answer remembered
//! for 300 s, or from the server.

use meguri_model::clock::Clock;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, Table};

use crate::endpoint::Endpoint;
use crate::error::{ErrorKind, JobError};
use crate::job::{HostKey, Job};
use crate::transport::{Method, Payload};
use crate::wire::meta::{ServerMeta, decode_meta, encode_meta_request};

/// The largest clock offset, either way, a check accepts (M2, R1515).
pub const MAX_CLOCK_OFFSET_SECS: i64 = 300;

/// How long a remembered answer serves the status (M6.1).
pub const REMEMBER_SECS: i64 = 300;

/// The local values a check compares with the server's (M1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalRecord {
    /// `col.mod`.
    pub modified: EpochMillis,
    /// `col.scm`.
    pub schema_modified: EpochMillis,
    /// `col.usn`: the server's update sequence number as of the last sync.
    pub usn: Usn,
    /// `col.ls`: the last sync, 0 when the collection never synced.
    pub last_sync: EpochMillis,
    /// The `cards` table has a row.
    pub has_cards: bool,
}

impl LocalRecord {
    /// Reads the record from an open collection (the `col` row and whether a
    /// card exists). Reads only.
    pub fn read(rows: &impl ReadRows) -> Result<LocalRecord, JobError> {
        let col = rows.col().map_err(|err| JobError::from_storage(&err))?;
        let has_cards = !rows
            .ids_after(Table::Cards, None, 1)
            .map_err(|err| JobError::from_storage(&err))?
            .is_empty();
        Ok(LocalRecord {
            modified: col.modified,
            schema_modified: col.schema_modified,
            usn: col.usn,
            last_sync: col.last_sync,
            has_cards,
        })
    }

    /// M5: the status the local `col` row tells alone, with no network.
    pub fn offline_status(&self) -> OfflineStatus {
        if self.last_sync.0 == 0 {
            OfflineStatus::Unknown
        } else if self.schema_modified > self.last_sync {
            OfflineStatus::OneWayNeeded
        } else if self.modified > self.last_sync {
            OfflineStatus::PendingChanges
        } else {
            OfflineStatus::NothingPending
        }
    }
}

/// M5's four answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineStatus {
    /// The collection never synced (`ls` 0): only the server can tell.
    Unknown,
    /// `scm` is past `ls`.
    OneWayNeeded,
    /// `mod` is past `ls`.
    PendingChanges,
    NothingPending,
}

/// What comparing the two records decides (M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// Equal `mod`: nothing to sync.
    NoChanges,
    /// A normal sync; `local_is_newer` is `lnewer` in `start` (W4).
    Normal { local_is_newer: bool },
    /// The schema times differ: a one-way sync, with the directions the two
    /// sides allow. Both true means the user chooses.
    OneWay { upload: bool, download: bool },
}

/// M3: the first matching row decides. Equal `mod` wins over differing
/// `scm`.
pub fn compare(local: &LocalRecord, server: &ServerMeta) -> CheckResult {
    if server.modified == local.modified {
        CheckResult::NoChanges
    } else if server.schema_modified != local.schema_modified {
        CheckResult::OneWay {
            upload: local.has_cards || server.empty,
            download: !server.empty || !local.has_cards,
        }
    } else {
        CheckResult::Normal {
            local_is_newer: local.modified > server.modified,
        }
    }
}

/// What a completed check returns (M4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub result: CheckResult,
    /// The answer's `msg` when not empty, exactly as received; the core never
    /// sanitizes or shows it (R1516: the app shows it after the sync).
    pub server_message: Option<String>,
    /// The server's record: its `usn` is what a normal sync sends back as
    /// each sent object's `usn` (W6.2), its `media_usn` starts media sync.
    pub server: ServerMeta,
    /// The local `col.usn`, sent as `minUsn` in `start`.
    pub local_usn: Usn,
    /// The new base when the check followed a redirect (transport.md T7).
    pub new_endpoint: Option<Endpoint>,
}

/// The status check (M2, M3): `meta`, then `cont`, then the clock, then the
/// comparison. A check that stops at `cont` or the clock makes no other
/// request and writes nothing anywhere (AE63). `local` is read when the job
/// builds the request; the check never writes.
pub async fn check(job: &mut Job, local: &LocalRecord, clock: &dyn Clock) -> Result<CheckOutcome, JobError> {
    let server = meta(job, clock).await?;
    let result = compare(local, &server);
    Ok(CheckOutcome {
        result,
        server_message: (!server.message.is_empty()).then(|| server.message.clone()),
        server,
        local_usn: local.usn,
        new_endpoint: job.new_endpoint().cloned(),
    })
}

/// What the meta probe returns (M7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub server: ServerMeta,
    pub new_endpoint: Option<Endpoint>,
}

/// The meta probe (M7, KTD8): M2 steps 1 to 3 and the server's record. It
/// compares nothing, writes nothing and leaves the remembered answer alone;
/// the optimizer's gate treats any failure as "server state not confirmed".
pub async fn probe(job: &mut Job, clock: &dyn Clock) -> Result<ProbeOutcome, JobError> {
    let server = meta(job, clock).await?;
    Ok(ProbeOutcome {
        server,
        new_endpoint: job.new_endpoint().cloned(),
    })
}

/// M2 steps 1 to 3: the request (one redirect at most), `cont`, the clock.
async fn meta(job: &mut Job, clock: &dyn Clock) -> Result<ServerMeta, JobError> {
    let device_time = EpochSecs::from_clock(clock);
    let body = match job.request(Method::Meta, Payload::Json(encode_meta_request())).await {
        Ok(body) => body.bytes().await,
        Err(err) => Err(err),
    }
    .map_err(|err| job.error_for(Method::Meta, err))?;
    let server = decode_meta(&body)?;
    if !server.can_continue {
        return Err(JobError::new(
            ErrorKind::ServerMessage { text: server.message },
            Some(Method::Meta),
            "the server refused to sync with this client (cont false)",
        ));
    }
    let offset_secs = server.server_time.0.saturating_sub(device_time.0);
    if offset_secs.unsigned_abs() > MAX_CLOCK_OFFSET_SECS.unsigned_abs() {
        return Err(JobError::new(
            ErrorKind::ClockIncorrect { offset_secs },
            Some(Method::Meta),
            format!("the server's clock is {offset_secs} s from the device's"),
        ));
    }
    Ok(server)
}

/// The sync status the app shows (M6; sync-ops.md `status` results).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// Nothing to sync, as far as the device knows.
    NoChanges,
    /// A normal sync is needed: local changes, or the server has changes.
    NormalSync,
    /// A one-way sync is needed, whichever directions are possible.
    FullSync,
}

impl SyncStatus {
    fn of(result: CheckResult) -> SyncStatus {
        match result {
            CheckResult::NoChanges => SyncStatus::NoChanges,
            CheckResult::Normal { .. } => SyncStatus::NormalSync,
            CheckResult::OneWay { .. } => SyncStatus::FullSync,
        }
    }
}

/// A status answer (M6): no server message (messages are shown only after a
/// sync, R1516), and the new base when the check followed a redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusOutcome {
    pub status: SyncStatus,
    pub new_endpoint: Option<Endpoint>,
}

/// The remembered answer (M6.1): at most one, for one base and host key,
/// serving the status for 300 s on the core's clock. Its owner keeps it with
/// the open collection and drops it (or calls [`StatusMemory::forget`]) when
/// the collection closes or is replaced by anything but a one-way sync.
#[derive(Debug, Default)]
pub struct StatusMemory {
    remembered: Option<Remembered>,
}

#[derive(Debug, Clone)]
struct Remembered {
    base: Endpoint,
    host_key: Option<HostKey>,
    status: SyncStatus,
    /// The whole second it was stored at.
    at: EpochSecs,
}

impl StatusMemory {
    pub fn new() -> StatusMemory {
        StatusMemory::default()
    }

    /// M6: the offline answer when it decides; else the remembered answer for
    /// the job's base and host key when it is less than 300 s old; else a
    /// check, whose answer is remembered from the second the check began. A
    /// failed check leaves the remembered answer as it was.
    pub async fn status(
        &mut self,
        job: &mut Job,
        local: &LocalRecord,
        clock: &dyn Clock,
    ) -> Result<StatusOutcome, JobError> {
        match local.offline_status() {
            OfflineStatus::OneWayNeeded => return Ok(answer(SyncStatus::FullSync)),
            OfflineStatus::PendingChanges => return Ok(answer(SyncStatus::NormalSync)),
            OfflineStatus::Unknown | OfflineStatus::NothingPending => {}
        }
        let began = EpochSecs::from_clock(clock);
        if let Some(status) = self.lookup(job.base(), job.host_key(), began) {
            return Ok(answer(status));
        }
        let outcome = check(job, local, clock).await?;
        let status = SyncStatus::of(outcome.result);
        // Under the base the answer came from: after a followed redirect,
        // the new endpoint the app passes from now on.
        self.store(job.base(), job.host_key(), status, began);
        Ok(StatusOutcome {
            status,
            new_endpoint: outcome.new_endpoint,
        })
    }

    /// M6.1: a completed sync job's own outcome, stored now: no changes after
    /// a normal sync that found none or ran to the end and after a successful
    /// one-way transfer, one-way needed after a normal sync that stopped for
    /// one. `base` is where the job ended (after any redirect). A failed or
    /// cancelled job stores nothing.
    pub fn remember(&mut self, base: &Endpoint, host_key: Option<&HostKey>, status: SyncStatus, clock: &dyn Clock) {
        self.store(base, host_key, status, EpochSecs::from_clock(clock));
    }

    /// Forgets the remembered answer: the collection closed, or was replaced
    /// by an import or a restore.
    pub fn forget(&mut self) {
        self.remembered = None;
    }

    fn store(&mut self, base: &Endpoint, host_key: Option<&HostKey>, status: SyncStatus, at: EpochSecs) {
        self.remembered = Some(Remembered {
            base: base.clone(),
            host_key: host_key.cloned(),
            status,
            at,
        });
    }

    /// The remembered status for this base and key, when stored at most 299
    /// whole seconds before `now`. One stored after `now` (the clock moved
    /// back) has expired.
    fn lookup(&self, base: &Endpoint, host_key: Option<&HostKey>, now: EpochSecs) -> Option<SyncStatus> {
        let remembered = self.remembered.as_ref()?;
        let age = now.0.checked_sub(remembered.at.0)?;
        let fresh = (0..REMEMBER_SECS).contains(&age);
        (fresh && &remembered.base == base && remembered.host_key.as_ref() == host_key).then_some(remembered.status)
    }
}

fn answer(status: SyncStatus) -> StatusOutcome {
    StatusOutcome {
        status,
        new_endpoint: None,
    }
}
