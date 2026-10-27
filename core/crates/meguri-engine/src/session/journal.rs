//! C12 ordered file worker. It owns no collection/SQLite/dispatcher/session lock.
use super::frozen::{Base, FrozenAnswer, error};
use crate::{EngineError, Generation, Qos, Seq, ThreadRegistry, monotonic};
use meguri_storage::atomic_file::{AtomicFile, Purpose, full_sync};
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
    Compact,
}
#[cfg(feature = "test-hooks")]
type StageHook = Arc<dyn Fn(JournalStage) + Send + Sync>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    path: Vec<u8>,
    native: (u64, u64),
    incarnation: [u64; 2],
    next_event: u64,
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
fn read(path: &Path, expected: &Header) -> Result<(State, u64, usize), EngineError> {
    read_bytes(&std::fs::read(path).map_err(error)?, expected)
}
fn read_bytes(bytes: &[u8], expected: &Header) -> Result<(State, u64, usize), EngineError> {
    let complete = bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    let mut header = None;
    let mut state = None;
    let mut number = 0;
    for line in bytes[..complete.saturating_sub(1)].split(|b| *b == b'\n') {
        let frame: Frame = serde_json::from_slice(line).map_err(error)?;
        number += 1;
        if frame.number != number || frame.checksum != checksum(frame.number, frame.payload.as_bytes()) {
            return Err(error("frame number/checksum mismatch; evidence retained"));
        }
        let record: Record = serde_json::from_str(&frame.payload).map_err(error)?;
        if number == 1 {
            let Record::Header(h) = record else {
                return Err(error("missing header"));
            };
            if h.version != 1 || h.path != expected.path {
                return Err(error("version/path mismatch"));
            }
            header = Some(h.clone());
            state = Some(State {
                next_event: h.next_event,
                header: h,
                events: BTreeMap::new(),
                failure: None,
                retired: false,
            });
        } else {
            apply(state.as_mut().ok_or_else(|| error("missing header"))?, record)?;
        }
    }
    let header = header.ok_or_else(|| error("missing complete header"))?;
    let state = state.unwrap();
    if !state.retired && header.native != expected.native {
        return Err(error("unexpected native collection identity; evidence retained"));
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
            version: 1,
            path: collection
                .canonicalize()
                .map_err(error)?
                .as_os_str()
                .as_bytes()
                .to_vec(),
            native: (native.dev(), native.ino()),
            incarnation: [0, 0],
            next_event: 1,
        };
        let original = RecoveryBinding::read(&path)?;
        let (loaded, _, _) = read_bytes(&original.bytes, &expected)?;
        // Even a retired file must bind to this old main in this recovery path.
        if loaded.header.native != expected.native {
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
        if !current.is_file() || (current.dev(), current.ino()) != expected.native {
            return Err(error("recovery main identity changed; evidence retained"));
        }
        let mut publication = AtomicFile::new(&path, Purpose::Sidecar)?;
        publication.file_mut().write_all(&marker).map_err(error)?;
        // Retain both inode and byte authorities before publication. An Err
        // from AtomicFile may follow rename or directory publication.
        let guard = Self {
            path,
            collection: collection.to_owned(),
            native: expected.native,
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
        self.marker.require(&self.path)?;
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

impl Journal {
    pub(crate) fn open(
        collection: &Path,
        native: (u64, u64),
        incarnation: [u64; 2],
        generation: Generation,
        threads: &ThreadRegistry,
    ) -> Result<Arc<Self>, EngineError> {
        use std::os::unix::ffi::OsStrExt;
        let path = collection.with_file_name("meguri-pending-answers.jsonl");
        let header = Header {
            version: 1,
            path: collection
                .canonicalize()
                .map_err(error)?
                .as_os_str()
                .as_bytes()
                .to_vec(),
            native,
            incarnation,
            next_event: 1,
        };
        if !path.try_exists().map_err(error)? {
            create(&path, &header, false)?;
        }
        let (mut loaded, mut number, complete) = read(&path, &header)?;
        if loaded.retired {
            create(&path, &header, true)?;
            loaded = read(&path, &header)?.0;
            number = 1;
        } else {
            // No acknowledgement came from a torn frame. Remove only that tail,
            // after validating the complete prefix and its bound identity.
            let file = OpenOptions::new().write(true).open(&path).map_err(error)?;
            if file.metadata().map_err(error)?.len() != complete as u64 {
                file.set_len(complete as u64).map_err(error)?;
                full_sync(&file)?;
            }
        }
        loaded.header.next_event = loaded.next_event;
        let file = OpenOptions::new().append(true).open(&path).map_err(error)?;
        let state = Arc::new(Mutex::new(loaded));
        let (tx, rx) = mpsc::channel();
        let shared = state.clone();
        let worker = threads
            .spawn_owned(
                "meguri-review-file",
                Qos::UserInitiated,
                "review-session",
                move |activity| {
                    file_loop(rx, file, path, shared, number, activity);
                },
            )
            .map_err(error)?;
        Ok(Arc::new(Self {
            tx,
            state,
            worker: Mutex::new(Some(worker)),
            generation,
        }))
    }
    pub(crate) fn check_identity(&self, native: (u64, u64)) -> Result<(), EngineError> {
        if self.state.lock().unwrap().header.native != native {
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
        self.append(Record::Retired)
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
            Work::Compact(reply) => {
                let result = (|| {
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
                    create(&path, &h, true)?;
                    file = OpenOptions::new().append(true).open(&path).map_err(error)?;
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
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[1])?;
                        written += 1;
                    }
                    if written != 0 {
                        full_sync(&file)?;
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
    fn recovery_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Header) {
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("collection.anki2");
        std::fs::write(&main, b"damaged main, no SQLite open permitted").unwrap();
        let metadata = std::fs::metadata(&main).unwrap();
        let h = Header {
            path: main.canonicalize().unwrap().as_os_str().as_bytes().to_vec(),
            native: (metadata.dev(), metadata.ino()),
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
        next.native.1 += 1;
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
                    h.native.1 += 1;
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
            native: (7, 9),
            incarnation: [11, 12],
            next_event: 1,
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
        expected.native.1 += 1;
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
