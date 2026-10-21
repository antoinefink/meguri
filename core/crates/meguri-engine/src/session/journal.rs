//! C12 ordered file worker. It owns no collection/SQLite/dispatcher/session lock.
use super::frozen::{Base, FrozenAnswer, error};
use crate::{EngineError, Generation, Qos, Seq, ThreadRegistry, monotonic};
use meguri_storage::atomic_file::{AtomicFile, Purpose, full_sync};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
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
    let bytes = std::fs::read(path).map_err(error)?;
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
    pub(crate) fn retire(&self) -> Result<(), EngineError> {
        self.append(Record::Retired)
    }
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
                let result = (|| {
                    if let Some(e) = &state.lock().unwrap().failure {
                        return Err(e.clone());
                    }
                    for (offset, (record, _, _)) in batch.iter().enumerate() {
                        apply(&mut validated, record.clone())?;
                        let bytes = encode(number + offset as u64 + 1, record)?;
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[0])?;
                        file.write_all(&bytes).map_err(error)?;
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[1])?;
                    }
                    full_sync(&file)?;
                    let covered_at = monotonic::now();
                    for (record, _, _) in &batch {
                        #[cfg(feature = "test-hooks")]
                        stage(&mut fault, &hook, record_stages(record)[2])?;
                        apply(&mut state.lock().unwrap(), record.clone())?;
                    }
                    number += batch.len() as u64;
                    if batch.iter().any(|(_, _, deadline)| covered_at > *deadline) {
                        return Err(error("acceptance durability deadline exceeded"));
                    }
                    Ok(())
                })();
                if let Err(e) = &result {
                    state.lock().unwrap().failure = Some(e.clone());
                }
                // Every receipt in this prefix is acknowledged only after
                // the full sync that covers its frame. Failure stays sticky.
                for (_, reply, _) in batch {
                    if let Some(reply) = reply {
                        let _ = reply.send(result.clone());
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
