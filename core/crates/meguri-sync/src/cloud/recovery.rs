//! V8.1 file preservation and storage-only salvage. Caller ownership must
//! exclude every other tracking open/write/checkpoint, across all builds.
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt},
    },
    path::{Path, PathBuf},
};

use meguri_model::{clock::Clock, rng::RandomSource, time::EpochMillis};
use meguri_package::backup::naming;
use meguri_storage::atomic_file::{self, AtomicFile, Purpose};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{
    open::{self, Probe, sidecar},
    store::{decode, encode},
    *,
};

const PREFIX: &str = "meguri-cloud.db.damaged-";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeIdentity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveJournal {
    version: u32,
    destination: String,
    main: NativeIdentity,
    /// JSON null explicitly records absence, never an unrecorded WAL.
    wal: Option<NativeIdentity>,
}

/// A pinned directory, not a lock. The precondition above covers path lookup
/// and private SQLite copies as well as the noreplace rename operations.
struct RecoveryDirectory {
    path: PathBuf,
    file: File,
    identity: NativeIdentity,
    #[cfg(test)]
    sync_count: std::cell::Cell<usize>,
    #[cfg(test)]
    fail_sync_at: std::cell::Cell<Option<usize>>,
}

impl RecoveryDirectory {
    fn open(path: &Path) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() {
            return Err(Error::RecoveryRefused);
        }
        let file = File::open(path)?;
        let pinned = file.metadata()?;
        if (metadata.dev(), metadata.ino()) != (pinned.dev(), pinned.ino()) {
            return Err(Error::RecoveryRefused);
        }
        Ok(Self {
            path: path.to_owned(),
            file,
            identity: NativeIdentity {
                device: pinned.dev(),
                inode: pinned.ino(),
            },
            #[cfg(test)]
            sync_count: std::cell::Cell::new(0),
            #[cfg(test)]
            fail_sync_at: std::cell::Cell::new(None),
        })
    }

    fn check(&self) -> Result<(), Error> {
        let metadata = fs::symlink_metadata(&self.path)?;
        if !metadata.is_dir()
            || self.identity
                != (NativeIdentity {
                    device: metadata.dev(),
                    inode: metadata.ino(),
                })
        {
            return Err(Error::RecoveryRefused);
        }
        Ok(())
    }

    fn before_sync(&self) -> Result<(), Error> {
        self.check()?;
        #[cfg(test)]
        {
            let count = self.sync_count.get() + 1;
            self.sync_count.set(count);
            if self.fail_sync_at.get() == Some(count) {
                return Err(Error::Io(std::io::ErrorKind::Other));
            }
        }
        Ok(())
    }

    fn sync(&self) -> Result<(), Error> {
        self.before_sync()?;
        self.file.sync_all()?;
        Ok(())
    }

    fn sync_file(&self, name: &str, expected: NativeIdentity) -> Result<(), Error> {
        self.check()?;
        let path = self.path.join(name);
        if identity(&path)? != Some(expected) {
            return Err(Error::RecoveryRefused);
        }
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        if expected
            != (NativeIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        {
            return Err(Error::RecoveryRefused);
        }
        self.before_sync()?;
        atomic_file::full_sync(&file)?;
        if identity(&path)? != Some(expected) {
            return Err(Error::RecoveryRefused);
        }
        Ok(())
    }

    fn move_new(&self, from: &str, to: &str) -> Result<(), Error> {
        self.check()?;
        atomic_file::move_new_in(&self.file, from.as_ref(), to.as_ref())?;
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        self.check()?;
        fs::remove_file(self.path.join(name))?;
        self.sync()
    }
}

fn identity(path: &Path) -> Result<Option<NativeIdentity>, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(NativeIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })),
        Ok(_) => Err(Error::RecoveryRefused),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn is_final_name(name: &str) -> bool {
    name.strip_prefix(PREFIX)
        .is_some_and(|stamp| naming::parse_name(&format!("backup-{stamp}.colpkg")).is_some())
}

pub(super) fn is_source_name(name: &str) -> bool {
    name.strip_suffix(".salvaging").is_some_and(is_final_name)
}

fn damaged_name(clock: &dyn Clock) -> Result<String, Error> {
    let backup = naming::name_at(EpochMillis(clock.now_ms()), clock).map_err(|_| Error::InvalidData)?;
    let stamp = backup
        .strip_prefix("backup-")
        .and_then(|s| s.strip_suffix(".colpkg"))
        .ok_or(Error::InvalidData)?;
    Ok(format!("{PREFIX}{stamp}"))
}

fn names(directory: &Path) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let filename = entry.file_name();
        if filename.as_bytes().starts_with(PREFIX.as_bytes()) && filename.to_str().is_none() {
            return Err(Error::RecoveryRefused);
        }
        // Refuse malformed owned markers rather than guessing a state.
        if let Some(name) = filename.to_str()
            && name.starts_with(PREFIX)
        {
            let valid = is_final_name(name)
                || is_source_name(name)
                || name.strip_suffix(".moving").is_some_and(is_final_name)
                || name
                    .strip_suffix("-wal")
                    .is_some_and(|n| is_final_name(n) || is_source_name(n));
            if !valid {
                return Err(Error::RecoveryRefused);
            }
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

pub(super) fn require_settled(path: &Path) -> Result<(), Error> {
    let directory = RecoveryDirectory::open(path)?;
    if names(path)?
        .iter()
        .any(|n| n.ends_with(".moving") || n.ends_with(".salvaging") || n.ends_with(".salvaging-wal"))
    {
        return Err(Error::RecoveryRequired);
    }
    // Absence may be visible after a failed journal-unlink fsync. Persist it
    // before any writable open or current-file creation, even in plain open.
    directory.sync()?;
    if identity(&path.join(TRACKING_FILE))?.is_none() && identity(&path.join("meguri-cloud.db-wal"))?.is_some() {
        return Err(Error::RecoveryRefused);
    }
    Ok(())
}

fn read_journal(directory: &RecoveryDirectory, name: &str) -> Result<(MoveJournal, NativeIdentity), Error> {
    directory.check()?;
    let journal_identity = identity(&directory.path.join(name))?.ok_or(Error::RecoveryRefused)?;
    let mut bytes = Vec::new();
    File::open(directory.path.join(name))?
        .take(16_385)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err(Error::RecoveryRefused);
    }
    // Option's normal serde default accepts an omitted member; require the
    // WAL field itself so absence is an explicit assertion in the journal.
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| Error::RecoveryRefused)?;
    if value.get("wal").is_none() {
        return Err(Error::RecoveryRefused);
    }
    let journal: MoveJournal = serde_json::from_slice(&bytes).map_err(|_| Error::RecoveryRefused)?;
    if journal.version != 1 || !is_final_name(&journal.destination) || name != format!("{}.moving", journal.destination)
    {
        return Err(Error::RecoveryRefused);
    }
    Ok((journal, journal_identity))
}

/// Returns the current, exactly identified main/WAL basenames. No mutation.
fn locations(directory: &RecoveryDirectory, journal: &MoveJournal) -> Result<(String, Option<String>), Error> {
    directory.check()?;
    let n = &journal.destination;
    if identity(&directory.path.join(n))?.is_some() || identity(&directory.path.join(format!("{n}-wal")))?.is_some() {
        return Err(Error::RecoveryRefused);
    }
    fn locate(
        directory: &RecoveryDirectory,
        source: &str,
        destination: &str,
        expected: NativeIdentity,
    ) -> Result<String, Error> {
        match (
            identity(&directory.path.join(source))?,
            identity(&directory.path.join(destination))?,
        ) {
            (Some(found), None) if found == expected => Ok(source.to_owned()),
            (None, Some(found)) if found == expected => Ok(destination.to_owned()),
            _ => Err(Error::RecoveryRefused),
        }
    }
    let main = locate(directory, TRACKING_FILE, &format!("{n}.salvaging"), journal.main)?;
    let wal = match journal.wal {
        Some(expected) => Some(locate(
            directory,
            "meguri-cloud.db-wal",
            &format!("{n}.salvaging-wal"),
            expected,
        )?),
        None => {
            if identity(&directory.path.join("meguri-cloud.db-wal"))?.is_some()
                || identity(&directory.path.join(format!("{n}.salvaging-wal")))?.is_some()
            {
                return Err(Error::RecoveryRefused);
            }
            None
        }
    };
    if main != TRACKING_FILE && wal.as_deref() == Some("meguri-cloud.db-wal") {
        return Err(Error::RecoveryRefused);
    }
    Ok((main, wal))
}

fn finish_move(
    directory: &RecoveryDirectory,
    name: &str,
    journal: &MoveJournal,
    journal_identity: NativeIdentity,
) -> Result<(), Error> {
    let (main, wal) = locations(directory, journal)?;
    // Visibility is not durability: also required when both moves are visible.
    directory.sync_file(name, journal_identity)?;
    directory.sync_file(&main, journal.main)?;
    if let (Some(wal), Some(expected)) = (&wal, journal.wal) {
        directory.sync_file(wal, expected)?;
    }
    directory.sync()?;
    let (_, wal) = locations(directory, journal)?;
    if wal.as_deref() == Some("meguri-cloud.db-wal") {
        directory.move_new("meguri-cloud.db-wal", &format!("{}.salvaging-wal", journal.destination))?;
        meguri_model::faultpoint!("cloud.tracking.move.after_wal")?;
    }
    let (main, _) = locations(directory, journal)?;
    if main == TRACKING_FILE {
        directory.move_new(TRACKING_FILE, &format!("{}.salvaging", journal.destination))?;
        meguri_model::faultpoint!("cloud.tracking.move.after_main")?;
    }
    locations(directory, journal)?;
    if identity(&directory.path.join(name))? != Some(journal_identity) {
        return Err(Error::RecoveryRefused);
    }
    directory.remove(name)?;
    Ok(())
}

fn replay_moves(directory: &RecoveryDirectory) -> Result<(), Error> {
    let mut journals = Vec::new();
    for name in names(&directory.path)?.into_iter().filter(|n| n.ends_with(".moving")) {
        let (journal, id) = read_journal(directory, &name)?;
        locations(directory, &journal)?;
        journals.push((name, journal, id));
    }
    let mut owned = std::collections::HashSet::new();
    for (_, journal, _) in &journals {
        let (main, wal) = locations(directory, journal)?;
        if !owned.insert(main) {
            return Err(Error::RecoveryRefused);
        }
        if let Some(wal) = wal
            && !owned.insert(wal)
        {
            return Err(Error::RecoveryRefused);
        }
    }
    for (name, journal, id) in journals {
        finish_move(directory, &name, &journal, id)?;
    }
    directory.sync()?;
    Ok(())
}

fn move_damaged(directory: &RecoveryDirectory, clock: &dyn Clock) -> Result<(), Error> {
    directory.check()?;
    let destination = damaged_name(clock)?;
    // A fixed clock can collide. Refusal preserves evidence; never draw a
    // replacement timestamp or overwrite a previously retained source.
    for suffix in ["", "-wal", ".moving", ".salvaging", ".salvaging-wal"] {
        if identity(&directory.path.join(format!("{destination}{suffix}")))?.is_some() {
            return Err(Error::RecoveryRefused);
        }
    }
    let journal = MoveJournal {
        version: 1,
        destination: destination.clone(),
        main: identity(&directory.path.join(TRACKING_FILE))?.ok_or(Error::RecoveryRefused)?,
        wal: identity(&directory.path.join("meguri-cloud.db-wal"))?,
    };
    let name = format!("{destination}.moving");
    let mut file = AtomicFile::new(&directory.path.join(&name), Purpose::Sidecar)?;
    file.file_mut()
        .write_all(&serde_json::to_vec(&journal).map_err(|_| Error::InvalidData)?)?;
    file.publish_new(|_| Ok(()))?;
    meguri_model::faultpoint!("cloud.tracking.move.after_journal")?;
    let (installed, id) = read_journal(directory, &name)?;
    finish_move(directory, &name, &installed, id)?;
    meguri_model::faultpoint!("cloud.tracking.after_move_aside")?;
    Ok(())
}

pub(super) fn recover(
    directory: &Path,
    scope: BuildScope,
    clock: &dyn Clock,
    random: &dyn RandomSource,
) -> Result<OpenOutcome, Error> {
    let directory = RecoveryDirectory::open(directory)?;
    replay_moves(&directory)?;
    let current = directory.path.join(TRACKING_FILE);
    match identity(&current)? {
        Some(_) => match open::probe(&current)? {
            Probe::Newer(found) => return Ok(OpenOutcome::NewerSchema { found }),
            Probe::Damaged => move_damaged(&directory, clock)?,
            Probe::Supported => {}
        },
        None => {
            if identity(&directory.path.join("meguri-cloud.db-wal"))?.is_some() {
                return Err(Error::RecoveryRefused);
            }
        }
    }
    // No journal exists, and its absence has crossed a directory barrier.
    // The new empty store is private to this function until all salvage ends.
    directory.sync()?;
    let outcome = open::open_current(&directory.path, scope)?;
    let (mut store, missing) = match outcome {
        OpenOutcome::Missing(store) => (store, true),
        OpenOutcome::Opened(store) => (store, false),
        other => return Ok(other),
    };
    for name in names(&directory.path)? {
        if is_source_name(&name) {
            identity(&directory.path.join(&name))?.ok_or(Error::RecoveryRefused)?;
            identity(&directory.path.join(format!("{name}-wal")))?;
            let final_name = name.strip_suffix(".salvaging").ok_or(Error::RecoveryRefused)?;
            if identity(&directory.path.join(format!("{final_name}-wal")))?.is_some() {
                // WAL-first final rename proves the durable copy commit.
                if identity(&directory.path.join(format!("{name}-wal")))?.is_some() {
                    return Err(Error::RecoveryRefused);
                }
                if identity(&directory.path.join(final_name))?.is_some() {
                    return Err(Error::RecoveryRefused);
                }
                directory.move_new(&name, final_name)?;
            } else {
                if identity(&directory.path.join(final_name))?.is_some() {
                    return Err(Error::RecoveryRefused);
                }
                let complete = salvage(&mut store, &directory.path.join(&name), &name, clock, random)?;
                // Include new main/WAL directory entries in the durable copy
                // commit before permitting any source finalization.
                for target in [TRACKING_FILE, "meguri-cloud.db-wal"] {
                    if let Some(id) = identity(&directory.path.join(target))? {
                        directory.sync_file(target, id)?;
                    }
                }
                directory.sync()?;
                meguri_model::faultpoint!("cloud.tracking.after_salvage")?;
                finalize(&directory, &name, complete)?;
            }
        } else if let Some(main) = name.strip_suffix("-wal").filter(|n| is_source_name(n))
            && identity(&directory.path.join(main))?.is_none()
            && identity(&directory.path.join(&name))?.is_some()
        {
            // Only after replay/absence durability may this be a completed
            // delete orphan. Never classify an in-progress move this way.
            let final_name = main.strip_suffix(".salvaging").ok_or(Error::RecoveryRefused)?;
            if identity(&directory.path.join(final_name))?.is_some()
                || identity(&directory.path.join(format!("{final_name}-wal")))?.is_some()
            {
                return Err(Error::RecoveryRefused);
            }
            directory.remove(&name)?;
        }
    }
    Ok(if missing {
        OpenOutcome::Missing(store)
    } else {
        OpenOutcome::Opened(store)
    })
}

fn finalize(directory: &RecoveryDirectory, name: &str, complete: bool) -> Result<(), Error> {
    let wal = format!("{name}-wal");
    if complete {
        directory.remove(name)?;
        if identity(&directory.path.join(&wal))?.is_some() {
            directory.remove(&wal)?;
        }
    } else {
        let final_name = name.strip_suffix(".salvaging").ok_or(Error::RecoveryRefused)?;
        if identity(&directory.path.join(final_name))?.is_some()
            || identity(&directory.path.join(format!("{final_name}-wal")))?.is_some()
        {
            return Err(Error::RecoveryRefused);
        }
        if identity(&directory.path.join(&wal))?.is_some() {
            directory.move_new(&wal, &format!("{final_name}-wal"))?;
        }
        directory.move_new(name, final_name)?;
    }
    Ok(())
}

pub(super) fn retained_sources(directory: &Path) -> Result<Vec<String>, Error> {
    let mut kept = Vec::new();
    for name in names(directory)? {
        if is_final_name(&name) {
            identity(&directory.join(&name))?.ok_or(Error::RecoveryRefused)?;
            kept.push(name);
        }
    }
    Ok(kept)
}

fn new_binding_id(random: &dyn RandomSource) -> BindingId {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&random.next_u64().to_be_bytes());
    bytes[8..].copy_from_slice(&random.next_u64().to_be_bytes());
    BindingId(bytes)
}

/// Only the injected process-wide source names salvage scratch space. A
/// collision is an error, never a fallback to OS entropy or a new seed.
struct PrivateSourceCopy(PathBuf);
impl PrivateSourceCopy {
    fn new(random: &dyn RandomSource) -> Result<Self, Error> {
        let path = std::env::temp_dir().join(format!(
            ".meguri-cloud-salvage-{:016x}{:016x}",
            random.next_u64(),
            random.next_u64()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for PrivateSourceCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The source is never opened by SQLite. WAL recovery/checkpointing, if
/// needed, affects a private copy only; query_only then protects its contents.
fn salvage(
    store: &mut TrackingStore,
    path: &Path,
    source: &str,
    clock: &dyn Clock,
    random: &dyn RandomSource,
) -> Result<bool, Error> {
    let private = PrivateSourceCopy::new(random)?;
    let copy = private.0.join(TRACKING_FILE);
    fs::copy(path, &copy)?;
    match fs::copy(sidecar(path, "-wal"), sidecar(&copy, "-wal")) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let connection = match Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE) {
        Ok(connection) => connection,
        Err(_) => return commit_copies(store, source, None, Vec::new(), false, clock, random),
    };
    if connection.execute_batch("PRAGMA query_only=ON").is_err() {
        return commit_copies(store, source, None, Vec::new(), false, clock, random);
    }
    let version: Option<i64> = connection.pragma_query_value(None, "user_version", |r| r.get(0)).ok();
    if version.is_some_and(|v| v > open::SCHEMA_VERSION) {
        return Err(Error::RecoveryRefused);
    }
    let mut readable = Vec::new();
    let mut complete = true;
    if let Ok(mut statement) = connection.prepare("SELECT id,binding,content FROM conflicts ORDER BY rowid") {
        match statement.query([]) {
            Ok(mut rows) => loop {
                match rows.next() {
                    Ok(Some(row)) => {
                        let decoded = (|| -> Result<_, Error> {
                            let id: Vec<u8> = row.get(0)?;
                            let copy: ConflictCopy = decode(&row.get::<_, String>(2)?)?;
                            if id != copy.id.0 {
                                return Err(Error::InvalidData);
                            }
                            let original = row
                                .get::<_, Vec<u8>>(1)
                                .ok()
                                .and_then(|v| v.try_into().ok())
                                .map(BindingId);
                            Ok((original, copy))
                        })();
                        match decoded {
                            Ok(copy) => readable.push(copy),
                            Err(_) => complete = false,
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        complete = false;
                        break;
                    }
                }
            },
            Err(_) => complete = false,
        };
    } else {
        // v1 has no conflicts table by definition; zero copies is complete
        // only if we can establish that exact old schema fact.
        let count: Option<i64> = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='conflicts'",
                [],
                |r| r.get(0),
            )
            .ok();
        complete = version == Some(1) && count == Some(0);
    }
    commit_copies(store, source, Some(&connection), readable, complete, clock, random)
}

fn commit_copies(
    store: &mut TrackingStore,
    source: &str,
    old: Option<&Connection>,
    copies: Vec<(Option<BindingId>, ConflictCopy)>,
    complete: bool,
    clock: &dyn Clock,
    random: &dyn RandomSource,
) -> Result<bool, Error> {
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut groups: Vec<(Option<BindingId>, BindingId)> = Vec::new();
    for (original, copy) in copies {
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM conflicts WHERE id=?1)",
            [copy.id.0.as_slice()],
            |r| r.get(0),
        )?;
        if exists {
            continue;
        }
        let owner = match groups.iter().find(|(prior, _)| *prior == original) {
            Some((_, owner)) => *owner,
            None => {
                let id = new_binding_id(random);
                let occupied: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE id=?1 UNION ALL SELECT 1 FROM salvage_bindings WHERE id=?1)", [id.0.as_slice()], |r| r.get(0))?;
                if occupied {
                    return Err(Error::Constraint);
                }
                let metadata = provenance(old, source, original, id, clock.now_ms());
                transaction.execute(
                    "INSERT INTO salvage_bindings(id,metadata) VALUES (?1,?2)",
                    params![id.0.as_slice(), encode(&metadata)?],
                )?;
                groups.push((original, id));
                id
            }
        };
        transaction.execute(
            "INSERT INTO conflicts(id,binding,content) VALUES (?1,?2,?3)",
            params![copy.id.0.as_slice(), owner.0.as_slice(), encode(&copy)?],
        )?;
    }
    transaction.commit()?;
    Ok(complete)
}

fn provenance(
    connection: Option<&Connection>,
    source: &str,
    original: Option<BindingId>,
    id: BindingId,
    now: i64,
) -> SalvageBinding {
    let mut result = SalvageBinding {
        id,
        source: source.to_owned(),
        original_binding: original,
        created_at_ms: now,
        original_created_at_ms: None,
        container: None,
        environment: None,
        account: None,
        epoch: None,
        record_key: None,
        publication: None,
    };
    if let (Some(connection), Some(original)) = (connection, original) {
        // Separate reads retain readable columns even if another is damaged.
        let column = |name: &str| -> Option<rusqlite::types::Value> {
            connection
                .query_row(
                    &format!("SELECT {name} FROM bindings WHERE id=?1"),
                    [original.0.as_slice()],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten()
        };
        if let Some(rusqlite::types::Value::Text(container)) = column("container") {
            result.container = ContainerId::new(container).ok();
        }
        if let Some(rusqlite::types::Value::Integer(environment)) = column("environment") {
            result.environment = match environment {
                0 => Some(Environment::Development),
                1 => Some(Environment::Production),
                _ => None,
            };
        }
        if let Some(rusqlite::types::Value::Integer(created)) = column("created_at_ms") {
            result.original_created_at_ms = Some(created);
        }
        if let Ok(Some(prior)) = connection
            .query_row(
                "SELECT metadata FROM salvage_bindings WHERE id=?1",
                [original.0.as_slice()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            && let Ok(prior) = decode::<SalvageBinding>(&prior)
        {
            result.container = prior.container;
            result.environment = prior.environment;
            result.account = prior.account;
            result.epoch = prior.epoch;
            result.record_key = prior.record_key;
            result.publication = prior.publication;
            result.original_created_at_ms = prior.original_created_at_ms;
        }
        if let Some(rusqlite::types::Value::Text(facts)) = column("facts")
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&facts)
        {
            fn field<T: serde::de::DeserializeOwned>(value: &serde_json::Value, key: &str) -> Option<T> {
                serde_json::from_value(value.get(key)?.clone()).ok()
            }
            result.account = field(&value, "account");
            result.epoch = field(&value, "epoch");
            result.record_key = field(&value, "record_key");
            result.publication = field(&value, "publication");
        }
    }
    result
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
