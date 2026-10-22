//! The open gate (`storage/open-and-connections` C2): one function, callable
//! on any file path (the live collection, and U124's staging copies before a
//! swap), that probes, refuses, copies, opens, upgrades and checks the
//! expected objects, in that order, stopping at the first refusal. Nothing
//! writes to a file that phase 2 refuses or creates a file next to it (C3,
//! R1479).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use crate::checkpoint::CheckpointOutcome;
use crate::conn_init::{self, Initializers};
use crate::connections::{READ_WRITE, Reader, ReaderFactory, ReaderRole, Writer, file_uri, sibling};
use crate::error::{OpenError, StorageError, UpgradeStage};
use crate::journal::JournalOptions;
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
    managed: bool,
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
            managed: false,
        }
    }

    /// Engine-owned collection with device-only durability metadata (U32).
    pub fn managed(mut self) -> Self {
        self.managed = true;
        self
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
    _registered: Registered,
    managed: bool,
    pub previous_clean_shutdown: bool,
    pub external_change: bool,
}

/// Paths of the collections open through the gate in this process, with how
/// many times each is open: the storage-only builder refuses them.
static OPEN_PATHS: Mutex<BTreeMap<PathBuf, usize>> = Mutex::new(BTreeMap::new());

/// A path in one form whatever way it was given: its directory resolved.
fn registry_key(path: &Path) -> PathBuf {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    match (dir.canonicalize(), path.file_name()) {
        (Ok(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

/// Whether a collection is open at `path` in this process.
pub(crate) fn is_open(path: &Path) -> bool {
    let key = registry_key(path);
    OPEN_PATHS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains_key(&key)
}

/// An open collection's place in [`OPEN_PATHS`], released when it drops.
#[derive(Debug)]
struct Registered(PathBuf);

impl Registered {
    fn new(path: &Path) -> Registered {
        let key = registry_key(path);
        *OPEN_PATHS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.clone())
            .or_default() += 1;
        Registered(key)
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        let mut paths = OPEN_PATHS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = paths.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                paths.remove(&self.0);
            }
        }
    }
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

    /// The write journal's options for this collection: the default spill
    /// threshold, spilling into the collection's directory.
    pub fn journal_options(&self) -> JournalOptions {
        match self.path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => JournalOptions::spilling_into(dir),
            _ => JournalOptions::spilling_into(Path::new(".")),
        }
    }

    /// Opens a reader of `role` (C6). The caller keeps it until the
    /// collection closes and closes it before [`OpenedCollection::close`].
    pub fn open_reader(&self, role: ReaderRole) -> Result<Reader, StorageError> {
        Reader::open(&self.path, role, &self.initializers)
    }

    /// What opens this collection's readers on other threads: the writer's
    /// thread owns the collection (U24).
    pub fn reader_factory(&self) -> ReaderFactory {
        ReaderFactory::new(&self.path, &self.initializers)
    }

    pub fn save_sidecar(&self, clean: bool) -> Result<(), StorageError> {
        use crate::ReadRows;
        let path = crate::atomic_file::parent(&self.path).join("meguri-sidecar.json");
        let col = self.writer.col()?;
        crate::sidecar::Sidecar::update(&path, |sidecar, _| {
            sidecar.clean_shutdown = clean;
            sidecar.last_mod = col.modified.0;
            sidecar.last_scm = col.schema_modified.0;
            sidecar.upgrade_in_progress = false;
        })
    }

    /// The storage side of closing (C10), once every reader is closed: the
    /// checkpoint primitive, then the writer closes and SQLite removes the
    /// `-wal` and `-shm` files. The writer closes even when the checkpoint
    /// was busy; the outcome says so.
    pub fn close(self) -> Result<CheckpointOutcome, StorageError> {
        let outcome = self.writer.checkpoint();
        if self.managed && outcome.as_ref().is_ok_and(|o| o.succeeded()) {
            self.save_sidecar(true)?;
        }
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

    // The managed path has passed all read-only refusals. Record upgrade intent
    // before the first write; failed upgrades deliberately leave this marker.
    let sidecar_path = crate::atomic_file::parent(path).join("meguri-sidecar.json");
    let (previous, known_previous) = if options.managed {
        crate::sidecar::Sidecar::update(&sidecar_path, |value, known| {
            let previous = value.clone();
            value.clean_shutdown = false;
            value.upgrade_in_progress = upgrading;
            (previous, known)
        })
        .map_err(|e| OpenError::Io { detail: e.to_string() })?
    } else {
        (crate::sidecar::Sidecar::default(), false)
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
    use crate::ReadRows;
    let col = writer.col().map_err(|e| OpenError::Io { detail: e.to_string() })?;
    let external_change = options.managed
        && known_previous
        && (previous.last_mod != col.modified.0 || previous.last_scm != col.schema_modified.0);
    let opened = OpenedCollection {
        path: path.to_path_buf(),
        writer,
        initializers: options.initializers.clone(),
        upgraded_from: upgrading.then_some(probed.version),
        pre_upgrade_copy,
        objects,
        _registered: Registered::new(path),
        managed: options.managed,
        previous_clean_shutdown: previous.clean_shutdown && !previous.upgrade_in_progress,
        external_change,
    };
    if options.managed {
        opened
            .save_sidecar(false)
            .map_err(|e| OpenError::Io { detail: e.to_string() })?;
    }
    Ok(opened)
}

/// Phase 1, with C3's hot-journal case. A refused file must keep its bytes
/// and its journal (R1479), and only a rollback shows the version, so phases
/// 1 and 2 first run on a copy of the file and its journal, rolled back in a
/// temporary directory. Only when the copy passes the refusals is the file
/// itself rolled back (opened read-write without changing its journal mode
/// and read once, so SQLite rolls the journal back) and probed again.
fn probe(path: &Path) -> Result<Probed, OpenError> {
    match version::probe(path) {
        Ok(probed) => Ok(probed),
        Err(ProbeError::Open(error)) => Err(error),
        Err(ProbeError::HotJournal) => {
            refuse::refuse(&probe_rolled_back_copy(path)?)?;
            roll_back_hot_journal(path)?;
            probe_after_rollback(path)
        }
    }
}

fn probe_after_rollback(path: &Path) -> Result<Probed, OpenError> {
    match version::probe(path) {
        Ok(probed) => Ok(probed),
        Err(ProbeError::Open(error)) => Err(error),
        Err(ProbeError::HotJournal) => Err(OpenError::Io {
            detail: "the hot rollback journal is still there after a rollback".to_owned(),
        }),
    }
}

/// Phase 1 on a copy of `path` and its hot journal, rolled back in a
/// temporary directory that is removed when it returns. `path` and its
/// journal are only read.
fn probe_rolled_back_copy(path: &Path) -> Result<Probed, OpenError> {
    let io = |e: std::io::Error| OpenError::Io {
        detail: format!("copying the file and its hot rollback journal: {e}"),
    };
    let dir = tempfile::Builder::new().prefix("meguri-probe-").tempdir().map_err(io)?;
    let copy = dir.path().join("collection.anki2");
    fs::copy(path, &copy).map_err(io)?;
    fs::copy(sibling(path, "-journal"), sibling(&copy, "-journal")).map_err(io)?;
    roll_back_hot_journal(&copy)?;
    probe_after_rollback(&copy)
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
