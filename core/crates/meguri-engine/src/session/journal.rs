//! C12 ordered file worker. It owns no collection/SQLite/dispatcher/session lock.
pub(in crate::session) mod managed;
use super::frozen::{Base, FrozenAnswer, error};
use crate::{EngineError, Generation, Qos, Seq, ThreadRegistry, monotonic};
use meguri_storage::atomic_file::{AtomicFile, Purpose, full_sync};
use meguri_storage::persistent_identity::PersistentIdentity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
    time::Duration,
};

#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalStage {
    IntentWrite,
    IntentSync,
    IntentSynced,
    PreparedWrite,
    PreparedSync,
    PreparedSynced,
    TerminalWrite,
    TerminalSync,
    TerminalSynced,
    MigrationPublished,
    Compact,
}
#[cfg(feature = "test-hooks")]
type StageHook = Arc<dyn Fn(JournalStage) + Send + Sync>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    path: Vec<u8>,
    native: Native,
    incarnation: [u64; 2],
    next_event: u64,
    #[serde(default, deserialize_with = "decode_notice", skip_serializing_if = "Option::is_none")]
    notice: Option<MigrationNotice>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum Native {
    Legacy((u64, u64)),
    Persistent(PersistentIdentity),
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MigrationReason {
    LegacyHeaderOnlyRebound,
    LegacyIdentityUpgraded,
    PristineV2Rebound,
}
impl MigrationReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::LegacyHeaderOnlyRebound => "legacy_header_only_rebound",
            Self::LegacyIdentityUpgraded => "legacy_identity_upgraded",
            Self::PristineV2Rebound => "pristine_v2_rebound",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct MigrationNotice {
    reason: MigrationReason,
    archive_sha256: [u8; 32],
}
fn decode_notice<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Option<MigrationNotice>, D::Error> {
    MigrationNotice::deserialize(decoder).map(Some)
}
fn corruption() -> EngineError {
    error("pending_answer_journal_corruption")
}
fn identity_mismatch() -> EngineError {
    error("pending_answer_identity_mismatch")
}
fn migration_io() -> EngineError {
    error("pending_answer_identity_migration_io")
}
fn migration_failure(failure: EngineError) -> EngineError {
    if matches!(&failure,EngineError::Storage(meguri_storage::StorageError::Io { detail })
        if detail == "pending_answer_identity_mismatch")
    {
        identity_mismatch()
    } else {
        migration_io()
    }
}
fn require_migration_source(binding: &RecoveryBinding, path: &Path) -> Result<(), meguri_storage::StorageError> {
    binding.require(path).map_err(|_| meguri_storage::StorageError::Io {
        detail: "pending_answer_identity_mismatch".into(),
    })
}
fn identity_matches(native: Native, durable: Native, live: (u64, u64)) -> bool {
    match native {
        Native::Legacy(pair) => pair == live,
        Native::Persistent(_) => native == durable,
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub row: [i64; 9],
    pub base: [u8; 32],
    pub post: [u8; 32],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Terminal {
    Done,
    Cancelled,
    Dropped,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Record {
    Header(Header),
    Intent { event: u64, answer: FrozenAnswer },
    Prepared { event: u64, receipt: Receipt },
    Terminal { event: u64, status: Terminal },
    Retired,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    number: u64,
    payload: String,
    checksum: [u8; 32],
}
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub answer: FrozenAnswer,
    pub receipt: Option<Receipt>,
    pub terminal: Option<Terminal>,
    pub committed: Option<(Generation, Seq)>,
}
#[derive(Clone)]
struct State {
    header: Header,
    events: BTreeMap<u64, Entry>,
    next_event: u64,
    failure: Option<EngineError>,
    retired: bool,
}
enum Work {
    Append(Box<Record>, Option<mpsc::SyncSender<Result<(), EngineError>>>, Duration),
    Barrier(mpsc::SyncSender<Result<(), EngineError>>),
    Compact(mpsc::SyncSender<Result<(), EngineError>>),
    Migrate(PersistentIdentity, mpsc::SyncSender<Result<(), EngineError>>),
    OpeningEvidence(Box<managed::Evidence>, mpsc::SyncSender<Result<(), EngineError>>),
    RetiredSource((u64, u64), mpsc::SyncSender<Result<RecoveryBinding, EngineError>>),
    #[cfg(feature = "test-hooks")]
    Fault(JournalStage, mpsc::SyncSender<()>),
    #[cfg(feature = "test-hooks")]
    Hook(StageHook, mpsc::SyncSender<()>),
    Stop,
}
pub(crate) struct Journal {
    tx: mpsc::Sender<Work>,
    state: Arc<Mutex<State>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    generation: Generation,
    live_native: (u64, u64),
    live_canonical: Vec<u8>,
    retired_binding: Mutex<Option<RecoveryBinding>>,
}
fn encode(number: u64, record: &Record) -> Result<Vec<u8>, EngineError> {
    let payload = serde_json::to_string(record).map_err(error)?;
    let checksum = checksum(number, payload.as_bytes());
    let mut bytes = serde_json::to_vec(&Frame {
        number,
        payload,
        checksum,
    })
    .map_err(error)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn checksum(number: u64, payload: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"meguri.pending.frame.v1\0");
    hash.update(number.to_le_bytes());
    hash.update(payload);
    hash.finalize().into()
}
fn apply(state: &mut State, record: Record) -> Result<(), EngineError> {
    if state.retired {
        return Err(error("record after retired incarnation"));
    }
    match record {
        Record::Header(_) => return Err(error("interior header")),
        Record::Intent { event, answer } => {
            if event != state.next_event {
                return Err(error("noncontiguous event sequence"));
            }
            if let Base::Predecessor(parent) = answer.base {
                let prior = state.events.get(&parent).ok_or_else(|| error("missing predecessor"))?;
                if parent >= event || prior.answer.card != answer.card {
                    return Err(error("invalid same-card dependency"));
                }
            }
            answer.request([0; 32])?;
            state.next_event = event.checked_add(1).ok_or_else(|| error("event sequence exhausted"))?;
            state.events.insert(
                event,
                Entry {
                    answer,
                    receipt: None,
                    terminal: None,
                    committed: None,
                },
            );
        }
        Record::Prepared { event, receipt } => {
            let e = state
                .events
                .get_mut(&event)
                .ok_or_else(|| error("receipt without intent"))?;
            if e.terminal.is_some()
                || e.receipt.as_ref().is_some_and(|r| r != &receipt)
                || receipt.row[1] != e.answer.card
            {
                return Err(error("identity changed or receipt after terminal"));
            }
            e.receipt = Some(receipt);
        }
        Record::Terminal { event, status } => {
            let e = state
                .events
                .get_mut(&event)
                .ok_or_else(|| error("terminal without intent"))?;
            if e.terminal.is_some_and(|previous| previous != status)
                || (status == Terminal::Done && e.receipt.is_none())
            {
                return Err(error("invalid terminal transition"));
            }
            e.terminal = Some(status);
        }
        Record::Retired => {
            if state.events.values().any(|e| e.terminal.is_none()) {
                return Err(error("retiring unresolved incarnation"));
            }
            state.retired = true;
        }
    }
    Ok(())
}
#[cfg(test)]
fn read(path: &Path, expected: &Header) -> Result<(State, u64, usize), EngineError> {
    read_bytes(&std::fs::read(path).map_err(error)?, expected)
}
#[cfg(test)]
fn read_bytes(bytes: &[u8], expected: &Header) -> Result<(State, u64, usize), EngineError> {
    let live = match expected.native {
        Native::Legacy(pair) => pair,
        Native::Persistent(PersistentIdentity::NonDarwin { device, inode }) => (device, inode),
        Native::Persistent(PersistentIdentity::Darwin { inode, .. }) => (7, inode),
    };
    read_bytes_bound(bytes, expected, live)
}
fn read_bytes_bound(bytes: &[u8], expected: &Header, live: (u64, u64)) -> Result<(State, u64, usize), EngineError> {
    let complete = bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    if complete == 0 {
        return Err(corruption());
    }
    let mut state = None;
    let mut number = 0;
    for line in bytes[..complete - 1].split(|b| *b == b'\n') {
        let frame: Frame = serde_json::from_slice(line).map_err(|_| corruption())?;
        number += 1;
        if frame.number != number || frame.checksum != checksum(frame.number, frame.payload.as_bytes()) {
            return Err(corruption());
        }
        let record: Record = serde_json::from_str(&frame.payload).map_err(|_| corruption())?;
        if number == 1 {
            let Record::Header(h) = record else {
                return Err(corruption());
            };
            let valid = match (h.version, h.native) {
                (1, Native::Legacy(_)) => h.notice.is_none(),
                (2, Native::Persistent(identity)) => identity.valid_for_platform(),
                _ => false,
            };
            if !valid || h.next_event == 0 {
                return Err(corruption());
            }
            if h.path != expected.path {
                return Err(identity_mismatch());
            }
            state = Some(State {
                next_event: h.next_event,
                header: h,
                events: BTreeMap::new(),
                failure: None,
                retired: false,
            });
        } else {
            apply(state.as_mut().ok_or_else(corruption)?, record).map_err(|_| corruption())?;
        }
    }
    let state = state.ok_or_else(corruption)?;
    if !state.retired && !identity_matches(state.header.native, expected.native, live) {
        // The sole legacy admission exception is one complete Header frame,
        // exact EOF, same path and inode, and device number alone changed.
        let header_only = matches!(state.header.native, Native::Legacy((device,inode))
            if device != live.0 && inode == live.1)
            && number == 1
            && complete == bytes.len();
        if !header_only {
            return Err(identity_mismatch());
        }
    }
    if bytes.len() != complete {
        tracing::warn!("ignoring incomplete pending-answer final frame");
    }
    Ok((state, number, complete))
}
fn create(path: &Path, header: &Header, replace: bool) -> Result<(), EngineError> {
    let mut file = AtomicFile::new(path, Purpose::Sidecar)?;
    file.file_mut()
        .write_all(&encode(1, &Record::Header(header.clone()))?)
        .map_err(error)?;
    if replace {
        file.publish(|_| Ok(()))?;
    } else {
        file.publish_new(|_| Ok(()))?;
    }
    Ok(())
}
/// Recovery-only replacement has no review worker and must never replay old
/// answers. Under advisory/exclusive ownership, preserve the entire validated
/// file (including a torn tail) before installing a retired binding marker.
/// Archives are permanent evidence, outside the temporary sweep namespace.
pub(crate) struct RecoveryJournal {
    path: PathBuf,
    collection: PathBuf,
    native: (u64, u64),
    original: RecoveryBinding,
    marker: RecoveryBinding,
}
impl RecoveryJournal {
    pub(crate) fn prepare(collection: &Path) -> Result<Option<Self>, EngineError> {
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        let path = collection.with_file_name("meguri-pending-answers.jsonl");
        if !path.try_exists().map_err(error)? {
            // A dangling link is evidence, not absence.
            if std::fs::symlink_metadata(&path).is_ok() {
                return Err(error("invalid recovery journal binding; evidence retained"));
            }
            return Ok(None);
        }
        // No SQLite open or recovery. Release this descriptor before any new
        // writer opens: closing a second descriptor can release POSIX locks.
        let main = recovery_file(collection)?;
        let native = main.metadata().map_err(error)?;
        let expected = Header {
            version: 2,
            path: collection
                .canonicalize()
                .map_err(error)?
                .as_os_str()
                .as_bytes()
                .to_vec(),
            native: Native::Persistent(meguri_storage::persistent_identity::read_from(
                &main,
                (native.dev(), native.ino()),
            )?),
            incarnation: [0, 0],
            next_event: 1,
            notice: None,
        };
        let original = RecoveryBinding::read(&path)?;
        let (loaded, _, _) = read_bytes_bound(&original.bytes, &expected, (native.dev(), native.ino()))?;
        // Even a retired file must bind to this old main in this recovery path.
        if !identity_matches(loaded.header.native, expected.native, (native.dev(), native.ino())) {
            return Err(error("unexpected recovery journal native identity; evidence retained"));
        }
        if loaded.retired {
            return Ok(None);
        }
        let digest = Sha256::digest(&original.bytes);
        let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let archive = collection.with_file_name(format!("meguri-pending-answers.{name}.recovery.jsonl"));
        if archive.try_exists().map_err(error)? {
            if recovery_bytes(&archive)? != original.bytes {
                return Err(error("recovery journal archive differs; evidence retained"));
            }
            full_sync(&recovery_file(&archive)?)?;
            meguri_storage::atomic_file::sync_directory(archive.parent().unwrap())?;
        } else {
            let mut copy = AtomicFile::new(&archive, Purpose::Sidecar)?;
            copy.file_mut().write_all(&original.bytes).map_err(error)?;
            copy.publish_new(|_| Ok(()))?;
        }
        let unresolved = loaded.events.values().filter(|e| e.terminal.is_none()).count();
        let mut header = loaded.header;
        header.next_event = loaded.next_event;
        let mut marker = encode(1, &Record::Header(header))?;
        marker.extend(encode(2, &Record::Retired)?);
        let current = std::fs::symlink_metadata(collection).map_err(error)?;
        if !current.is_file() || (current.dev(), current.ino()) != (native.dev(), native.ino()) {
            return Err(error("recovery main identity changed; evidence retained"));
        }
        let mut publication = AtomicFile::new(&path, Purpose::Sidecar)?;
        publication.file_mut().write_all(&marker).map_err(error)?;
        // Retain both inode and byte authorities before publication. An Err
        // from AtomicFile may follow rename or directory publication.
        let guard = Self {
            path,
            collection: collection.to_owned(),
            native: (native.dev(), native.ino()),
            original,
            marker: RecoveryBinding {
                file: publication.file_mut().try_clone().map_err(error)?,
                bytes: marker,
            },
        };
        let published = publication
            .publish(|_| guard.original.require(&guard.path))
            .and_then(|()| guard.marker.require(&guard.path));
        if let Err(publication_error) = published {
            return match guard.finish(false) {
                Ok(()) => Err(publication_error.into()),
                Err(rollback_error) => Err(error(format!(
                    "recovery marker publication failed: {publication_error}; rollback failed or uncertain: {rollback_error}; archive retained"
                ))),
            };
        }
        tracing::warn!(path = %archive.display(), unresolved, "old pending-answer evidence archived for recovery replacement; not replayed");
        Ok(Some(guard))
    }

    /// A proven preactivation failure restores the exact old active file. The
    /// archive remains durable across failures/crashes. Never roll back after
    /// activation or an uncertain activation.
    pub(crate) fn finish(self, activated: bool) -> Result<(), EngineError> {
        if activated {
            return Ok(());
        }
        if recovery_identity(&self.collection)? != self.native {
            return Err(error(
                "recovery main binding is uncertain; retired marker and archive retained",
            ));
        }
        if self.original.matches(&self.path)? {
            return self.original.sync(&self.path);
        }
        if let Err(error) = self.marker.require(&self.path) {
            meguri_model::faultpoint!("session.journal.recovery.rollback_refused")
                .map_err(meguri_storage::StorageError::from)?;
            return Err(error.into());
        }
        let mut rollback = AtomicFile::new(&self.path, Purpose::Sidecar)?;
        rollback.file_mut().write_all(&self.original.bytes).map_err(error)?;
        let restored = RecoveryBinding {
            file: rollback.file_mut().try_clone().map_err(error)?,
            bytes: self.original.bytes,
        };
        match rollback.publish(|_| self.marker.require(&self.path)) {
            Ok(()) => Ok(()),
            Err(publication_error) => {
                // A post-rename error can still be completed durably, but an
                // unknown binding must never be overwritten with old answers.
                restored.require(&self.path).map_err(|proof_error| {
                    error(format!(
                        "journal rollback publication failed: {publication_error}; binding uncertain: {proof_error}; archive retained"
                    ))
                })?;
                restored.sync(&self.path)
            }
        }
    }
}

struct RecoveryBinding {
    file: File,
    bytes: Vec<u8>,
}
impl RecoveryBinding {
    fn read(path: &Path) -> Result<Self, EngineError> {
        let mut file = recovery_file(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(error)?;
        let binding = Self { file, bytes };
        binding.require(path)?;
        Ok(binding)
    }
    fn matches(&self, path: &Path) -> Result<bool, EngineError> {
        use std::os::unix::fs::MetadataExt;
        let mut current = recovery_file(path)?;
        let expected = self.file.metadata().map_err(error)?;
        let named = current.metadata().map_err(error)?;
        if (named.dev(), named.ino()) != (expected.dev(), expected.ino()) {
            return Ok(false);
        }
        let mut bytes = Vec::new();
        current.read_to_end(&mut bytes).map_err(error)?;
        let named = std::fs::symlink_metadata(path).map_err(error)?;
        Ok(named.is_file()
            && named.nlink() == 1
            && (named.dev(), named.ino()) == (expected.dev(), expected.ino())
            && bytes == self.bytes)
    }
    fn require(&self, path: &Path) -> Result<(), meguri_storage::StorageError> {
        match self.matches(path) {
            Ok(true) => Ok(()),
            result => Err(meguri_storage::StorageError::Io {
                detail: format!("recovery journal binding changed ({result:?}); evidence retained"),
            }),
        }
    }
    fn sync(&self, path: &Path) -> Result<(), EngineError> {
        self.require(path)?;
        full_sync(&self.file)?;
        meguri_storage::atomic_file::sync_directory(path.parent().unwrap())?;
        self.require(path)?;
        Ok(())
    }
}
pub(crate) fn recovery_identity(collection: &Path) -> Result<(u64, u64), EngineError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = recovery_file(collection)?.metadata().map_err(error)?;
    Ok((metadata.dev(), metadata.ino()))
}
fn recovery_file(path: &Path) -> Result<File, EngineError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(error)?;
    let metadata = file.metadata().map_err(error)?;
    let named = std::fs::symlink_metadata(path).map_err(error)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || !named.is_file()
        || (metadata.dev(), metadata.ino()) != (named.dev(), named.ino())
    {
        return Err(error("unsafe recovery journal file binding; evidence retained"));
    }
    Ok(file)
}
fn recovery_bytes(path: &Path) -> Result<Vec<u8>, EngineError> {
    let mut bytes = Vec::new();
    recovery_file(path)?.read_to_end(&mut bytes).map_err(error)?;
    Ok(bytes)
}

fn same_file(left: &File, right: &File) -> Result<(), EngineError> {
    use std::os::unix::fs::MetadataExt;
    let left = left.metadata().map_err(error)?;
    let right = right.metadata().map_err(error)?;
    if !left.is_file() || left.nlink() != 1 || (left.dev(), left.ino()) != (right.dev(), right.ino()) {
        return Err(identity_mismatch());
    }
    Ok(())
}
fn append_file(path: &Path) -> Result<File, EngineError> {
    use std::os::unix::fs::OpenOptionsExt;
    let retained = recovery_file(path)?;
    let file = OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(error)?;
    same_file(&file, &retained)?;
    Ok(file)
}
fn require_live(collection: &Path, native: (u64, u64), canonical: &[u8]) -> Result<(), meguri_storage::StorageError> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    let metadata = std::fs::metadata(collection)?;
    if !metadata.is_file()
        || (metadata.dev(), metadata.ino()) != native
        || collection.canonicalize()?.as_os_str().as_bytes() != canonical
    {
        return Err(meguri_storage::StorageError::Io {
            detail: "pending_answer_identity_mismatch".into(),
        });
    }
    Ok(())
}
struct Migration {
    collection: PathBuf,
    live: (u64, u64),
    original: RecoveryBinding,
    current_bytes: Vec<u8>,
    next: Header,
    reason: MigrationReason,
}
impl Migration {
    fn archive_path(&self) -> PathBuf {
        let hash = Sha256::digest(&self.original.bytes);
        let name: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        self.collection
            .with_file_name(format!("meguri-pending-answers.{name}.identity.jsonl"))
    }
    fn publish(&self, path: &Path, state: &State) -> Result<(Header, RecoveryBinding), EngineError> {
        self.publish_with(path, state, || {})
    }
    fn publish_with(
        &self,
        path: &Path,
        state: &State,
        archive_durable: impl FnOnce(),
    ) -> Result<(Header, RecoveryBinding), EngineError> {
        self.publish_checked(path, state, archive_durable, || Ok(()))
    }
    fn publish_checked(
        &self,
        path: &Path,
        state: &State,
        archive_durable: impl FnOnce(),
        check: impl Fn() -> Result<(), EngineError>,
    ) -> Result<(Header, RecoveryBinding), EngineError> {
        check()?;
        if state.retired || state.events.values().any(|e| e.terminal.is_none()) {
            return Err(corruption());
        }
        let current = RecoveryBinding {
            file: self.original.file.try_clone().map_err(|_| migration_io())?,
            bytes: self.current_bytes.clone(),
        };
        current.require(path).map_err(|_| identity_mismatch())?;
        require_live(&self.collection, self.live, &self.next.path).map_err(|_| identity_mismatch())?;
        // A permanent exclusive archive must stand durably before publication.
        // A crash may have already published it: verify exact bytes and repeat
        // file/directory sync, without ever overwriting a collision or link.
        let archive = self.archive_path();
        let archival = (|| -> Result<RecoveryBinding, EngineError> {
            let mut published = None;
            match std::fs::symlink_metadata(&archive) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let mut copy = AtomicFile::new(&archive, Purpose::Sidecar)?;
                    copy.file_mut().write_all(&self.original.bytes).map_err(error)?;
                    published = Some(RecoveryBinding {
                        file: copy.file_mut().try_clone().map_err(error)?,
                        bytes: self.original.bytes.clone(),
                    });
                    copy.publish_new(|_| require_migration_source(&current, path))?;
                }
                Err(e) => return Err(error(e)),
                Ok(_) => {}
            }
            if let Some(published) = &published {
                published.require(&archive)?;
            }
            let archived = RecoveryBinding::read(&archive)?;
            if archived.bytes != self.original.bytes {
                return Err(migration_io());
            }
            archived.sync(&archive)?;
            Ok(archived)
        })();
        let archived = archival.map_err(migration_failure)?;
        archive_durable();
        check()?;
        archived.require(&archive).map_err(|_| identity_mismatch())?;
        current.require(path).map_err(|_| identity_mismatch())?;
        require_live(&self.collection, self.live, &self.next.path).map_err(|_| identity_mismatch())?;
        let mut h = self.next.clone();
        h.next_event = if self.reason == MigrationReason::LegacyIdentityUpgraded {
            state.next_event
        } else {
            self.next.next_event
        };
        h.notice = Some(MigrationNotice {
            reason: self.reason,
            archive_sha256: Sha256::digest(&self.original.bytes).into(),
        });
        let bytes = encode(1, &Record::Header(h.clone()))?;
        let publication = (|| -> Result<RecoveryBinding, EngineError> {
            let mut next = AtomicFile::new(path, Purpose::Sidecar)?;
            next.file_mut().write_all(&bytes).map_err(error)?;
            let landed = RecoveryBinding {
                file: next.file_mut().try_clone().map_err(error)?,
                bytes,
            };
            next.publish(|_| {
                archived.require(&archive)?;
                check().map_err(|_| meguri_storage::StorageError::Io {
                    detail: "pending_answer_identity_mismatch".into(),
                })?;
                require_migration_source(&current, path)?;
                require_live(&self.collection, self.live, &self.next.path)
            })?;
            landed.sync(path)?;
            Ok(landed)
        })();
        let landed = publication.map_err(migration_failure)?;
        archived.require(&archive).map_err(|_| identity_mismatch())?;
        check()?;
        Ok((h, landed))
    }
}

// Current live checks never use a retained header's historical locator.
struct CurrentBinding<'a> {
    collection: &'a Path,
    native: (u64, u64),
    canonical: &'a [u8],
}

impl Journal {
    pub(crate) fn open(
        collection: &Path,
        native: (u64, u64),
        persistent: PersistentIdentity,
        incarnation: [u64; 2],
        generation: Generation,
        threads: &ThreadRegistry,
        fresh_incarnation: impl FnOnce() -> Result<[u64; 2], EngineError>,
    ) -> Result<Arc<Self>, EngineError> {
        use std::os::unix::ffi::OsStrExt;
        let path = collection.with_file_name("meguri-pending-answers.jsonl");
        let mut header = Header {
            version: 2,
            path: collection
                .canonicalize()
                .map_err(error)?
                .as_os_str()
                .as_bytes()
                .to_vec(),
            native: Native::Persistent(persistent),
            incarnation,
            next_event: 1,
            notice: None,
        };
        if !persistent.valid_for_platform() {
            return Err(identity_mismatch());
        }
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                require_live(collection, native, &header.path)?;
                create(&path, &header, false)?;
            }
            Err(e) => return Err(error(e)),
            Ok(_) => {}
        }
        let mut original = RecoveryBinding::read(&path)?;
        let (mut loaded, mut number, mut complete) = read_bytes_bound(&original.bytes, &header, native)?;
        let mut migration = None;
        let rebound = loaded.header.version == 1 && loaded.header.native != Native::Legacy(native);
        if (loaded.retired || rebound) && header.incarnation == loaded.header.incarnation {
            header.incarnation = fresh_incarnation()?;
            if header.incarnation == loaded.header.incarnation {
                return Err(identity_mismatch());
            }
        }
        if loaded.retired {
            original.require(&path)?;
            require_live(collection, native, &header.path)?;
            create(&path, &header, true)?;
            original = RecoveryBinding::read(&path)?;
            (loaded, number, complete) = read_bytes_bound(&original.bytes, &header, native)?;
        } else if loaded.header.version == 1 {
            let reason = if loaded.header.native == Native::Legacy(native) {
                MigrationReason::LegacyIdentityUpgraded
            } else {
                MigrationReason::LegacyHeaderOnlyRebound
            };
            let mut next = header.clone();
            if reason == MigrationReason::LegacyIdentityUpgraded {
                next.incarnation = loaded.header.incarnation;
            }
            migration = Some(Migration {
                collection: collection.to_owned(),
                live: native,
                current_bytes: original.bytes[..complete].to_vec(),
                original: RecoveryBinding {
                    file: original.file.try_clone().map_err(error)?,
                    bytes: original.bytes.clone(),
                },
                next,
                reason,
            });
        }
        Self::initialize_retained(
            CurrentBinding {
                collection,
                native,
                canonical: &header.path,
            },
            generation,
            threads,
            (original, loaded, number, complete),
            migration,
            None,
        )
    }
    // Both strict and qualified managed opens initialize the source they decoded.
    // The live canonical binding is independent of the historical header locator.
    fn initialize_retained(
        CurrentBinding {
            collection,
            native,
            canonical,
        }: CurrentBinding<'_>,
        generation: Generation,
        threads: &ThreadRegistry,
        (original, mut loaded, number, complete): (RecoveryBinding, State, u64, usize),
        migration: Option<Migration>,
        mut opening: Option<managed::Evidence>,
    ) -> Result<Arc<Self>, EngineError> {
        let path = collection.with_file_name("meguri-pending-answers.jsonl");
        if let Some(evidence) = &opening {
            evidence.require()?;
        }
        original.require(&path)?;
        require_live(collection, native, canonical)?;
        // No acknowledgement came from a torn frame. A legacy migration keeps
        // the entire original (including that tail) in its permanent archive.
        if original.bytes.len() != complete {
            use std::os::unix::fs::OpenOptionsExt;
            let file = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
                .map_err(error)?;
            same_file(&file, &original.file)?;
            file.set_len(complete as u64).map_err(error)?;
            full_sync(&file)?;
            if let Some(evidence) = &mut opening {
                evidence.truncated(complete);
            }
        }
        // Re-establish durability even when a prior process died after rename.
        let active = RecoveryBinding::read(&path)?;
        same_file(&active.file, &original.file)?;
        if active.bytes != original.bytes[..complete] {
            return Err(identity_mismatch());
        }
        active.sync(&path)?;
        require_live(collection, native, canonical)?;
        loaded.header.next_event = loaded.next_event;
        let file = append_file(&path)?;
        same_file(&file, &active.file)?;
        active.require(&path)?;
        if let Some(evidence) = &opening {
            evidence.require_append(&file)?;
            evidence.require()?;
        }
        let state = Arc::new(Mutex::new(loaded));
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        let worker = threads
            .spawn_owned(
                "meguri-review-file",
                Qos::UserInitiated,
                "review-session",
                move |activity| {
                    file_loop(rx, file, path, shared, number, (migration, None), activity);
                },
            )
            .map_err(error)?;
        let journal = Arc::new(Self {
            tx,
            state,
            worker: Mutex::new(Some(worker)),
            generation,
            live_native: native,
            live_canonical: canonical.to_vec(),
            retired_binding: Mutex::new(None),
        });
        if let Some(evidence) = opening {
            let (tx, rx) = mpsc::sync_channel(1);
            journal
                .tx
                .send(Work::OpeningEvidence(Box::new(evidence), tx))
                .map_err(error)?;
            rx.recv().map_err(error)??;
        }
        Ok(journal)
    }
    pub(crate) fn finish_identity_migration(
        &self,
        native: (u64, u64),
        persistent: PersistentIdentity,
    ) -> Result<(), EngineError> {
        self.check_identity(native)?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Migrate(persistent, tx)).map_err(error)?;
        rx.recv().map_err(error)??;
        let state = self.state.lock().unwrap();
        if state.header.native != Native::Persistent(persistent) {
            return Err(identity_mismatch());
        }
        if let Some(notice) = &state.header.notice {
            tracing::info!(
                event = "pending_answer_identity_migration",
                reason = notice.reason.as_str(),
                migration_replayed_answers = 0u64
            );
        }
        Ok(())
    }
    pub(crate) fn check_persistent_identity(&self, persistent: PersistentIdentity) -> Result<(), EngineError> {
        if self.state.lock().unwrap().header.native != Native::Persistent(persistent) {
            return Err(identity_mismatch());
        }
        Ok(())
    }
    pub(crate) fn check_identity(&self, native: (u64, u64)) -> Result<(), EngineError> {
        if self.live_native != native {
            return Err(error(
                "unexpected native collection identity on resume; evidence retained",
            ));
        }
        Ok(())
    }
    pub(crate) fn check(&self, generation: Generation) -> Result<(), EngineError> {
        let state = self.state.lock().unwrap();
        if let Some(e) = &state.failure {
            return Err(e.clone());
        }
        if state.retired || self.generation != generation {
            return Err(error("stale incarnation/generation"));
        }
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub(crate) fn fail_next(&self, stage: JournalStage) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Fault(stage, tx)).map_err(error)?;
        rx.recv().map_err(error)
    }
    #[cfg(feature = "test-hooks")]
    pub(crate) fn set_hook(&self, hook: StageHook) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Hook(hook, tx)).map_err(error)?;
        rx.recv().map_err(error)
    }
    pub(crate) fn next_event(&self) -> u64 {
        self.state.lock().unwrap().header.next_event
    }
    pub(crate) fn enqueue(&self, answer: FrozenAnswer) -> Result<u64, EngineError> {
        self.check(self.generation)?;
        // Serial callers enqueue in event order. The in-memory reservation is
        // separate from the file worker's validated next-event counter.
        let mut state = self.state.lock().unwrap();
        let event = state.header.next_event;
        state.header.next_event = event.checked_add(1).ok_or_else(|| error("event sequence exhausted"))?;
        self.tx
            .send(Work::Append(
                Box::new(Record::Intent { event, answer }),
                None,
                monotonic::now() + Duration::from_secs(1),
            ))
            .map_err(error)?;
        Ok(event)
    }
    fn append(&self, record: Record) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Work::Append(
                Box::new(record),
                Some(tx),
                monotonic::now() + Duration::from_secs(1),
            ))
            .map_err(error)?;
        rx.recv().map_err(error)?
    }
    pub(crate) fn prepare(&self, event: u64, receipt: Receipt, generation: Generation) -> Result<(), EngineError> {
        self.check(generation)?;
        self.append(Record::Prepared { event, receipt })?;
        self.check(generation)
    }
    pub(crate) fn terminal(&self, event: u64, status: Terminal) -> Result<(), EngineError> {
        self.append(Record::Terminal { event, status })
    }
    pub(crate) fn committed(&self, event: u64, generation: Generation, seq: Seq) {
        if let Some(e) = self.state.lock().unwrap().events.get_mut(&event) {
            e.committed = Some((generation, seq));
        }
    }
    pub(crate) fn entries(&self) -> Vec<(u64, Entry)> {
        self.state
            .lock()
            .unwrap()
            .events
            .iter()
            .map(|(id, e)| (*id, e.clone()))
            .collect()
    }
    pub(crate) fn resolved_base(&self, base: &Base) -> Result<[u8; 32], EngineError> {
        match base {
            Base::Row(d) => Ok(*d),
            Base::Predecessor(id) => self
                .state
                .lock()
                .unwrap()
                .events
                .get(id)
                .filter(|e| e.terminal.is_none() || e.terminal == Some(Terminal::Done))
                .and_then(|e| e.receipt.as_ref())
                .map(|r| r.post)
                .ok_or_else(|| error("failed or unresolved predecessor")),
        }
    }
    pub(crate) fn has_committed(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .events
            .values()
            .any(|e| e.terminal.is_none() && e.committed.is_some())
    }
    /// Called only after successful real WAL coverage, outside commit lock.
    pub(crate) fn covered(&self, generation: Generation, through: Seq) -> Result<(), EngineError> {
        self.check(generation)?;
        let ids = self
            .entries()
            .into_iter()
            .filter(|(_, e)| {
                e.terminal.is_none() && e.committed.is_some_and(|(g, seq)| g == generation && seq <= through)
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let mut acknowledgements = Vec::with_capacity(ids.len());
        for event in ids {
            let (tx, rx) = mpsc::sync_channel(1);
            self.tx
                .send(Work::Append(
                    Box::new(Record::Terminal {
                        event,
                        status: Terminal::Done,
                    }),
                    Some(tx),
                    monotonic::now() + Duration::from_secs(1),
                ))
                .map_err(error)?;
            acknowledgements.push(rx);
        }
        for ack in acknowledgements {
            ack.recv().map_err(error)??;
        }
        Ok(())
    }
    pub(crate) fn barrier(&self) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Barrier(tx)).map_err(error)?;
        rx.recv().map_err(error)?
    }
    pub(crate) fn compact(&self) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Compact(tx)).map_err(error)?;
        rx.recv().map_err(error)?
    }
    pub(crate) fn barrier_until(&self, deadline: Duration) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Barrier(tx)).map_err(error)?;
        wait_until(rx, deadline)
    }
    pub(crate) fn compact_until(&self, deadline: Duration) -> Result<(), EngineError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(Work::Compact(tx)).map_err(error)?;
        wait_until(rx, deadline)
    }
    pub(crate) fn retire(&self) -> Result<(), EngineError> {
        self.append(Record::Retired)?;
        if self.state.lock().unwrap().header.path != self.live_canonical {
            let (tx, rx) = mpsc::sync_channel(1);
            self.tx.send(Work::RetiredSource(self.live_native, tx)).map_err(error)?;
            // Capture before releasing the old collection to the swap. Later
            // rebind must consume these exact bytes on this exact descriptor.
            *self.retired_binding.lock().unwrap() = Some(rx.recv().map_err(error)??);
        }
        Ok(())
    }
    // Only the still-owned worker can attest the exact file it durably retired.
    // The stored locator alone never authorizes opening a different path.
    fn retired_source(&self, canonical: &[u8]) -> Result<Option<RecoveryBinding>, EngineError> {
        if canonical != self.live_canonical {
            return Err(identity_mismatch());
        }
        let state = self.state.lock().unwrap();
        if state.header.path == canonical {
            return Ok(None);
        }
        if let Some(failure) = &state.failure {
            return Err(failure.clone());
        }
        if !state.retired
            || state.header.version != 2
            || !matches!(state.header.native, Native::Persistent(identity @ PersistentIdentity::Darwin { .. })
                if identity.valid_for_platform())
        {
            return Err(identity_mismatch());
        }
        drop(state);
        let retired_binding = self.retired_binding.lock().unwrap();
        let source = retired_binding.as_ref().ok_or_else(identity_mismatch)?;
        Ok(Some(RecoveryBinding {
            file: source.file.try_clone().map_err(error)?,
            bytes: source.bytes.clone(),
        }))
    }
}
fn wait_until(reply: mpsc::Receiver<Result<(), EngineError>>, deadline: Duration) -> Result<(), EngineError> {
    reply
        .recv_timeout(deadline.saturating_sub(monotonic::now()))
        .map_err(|failure| match failure {
            mpsc::RecvTimeoutError::Timeout => EngineError::SuspendDeadline {
                phase: "review journal".into(),
            },
            mpsc::RecvTimeoutError::Disconnected => error(failure),
        })?
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = self.tx.send(Work::Stop);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}
fn file_loop(
    rx: mpsc::Receiver<Work>,
    mut file: File,
    path: PathBuf,
    state: Arc<Mutex<State>>,
    mut number: u64,
    (mut migration, mut opening): (Option<Migration>, Option<managed::Evidence>),
    activity: &crate::threads::ThreadActivity,
) {
    #[cfg(feature = "test-hooks")]
    let mut fault = None;
    #[cfg(feature = "test-hooks")]
    let mut hook: Option<StageHook> = None;
    // Validation belongs to the file owner. Publish only records covered by
    // full sync; never clone the accumulated event map for an append, or hold
    // the shared UI-facing mutex across file I/O.
    let mut validated = state.lock().unwrap().clone();
    let mut deferred = None;
    while let Some(work) = deferred.take().or_else(|| rx.recv().ok()) {
        let _busy = activity.busy();
        match work {
            Work::Stop => break,
            #[cfg(feature = "test-hooks")]
            Work::Fault(stage, reply) => {
                fault = Some(stage);
                let _ = reply.send(());
            }
            #[cfg(feature = "test-hooks")]
            Work::Hook(value, reply) => {
                hook = Some(value);
                let _ = reply.send(());
            }
            Work::Barrier(reply) => {
                let _ = reply.send(state.lock().unwrap().failure.clone().map_or(Ok(()), Err));
            }
            Work::RetiredSource(native, reply) => {
                let result = (|| {
                    if let Some(failure) = &state.lock().unwrap().failure {
                        return Err(failure.clone());
                    }
                    let source = RecoveryBinding::read(&path)?;
                    same_file(&file, &source.file)?;
                    let (loaded, frames, complete) = read_bytes_bound(&source.bytes, &validated.header, native)?;
                    if !validated.retired
                        || !loaded.retired
                        || loaded.header.path != validated.header.path
                        || loaded.header.native != validated.header.native
                        || loaded.header.incarnation != validated.header.incarnation
                        || loaded.next_event != validated.next_event
                        || frames != number
                        || complete != source.bytes.len()
                    {
                        return Err(identity_mismatch());
                    }
                    // Retired was acknowledged by the preceding append's full
                    // sync on this worker descriptor; retain its exact bytes.
                    source.require(&path)?;
                    Ok(source)
                })();
                let _ = reply.send(result);
            }
            Work::OpeningEvidence(evidence, reply) => {
                let result = evidence.require();
                if result.is_ok() {
                    opening = Some(*evidence);
                } else if let Err(e) = &result {
                    state.lock().unwrap().failure = Some(e.clone());
                }
                let _ = reply.send(result);
            }
            Work::Migrate(persistent, reply) => {
                let result = (|| {
                    if let Some(e) = &state.lock().unwrap().failure {
                        return Err(e.clone());
                    }
                    if let Some(evidence) = &opening {
                        evidence.require()?;
                    }
                    if let Some(pending) = &migration {
                        if pending.next.native != Native::Persistent(persistent) {
                            return Err(identity_mismatch());
                        }
                        let snapshot = state.lock().unwrap().clone();
                        let (h, landed) = pending.publish(&path, &snapshot)?;
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, JournalStage::MigrationPublished)?;
                        require_live(&pending.collection, pending.live, &h.path)?;
                        let append = append_file(&path)?;
                        same_file(&append, &landed.file)?;
                        landed.require(&path).map_err(|_| identity_mismatch())?;
                        require_live(&pending.collection, pending.live, &h.path)?;
                        file = append;
                        {
                            let mut s = state.lock().unwrap();
                            s.next_event = h.next_event;
                            s.header = h.clone();
                            s.events.clear();
                            validated = s.clone();
                        }
                        number = 1;
                        migration = None;
                    }
                    if let Some(evidence) = &opening {
                        evidence.require()?;
                    }
                    opening = None;
                    Ok(())
                })();
                if let Err(e) = &result {
                    state.lock().unwrap().failure = Some(e.clone());
                }
                let _ = reply.send(result);
            }
            Work::Compact(reply) => {
                let result = (|| {
                    if migration.is_some() {
                        return Ok(());
                    }
                    let h = {
                        let s = state.lock().unwrap();
                        if let Some(e) = &s.failure {
                            return Err(e.clone());
                        }
                        if s.events.values().any(|e| e.terminal.is_none()) || s.header.next_event != s.next_event {
                            return Ok(());
                        }
                        let mut h = s.header.clone();
                        h.next_event = s.next_event;
                        h
                    };
                    #[cfg(feature = "test-hooks")]
                    stage(&mut fault, &hook, JournalStage::Compact)?;
                    // Acceptances can enqueue while this file operation runs.
                    // The worker will append them after the new header; no UI
                    // mutex is held across full sync or directory publication.
                    if let Some(evidence) = &mut opening {
                        evidence.compact(&h)?;
                        file = append_file(&path)?;
                        evidence.require_append(&file)?;
                    } else {
                        create(&path, &h, true)?;
                        file = OpenOptions::new().append(true).open(&path).map_err(error)?;
                    }
                    state.lock().unwrap().events.retain(|id, _| *id >= h.next_event);
                    validated.events.retain(|id, _| *id >= h.next_event);
                    number = 1;
                    Ok(())
                })();
                if let Err(e) = &result {
                    state.lock().unwrap().failure = Some(e.clone());
                }
                let _ = reply.send(result);
            }
            Work::Append(record, reply, deadline) => {
                // Drain only an already-queued, bounded prefix. Control work
                // and barriers retain their exact place in the file order.
                let mut batch = vec![(*record, reply, deadline)];
                while batch.len() < 64 {
                    match rx.try_recv() {
                        Ok(Work::Append(record, reply, deadline)) => batch.push((*record, reply, deadline)),
                        Ok(other) => {
                            deferred = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                let mut cancelled_prepares = vec![false; batch.len()];
                let mut written = 0;
                let result = (|| {
                    if let Some(e) = &state.lock().unwrap().failure {
                        return Err(e.clone());
                    }
                    if let Some(evidence) = &opening {
                        evidence.require_append(&file)?;
                    }
                    for (offset, (record, _, _)) in batch.iter().enumerate() {
                        // Live request admission, not disk-record validation:
                        // undo may durably cancel between the writer's check
                        // and its preparation request. An earlier cancellation
                        // in this prefix must also be synced before refusal.
                        if let Record::Prepared { event, .. } = record
                            && !validated.retired
                            && validated
                                .events
                                .get(event)
                                .is_some_and(|e| e.terminal == Some(Terminal::Cancelled))
                        {
                            cancelled_prepares[offset] = true;
                            continue;
                        }
                        apply(&mut validated, record.clone())?;
                        let bytes = encode(number + written + 1, record)?;
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[0])?;
                        file.write_all(&bytes).map_err(error)?;
                        if let Some(evidence) = &mut opening {
                            evidence.appended(&bytes);
                        }
                        if let Some(pending) = &mut migration {
                            pending.current_bytes.extend(&bytes);
                        }
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[1])?;
                        written += 1;
                    }
                    if written != 0 {
                        full_sync(&file)?;
                    }
                    if let Some(evidence) = &opening {
                        evidence.require_append(&file)?;
                    }
                    let covered_at = monotonic::now();
                    for ((record, _, _), cancelled) in batch.iter().zip(&cancelled_prepares) {
                        if *cancelled {
                            continue;
                        }
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[2])?;
                        apply(&mut state.lock().unwrap(), record.clone())?;
                    }
                    number += written;
                    if batch
                        .iter()
                        .zip(&cancelled_prepares)
                        .any(|((_, _, deadline), cancelled)| !*cancelled && covered_at > *deadline)
                    {
                        return Err(error("acceptance durability deadline exceeded"));
                    }
                    Ok(())
                })();
                if let Err(e) = &result {
                    state.lock().unwrap().failure = Some(e.clone());
                }
                // Every receipt in this prefix is acknowledged only after
                // the full sync that covers its frame. Failure stays sticky.
                for ((_, reply, _), cancelled) in batch.into_iter().zip(cancelled_prepares) {
                    if let Some(reply) = reply {
                        let _ = reply.send(if result.is_ok() && cancelled {
                            Err(EngineError::Cancelled)
                        } else {
                            result.clone()
                        });
                    }
                }
            }
        }
    }
}

#[cfg(feature = "test-hooks")]
fn record_stages(record: &Record) -> [JournalStage; 3] {
    match record {
        Record::Intent { .. } => [
            JournalStage::IntentWrite,
            JournalStage::IntentSync,
            JournalStage::IntentSynced,
        ],
        Record::Prepared { .. } => [
            JournalStage::PreparedWrite,
            JournalStage::PreparedSync,
            JournalStage::PreparedSynced,
        ],
        _ => [
            JournalStage::TerminalWrite,
            JournalStage::TerminalSync,
            JournalStage::TerminalSynced,
        ],
    }
}

#[cfg(feature = "test-hooks")]
fn stage(fault: &mut Option<JournalStage>, hook: &Option<StageHook>, stage: JournalStage) -> Result<(), EngineError> {
    if *fault == Some(stage) {
        *fault = None;
        return Err(EngineError::Storage(meguri_storage::StorageError::Io {
            detail: format!("injected review journal IO failure at {stage:?}"),
        }));
    }
    if let Some(hook) = hook {
        hook(stage);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-hooks")]
    #[test]
    fn c12_managed_open_rebinds_pristine_v2_after_path_change_before_ready() {
        use crate::{Collection, Engine, EngineConfig, LifecycleState, OpenConfig, OpenStep, Ports};
        use crate::{sched::queue::QueueProvider, session::ReviewService};
        use meguri_model::{
            clock::{StepClock, UtcOffset},
            rng::SeededRandom,
        };
        use meguri_storage::{ReadRows, ReaderRole, Table};
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};

        // Observe publication after the actual review-session step, while the
        // privileged opening handle is still the only admitted collection.
        struct BeforeReady(Mutex<Option<(Vec<u8>, Vec<u8>)>>, PathBuf);
        impl OpenStep for BeforeReady {
            fn name(&self) -> &str {
                "pristine-v2-publication-evidence"
            }
            fn run(&self, c: &Collection) -> Result<(), EngineError> {
                let active = std::fs::read(c.path().with_file_name("meguri-pending-answers.jsonl")).map_err(error)?;
                let archive = std::fs::read(&self.1).map_err(error)?;
                *self.0.lock().unwrap() = Some((active, archive));
                Ok(())
            }
        }
        fn managed_open(
            path: &Path,
            clock: Arc<StepClock>,
            evidence: Option<Arc<BeforeReady>>,
        ) -> (Engine, Arc<ReviewService>, Result<Collection, EngineError>) {
            let engine = Engine::new(EngineConfig::default()).unwrap();
            let queue = Arc::new(QueueProvider::default());
            let review = ReviewService::new(&engine, queue.clone()).unwrap();
            let mut ports = Ports::new()
                .observer(queue.clone())
                .generation_listener(queue)
                .open_step(review.clone())
                .observer(review.clone())
                .generation_listener(review.clone())
                .suspend_participant(review.clone());
            if let Some(evidence) = evidence {
                ports = ports.open_step(evidence);
            }
            let opened = engine.open_raw(
                path,
                OpenConfig::new(clock, Arc::new(SeededRandom::new(43))).ports(ports),
            );
            (engine, review, opened)
        }
        fn header_json(bytes: &[u8]) -> serde_json::Value {
            assert_eq!(bytes.last(), Some(&b'\n'));
            assert_eq!(
                bytes.iter().filter(|b| **b == b'\n').count(),
                1,
                "exact header-only EOF"
            );
            let frame: Frame = serde_json::from_slice(&bytes[..bytes.len() - 1]).unwrap();
            assert_eq!(frame.number, 1);
            assert_eq!(frame.checksum, checksum(1, frame.payload.as_bytes()));
            let payload: serde_json::Value = serde_json::from_str(&frame.payload).unwrap();
            payload["Header"].clone()
        }

        let dir = tempfile::tempdir().unwrap();
        let original_path = dir.path().join("collection.anki2");
        let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
        crate::bootstrap::create(&original_path, clock.as_ref(), &SeededRandom::new(43)).unwrap();
        let (engine, review, opened) = managed_open(&original_path, clock.clone(), None);
        let collection = opened.unwrap();
        engine.close().unwrap();
        drop(collection);
        drop(review);
        drop(engine);

        let collection_bytes = std::fs::read(&original_path).unwrap();
        let journal_path = original_path.with_file_name("meguri-pending-answers.jsonl");
        let original = std::fs::read(&journal_path).unwrap();
        let old_header = header_json(&original);
        assert_eq!(old_header["version"], 2);
        assert_eq!(old_header["next_event"], 1);
        assert!(old_header.get("notice").is_none());
        let persistent: PersistentIdentity = serde_json::from_value(old_header["native"].clone()).unwrap();
        assert!(persistent.valid_for_platform());

        // Rename only the collection, keeping its inode, contents and the
        // colocated journal exactly as the first managed open left them.
        let moved = dir.path().join("renamed.anki2");
        std::fs::rename(&original_path, &moved).unwrap();
        let current_path = moved.canonicalize().unwrap().as_os_str().as_bytes().to_vec();
        assert_ne!(old_header["path"], serde_json::json!(current_path));
        assert_eq!(std::fs::read(&journal_path).unwrap(), original);
        let hash: [u8; 32] = Sha256::digest(&original).into();
        let hash_text: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let archive_path = moved.with_file_name(format!("meguri-pending-answers.{hash_text}.identity.jsonl"));

        // Raw strict open remains a refusal, even for this unused v2 journal.
        let raw_engine = Engine::new(EngineConfig::default()).unwrap();
        let raw_collection = raw_engine
            .open_raw(&moved, OpenConfig::new(clock.clone(), Arc::new(SeededRandom::new(43))))
            .unwrap();
        let metadata = std::fs::metadata(&moved).unwrap();
        let raw = Journal::open(
            &moved,
            (metadata.dev(), metadata.ino()),
            persistent,
            [31, 32],
            raw_collection.generation(),
            raw_engine.threads(),
            || Ok([33, 34]),
        );
        let raw_error = raw.err(); // Drop/join any unexpectedly created file worker.
        let raw_close = raw_engine.close();
        drop(raw_collection);
        drop(raw_engine);
        raw_close.unwrap();
        assert_eq!(raw_error, Some(identity_mismatch()));
        assert_eq!(std::fs::read(&journal_path).unwrap(), original);
        assert_eq!(std::fs::read(&moved).unwrap(), collection_bytes);
        assert!(!archive_path.exists(), "raw refusal must not migrate evidence");

        let evidence = Arc::new(BeforeReady(Mutex::new(None), archive_path.clone()));
        let (engine, review, opened) = managed_open(&moved, clock, Some(evidence.clone()));
        let ready = matches!(engine.state(), LifecycleState::Ready { .. });
        let revlogs = opened.as_ref().ok().map(|c| {
            c.reader(ReaderRole::Query)
                .and_then(|r| r.ids_after(Table::Revlog, None, 1).map_err(EngineError::from))
        });
        let open_error = opened.err();
        let closed = engine.close();
        drop(review);
        drop(engine); // Release/join all workers before the intentional RED.
        closed.unwrap();
        assert!(
            ready && open_error.is_none(),
            "C12.5: managed opening must rebind the exact unused v2 journal before Ready: {open_error:?}"
        );
        assert!(revlogs.unwrap().unwrap().is_empty(), "zero replayed answers");
        assert_eq!(std::fs::read(&moved).unwrap(), collection_bytes);
        let (published, archived) = evidence
            .0
            .lock()
            .unwrap()
            .take()
            .expect("publication observed before Ready");
        assert_eq!(archived, original, "exact original archive must precede Ready");
        assert_eq!(std::fs::read(&archive_path).unwrap(), original);
        assert_eq!(std::fs::read(&journal_path).unwrap(), published);
        let landed = header_json(&published);
        assert_eq!(landed["version"], 2);
        assert_eq!(landed["path"], serde_json::json!(current_path));
        assert_eq!(landed["native"], old_header["native"]);
        assert_ne!(landed["incarnation"], old_header["incarnation"]);
        assert_eq!(landed["next_event"], 1);
        assert_eq!(landed["notice"]["reason"], "pristine_v2_rebound");
        assert_eq!(landed["notice"]["archive_sha256"], serde_json::json!(hash));
    }

    #[test]
    fn c12_v2_durable_identity_changes_and_compacted_highwater() {
        let mut h = header_v2();
        h.next_event = 41;
        let bytes = encode(1, &Record::Header(h.clone())).unwrap();
        let (loaded, frames, complete) = read_bytes(&bytes, &h).unwrap();
        assert_eq!(loaded.next_event, 41);
        assert_eq!(frames, 1);
        assert_eq!(complete, bytes.len());
        for changed in [
            Native::Persistent(test_persistent((8, 10))),
            Native::Persistent(PersistentIdentity::Darwin {
                volume_uuid: [4; 16],
                inode: 9,
            }),
        ] {
            let mut expected = h.clone();
            expected.native = changed;
            assert!(read_bytes(&bytes, &expected).is_err());
        }
        let mut expected = h.clone();
        expected.path.push(b'x');
        assert!(read_bytes(&bytes, &expected).is_err());
        // The durable Darwin key has no live device field to compare.
        let uuid = PersistentIdentity::Darwin {
            volume_uuid: [3; 16],
            inode: 9,
        };
        assert!(identity_matches(
            Native::Persistent(uuid),
            Native::Persistent(uuid),
            (7, 9)
        ));
        assert!(identity_matches(
            Native::Persistent(uuid),
            Native::Persistent(uuid),
            (8, 9)
        ));
        for changed in [
            PersistentIdentity::Darwin {
                volume_uuid: [4; 16],
                inode: 9,
            },
            PersistentIdentity::Darwin {
                volume_uuid: [3; 16],
                inode: 10,
            },
        ] {
            assert!(!identity_matches(
                Native::Persistent(uuid),
                Native::Persistent(changed),
                (7, 9)
            ));
        }
    }

    #[test]
    fn c12_legacy_header_only_rebind_is_exact_and_event_mismatches_never_replay() {
        let h = header();
        let mut expected = header_v2();
        expected.path = h.path.clone();
        let bytes = encode(1, &Record::Header(h.clone())).unwrap();
        let (state, _, complete) = read_bytes_bound(&bytes, &expected, (8, 9)).unwrap();
        assert!(state.events.is_empty());
        assert_eq!(complete, bytes.len());
        let mut compacted = h.clone();
        compacted.next_event = 37;
        assert!(read_bytes_bound(&encode(1, &Record::Header(compacted)).unwrap(), &expected, (8, 9)).is_ok());
        for tail in [b"torn".as_slice(), b"\n", b"bad\n"] {
            let mut bad = bytes.clone();
            bad.extend(tail);
            assert!(read_bytes_bound(&bad, &expected, (8, 9)).is_err());
        }
        for terminal in [
            None,
            Some(Terminal::Done),
            Some(Terminal::Cancelled),
            Some(Terminal::Dropped),
        ] {
            let mut records = vec![Record::Header(h.clone()), intent()];
            if terminal == Some(Terminal::Done) {
                records.push(Record::Prepared {
                    event: 1,
                    receipt: receipt(),
                });
            }
            if let Some(status) = terminal {
                records.push(Record::Terminal { event: 1, status });
            }
            let bytes = records
                .iter()
                .enumerate()
                .flat_map(|(i, r)| encode(i as u64 + 1, r).unwrap())
                .collect::<Vec<_>>();
            assert!(read_bytes_bound(&bytes, &expected, (8, 9)).is_err());
        }
        for native in [(8, 10), (7, 10)] {
            assert!(read_bytes_bound(&bytes, &expected, native).is_err());
        }
    }

    #[test]
    fn c12_strict_v2_identity_notice_version_and_empty_input_refuse() {
        let h = header_v2();
        for bytes in [vec![], b"incomplete".to_vec()] {
            assert!(read_bytes(&bytes, &h).is_err());
        }
        for payload in [
            r#"{"Header":{"version":2,"path":[],"native":{"platform":"unknown","inode":9},"incarnation":[1,2],"next_event":1}}"#,
            r#"{"Header":{"version":2,"path":[],"native":{"platform":"darwin","volume_uuid":[1],"inode":9},"incarnation":[1,2],"next_event":1}}"#,
        ] {
            let bytes = raw_frame(payload);
            assert!(read_bytes(&bytes, &h).is_err());
        }
        for case in [
            "zero_uuid",
            "platform",
            "tag",
            "short_uuid",
            "short_hash",
            "reason",
            "notice_extra",
            "null_notice",
            "native_extra",
            "zero_counter",
            "version_native",
        ] {
            let mut record = serde_json::to_value(Record::Header(h.clone())).unwrap();
            let header = &mut record["Header"];
            match case {
                "zero_uuid" => {
                    header["native"] = serde_json::json!({"platform":"darwin","volume_uuid":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"inode":9})
                }
                "platform" => {
                    header["native"] = if cfg!(target_vendor = "apple") {
                        serde_json::json!({"platform":"non_darwin","device":7,"inode":9})
                    } else {
                        serde_json::json!({"platform":"darwin","volume_uuid":[3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3],"inode":9})
                    };
                }
                "tag" => header["native"]["platform"] = "unknown".into(),
                "short_uuid" => header["native"] = serde_json::json!({"platform":"darwin","volume_uuid":[3],"inode":9}),
                "short_hash" => {
                    header["notice"] = serde_json::json!({"reason":"legacy_identity_upgraded","archive_sha256":[1]})
                }
                "reason" | "notice_extra" => {
                    header["notice"] = serde_json::to_value(MigrationNotice {
                        reason: MigrationReason::LegacyIdentityUpgraded,
                        archive_sha256: [5; 32],
                    })
                    .unwrap();
                    if case == "reason" {
                        header["notice"]["reason"] = "unknown".into();
                    } else {
                        header["notice"]["extra"] = true.into();
                    }
                }
                "null_notice" => header["notice"] = serde_json::Value::Null,
                "native_extra" => header["native"]["extra"] = true.into(),
                "zero_counter" => header["next_event"] = 0.into(),
                "version_native" => header["version"] = 1.into(),
                _ => unreachable!(),
            }
            assert!(
                read_bytes(&raw_frame(&serde_json::to_string(&record).unwrap()), &h).is_err(),
                "{case}"
            );
        }
        let mut notice = h.clone();
        notice.notice = Some(MigrationNotice {
            reason: MigrationReason::LegacyIdentityUpgraded,
            archive_sha256: [5; 32],
        });
        assert!(read_bytes(&encode(1, &Record::Header(notice)).unwrap(), &h).is_ok());
        let mut invalid = h.clone();
        invalid.version = 99;
        assert!(read_bytes(&encode(1, &Record::Header(invalid)).unwrap(), &h).is_err());
    }

    fn raw_frame(payload: &str) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&Frame {
            number: 1,
            payload: payload.into(),
            checksum: checksum(1, payload.as_bytes()),
        })
        .unwrap();
        bytes.push(b'\n');
        bytes
    }
    fn header_v2() -> Header {
        Header {
            version: 2,
            native: Native::Persistent(test_persistent((7, 9))),
            notice: None,
            ..header()
        }
    }
    fn test_persistent((device, inode): (u64, u64)) -> PersistentIdentity {
        #[cfg(target_vendor = "apple")]
        {
            let _ = device;
            PersistentIdentity::Darwin {
                volume_uuid: [3; 16],
                inode,
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            PersistentIdentity::NonDarwin { device, inode }
        }
    }

    fn migration_fixture(reason: MigrationReason) -> (tempfile::TempDir, PathBuf, Migration, State) {
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        let dir = tempfile::tempdir().unwrap();
        let collection = dir.path().join("collection.anki2");
        std::fs::write(&collection, b"generated, no SQLite").unwrap();
        let metadata = std::fs::metadata(&collection).unwrap();
        let live = (metadata.dev(), metadata.ino());
        let mut h = header();
        h.path = collection.canonicalize().unwrap().as_os_str().as_bytes().to_vec();
        h.native = Native::Legacy(if reason == MigrationReason::LegacyHeaderOnlyRebound {
            (live.0 ^ 1, live.1)
        } else {
            live
        });
        h.next_event = 17;
        let path = collection.with_file_name("meguri-pending-answers.jsonl");
        let bytes = encode(1, &Record::Header(h.clone())).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let next = Header {
            version: 2,
            native: Native::Persistent(test_persistent(live)),
            incarnation: if reason == MigrationReason::LegacyHeaderOnlyRebound {
                [31, 32]
            } else {
                h.incarnation
            },
            next_event: 1,
            notice: None,
            ..h.clone()
        };
        let state = read_bytes_bound(&bytes, &next, live).unwrap().0;
        let migration = Migration {
            original: RecoveryBinding::read(&path).unwrap(),
            current_bytes: bytes,
            collection,
            live,
            next,
            reason,
        };
        (dir, path, migration, state)
    }
    #[cfg(feature = "test-hooks")]
    #[test]
    fn c12_migration_published_single_link_replacement_refuses_append_and_ready_retaining_evidence() {
        use crate::{Collection, Engine, EngineConfig, LifecycleState, OpenConfig, OpenStep, Ports};
        use meguri_model::{
            clock::{StepClock, UtcOffset},
            rng::SeededRandom,
        };
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct ExchangeAfterPublish {
            registry: ThreadRegistry,
            offset_legacy_device: bool,
            exchange_count: Arc<AtomicUsize>,
            inspection_count: Arc<AtomicUsize>,
        }
        impl OpenStep for ExchangeAfterPublish {
            fn name(&self) -> &str {
                "journal-publication-inode-exchange"
            }
            fn run(&self, opened: &Collection) -> Result<(), EngineError> {
                let file_stat = std::fs::metadata(opened.path()).unwrap();
                let native_pair = (file_stat.dev(), file_stat.ino());
                let stable_identity = test_persistent(native_pair);
                let v1_header = Header {
                    path: opened.path().canonicalize().unwrap().as_os_str().as_bytes().to_vec(),
                    native: Native::Legacy((native_pair.0 ^ u64::from(self.offset_legacy_device), native_pair.1)),
                    ..header()
                };
                let v1_bytes = encode(1, &Record::Header(v1_header)).unwrap();
                let fixture_path = opened.path().with_file_name("meguri-pending-answers.jsonl");
                std::fs::write(&fixture_path, &v1_bytes).unwrap();
                let pending_log = Journal::open(
                    opened.path(),
                    native_pair,
                    stable_identity,
                    [31, 32],
                    opened.generation(),
                    &self.registry,
                    || Ok([33, 34]),
                )?;
                let retained_path = fixture_path.with_file_name("retained-publication.jsonl");
                let captured_path = fixture_path.clone();
                let captured_retained_path = retained_path.clone();
                let exchange_count = self.exchange_count.clone();
                pending_log.set_hook(Arc::new(move |checkpoint| {
                    if checkpoint == JournalStage::MigrationPublished {
                        let v2_bytes = std::fs::read(&captured_path).unwrap();
                        std::fs::rename(&captured_path, &captured_retained_path).unwrap();
                        std::fs::write(&captured_path, &v2_bytes).unwrap();
                        let file_stat = std::fs::symlink_metadata(&captured_path).unwrap();
                        assert!(file_stat.is_file() && file_stat.nlink() == 1);
                        assert!(file_stat.ino() != std::fs::metadata(&captured_retained_path).unwrap().ino());
                        exchange_count.fetch_add(1, Ordering::SeqCst);
                    }
                }))?;
                let migration_outcome = pending_log.finish_identity_migration(native_pair, stable_identity);
                assert_eq!(self.exchange_count.load(Ordering::SeqCst), 1);
                assert!(
                    migration_outcome.is_err(),
                    "migration must reject an exchanged inode even when its bytes match"
                );
                assert_eq!(pending_log.state.lock().unwrap().header.version, 1);
                assert!(pending_log.check(opened.generation()).is_err());
                assert!(pending_log.barrier().is_err());
                let v2_bytes = std::fs::read(&retained_path).unwrap();
                assert!(std::fs::read(&fixture_path).unwrap() == v2_bytes);
                let decoded = read_bytes_bound(
                    &v2_bytes,
                    &v2_binding_header(opened.path(), stable_identity),
                    native_pair,
                )
                .unwrap()
                .0;
                assert_eq!(decoded.header.version, 2);
                let digest_text: String = Sha256::digest(&v1_bytes)
                    .iter()
                    .map(|octet| format!("{octet:02x}"))
                    .collect();
                let archive_path =
                    fixture_path.with_file_name(format!("meguri-pending-answers.{digest_text}.identity.jsonl"));
                assert!(std::fs::read(archive_path).unwrap() == v1_bytes);
                self.inspection_count.fetch_add(1, Ordering::SeqCst);
                migration_outcome
            }
        }
        fn v2_binding_header(fixture_path: &Path, stable_identity: PersistentIdentity) -> Header {
            Header {
                path: fixture_path.canonicalize().unwrap().as_os_str().as_bytes().to_vec(),
                native: Native::Persistent(stable_identity),
                ..header_v2()
            }
        }
        for offset_legacy_device in [false, true] {
            let scratch = tempfile::tempdir().unwrap();
            let fixture_path = scratch.path().join("collection.anki2");
            let time_source = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
            crate::bootstrap::create(&fixture_path, time_source.as_ref(), &SeededRandom::new(43)).unwrap();
            let runtime = Engine::new(EngineConfig::default()).unwrap();
            let exchange_count = Arc::new(AtomicUsize::new(0));
            let inspection_count = Arc::new(AtomicUsize::new(0));
            let integrations = Ports::new().open_step(Arc::new(ExchangeAfterPublish {
                registry: runtime.threads().clone(),
                offset_legacy_device,
                exchange_count: exchange_count.clone(),
                inspection_count: inspection_count.clone(),
            }));
            assert!(
                runtime
                    .open_raw(
                        &fixture_path,
                        OpenConfig::new(time_source, Arc::new(SeededRandom::new(43))).ports(integrations)
                    )
                    .is_err()
            );
            assert_eq!(exchange_count.load(Ordering::SeqCst), 1);
            assert_eq!(
                inspection_count.load(Ordering::SeqCst),
                1,
                "the open step reached every evidence assertion"
            );
            assert!(matches!(runtime.state(), LifecycleState::Failed { .. }));
        }
    }
    #[test]
    fn c12_migration_archives_exact_bytes_and_preserves_recovered_highwater_not_header_counter() {
        for reason in [
            MigrationReason::LegacyHeaderOnlyRebound,
            MigrationReason::LegacyIdentityUpgraded,
        ] {
            let (_dir, path, mut migration, mut state) = migration_fixture(reason);
            let original = migration.original.bytes.clone();
            if reason == MigrationReason::LegacyIdentityUpgraded {
                let answer = super::super::frozen::test_answer();
                for record in [
                    Record::Intent { event: 17, answer },
                    Record::Terminal {
                        event: 17,
                        status: Terminal::Cancelled,
                    },
                ] {
                    apply(&mut state, record.clone()).unwrap();
                    migration
                        .current_bytes
                        .extend(encode(if state.events[&17].terminal.is_some() { 3 } else { 2 }, &record).unwrap());
                }
                std::fs::write(&path, &migration.current_bytes).unwrap();
                assert_eq!(state.next_event, 18);
                assert_eq!(state.header.next_event, 17);
            }
            let (header, _binding) = migration.publish(&path, &state).unwrap();
            assert_eq!(std::fs::read(migration.archive_path()).unwrap(), original);
            assert_eq!(
                header.next_event,
                if reason == MigrationReason::LegacyIdentityUpgraded {
                    state.next_event
                } else {
                    1
                }
            );
            assert_eq!(header.incarnation, migration.next.incarnation);
            assert_eq!(header.notice.as_ref().unwrap().reason, reason);
            let landed = read_bytes_bound(&std::fs::read(&path).unwrap(), &header, migration.live)
                .unwrap()
                .0;
            assert!(landed.events.is_empty());
            assert_eq!(landed.header, header);
        }
    }
    #[test]
    fn c12_migration_refuses_unresolved_replacement_and_archive_collision() {
        for failure in ["unresolved", "source", "main", "archive", "symlink"] {
            let (dir, path, migration, mut state) = migration_fixture(MigrationReason::LegacyIdentityUpgraded);
            match failure {
                "unresolved" => {
                    let mut record = intent();
                    if let Record::Intent { event, .. } = &mut record {
                        *event = 17;
                    }
                    apply(&mut state, record).unwrap();
                }
                "source" => {
                    std::fs::rename(&path, dir.path().join("original")).unwrap();
                    std::fs::write(&path, &migration.current_bytes).unwrap();
                }
                "main" => {
                    std::fs::rename(&migration.collection, dir.path().join("original-main")).unwrap();
                    std::fs::write(&migration.collection, b"replacement").unwrap();
                }
                "archive" => std::fs::write(migration.archive_path(), b"different").unwrap(),
                "symlink" => std::os::unix::fs::symlink(&path, migration.archive_path()).unwrap(),
                _ => unreachable!(),
            }
            let before = std::fs::read(&path).unwrap();
            assert!(migration.publish(&path, &state).is_err(), "{failure}");
            assert_eq!(std::fs::read(&path).unwrap(), before, "{failure}");
        }
    }
    #[cfg(feature = "test-hooks")]
    #[test]
    fn c12_migration_faults_preserve_exact_archive_and_refuse_until_durable_publication() {
        use meguri_model::faultpoint::{self, Fault};
        for active in [false, true] {
            for point in [
                "storage.atomic.after_write",
                "storage.atomic.after_sync",
                "storage.atomic.before_rename",
                "storage.atomic.after_rename",
            ] {
                let (_dir, path, migration, state) = migration_fixture(MigrationReason::LegacyHeaderOnlyRebound);
                let original = std::fs::read(&path).unwrap();
                if !active {
                    faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
                }
                let failed = migration.publish_with(&path, &state, || {
                    if active {
                        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
                    }
                });
                faultpoint::disarm(point);
                assert!(failed.is_err(), "{active} {point}");
                let now = std::fs::read(&path).unwrap();
                if now != original {
                    // Rename landed but directory durability was not acknowledged.
                    let landed = RecoveryBinding::read(&path).unwrap();
                    landed.sync(&path).unwrap();
                    let recovered = read_bytes_bound(&now, &migration.next, migration.live).unwrap().0;
                    assert_eq!(recovered.header.version, 2);
                    assert!(recovered.events.is_empty());
                    assert_eq!(std::fs::read(migration.archive_path()).unwrap(), original);
                } else {
                    // Known original binding: completing the same verified steps
                    // reuses an exact archive rather than replacing evidence.
                    migration.publish(&path, &state).unwrap();
                    assert_eq!(std::fs::read(migration.archive_path()).unwrap(), original);
                }
            }
        }
    }

    fn recovery_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Header) {
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("collection.anki2");
        std::fs::write(&main, b"damaged main, no SQLite open permitted").unwrap();
        let metadata = std::fs::metadata(&main).unwrap();
        let h = Header {
            path: main.canonicalize().unwrap().as_os_str().as_bytes().to_vec(),
            native: Native::Legacy((metadata.dev(), metadata.ino())),
            ..header()
        };
        let journal = main.with_file_name("meguri-pending-answers.jsonl");
        let mut bytes = encode(1, &Record::Header(h.clone())).unwrap();
        bytes.extend(encode(2, &intent()).unwrap());
        bytes.extend(b"torn final frame");
        std::fs::write(&journal, bytes).unwrap();
        (dir, main, journal, h)
    }
    #[test]
    fn recovery_archive_keeps_unresolved_and_torn_bytes_before_retired_marker_and_rollback() {
        let (dir, main, journal, h) = recovery_fixture();
        let original = std::fs::read(&journal).unwrap();
        let main_bytes = std::fs::read(&main).unwrap();
        let pending = RecoveryJournal::prepare(&main).unwrap().unwrap();
        let marker = read(&journal, &h).unwrap().0;
        assert!(marker.retired);
        assert!(marker.events.is_empty());
        let archive = std::fs::read_dir(dir.path())
            .unwrap()
            .map(Result::unwrap)
            .find(|e| e.file_name().to_string_lossy().ends_with(".recovery.jsonl"))
            .unwrap();
        assert_eq!(std::fs::read(archive.path()).unwrap(), original);
        pending.finish(false).unwrap();
        assert_eq!(std::fs::read(&journal).unwrap(), original);
        assert_eq!(std::fs::read(&main).unwrap(), main_bytes);
        // An interrupted handoff leaves a durable retired marker and exact
        // archived bytes; a new native identity may bind, never replaying them.
        drop(RecoveryJournal::prepare(&main).unwrap());
        let mut next = h;
        next.native = Native::Legacy((7, 99));
        assert!(read(&journal, &next).unwrap().0.retired);
        assert_eq!(std::fs::read(archive.path()).unwrap(), original);
    }
    #[test]
    fn recovery_archive_rejects_wrong_identity_corruption_and_aliases_without_writing() {
        use std::os::unix::fs::symlink;
        for case in ["identity", "checksum", "symlink", "hardlink"] {
            let (dir, main, journal, mut h) = recovery_fixture();
            match case {
                "identity" => {
                    h.native = Native::Legacy((7, 99));
                    std::fs::write(&journal, encode(1, &Record::Header(h)).unwrap()).unwrap();
                }
                "checksum" => {
                    let mut bytes = std::fs::read(&journal).unwrap();
                    bytes[0] = b'x';
                    std::fs::write(&journal, bytes).unwrap();
                }
                "symlink" => {
                    let evidence = dir.path().join("evidence");
                    std::fs::rename(&journal, &evidence).unwrap();
                    symlink(evidence, &journal).unwrap();
                }
                "hardlink" => std::fs::hard_link(&journal, dir.path().join("evidence")).unwrap(),
                _ => unreachable!(),
            }
            let before = std::fs::read(&journal).unwrap();
            assert!(RecoveryJournal::prepare(&main).is_err(), "{case}");
            assert_eq!(std::fs::read(&journal).unwrap(), before, "{case}");
            assert!(
                !std::fs::read_dir(dir.path())
                    .unwrap()
                    .map(Result::unwrap)
                    .any(|e| e.file_name().to_string_lossy().ends_with(".recovery.jsonl"))
            );
        }
    }

    fn header() -> Header {
        Header {
            version: 1,
            path: b"/synthetic/collection.anki2".to_vec(),
            native: Native::Legacy((7, 9)),
            incarnation: [11, 12],
            next_event: 1,
            notice: None,
        }
    }
    fn write_frames(records: &[Record]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending.jsonl");
        let bytes = records
            .iter()
            .enumerate()
            .flat_map(|(i, r)| encode(i as u64 + 1, r).unwrap())
            .collect::<Vec<_>>();
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }
    fn intent() -> Record {
        Record::Intent {
            event: 1,
            answer: super::super::frozen::test_answer(),
        }
    }
    fn receipt() -> Receipt {
        Receipt {
            row: [19, 3, -1, 4, 3, 0, 433, 200, 0],
            base: [1; 32],
            post: [3; 32],
        }
    }
    #[test]
    fn c12_codec_torn_tail_retains_prefix_and_complete_checksum_corruption_is_fatal() {
        let (_dir, path) = write_frames(&[Record::Header(header()), intent()]);
        let mut bytes = std::fs::read(&path).unwrap();
        let prefix = bytes.len();
        bytes.extend_from_slice(b"{\"number\":3");
        std::fs::write(&path, &bytes).unwrap();
        let (state, frames, complete) = read(&path, &header()).unwrap();
        assert_eq!(frames, 2);
        assert_eq!(complete, prefix);
        assert_eq!(state.events.len(), 1);
        bytes.truncate(prefix);
        let at = bytes.iter().position(|b| *b == b'H').unwrap();
        bytes[at] = b'X';
        std::fs::write(&path, bytes).unwrap();
        assert!(read(&path, &header()).is_err());
    }
    #[test]
    fn c12_codec_missing_frame_and_unknown_version_forbid_ready_without_removing_evidence() {
        let (_dir, path) = write_frames(&[Record::Header(header())]);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&encode(3, &intent()).unwrap()).unwrap();
        drop(file);
        let before = std::fs::read(&path).unwrap();
        assert!(read(&path, &header()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut h = header();
        h.version = 99;
        std::fs::write(&path, encode(1, &Record::Header(h)).unwrap()).unwrap();
        assert!(read(&path, &header()).is_err());
    }
    #[test]
    fn c12_codec_immutable_prepared_identity_and_complete_row_are_required() {
        let r = receipt();
        let (_dir, path) = write_frames(&[
            Record::Header(header()),
            intent(),
            Record::Prepared {
                event: 1,
                receipt: r.clone(),
            },
        ]);
        assert_eq!(read(&path, &header()).unwrap().0.events[&1].receipt.as_ref(), Some(&r));
        let mut changed = r;
        changed.row[0] += 1;
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(
            &encode(
                4,
                &Record::Prepared {
                    event: 1,
                    receipt: changed,
                },
            )
            .unwrap(),
        )
        .unwrap();
        drop(file);
        assert!(read(&path, &header()).is_err());
    }
    #[test]
    fn c12_codec_persisted_prepared_after_any_terminal_remains_invalid() {
        for status in [Terminal::Cancelled, Terminal::Dropped, Terminal::Done] {
            let mut frames = vec![Record::Header(header()), intent()];
            if status == Terminal::Done {
                frames.push(Record::Prepared {
                    event: 1,
                    receipt: receipt(),
                });
            }
            frames.push(Record::Terminal { event: 1, status });
            let (_dir, path) = write_frames(&frames);
            assert!(read(&path, &header()).is_ok());
            frames.push(Record::Prepared {
                event: 1,
                receipt: receipt(),
            });
            let (_dir, path) = write_frames(&frames);
            assert!(read(&path, &header()).is_err());
        }
    }
    #[test]
    fn c12_codec_unexpected_native_identity_is_retained_and_retired_identity_can_rebind() {
        let (_dir, path) = write_frames(&[Record::Header(header())]);
        let mut expected = header();
        expected.native = Native::Legacy((7, 10));
        assert!(read(&path, &expected).is_err());
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&encode(2, &Record::Retired).unwrap()).unwrap();
        drop(file);
        assert!(read(&path, &expected).unwrap().0.retired);
    }
    #[test]
    fn c12_codec_same_card_dependencies_cannot_reference_missing_or_future_events() {
        let mut answer = super::super::frozen::test_answer();
        answer.base = Base::Predecessor(8);
        let (_dir, path) = write_frames(&[Record::Header(header()), Record::Intent { event: 1, answer }]);
        assert!(read(&path, &header()).is_err());
    }
    #[test]
    fn c12_codec_done_without_prepared_receipt_and_interior_blank_frame_are_corruption() {
        let (_dir, path) = write_frames(&[
            Record::Header(header()),
            intent(),
            Record::Terminal {
                event: 1,
                status: Terminal::Done,
            },
        ]);
        assert!(read(&path, &header()).is_err());
        let mut bytes = encode(1, &Record::Header(header())).unwrap();
        bytes.push(b'\n');
        bytes.extend(encode(2, &intent()).unwrap());
        std::fs::write(&path, bytes).unwrap();
        assert!(read(&path, &header()).is_err());
    }
}
