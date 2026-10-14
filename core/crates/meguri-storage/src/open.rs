//! The open gate (`storage/open-and-connections` C2): one function, callable
//! on any file path (the live collection, and U124's staging copies before a
//! swap), that probes, refuses, copies, opens, upgrades and checks the
//! expected objects, in that order, stopping at the first refusal. Before
//! the writer opens, nothing has written to the file or created a file next
//! to it (C3).

use std::fs;
use std::path::{Path, PathBuf};

use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use crate::checkpoint::CheckpointOutcome;
use crate::conn_init::{self, Initializers};
use crate::connections::{READ_WRITE, Reader, ReaderRole, Writer, file_uri, sibling};
use crate::error::{OpenError, StorageError, UpgradeStage};
use crate::schema::expected_objects::{self, ExpectedObjects};
use crate::schema::upgrade::{self, UpgradeContext};
use crate::schema::version::{self, ProbeError, Probed};
use crate::schema::{SCHEMA_VERSION, refuse};

/// What the gate reports while it opens (C2.4). U24 turns these into the
/// lifecycle states `Opening` and `Upgrading`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenEvent {
    Opening,
    /// An upgrade step starts: the one that sets `col.ver` to `step`.
    Upgrading {
        from: i64,
        step: u32,
    },
}

/// What the gate needs besides the path.
pub struct OpenOptions<'a> {
    clock: &'a dyn Clock,
    random: &'a dyn RandomSource,
    initializers: Initializers,
    progress: Option<&'a dyn Fn(OpenEvent)>,
}

impl<'a> OpenOptions<'a> {
    /// The upgrade stamps renamed decks with `clock`'s instant and draws a
    /// new note type id from `random` (KTD18).
    pub fn new(clock: &'a dyn Clock, random: &'a dyn RandomSource) -> OpenOptions<'a> {
        OpenOptions {
            clock,
            random,
            initializers: Initializers::default(),
            progress: None,
        }
    }

    /// The initializers every writer and reader of the collection runs
    /// (C5).
    pub fn initializers(mut self, initializers: Initializers) -> OpenOptions<'a> {
        self.initializers = initializers;
        self
    }

    pub fn progress(mut self, progress: &'a dyn Fn(OpenEvent)) -> OpenOptions<'a> {
        self.progress = Some(progress);
        self
    }

    fn report(&self, event: OpenEvent) {
        if let Some(progress) = self.progress {
            progress(event);
        }
    }
}

/// A collection the gate opened: its writer, the version it was upgraded
/// from and what the expected-object check found besides the required
/// tables.
#[derive(Debug)]
pub struct OpenedCollection {
    path: PathBuf,
    writer: Writer,
    initializers: Initializers,
    upgraded_from: Option<i64>,
    pre_upgrade_copy: Option<PathBuf>,
    objects: ExpectedObjects,
}

impl OpenedCollection {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn writer(&self) -> &Writer {
        &self.writer
    }

    /// The version the file had, when the gate upgraded it.
    pub fn upgraded_from(&self) -> Option<i64> {
        self.upgraded_from
    }

    /// The pre-upgrade copy the gate made (`storage/schema-upgrade` U2).
    /// The storage layer never deletes it.
    pub fn pre_upgrade_copy(&self) -> Option<&Path> {
        self.pre_upgrade_copy.as_deref()
    }

    /// Missing indexes and foreign objects (`storage/schema-18` S6.1); no
    /// table is missing.
    pub fn expected_objects(&self) -> &ExpectedObjects {
        &self.objects
    }

    /// Opens a reader of `role` (C6). The caller keeps it until the
    /// collection closes and closes it before [`OpenedCollection::close`].
    pub fn open_reader(&self, role: ReaderRole) -> Result<Reader, StorageError> {
        Reader::open(&self.path, role, &self.initializers)
    }

    /// The storage side of closing (C10), once every reader is closed: the
    /// checkpoint primitive, then the writer closes and SQLite removes the
    /// `-wal` and `-shm` files. The writer closes even when the checkpoint
    /// was busy; the outcome says so.
    pub fn close(self) -> Result<CheckpointOutcome, StorageError> {
        let outcome = self.writer.checkpoint();
        self.writer.conn.close().map_err(|(_, e)| StorageError::from(e))?;
        outcome
    }
}

/// The open gate (C2) on the file at `path`.
pub fn open_collection(path: &Path, options: &OpenOptions<'_>) -> Result<OpenedCollection, OpenError> {
    options.report(OpenEvent::Opening);
    match fs::metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(OpenError::NotFound),
        Err(e) => return Err(OpenError::Io { detail: e.to_string() }),
    }

    // Phases 1 and 2.
    let probed = probe(path)?;
    refuse::refuse(&probed)?;

    // Phase 3.
    let upgrading = probed.version < SCHEMA_VERSION;
    let pre_upgrade_copy = if upgrading {
        let copy = pre_upgrade_copy(path, probed.version)?;
        meguri_model::faultpoint!("storage.upgrade.after_copy").map_err(|e| OpenError::UpgradeFailed {
            version: probed.version,
            stage: UpgradeStage::Copy,
            detail: e.to_string(),
        })?;
        Some(copy)
    } else {
        None
    };

    // Phase 4.
    let writer = Writer::open(path, &options.initializers)?;
    if upgrading {
        let context = UpgradeContext {
            clock: options.clock,
            random: options.random,
            progress: &|step| {
                options.report(OpenEvent::Upgrading {
                    from: probed.version,
                    step,
                })
            },
        };
        upgrade::upgrade(&writer.conn, probed.version, &context)?;
    }

    // Phase 5.
    let objects = expected_objects::read(&writer.conn).map_err(|e| OpenError::from_sqlite(&e))?;
    let objects = expected_objects::check(&objects);
    if !objects.missing_tables.is_empty() {
        let missing_tables = objects.missing_tables;
        // The writer closes; an upgrade committed in phase 4 stays committed.
        drop(writer);
        return Err(OpenError::NotACollection { missing_tables });
    }
    Ok(OpenedCollection {
        path: path.to_path_buf(),
        writer,
        initializers: options.initializers.clone(),
        upgraded_from: upgrading.then_some(probed.version),
        pre_upgrade_copy,
        objects,
    })
}

/// Phase 1, with C3's hot-journal case: the file is opened read-write
/// without changing its journal mode and read once, so SQLite rolls the
/// journal back, then probed again.
fn probe(path: &Path) -> Result<Probed, OpenError> {
    match version::probe(path) {
        Ok(probed) => Ok(probed),
        Err(ProbeError::Open(error)) => Err(error),
        Err(ProbeError::HotJournal) => {
            roll_back_hot_journal(path)?;
            match version::probe(path) {
                Ok(probed) => Ok(probed),
                Err(ProbeError::Open(error)) => Err(error),
                Err(ProbeError::HotJournal) => Err(OpenError::Io {
                    detail: "the hot rollback journal is still there after a rollback".to_owned(),
                }),
            }
        }
    }
}

fn roll_back_hot_journal(path: &Path) -> Result<(), OpenError> {
    let conn = Connection::open_with_flags(file_uri(path, None), READ_WRITE).map_err(|e| OpenError::from_sqlite(&e))?;
    conn_init::probe(&conn).map_err(|detail| OpenError::ConnectionInit { detail })?;
    conn.query_row("SELECT ver FROM col LIMIT 1", [], |_| Ok(()))
        .map_err(|e| OpenError::from_sqlite(&e))?;
    conn.close().map_err(|(_, e)| OpenError::from_sqlite(&e))
}

/// Phase 3 (`storage/schema-upgrade` U2): SQLite's online backup from a
/// read-only connection to the original, opened as the probe opens it, into
/// `<copy>.tmp`, synced, then renamed over `<name>.pre-upgrade-v<V>`.
fn pre_upgrade_copy(path: &Path, version: i64) -> Result<PathBuf, OpenError> {
    let failed = |detail: String| OpenError::UpgradeFailed {
        version,
        stage: UpgradeStage::Copy,
        detail,
    };
    let copy = sibling(path, &format!(".pre-upgrade-v{version}"));
    let tmp = sibling(&copy, ".tmp");
    let result = (|| {
        remove_if_present(&tmp)?;
        remove_if_present(&sibling(&tmp, "-journal"))?;
        let source = version::open_read_only(path).map_err(|e| match e {
            ProbeError::Open(error) => error.to_string(),
            ProbeError::HotJournal => "hot rollback journal".to_owned(),
        })?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut target = Connection::open_with_flags(file_uri(&tmp, None), flags).map_err(|e| e.to_string())?;
        conn_init::probe(&target)?;
        match Backup::new(&source, &mut target).and_then(|backup| backup.step(-1)) {
            Ok(StepResult::Done) => {}
            Ok(other) => return Err(format!("the backup stopped before the end: {other:?}")),
            Err(e) => return Err(e.to_string()),
        }
        target.close().map_err(|(_, e)| e.to_string())?;
        source.close().map_err(|(_, e)| e.to_string())?;
        fs::File::open(&tmp)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&tmp, &copy).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => Ok(copy),
        Err(detail) => {
            let _ = fs::remove_file(&tmp);
            Err(failed(detail))
        }
    }
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
