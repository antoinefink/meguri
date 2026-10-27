//! Production local backups: reuse the forced safety writer, add B5 retention.
pub mod listing;
pub mod naming;
pub mod retention;
pub use meguri_engine::backup_limits::{self, BackupLimits};

/// Additive Rust result for U127, without changing safety or FFI receipts.
#[derive(Debug)]
pub struct BackupWritten {
    pub receipt: BackupReceipt,
    pub guard: meguri_engine::ports::GuardedBackup,
    pub deleted_names: Vec<String>,
    pub retention_failures: Vec<retention::RetentionFailure>,
}
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use meguri_engine::{
    CancelToken, Collection, Engine, EngineError, WeakEngine,
    device_state::{DeviceState, LastBackup},
    ports::{BackupFailureCause, BackupPublication, BackupReceipt, BackupService, SafetyBackupFailure},
};
use meguri_model::{clock::Clock, rng::RandomSource, time::EpochMillis};
use meguri_storage::{ReaderRole, StorageError, atomic_file::Purpose, space::SpaceProvider};

use crate::{CollectionPackageWriteContext, CollectionPackageWriteError as WriteError, Finalisation};

pub struct ForcedBackupProvider {
    engine: WeakEngine,
    collection: PathBuf,
    backups: PathBuf,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    space: Arc<dyn SpaceProvider>,
    // Snapshot order, not numeric col.mod order (undo can lower it), owns the
    // device reference. Keep the lane through durable metadata publication.
    lane: Mutex<()>,
    protected: Arc<Mutex<std::collections::BTreeMap<String, usize>>>,
}

impl ForcedBackupProvider {
    pub fn new(
        engine: Engine,
        collection: PathBuf,
        backups: PathBuf,
        clock: Arc<dyn Clock>,
        random: Arc<dyn RandomSource>,
        space: Arc<dyn SpaceProvider>,
    ) -> Self {
        Self {
            engine: engine.downgrade(),
            collection,
            backups,
            clock,
            random,
            space,
            lane: Mutex::new(()),
            protected: Arc::default(),
        }
    }

    pub fn force(&self, collection: &Collection, cancel: &CancelToken) -> Result<BackupWritten, EngineError> {
        self.force_inner(collection, cancel, false)
    }

    fn engine(&self) -> Result<Engine, EngineError> {
        self.engine.upgrade().ok_or_else(|| {
            failure(
                engine_cause(EngineError::CollectionUnavailable {
                    reason: "the backup provider's engine has dropped".into(),
                }),
                BackupPublication::Unpublished,
            )
        })
    }

    fn pin(&self, name: &str) -> Arc<dyn meguri_engine::ports::BackupProtection> {
        *self
            .protected
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default() += 1;
        Arc::new(BackupPin {
            name: name.to_owned(),
            protected: self.protected.clone(),
        })
    }

    fn force_inner(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
        protect: bool,
    ) -> Result<BackupWritten, EngineError> {
        let _lane = self.lane.lock().unwrap_or_else(PoisonError::into_inner);
        let fail = |cause| failure(cause, BackupPublication::Unpublished);
        cancel.check().map_err(|e| fail(engine_cause(e)))?;
        let engine = self.engine()?;
        // Read through the handle supplied by the exclusive caller.
        let lease = collection
            .reader(ReaderRole::Maintenance)
            .map_err(|e| fail(engine_cause(e)))?;
        if collection.generation() != engine.generation() {
            return Err(fail(engine_cause(EngineError::StaleGeneration {
                planned: collection.generation(),
                current: engine.generation(),
            })));
        }
        let limits = backup_limits::read(&*lease).map_err(|e| fail(engine_cause(e)))?;
        let configured = self.collection.canonicalize().map_err(|e| fail(io_cause(e)))?;
        let actual = collection.path().canonicalize().map_err(|e| fail(io_cause(e)))?;
        let (source, _) = lease.live_snapshot_source().map_err(|e| fail(engine_cause(e.into())))?;
        if actual != configured || source.canonicalize().map_err(|e| fail(io_cause(e)))? != configured {
            return Err(fail(engine_cause(EngineError::InvalidArgument {
                message: "safety backup collection differs from configured destination".into(),
            })));
        }
        let directory = configured.parent().ok_or_else(|| {
            fail(BackupFailureCause::Io {
                detail: "collection has no parent".into(),
            })
        })?;
        let is_cancelled = || cancel.is_cancelled();
        let context = CollectionPackageWriteContext {
            collection_directory: directory,
            purpose: Purpose::Backup,
            finalisation: Finalisation::ExclusiveCreate,
            clock: self.clock.as_ref(),
            random: self.random.as_ref(),
            space: self.space.as_ref(),
            is_cancelled: &is_cancelled,
        };
        for attempt in 0..3 {
            cancel.check().map_err(|e| fail(engine_cause(e)))?;
            let instant = EpochMillis(self.clock.now_ms());
            let name = free_name(&self.backups, instant, self.clock.as_ref()).map_err(|e| fail(e.into()))?;
            let destination = self.backups.join(&name);
            // Preflight the actual DB + WAL before even making backup parents.
            // The common writer repeats capacity checks immediately before its
            // single VACUUM; this check makes missing-directory creation safe.
            let (_, size) = lease.live_snapshot_source().map_err(|e| fail(engine_cause(e.into())))?;
            crate::writer::preflight_publication(size, &destination, &context).map_err(|e| fail(e.into()))?;
            cancel.check().map_err(|e| fail(engine_cause(e)))?;
            create_backup_directory(&self.backups).map_err(|e| fail(engine_cause(e.into())))?;
            // Pin before publication and its metadata callbacks. A pin failure
            // skips retention, never a recoverable backup; do not reopen later.
            let retention_directory = retention::PinnedDirectory::open(&self.backups);
            let written = match crate::write_collection_package_from_live(&lease, &destination, &context) {
                Ok(written) => written,
                Err(WriteError::DestinationExists) if attempt < 2 => continue,
                Err(error) => {
                    let publication = if matches!(error, WriteError::Io { .. } | WriteError::DiskFull) {
                        BackupPublication::Uncertain { name, instant }
                    } else {
                        BackupPublication::Unpublished
                    };
                    return Err(failure(error.into(), publication));
                }
            };
            // The publication lane prevents another retention pass until this
            // actual name has a lifetime pin, including metadata callbacks.
            let protection = protect.then(|| self.pin(&name));
            let recorded: Result<BackupReceipt, EngineError> = (|| {
                let size_bytes = fs::metadata(&destination)
                    .map_err(|e| {
                        failure(
                            io_cause(e),
                            BackupPublication::Uncertain {
                                name: name.clone(),
                                instant,
                            },
                        )
                    })?
                    .len();
                let receipt = BackupReceipt {
                    name: name.clone(),
                    size_bytes,
                    instant,
                    snapshot_modified: written.snapshot_modified,
                };
                // Cancellation after publication cannot discard either the file or
                // its record. The importing worker may honour it after this returns.
                record(directory, &receipt)
                    .map_err(|e| failure(engine_cause(e.into()), BackupPublication::Durable(receipt.clone())))?;
                Ok(receipt)
            })();
            // Publication is durable. Neither late cancellation nor metadata
            // failure may bypass retention or lose the primary error.
            let retained = match naming::today(EpochMillis(self.clock.now_ms()), self.clock.as_ref()) {
                Ok(today) => retention::thin_pinned(retention_directory, today, limits, &{
                    let mut names = self
                        .protected
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .keys()
                        .cloned()
                        .collect::<std::collections::BTreeSet<_>>();
                    names.insert(name.clone());
                    names
                }),
                Err(error) => {
                    tracing::warn!(%error, "backup retention date unavailable");
                    retention::RetentionReport {
                        deleted_names: Vec::new(),
                        failures: vec![retention::RetentionFailure {
                            name: None,
                            detail: error.to_string(),
                        }],
                    }
                }
            };
            let receipt = recorded?;
            // lease drops here, before the caller invokes verified replacement.
            let guard = meguri_engine::ports::GuardedBackup {
                receipt: Some(receipt.clone()),
                protection,
            };
            return Ok(BackupWritten {
                guard,
                receipt,
                deleted_names: retained.deleted_names,
                retention_failures: retained.failures,
            });
        }
        unreachable!("each publication attempt returns or retries within its bound")
    }
}

struct BackupPin {
    name: String,
    protected: Arc<Mutex<std::collections::BTreeMap<String, usize>>>,
}
impl Drop for BackupPin {
    fn drop(&mut self) {
        let mut names = self.protected.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = names.get_mut(&self.name) {
            *count -= 1;
            if *count == 0 {
                names.remove(&self.name);
            }
        }
    }
}

impl BackupService for ForcedBackupProvider {
    fn before_safety_operation(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
    ) -> Result<meguri_engine::ports::GuardedBackup, EngineError> {
        self.force_inner(collection, cancel, true).map(|written| written.guard)
    }

    fn protect_backup(
        &self,
        name: &str,
    ) -> Result<Option<Arc<dyn meguri_engine::ports::BackupProtection>>, EngineError> {
        let _lane = self.lane.lock().unwrap_or_else(PoisonError::into_inner);
        let _engine = self.engine()?;
        // Missing/unreadable proof falls back to a fresh backup.
        Ok(listing::list(&self.backups)
            .ok()
            .filter(|entries| entries.iter().any(|entry| entry.name == name))
            .map(|_| self.pin(name)))
    }

    fn before_manual_transfer(
        &self,
        collection: Option<&Collection>,
        cancel: &CancelToken,
    ) -> Result<meguri_engine::ports::TransferBackup, EngineError> {
        use meguri_engine::ports::TransferBackup;
        if let Some(collection) = collection {
            match self.force(collection, cancel) {
                Ok(written) => return Ok(TransferBackup::Collection(written.receipt)),
                Err(EngineError::SafetyBackupFailed(ref error)) if corrupt_snapshot(&error.cause) => {}
                Err(error) => return Err(error),
            }
        }
        // Caller holds Working (writer drained) or initial replacement's
        // advisory ownership. No checkpoint or quick_check is run for T7.
        let fail = |cause| failure(cause, BackupPublication::Unpublished);
        cancel.check().map_err(|e| fail(engine_cause(e)))?;
        let _engine = self.engine()?;
        let main_size = match fs::symlink_metadata(&self.collection) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TransferBackup::Absent),
            Err(e) => return Err(fail(io_cause(e))),
            Ok(m) if m.file_type().is_file() => m.len(),
            Ok(_) => {
                return Err(fail(BackupFailureCause::Io {
                    detail: "raw backup source is not regular".into(),
                }));
            }
        };
        let wal_path = self.collection.with_file_name("collection.anki2-wal");
        let wal_size = match fs::symlink_metadata(&wal_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(fail(io_cause(e))),
            Ok(m) if m.file_type().is_file() => m.len(),
            Ok(_) => {
                return Err(fail(BackupFailureCause::Io {
                    detail: "raw WAL is not regular".into(),
                }));
            }
        };
        let needed = main_size
            .checked_add(wal_size)
            .and_then(|s| s.checked_add(4 * 1024 * 1024))
            .ok_or_else(|| {
                fail(BackupFailureCause::Io {
                    detail: "raw backup space overflow".into(),
                })
            })?;
        let mut ancestor = self.backups.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| {
                fail(BackupFailureCause::Io {
                    detail: "backup ancestor".into(),
                })
            })?;
        }
        meguri_storage::space::require_space(self.space.as_ref(), ancestor, needed)
            .map_err(|e| fail(engine_cause(e.into())))?;
        create_backup_directory(&self.backups).map_err(|e| fail(engine_cause(e.into())))?;
        let instant = EpochMillis(self.clock.now_ms());
        let wall = instant
            .0
            .div_euclid(1000)
            .checked_add(i64::from(self.clock.utc_offset_at(instant.0).seconds()))
            .ok_or_else(|| {
                fail(BackupFailureCause::Io {
                    detail: "raw backup time overflow".into(),
                })
            })?;
        for advance in 0..=60 {
            let name = backup_name(wall.checked_add(advance).ok_or_else(|| {
                fail(BackupFailureCause::Io {
                    detail: "raw backup time overflow".into(),
                })
            })?)
            .map_err(|e| fail(e.into()))?;
            let main = name.replacen("backup-", "corrupt-", 1).replace(".colpkg", ".anki2");
            let wal = format!("{main}-wal");
            if self.backups.join(&main).symlink_metadata().is_ok() || self.backups.join(&wal).symlink_metadata().is_ok()
            {
                continue;
            }
            cancel.check().map_err(|e| fail(engine_cause(e)))?;
            if wal_size > 0 {
                crate::server_copy::copy_raw(&wal_path, &self.backups.join(&wal)).map_err(|e| fail(e.into()))?;
            }
            if let Err(error) = crate::server_copy::copy_raw(&self.collection, &self.backups.join(&main)) {
                // A sync error can follow publication of the main file. Keep
                // both recovery files whenever that destination is visible.
                if self.backups.join(&main).symlink_metadata().is_err() && wal_size > 0 {
                    let _ = fs::remove_file(self.backups.join(&wal));
                    let _ = meguri_storage::atomic_file::sync_directory(&self.backups);
                }
                return Err(fail(error.into()));
            }
            return Ok(TransferBackup::Raw {
                main,
                wal: (wal_size > 0).then_some(wal),
                instant,
            });
        }
        Err(fail(BackupFailureCause::DestinationExists))
    }

    fn before_deck_delete(&self) -> Result<(), EngineError> {
        let engine = self.engine()?;
        let collection = engine
            .collection()
            .map_err(|e| failure(engine_cause(e), BackupPublication::Unpublished))?;
        self.force(&collection, &CancelToken::default()).map(|_| ())
    }

    fn before_colpkg_import(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
    ) -> Result<BackupReceipt, EngineError> {
        self.force(collection, cancel).map(|written| written.receipt)
    }
}

// New directory entries need their parents synced too; syncing only the
// package's immediate directory does not make missing ancestors durable.
pub(crate) fn create_backup_directory(directory: &Path) -> Result<(), StorageError> {
    let mut missing = Vec::new();
    let mut ancestor = directory;
    loop {
        match ancestor.symlink_metadata() {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(ancestor.to_owned());
                ancestor = ancestor
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
            }
            Err(e) => return Err(e.into()),
        }
    }
    fs::create_dir_all(directory)?;
    if !missing.is_empty() {
        for path in &missing {
            meguri_storage::atomic_file::sync_directory(path)?;
        }
        meguri_storage::atomic_file::sync_directory(ancestor)?;
    }
    Ok(())
}

fn record(directory: &Path, receipt: &BackupReceipt) -> Result<(), StorageError> {
    DeviceState::for_collection(&directory.join("collection.anki2")).set_last_backup(&LastBackup {
        snapshot_mod: receipt.snapshot_modified.0,
        instant: receipt.instant.0,
        name: receipt.name.clone(),
    })
}

fn free_name(directory: &Path, instant: EpochMillis, clock: &dyn Clock) -> Result<String, WriteError> {
    let second = instant.0.div_euclid(1000);
    let offset = i64::from(clock.utc_offset_at(instant.0).seconds());
    for advance in 0..=60 {
        let wall = second
            .checked_add(offset)
            .and_then(|s| s.checked_add(advance))
            .ok_or_else(|| crate::corrupt("backup date overflow"))?;
        let name = backup_name(wall)?;
        match directory.join(&name).symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(name),
            Ok(_) => {} // A dangling symlink also occupies the name.
            Err(e) => return Err(e.into()),
        }
    }
    Err(WriteError::DestinationExists)
}

// Convert a bounded Gregorian day by counting complete years and months. This
// needs no locale or timezone database; year zero has Gregorian leap rules.
pub(crate) fn backup_name(wall_seconds: i64) -> Result<String, WriteError> {
    let day = wall_seconds.div_euclid(86_400);
    let year_start = |year: i64| {
        let prior = year - 1;
        365 * (year - 1970) + prior.div_euclid(4) - prior.div_euclid(100) + prior.div_euclid(400) - 477
    };
    if day < year_start(0) || day >= year_start(10_000) {
        return Err(crate::corrupt("backup year must have four ASCII digits"));
    }
    let (mut low, mut high) = (0, 10_000);
    while low + 1 < high {
        let middle = (low + high) / 2;
        if year_start(middle) <= day {
            low = middle;
        } else {
            high = middle;
        }
    }
    let year = low;
    let leap = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
    let months = [31, 28 + i64::from(leap), 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut remaining = day - year_start(year);
    let mut month = 1;
    for length in months {
        if remaining < length {
            break;
        }
        remaining -= length;
        month += 1;
    }
    let time = wall_seconds.rem_euclid(86_400);
    Ok(format!(
        "backup-{year:04}-{month:02}-{:02}-{:02}.{:02}.{:02}.colpkg",
        remaining + 1,
        time / 3600,
        time / 60 % 60,
        time % 60
    ))
}

fn failure(cause: BackupFailureCause, publication: BackupPublication) -> EngineError {
    EngineError::SafetyBackupFailed(Box::new(SafetyBackupFailure { cause, publication }))
}
fn engine_cause(error: EngineError) -> BackupFailureCause {
    match error {
        EngineError::Cancelled => BackupFailureCause::Cancelled,
        other => BackupFailureCause::Engine(Box::new(other)),
    }
}
fn io_cause(error: std::io::Error) -> BackupFailureCause {
    engine_cause(StorageError::from(error).into())
}
impl From<WriteError> for BackupFailureCause {
    fn from(package_failure: WriteError) -> Self {
        use WriteError as Input;
        match package_failure {
            Input::Cancelled => Self::Cancelled,
            Input::DestinationExists => Self::DestinationExists,
            Input::DiskFull => Self::DiskFull,
            Input::SourceNotFinalised => Self::SourceNotFinalised,
            Input::TooNew => Self::TooNew,
            Input::UnsupportedMedia => Self::UnsupportedMedia,
            Input::UnsupportedFormat => Self::UnsupportedFormat,
            Input::SchemaUnsupported { gate } => Self::SchemaUnsupported { gate },
            Input::InsufficientSpace { needed, available } => Self::InsufficientSpace { needed, available },
            Input::Corrupt { detail } => Self::Corrupt { detail },
            Input::Io { detail } => Self::Io { detail },
            export_failure @ (Input::ExportFormatLimit
            | Input::ExportSourceInvalid { .. }
            | Input::ExportSourceChanged) => {
                // RF2 and forced backup retain collection-only publication.
                Self::Io {
                    detail: export_failure.to_string(),
                }
            }
        }
    }
}

fn corrupt_snapshot(cause: &BackupFailureCause) -> bool {
    matches!(cause,
        BackupFailureCause::Engine(error) if matches!(error.as_ref(),
            EngineError::Storage(StorageError::Sqlite { code: 11 | 26, .. })
            | EngineError::Open(meguri_storage::OpenError::Corrupt { .. })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::clock::{FixedClock, UtcOffset};

    #[test]
    fn names_use_floor_seconds_offsets_and_gregorian_boundaries() {
        assert_eq!(backup_name(-1).unwrap(), "backup-1969-12-31-23.59.59.colpkg");
        assert_eq!(backup_name(0).unwrap(), "backup-1970-01-01-00.00.00.colpkg");
        assert_eq!(backup_name(951_782_400).unwrap(), "backup-2000-02-29-00.00.00.colpkg");
        assert_eq!(backup_name(4_107_542_400).unwrap(), "backup-2100-03-01-00.00.00.colpkg");
        assert!(backup_name(i64::MAX).is_err());
        assert!(backup_name(i64::MIN).is_err());
        let directory = tempfile::tempdir().unwrap();
        let clock = FixedClock::new(1_894_737_600_999, UtcOffset::from_seconds(9 * 3600).unwrap());
        assert_eq!(
            free_name(directory.path(), EpochMillis(clock.now_ms()), &clock).unwrap(),
            "backup-2030-01-16-05.00.00.colpkg"
        );
    }

    #[test]
    fn dangling_symlink_occupies_a_name_and_all_61_names_refuse_without_writes() {
        let directory = tempfile::tempdir().unwrap();
        let clock = FixedClock::new(0, UtcOffset::UTC);
        let first = backup_name(0).unwrap();
        std::os::unix::fs::symlink("absent-target", directory.path().join(&first)).unwrap();
        assert_eq!(
            free_name(directory.path(), EpochMillis(0), &clock).unwrap(),
            backup_name(1).unwrap()
        );
        for second in 1..=60 {
            fs::write(directory.path().join(backup_name(second).unwrap()), b"foreign sentinel").unwrap();
        }
        assert_eq!(
            free_name(directory.path(), EpochMillis(0), &clock),
            Err(WriteError::DestinationExists)
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 61);
        assert!(
            directory
                .path()
                .join(first)
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
