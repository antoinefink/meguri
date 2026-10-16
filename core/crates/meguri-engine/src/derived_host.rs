//! The derived store's host (`specs/storage/derived-store.md`, U26): keeps
//! `meguri-derived.db` current for the open collection.
//!
//! - The in-core observer captures each component's delta on the collection
//!   writer thread and sends it to the derived writer (D6); sending never
//!   blocks.
//! - The derived writer (`meguri-derived-writer`, utility) is the only thread
//!   that writes the file: it binds the store at open (D5), applies commits,
//!   runs refreshes' batches, swaps rebuilt tables in and publishes readiness
//!   (D8).
//! - A refresh (rebuild or catch-up) reads one maintenance-reader snapshot on
//!   a builder thread (`meguri-derived-builder`, utility) and hands its
//!   batches to the writer (D7).
//! - Readers attach the file as `derived` for the generation the store is
//!   bound to (D9); `is_current` tells a consumer whether a component holds
//!   exactly the commits its snapshot sees, from the stamp's `seq` (D8).
//!
//! U38 builds the host before the collection opens, passes its ports to
//! `Engine::open_raw` (its open step first) and closes it after
//! `Engine::close` (D11).

use std::any::Any;
use std::collections::VecDeque;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use meguri_model::time::EpochMillis;
use meguri_storage::derived::{self, CollectionMark, DerivedFile, Found, Registry, Stamp, Target, Verdict};
pub use meguri_storage::derived::{ComponentSchema, ComponentTables, IndexDef, TableDef};
use meguri_storage::{ReadRows, Reader, ReaderRole};

use crate::changes::observers::ObservedCommit;
use crate::changes::stream::{ChangeStream, EventKind, ExtensionEvent, Seq};
use crate::collection::{Collection, CollectionCore};
use crate::error::{EngineError, panic_message};
use crate::lifecycle::{Engine, EngineInner, Generation, LifecycleState};
use crate::monotonic;
use crate::ports::{ChangeObserver, GenerationListener, OpenStep, Ports, SuspendParticipant};
use crate::readers::Snapshot;
use crate::threads::{Qos, ThreadActivity, ThreadRegistry};

/// Progress is published at most this often per component (D8, R1858).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// A refresh keeping more deltas than this starts again (D7).
const MAX_KEPT: usize = 100_000;

/// What a component takes from one commit, on the collection writer thread
/// (D4, D6).
pub enum Capture<D> {
    /// The commit does not concern the component.
    Nothing,
    /// Owned data the derived writer applies.
    Delta(D),
    /// Too much to apply (a large sync): rebuild the component instead.
    Rebuild,
}

/// The end of a component's catch-up (D4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchUp {
    Done,
    /// The component cannot catch up from what it was given: rebuild it.
    Rebuild,
}

/// A derived component (D4): its tables, how it follows commits, and how
/// it is built from the collection.
pub trait DerivedComponent: Send + Sync + 'static {
    type Delta: Send + 'static;

    /// Its name, version, tables and indexes; read once, at registration.
    fn schema(&self) -> ComponentSchema;

    /// Runs on the collection writer thread after each commit with a
    /// non-empty change set: keep it short, copy what `apply` needs.
    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<Self::Delta>;

    /// Writes one delta into the live tables, idempotently (D3).
    fn apply(&self, tables: &ComponentTables<'_>, delta: Self::Delta) -> Result<(), EngineError>;

    /// Fills the (empty) tables from the snapshot, in batches through
    /// `sink`, reporting progress. Stops when a batch returns an error.
    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError>;

    /// Brings the live tables up to date from the commit whose `col.mod` is
    /// `since` (after a crash, D5.2), in batches through `sink`. Without an
    /// override, the component is rebuilt.
    fn catch_up(
        &self,
        source: &Snapshot<'_>,
        since: EpochMillis,
        sink: &mut BuildSink,
    ) -> Result<CatchUp, EngineError> {
        let _ = (source, since, sink);
        Ok(CatchUp::Rebuild)
    }
}

/// A component's readiness for one generation (D8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// The tables hold the current content.
    Ready,
    /// A rebuild or catch-up runs.
    Building { completed: u64, total: Option<u64> },
    /// The content is not current: waiting for a refresh, failed, or the
    /// store is not bound.
    Stale,
}

/// The change-stream event of a readiness change (`engine/change-stream` S3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedReadiness {
    pub component: String,
    pub generation: Generation,
    pub readiness: Readiness,
}

impl ExtensionEvent for DerivedReadiness {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The change-stream event of the derived writer's progress (D8): the store
/// holds every commit up to `through` in each component `Ready` for
/// `generation`. A consumer that found a component not current asks
/// [`DerivedHost::is_current`] again with a later snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedApplied {
    pub generation: Generation,
    pub through: Seq,
}

impl ExtensionEvent for DerivedApplied {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Why the store is invalidated (D10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalidation {
    /// Check Database ran (U128).
    CheckDatabase,
    /// Another process changed the collection (U32, R1453).
    ExternalChange,
}

type AnyDelta = Box<dyn Any + Send>;
type Batch = Box<dyn FnOnce(&ComponentTables<'_>) -> Result<(), EngineError> + Send>;

/// A component behind its delta type.
trait Erased: Send + Sync {
    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<AnyDelta>;
    fn apply(&self, tables: &ComponentTables<'_>, delta: AnyDelta) -> Result<(), EngineError>;
    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError>;
    fn catch_up(&self, source: &Snapshot<'_>, since: EpochMillis, sink: &mut BuildSink)
    -> Result<CatchUp, EngineError>;
}

struct Adapter<C>(C);

impl<C: DerivedComponent> Erased for Adapter<C> {
    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<AnyDelta> {
        match self.0.capture(commit) {
            Capture::Nothing => Capture::Nothing,
            Capture::Delta(delta) => Capture::Delta(Box::new(delta)),
            Capture::Rebuild => Capture::Rebuild,
        }
    }

    fn apply(&self, tables: &ComponentTables<'_>, delta: AnyDelta) -> Result<(), EngineError> {
        match delta.downcast::<C::Delta>() {
            Ok(delta) => self.0.apply(tables, *delta),
            Err(_) => Err(EngineError::internal("a derived delta of another component")),
        }
    }

    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError> {
        self.0.build(source, sink)
    }

    fn catch_up(
        &self,
        source: &Snapshot<'_>,
        since: EpochMillis,
        sink: &mut BuildSink,
    ) -> Result<CatchUp, EngineError> {
        self.0.catch_up(source, since, sink)
    }
}

/// What the derived writer is asked to do.
enum Msg {
    /// The open step's collection, its mark and the seq of its snapshot.
    Bind {
        collection: Collection,
        mark: CollectionMark,
        seq: Seq,
    },
    Commit(CommitMsg),
    /// The collection was replaced (L8); the mark is `None` when it could not
    /// be read.
    Replaced {
        collection: Collection,
        mark: Option<(CollectionMark, Seq)>,
        ack: SyncSender<()>,
    },
    /// A batch of refresh `refresh`.
    Write {
        refresh: u64,
        batch: Batch,
        ack: SyncSender<Result<(), EngineError>>,
    },
    /// Refresh `refresh` ended; `snapshot` is its snapshot's seq.
    Done {
        refresh: u64,
        snapshot: Option<Seq>,
        outcome: Outcome,
    },
    Invalidate(Invalidation),
    Flush(SyncSender<()>),
    Suspend(SyncSender<()>),
    Resume(SyncSender<()>),
    Close(SyncSender<()>),
}

struct CommitMsg {
    seq: Seq,
    generation: Generation,
    mark: Option<CollectionMark>,
    deltas: Vec<(usize, Capture<AnyDelta>)>,
}

enum Outcome {
    Built,
    CaughtUp,
    NeedsRebuild,
    /// The collection entered an exclusive phase: start it again later.
    Deferred,
    /// The collection closed or was replaced, or the writer stopped it.
    Cancelled,
    Failed(EngineError),
}

/// Where readers attach the store from (D9).
#[derive(Clone, Debug)]
struct Bound {
    generation: Generation,
    path: PathBuf,
    epoch: u64,
}

struct SlotView {
    name: String,
    generation: Generation,
    readiness: Readiness,
    /// The refresh allowed to publish progress.
    refresh: Option<u64>,
    last_progress: Option<Duration>,
}

struct SharedState {
    bound: Option<Bound>,
    /// An unprivileged handle of the bound generation.
    collection: Option<Collection>,
    slots: Vec<SlotView>,
    /// No refresh is queued or running.
    idle: bool,
    closed: bool,
}

struct Shared {
    engine: Weak<EngineInner>,
    stream: ChangeStream,
    threads: ThreadRegistry,
    registry: Registry,
    components: Vec<Arc<dyn Erased>>,
    /// The seq of the last commit the observer sent to the derived writer.
    sent: Arc<AtomicU64>,
    state: Mutex<SharedState>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, SharedState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records and publishes a readiness change; `cause` is the seq of the
    /// commit that started it (D8).
    fn set_readiness(
        &self,
        state: &mut SharedState,
        slot: usize,
        generation: Generation,
        readiness: Readiness,
        cause: Option<Seq>,
    ) {
        let view = &mut state.slots[slot];
        if view.generation == generation && view.readiness == readiness {
            return;
        }
        view.generation = generation;
        view.readiness = readiness.clone();
        let event = DerivedReadiness {
            component: view.name.clone(),
            generation,
            readiness,
        };
        self.stream
            .publish(generation, cause, EventKind::Extension(Arc::new(event)));
        self.changed.notify_all();
    }

    fn engine(&self) -> Option<Engine> {
        self.engine.upgrade().map(|inner| Engine { inner })
    }
}

/// Builds a [`DerivedHost`]: register every component, then start.
pub struct DerivedHostBuilder {
    engine: Engine,
    schemas: Vec<ComponentSchema>,
    components: Vec<Arc<dyn Erased>>,
}

impl DerivedHostBuilder {
    pub fn component<C: DerivedComponent>(mut self, component: C) -> DerivedHostBuilder {
        self.schemas.push(component.schema());
        self.components.push(Arc::new(Adapter(component)));
        self
    }

    /// Checks the components (D4) and starts the derived writer.
    pub fn start(self) -> Result<DerivedHost, EngineError> {
        let registry = Registry::new(self.schemas)?;
        let slots = registry
            .schemas()
            .iter()
            .map(|schema| SlotView {
                name: schema.name.clone(),
                generation: Generation(0),
                readiness: Readiness::Stale,
                refresh: None,
                last_progress: None,
            })
            .collect();
        let shared = Arc::new(Shared {
            engine: Arc::downgrade(&self.engine.inner),
            stream: self.engine.stream().clone(),
            threads: self.engine.threads().clone(),
            registry,
            components: self.components,
            sent: Arc::default(),
            state: Mutex::new(SharedState {
                bound: None,
                collection: None,
                slots,
                idle: true,
                closed: false,
            }),
            changed: Condvar::new(),
        });
        let (sender, inbox) = mpsc::channel();
        let worker = Worker::new(Arc::clone(&shared), inbox, sender.clone());
        let writer = shared
            .threads
            .spawn("meguri-derived-writer", Qos::Utility, move |activity| {
                worker.run(activity)
            })
            .map_err(|e| EngineError::internal(format!("cannot start the derived writer: {e}")))?;
        Ok(DerivedHost {
            shared,
            sender,
            writer: Mutex::new(Some(writer)),
        })
    }
}

/// The derived store of one open collection (D5 to D11).
pub struct DerivedHost {
    shared: Arc<Shared>,
    sender: Sender<Msg>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for DerivedHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.shared.lock();
        f.debug_struct("DerivedHost")
            .field("bound", &state.bound)
            .field(
                "components",
                &state.slots.iter().map(|s| (&s.name, &s.readiness)).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl DerivedHost {
    pub fn builder(engine: &Engine) -> DerivedHostBuilder {
        DerivedHostBuilder {
            engine: engine.clone(),
            schemas: Vec::new(),
            components: Vec::new(),
        }
    }

    /// `ports` with the store's ports: its open step first among the open
    /// steps, its observer, generation listener and suspend participant.
    pub fn ports(&self, ports: Ports) -> Ports {
        let port = Arc::new(HostPort {
            shared: Arc::clone(&self.shared),
            sender: self.sender.clone(),
        });
        let mut ports = ports
            .observer(Arc::new(HostObserver {
                sender: self.sender.clone(),
                components: self.shared.components.clone(),
                sent: Arc::clone(&self.shared.sent),
            }))
            .generation_listener(port.clone())
            .suspend_participant(port.clone());
        ports.open_steps.insert(0, port);
        ports
    }

    /// The readiness of `component`, with the generation it is for.
    pub fn readiness(&self, component: &str) -> Option<DerivedReadiness> {
        let state = self.shared.lock();
        state
            .slots
            .iter()
            .find(|slot| slot.name == component)
            .map(|slot| DerivedReadiness {
                component: slot.name.clone(),
                generation: slot.generation,
                readiness: slot.readiness.clone(),
            })
    }

    /// Whether `component` is `Ready` for `generation`: it follows the
    /// commits, but the latest may not have reached it yet. Before reading
    /// its tables, ask [`DerivedHost::is_current`].
    pub fn is_ready(&self, component: &str, generation: Generation) -> bool {
        self.readiness(component)
            .is_some_and(|r| r.generation == generation && r.readiness == Readiness::Ready)
    }

    /// Whether `component`'s tables, as `snapshot` reads them, hold exactly
    /// the commits the snapshot sees (D8). Commits reach the store after
    /// they are published, so a `Ready` component can be behind the
    /// snapshot; and SQLite fixes the snapshot's view of the store at its
    /// first read of `derived`, not when the snapshot began, so that view
    /// can also hold later commits. The answer comes from the store as the
    /// snapshot reads it, so it holds for each of the snapshot's reads of
    /// `derived`, before or after it: ask before the first. When it is
    /// false, read without the component, and ask again with a later
    /// snapshot at the next [`DerivedApplied`] or `Ready`.
    pub fn is_current(&self, component: &str, snapshot: &Snapshot<'_>) -> bool {
        let seq = snapshot.seq();
        // In this order: once every op up to the snapshot's has run the
        // observers, `sent` is at or after each commit the store was sent.
        let observed = self.shared.stream.settled_through() >= seq;
        let sent = Seq(self.shared.sent.load(Ordering::SeqCst));
        let bound = self
            .shared
            .lock()
            .bound
            .as_ref()
            .is_some_and(|bound| bound.generation == snapshot.generation());
        if !bound {
            return false;
        }
        match snapshot.derived_seq(component) {
            // The store holds every commit up to `held`; the commits after
            // it that it was not sent change nothing in it. A view holding
            // a commit after `seq` does not match the collection the
            // snapshot sees.
            Ok(Some(held)) => {
                let held = Seq(u64::try_from(held).unwrap_or(0));
                held == seq || (held < seq && observed && held >= sent)
            }
            Ok(None) => false,
            Err(error) => {
                tracing::warn!(%error, component, "reading the derived store's stamp");
                false
            }
        }
    }

    /// Rebuilds every component through staging (D10).
    pub fn invalidate(&self, why: Invalidation) {
        let _ = self.sender.send(Msg::Invalidate(why));
    }

    /// Returns once every commit observed before the call is applied or
    /// kept by a refresh.
    pub fn flush(&self) -> Result<(), EngineError> {
        ask(&self.sender, Msg::Flush)
    }

    /// Waits until everything sent before the call is handled and no refresh
    /// is queued or running, at most `timeout`; whether it got there.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = monotonic::now() + timeout;
        if self.flush().is_err() {
            return true;
        }
        let mut state = self.shared.lock();
        loop {
            if state.idle || state.closed {
                return true;
            }
            let now = monotonic::now();
            if now >= deadline {
                return false;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The store's file, while it is bound.
    pub fn path(&self) -> Option<PathBuf> {
        self.shared.lock().bound.as_ref().map(|bound| bound.path.clone())
    }

    /// Applies what is pending, marks the store clean and closes it (D11).
    /// Call it after `Engine::close`.
    pub fn close(&self) -> Result<(), EngineError> {
        let result = ask(&self.sender, Msg::Close);
        let writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        result
    }
}

impl Drop for DerivedHost {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(%error, "closing the derived store");
        }
    }
}

/// Sends a message with an acknowledgement and waits for it; a stopped
/// writer counts as done.
fn ask(sender: &Sender<Msg>, message: impl FnOnce(SyncSender<()>) -> Msg) -> Result<(), EngineError> {
    let (ack, done) = mpsc::sync_channel(1);
    if sender.send(message(ack)).is_err() {
        return Ok(());
    }
    let _ = done.recv();
    Ok(())
}

/// The collection mark and the seq of one snapshot of `collection`.
fn read_mark(collection: &Collection) -> Result<(CollectionMark, Seq), EngineError> {
    let lease = collection.reader(ReaderRole::Maintenance)?;
    let snapshot = lease.snapshot()?;
    let col = snapshot.col()?;
    Ok((CollectionMark::of(&col), snapshot.seq()))
}

fn unprivileged(core: &Arc<CollectionCore>, generation: Generation) -> Collection {
    Collection {
        core: Arc::clone(core),
        generation,
        privilege: None,
    }
}

/// The observer: captures on the collection writer thread (D6). It holds
/// no `Shared`: that keeps a `Collection`, which keeps the observers.
struct HostObserver {
    sender: Sender<Msg>,
    components: Vec<Arc<dyn Erased>>,
    /// `Shared::sent`.
    sent: Arc<AtomicU64>,
}

impl ChangeObserver for HostObserver {
    fn name(&self) -> &str {
        "derived-store"
    }

    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        let mut deltas = Vec::new();
        for (slot, component) in self.components.iter().enumerate() {
            let captured = panic::catch_unwind(AssertUnwindSafe(|| component.capture(commit)));
            match captured {
                Ok(Capture::Nothing) => {}
                Ok(capture) => deltas.push((slot, capture)),
                Err(payload) => {
                    tracing::warn!(message = %panic_message(payload.as_ref()), "a derived capture panicked");
                    deltas.push((slot, Capture::Rebuild));
                }
            }
        }
        let mark = commit.collection.map(CollectionMark::of);
        if deltas.is_empty() && mark.is_none() {
            return Ok(());
        }
        // Before the commit is published (`is_current`).
        self.sent.fetch_max(commit.seq.0, Ordering::SeqCst);
        let _ = self.sender.send(Msg::Commit(CommitMsg {
            seq: commit.seq,
            generation: commit.generation,
            mark,
            deltas,
        }));
        Ok(())
    }
}

/// The open step, generation listener and suspend participant.
struct HostPort {
    shared: Arc<Shared>,
    sender: Sender<Msg>,
}

impl OpenStep for HostPort {
    fn name(&self) -> &str {
        "derived-store"
    }

    /// D5: the mark and seq in one snapshot, then the rest on the derived
    /// writer. Never fails the open.
    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        let handle = unprivileged(&collection.core, collection.generation);
        let attachment = Arc::new(Attachment {
            shared: Arc::downgrade(&self.shared),
        });
        if collection.core.derived.set(attachment).is_err() {
            tracing::warn!("a second derived store for one collection is ignored");
            return Ok(());
        }
        match read_mark(&handle) {
            Ok((mark, seq)) => {
                self.shared.lock().collection = Some(handle.clone());
                let _ = self.sender.send(Msg::Bind {
                    collection: handle,
                    mark,
                    seq,
                });
            }
            Err(error) => tracing::warn!(%error, "the derived store cannot read the collection mark"),
        }
        Ok(())
    }
}

impl GenerationListener for HostPort {
    fn name(&self) -> &str {
        "derived-store"
    }

    /// D10: returns once the old rows are gone and the new generation is
    /// bound.
    fn collection_replaced(&self, _old: Generation, new: Generation) {
        let Some(core) = self.shared.lock().collection.as_ref().map(|c| Arc::clone(&c.core)) else {
            return;
        };
        let handle = unprivileged(&core, new);
        let mark = match read_mark(&handle) {
            Ok(mark) => Some(mark),
            Err(error) => {
                tracing::warn!(%error, "the derived store cannot read the new collection's mark");
                None
            }
        };
        let _ = ask(&self.sender, |ack| Msg::Replaced {
            collection: handle,
            mark,
            ack,
        });
    }
}

impl SuspendParticipant for HostPort {
    fn name(&self) -> &str {
        "derived-store"
    }

    fn flush(&self) -> Result<(), EngineError> {
        ask(&self.sender, Msg::Flush)
    }

    fn suspend(&self) -> Result<(), EngineError> {
        ask(&self.sender, Msg::Suspend)
    }

    fn resume(&self) -> Result<(), EngineError> {
        ask(&self.sender, Msg::Resume)
    }
}

/// Attaches the store to collection readers (D9).
pub(crate) struct Attachment {
    shared: Weak<Shared>,
}

impl Attachment {
    /// Attaches the store's file to a reader leased for `generation` when
    /// the store is bound to it, and detaches it otherwise.
    pub(crate) fn ensure(&self, reader: &mut Reader, generation: Generation) {
        let wanted = self.shared.upgrade().and_then(|shared| {
            shared
                .lock()
                .bound
                .clone()
                .filter(|bound| bound.generation == generation)
        });
        let result = match wanted {
            Some(bound) => reader.attach_derived(&bound.path, bound.epoch),
            None => reader.detach_derived(),
        };
        if let Err(error) = result {
            tracing::warn!(%error, role = ?reader.role(), "attaching the derived store to a reader");
        }
    }
}

/// What a component's slot is doing, as the writer sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotState {
    Ready,
    /// Waiting for its refresh: deltas are dropped, the refresh's snapshot
    /// will hold them.
    Queued,
    /// Its refresh runs: deltas are kept.
    Refreshing,
    /// Not current and not queued (a failed refresh).
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Rebuild,
    CatchUp { since: i64 },
}

/// A refresh waiting to run.
#[derive(Clone, Copy, Debug)]
struct Queued {
    slot: usize,
    kind: Kind,
    /// The seq of the commit that made the component stale; every readiness
    /// event of the refresh carries it (D8).
    cause: Option<Seq>,
}

struct Running {
    id: u64,
    slot: usize,
    kind: Kind,
    cause: Option<Seq>,
    abort: Arc<AtomicBool>,
    kept: Vec<(Seq, AnyDelta)>,
    /// Too much was kept, or a commit asked for a rebuild: start again.
    restart: bool,
    /// A commit asked for a rebuild: start again as one, even a catch-up,
    /// which may not see what that commit changed (D6).
    rebuild: bool,
    /// A batch failed: the refresh fails, whatever its build made of it.
    failed_batch: Option<EngineError>,
}

impl Running {
    /// The refresh to queue in its place.
    fn queued(&self) -> Queued {
        Queued {
            slot: self.slot,
            kind: if self.rebuild { Kind::Rebuild } else { self.kind },
            cause: self.cause,
        }
    }

    /// Keeps a commit's capture for the refresh's last transaction (D6, D7).
    fn keep(&mut self, seq: Seq, capture: Capture<AnyDelta>) {
        match capture {
            Capture::Nothing => {}
            Capture::Rebuild => {
                if !self.rebuild {
                    self.cause = Some(seq);
                }
                self.rebuild = true;
                self.restart = true;
                self.kept.clear();
            }
            Capture::Delta(_) if self.restart => {}
            Capture::Delta(delta) if self.kept.len() < MAX_KEPT => self.kept.push((seq, delta)),
            Capture::Delta(_) => {
                self.restart = true;
                self.kept.clear();
            }
        }
    }
}

/// Runs a component's code on the derived writer: a panic is its error, so
/// it fails that component's part and never stops the writer (D6, D7).
fn guarded<T>(work: impl FnOnce() -> Result<T, EngineError>) -> Result<T, EngineError> {
    panic::catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())))
}

/// The derived writer's own state.
struct Worker {
    shared: Arc<Shared>,
    inbox: Receiver<Msg>,
    sender: Sender<Msg>,
    /// A message read while batching commits, handled next.
    pending: Option<Msg>,
    file: Option<DerivedFile>,
    collection: Option<Collection>,
    generation: Option<Generation>,
    bind_seq: Seq,
    /// The seq of the last commit handled: every ready component holds the
    /// commits up to it (the stamp's `seq`, D3).
    through: Seq,
    epoch: u64,
    mark: Option<CollectionMark>,
    slots: Vec<SlotState>,
    queue: VecDeque<Queued>,
    running: Option<Running>,
    next_refresh: u64,
    /// The refreshes whose builder has not ended yet, stopped ones too: each
    /// may still hold a collection snapshot.
    builders: Vec<u64>,
    /// Suspends waiting for those builders to end (D11).
    suspend_acks: Vec<SyncSender<()>>,
    /// Failed refreshes in a row, per component.
    failures: Vec<u32>,
    suspended: bool,
    stop: bool,
}

impl Worker {
    fn new(shared: Arc<Shared>, inbox: Receiver<Msg>, sender: Sender<Msg>) -> Worker {
        let count = shared.components.len();
        Worker {
            shared,
            inbox,
            sender,
            pending: None,
            file: None,
            collection: None,
            generation: None,
            bind_seq: Seq(0),
            through: Seq(0),
            epoch: 0,
            mark: None,
            slots: vec![SlotState::Failed; count],
            queue: VecDeque::new(),
            running: None,
            next_refresh: 1,
            builders: Vec::new(),
            suspend_acks: Vec::new(),
            failures: vec![0; count],
            suspended: false,
            stop: false,
        }
    }

    fn run(mut self, activity: &ThreadActivity) {
        while !self.stop {
            {
                let _busy = activity.busy();
                self.start_refresh();
                self.publish_idle();
            }
            let message = match self.pending.take() {
                Some(message) => message,
                // Idle: wait without a timeout (R1854).
                None => match self.inbox.recv() {
                    Ok(message) => message,
                    Err(_) => break,
                },
            };
            let _busy = activity.busy();
            self.handle(message);
        }
        self.publish_idle();
    }

    fn schema(&self, slot: usize) -> &ComponentSchema {
        &self.shared.registry.schemas()[slot]
    }

    fn handle(&mut self, message: Msg) {
        match message {
            Msg::Bind { collection, mark, seq } => self.bind(collection, mark, seq),
            Msg::Commit(commit) => {
                let mut batch = vec![commit];
                // Several queued commits in one transaction.
                while batch.len() < 256 {
                    match self.inbox.try_recv() {
                        Ok(Msg::Commit(commit)) => batch.push(commit),
                        Ok(other) => {
                            self.pending = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                self.apply_commits(batch);
            }
            Msg::Replaced { collection, mark, ack } => {
                self.replaced(collection, mark);
                let _ = ack.send(());
            }
            Msg::Write { refresh, batch, ack } => {
                let _ = ack.send(self.write_batch(refresh, batch));
            }
            Msg::Done {
                refresh,
                snapshot,
                outcome,
            } => {
                self.builders.retain(|id| *id != refresh);
                self.done(refresh, snapshot, outcome);
                self.ack_suspend();
            }
            Msg::Invalidate(why) => self.invalidate(why),
            Msg::Flush(ack) => {
                let _ = ack.send(());
            }
            Msg::Suspend(ack) => {
                // The running refresh stops at its next batch; the writer
                // goes on serving messages (refusing that refresh's batches)
                // and answers once its builder let go of its snapshot.
                self.requeue_running();
                self.suspended = true;
                self.suspend_acks.push(ack);
                self.ack_suspend();
            }
            Msg::Resume(ack) => {
                self.suspended = false;
                self.mark_clean(false);
                let _ = ack.send(());
            }
            Msg::Close(ack) => {
                self.close();
                let _ = ack.send(());
            }
        }
    }

    /// Answers the waiting suspends once no builder may hold a collection
    /// snapshot, after marking the store clean when it is still suspended
    /// (D11).
    fn ack_suspend(&mut self) {
        if self.suspend_acks.is_empty() || !self.builders.is_empty() {
            return;
        }
        if self.suspended {
            self.mark_clean(true);
        }
        for ack in self.suspend_acks.drain(..) {
            let _ = ack.send(());
        }
    }

    /// D5.
    fn bind(&mut self, collection: Collection, mark: CollectionMark, seq: Seq) {
        let path = derived::derived_path(collection.path());
        let generation = collection.generation();
        let (mut file, found) = match DerivedFile::open(&path) {
            Ok(opened) => opened,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "the derived store cannot be opened");
                return;
            }
        };
        if let Found::Recreated { reason } = &found {
            tracing::warn!(%reason, path = %path.display(), "the derived store was replaced");
        }
        let through = self.through.max(seq);
        let verdicts = match bind_file(&mut file, &self.shared.registry, &mark, through) {
            Ok(verdicts) => verdicts,
            Err(error) => {
                tracing::warn!(%error, "binding the derived store");
                return;
            }
        };
        self.epoch += 1;
        self.file = Some(file);
        self.collection = Some(collection.clone());
        self.generation = Some(generation);
        self.bind_seq = seq;
        self.through = through;
        self.mark = Some(mark);
        self.queue.clear();
        let mut state = self.shared.lock();
        state.bound = Some(Bound {
            generation,
            path,
            epoch: self.epoch,
        });
        state.collection = Some(collection);
        for (slot, verdict) in verdicts.into_iter().enumerate() {
            let readiness = match verdict {
                Verdict::Fresh => {
                    self.slots[slot] = SlotState::Ready;
                    Readiness::Ready
                }
                Verdict::CatchUp { since } => {
                    self.slots[slot] = SlotState::Queued;
                    self.queue.push_back(Queued {
                        slot,
                        kind: Kind::CatchUp { since },
                        cause: None,
                    });
                    Readiness::Stale
                }
                Verdict::Rebuild(reason) => {
                    tracing::info!(component = %self.schema(slot).name, ?reason, "the derived component rebuilds");
                    self.slots[slot] = SlotState::Queued;
                    self.queue.push_back(Queued {
                        slot,
                        kind: Kind::Rebuild,
                        cause: None,
                    });
                    Readiness::Stale
                }
            };
            self.shared.set_readiness(&mut state, slot, generation, readiness, None);
        }
    }

    /// D6.
    fn apply_commits(&mut self, batch: Vec<CommitMsg>) {
        let Some(through) = batch.iter().map(|c| c.seq).max() else {
            return;
        };
        self.through = self.through.max(through);
        let Some(generation) = self.generation else { return };
        let mut commits: Vec<CommitMsg> = batch
            .into_iter()
            .filter(|c| c.generation == generation && c.seq > self.bind_seq)
            .collect();
        let last = commits.last().map_or(through, |c| c.seq);
        // Before the transaction, whatever becomes of it: the running
        // refresh keeps its deltas in memory for its last transaction, and a
        // "rebuild" capture makes a waiting catch-up a rebuild.
        for commit in &mut commits {
            let deltas = std::mem::take(&mut commit.deltas);
            for (slot, capture) in deltas {
                if let Some(running) = self.running.as_mut().filter(|r| r.slot == slot) {
                    running.keep(commit.seq, capture);
                } else if self.slots[slot] == SlotState::Queued && matches!(capture, Capture::Rebuild) {
                    self.rebuild_queued(slot, commit.seq);
                } else {
                    commit.deltas.push((slot, capture));
                }
            }
        }
        if let Err(error) = meguri_model::faultpoint!("engine.derived.before_apply") {
            tracing::warn!(%error, "applying derived commits");
            self.all_ready_stale(last);
            return;
        }
        let shared = Arc::clone(&self.shared);
        let held = stored_seq(self.through);
        let Some(file) = self.file.as_mut() else { return };
        let mut failed: Vec<(usize, Seq)> = Vec::new();
        let mut mark = self.mark;
        let result = (|| -> Result<(), EngineError> {
            let tx = file.begin()?;
            for commit in commits {
                for (slot, capture) in commit.deltas {
                    // Only a ready component applies: a queued one's refresh
                    // reads a later snapshot, a failed one waits for the next
                    // bind or invalidation, the refreshing one kept its delta.
                    if self.slots[slot] != SlotState::Ready {
                        continue;
                    }
                    match capture {
                        Capture::Nothing => {}
                        Capture::Rebuild => {
                            self.slots[slot] = SlotState::Queued;
                            failed.push((slot, commit.seq));
                        }
                        Capture::Delta(delta) => {
                            let schema = &shared.registry.schemas()[slot];
                            let component = &shared.components[slot];
                            let savepoint = tx.savepoint()?;
                            match guarded(|| component.apply(&tx.tables(schema, Target::Live), delta)) {
                                Ok(()) => savepoint.release()?,
                                Err(error) => {
                                    savepoint.rollback()?;
                                    tracing::warn!(component = %schema.name, %error, "a derived apply failed");
                                    self.slots[slot] = SlotState::Queued;
                                    failed.push((slot, commit.seq));
                                }
                            }
                        }
                    }
                }
                if let Some(next) = commit.mark {
                    mark = Some(next);
                }
            }
            for (slot, _) in &failed {
                tx.set_ready(&shared.registry.schemas()[*slot].name, false)?;
            }
            if let Some(mark) = mark
                && mark != self.mark.unwrap_or(mark)
            {
                tx.set_stamp(&Stamp { mark, clean: false })?;
            }
            tx.set_seq(held)?;
            tx.commit()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.mark = mark;
                for (slot, cause) in failed {
                    self.queue_refresh(slot, Kind::Rebuild, Some(cause));
                }
                self.publish_applied(generation);
            }
            Err(error) => {
                tracing::warn!(%error, "a derived transaction failed");
                for (slot, _) in failed {
                    self.slots[slot] = SlotState::Ready;
                }
                self.all_ready_stale(last);
            }
        }
    }

    /// Marks every ready component stale and queues its rebuild, because the
    /// commits up to `cause` did not reach it.
    fn all_ready_stale(&mut self, cause: Seq) {
        let ready: Vec<usize> = (0..self.slots.len())
            .filter(|slot| self.slots[*slot] == SlotState::Ready)
            .collect();
        if let Some(file) = self.file.as_mut()
            && let Ok(tx) = file.begin()
        {
            for slot in &ready {
                let _ = tx.set_ready(&self.shared.registry.schemas()[*slot].name, false);
            }
            let _ = tx.commit();
        }
        for slot in ready {
            self.queue_refresh(slot, Kind::Rebuild, Some(cause));
        }
    }

    /// Tells consumers the store holds every commit up to `through`, when a
    /// component can be read (D8).
    fn publish_applied(&self, generation: Generation) {
        let state = self.shared.lock();
        let readable = state
            .slots
            .iter()
            .any(|slot| slot.generation == generation && slot.readiness == Readiness::Ready);
        if readable {
            let event = DerivedApplied {
                generation,
                through: self.through,
            };
            self.shared
                .stream
                .publish(generation, Some(self.through), EventKind::Extension(Arc::new(event)));
        }
    }

    /// A commit asked `slot` for a rebuild while its refresh waits: a
    /// catch-up may not see what that commit changed, so it rebuilds (D6).
    fn rebuild_queued(&mut self, slot: usize, cause: Seq) {
        if let Some(queued) = self.queue.iter_mut().find(|queued| queued.slot == slot)
            && queued.kind != Kind::Rebuild
        {
            queued.kind = Kind::Rebuild;
            queued.cause = Some(cause);
        }
    }

    /// Queues a refresh of `slot` once and publishes `Stale`.
    fn queue_refresh(&mut self, slot: usize, kind: Kind, cause: Option<Seq>) {
        self.slots[slot] = SlotState::Queued;
        if !self.queue.iter().any(|queued| queued.slot == slot) {
            self.queue.push_back(Queued { slot, kind, cause });
        }
        if let Some(generation) = self.generation {
            let mut state = self.shared.lock();
            self.shared
                .set_readiness(&mut state, slot, generation, Readiness::Stale, cause);
        }
    }

    /// Starts the next queued refresh when none runs (D7).
    fn start_refresh(&mut self) {
        if self.running.is_some() || self.suspended || self.stop || self.file.is_none() {
            return;
        }
        let (Some(collection), Some(generation)) = (self.collection.clone(), self.generation) else {
            return;
        };
        let Some(Queued { slot, kind, cause }) = self.queue.pop_front() else {
            return;
        };
        let schema = self.schema(slot).clone();
        if kind == Kind::Rebuild {
            let staged = self.file.as_mut().map(|file| -> Result<(), EngineError> {
                let tx = file.begin()?;
                tx.begin_staging(&schema)?;
                tx.commit()?;
                Ok(())
            });
            if let Some(Err(error)) = staged {
                tracing::warn!(component = %schema.name, %error, "creating the staging tables");
                self.slots[slot] = SlotState::Failed;
                return;
            }
        }
        let id = self.next_refresh;
        self.next_refresh += 1;
        let abort = Arc::new(AtomicBool::new(false));
        self.slots[slot] = SlotState::Refreshing;
        self.running = Some(Running {
            id,
            slot,
            kind,
            cause,
            abort: Arc::clone(&abort),
            kept: Vec::new(),
            restart: false,
            rebuild: false,
            failed_batch: None,
        });
        {
            let mut state = self.shared.lock();
            state.slots[slot].refresh = Some(id);
            state.slots[slot].last_progress = None;
            self.shared.set_readiness(
                &mut state,
                slot,
                generation,
                Readiness::Building {
                    completed: 0,
                    total: None,
                },
                cause,
            );
        }
        let builder = Builder {
            shared: Arc::clone(&self.shared),
            sender: self.sender.clone(),
            collection,
            slot,
            kind,
            cause,
            refresh: id,
            abort,
        };
        let spawned = self
            .shared
            .threads
            .spawn("meguri-derived-builder", Qos::Utility, move |activity| {
                builder.run(activity)
            });
        match spawned {
            Ok(_) => self.builders.push(id),
            Err(error) => {
                tracing::warn!(%error, "cannot start a derived builder");
                self.running = None;
                self.forget_refresh(slot);
                self.fail(slot, cause);
            }
        }
    }

    fn forget_refresh(&self, slot: usize) {
        self.shared.lock().slots[slot].refresh = None;
    }

    /// Runs one batch of the running refresh (D7). A failed batch fails the
    /// refresh.
    fn write_batch(&mut self, refresh: u64, batch: Batch) -> Result<(), EngineError> {
        let Some(running) = self.running.as_mut().filter(|r| r.id == refresh) else {
            return Err(EngineError::Cancelled);
        };
        let target = match running.kind {
            Kind::Rebuild => Target::Staging,
            Kind::CatchUp { .. } => Target::Live,
        };
        let schema = &self.shared.registry.schemas()[running.slot];
        let result = match self.file.as_mut() {
            None => Err(EngineError::Cancelled),
            Some(file) => (|| -> Result<(), EngineError> {
                let tx = file.begin()?;
                guarded(|| batch(&tx.tables(schema, target)))?;
                tx.commit()?;
                Ok(())
            })(),
        };
        if let Err(error) = &result {
            tracing::warn!(component = %schema.name, %error, "a derived refresh batch failed");
            running.failed_batch.get_or_insert_with(|| error.clone());
        }
        result
    }

    /// A refresh ended (D7).
    fn done(&mut self, refresh: u64, snapshot: Option<Seq>, outcome: Outcome) {
        if self.running.as_ref().is_none_or(|r| r.id != refresh) {
            return;
        }
        let Some(running) = self.running.take() else { return };
        self.forget_refresh(running.slot);
        let (slot, kind, cause) = (running.slot, running.kind, running.cause);
        let schema = self.schema(slot).clone();
        if running.restart {
            self.discard_staging(&schema, kind);
            self.slots[slot] = SlotState::Queued;
            self.queue.push_front(running.queued());
            return;
        }
        let outcome = match (outcome, &running.failed_batch) {
            (Outcome::Built | Outcome::CaughtUp, Some(error)) => Outcome::Failed(error.clone()),
            (outcome, _) => outcome,
        };
        match outcome {
            Outcome::Built | Outcome::CaughtUp => {
                let snapshot = snapshot.unwrap_or_default();
                match self.finish(&schema, slot, running, snapshot) {
                    Ok(()) => {
                        self.failures[slot] = 0;
                        self.slots[slot] = SlotState::Ready;
                        if let Some(generation) = self.generation {
                            let mut state = self.shared.lock();
                            self.shared
                                .set_readiness(&mut state, slot, generation, Readiness::Ready, cause);
                        }
                    }
                    Err(error) => {
                        tracing::warn!(component = %schema.name, %error, "finishing a derived refresh");
                        self.discard_staging(&schema, Kind::Rebuild);
                        self.retry_or_fail(slot, cause);
                    }
                }
            }
            Outcome::NeedsRebuild => {
                self.slots[slot] = SlotState::Queued;
                self.queue.push_front(Queued {
                    slot,
                    kind: Kind::Rebuild,
                    cause,
                });
            }
            Outcome::Deferred => {
                self.discard_staging(&schema, kind);
                self.slots[slot] = SlotState::Queued;
                self.queue.push_front(running.queued());
                if let Some(generation) = self.generation {
                    let mut state = self.shared.lock();
                    self.shared
                        .set_readiness(&mut state, slot, generation, Readiness::Stale, cause);
                }
            }
            Outcome::Cancelled => {
                self.discard_staging(&schema, kind);
                self.fail(slot, cause);
            }
            Outcome::Failed(error) => {
                tracing::warn!(component = %schema.name, %error, "a derived refresh failed");
                self.discard_staging(&schema, kind);
                self.retry_or_fail(slot, cause);
            }
        }
    }

    /// The last transaction of a refresh: the swap (rebuild), the deltas kept
    /// after the snapshot, `ready` 1. The component then holds every commit
    /// the writer handled: the stamp's `seq` says so. Its builder sent `Done`
    /// only once every commit the snapshot sees had run the observers, so
    /// those after `through` were not sent: they concern no component. The
    /// stamp is the whole store's: never the snapshot's seq, which the other
    /// ready components may not hold yet.
    fn finish(
        &mut self,
        schema: &ComponentSchema,
        slot: usize,
        running: Running,
        snapshot: Seq,
    ) -> Result<(), EngineError> {
        let component = Arc::clone(&self.shared.components[slot]);
        let held = stored_seq(self.through);
        let file = self.file.as_mut().ok_or(EngineError::Cancelled)?;
        let tx = file.begin()?;
        if running.kind == Kind::Rebuild {
            tx.finish_staging(schema)?;
        }
        for (seq, delta) in running.kept {
            if seq > snapshot {
                guarded(|| component.apply(&tx.tables(schema, Target::Live), delta))?;
            }
        }
        tx.set_ready(&schema.name, true)?;
        tx.set_seq(held)?;
        tx.commit()?;
        Ok(())
    }

    fn discard_staging(&mut self, schema: &ComponentSchema, kind: Kind) {
        if kind != Kind::Rebuild {
            return;
        }
        if let Some(file) = self.file.as_mut()
            && let Ok(tx) = file.begin()
            && tx.discard_staging(schema).is_ok()
        {
            let _ = tx.commit();
        }
    }

    /// A failed refresh is rebuilt once more; a second failure in a row
    /// leaves the component `Stale` until the next bind or invalidation.
    fn retry_or_fail(&mut self, slot: usize, cause: Option<Seq>) {
        self.failures[slot] += 1;
        if self.failures[slot] < 2 {
            self.queue_refresh(slot, Kind::Rebuild, cause);
        } else {
            self.fail(slot, cause);
        }
    }

    fn fail(&mut self, slot: usize, cause: Option<Seq>) {
        self.slots[slot] = SlotState::Failed;
        if let Some(generation) = self.generation {
            let mut state = self.shared.lock();
            self.shared
                .set_readiness(&mut state, slot, generation, Readiness::Stale, cause);
        }
    }

    /// Stops the running refresh: its builder lets go of its reader at its
    /// next batch. Returns it; its `Done` is ignored.
    fn abort_running(&mut self) -> Option<Running> {
        let running = self.running.take()?;
        // Under the lifecycle lock, so a builder waiting for the exclusive
        // phase to end cannot miss it.
        match self.shared.engine() {
            Some(engine) => {
                let guard = engine.lock_state();
                running.abort.store(true, Ordering::SeqCst);
                drop(guard);
                engine.inner.changed.notify_all();
            }
            None => running.abort.store(true, Ordering::SeqCst),
        }
        self.forget_refresh(running.slot);
        Some(running)
    }

    /// Stops the running refresh and queues it first again.
    fn requeue_running(&mut self) {
        if let Some(running) = self.abort_running() {
            let schema = self.schema(running.slot).clone();
            self.discard_staging(&schema, running.kind);
            self.slots[running.slot] = SlotState::Queued;
            self.queue.push_front(running.queued());
        }
    }

    /// D10: empties every component and binds the new generation.
    fn replaced(&mut self, collection: Collection, mark: Option<(CollectionMark, Seq)>) {
        if self.file.is_none()
            && let Some((mark, seq)) = mark
        {
            // The store could not be opened before: try again for this one.
            self.bind(collection, mark, seq);
            return;
        }
        self.abort_running();
        self.queue.clear();
        let generation = collection.generation();
        if let Some((_, seq)) = mark {
            self.through = self.through.max(seq);
        }
        let held = stored_seq(self.through);
        let schemas = self.shared.registry.schemas().to_vec();
        let reset = self.file.as_mut().map(|file| -> Result<(), EngineError> {
            let tx = file.begin()?;
            for schema in &schemas {
                tx.reset_component(schema)?;
            }
            if let Some((mark, _)) = mark {
                tx.set_stamp(&Stamp { mark, clean: false })?;
                tx.set_seq(held)?;
            }
            tx.commit()?;
            Ok(())
        });
        let usable = matches!(reset, Some(Ok(()))) && mark.is_some();
        if let Some(Err(error)) = &reset {
            tracing::warn!(%error, "emptying the derived store after a replacement");
        }
        self.generation = Some(generation);
        self.collection = Some(collection.clone());
        self.bind_seq = mark.map(|(_, seq)| seq).unwrap_or_default();
        self.mark = mark.map(|(mark, _)| mark);
        let mut state = self.shared.lock();
        state.collection = Some(collection);
        state.bound = match (&state.bound, usable) {
            (Some(bound), true) => Some(Bound {
                generation,
                ..bound.clone()
            }),
            _ => None,
        };
        for slot in 0..self.slots.len() {
            if usable {
                self.slots[slot] = SlotState::Queued;
                self.queue.push_back(Queued {
                    slot,
                    kind: Kind::Rebuild,
                    cause: None,
                });
            } else {
                self.slots[slot] = SlotState::Failed;
            }
            state.slots[slot].refresh = None;
            self.shared
                .set_readiness(&mut state, slot, generation, Readiness::Stale, None);
        }
        if !usable {
            // Nothing can be read through a store that does not describe the
            // new collection.
            drop(state);
            if let Some(file) = self.file.take() {
                let _ = file.close();
            }
        }
    }

    fn invalidate(&mut self, why: Invalidation) {
        let Some(generation) = self.generation else { return };
        if self.file.is_none() {
            return;
        }
        tracing::info!(?why, "the derived store is invalidated");
        if let Some(running) = self.abort_running() {
            let schema = self.schema(running.slot).clone();
            self.discard_staging(&schema, running.kind);
        }
        self.queue.clear();
        if let Some(file) = self.file.as_mut()
            && let Ok(tx) = file.begin()
        {
            for schema in self.shared.registry.schemas() {
                let _ = tx.set_ready(&schema.name, false);
            }
            let _ = tx.commit();
        }
        let mut state = self.shared.lock();
        for slot in 0..self.slots.len() {
            self.slots[slot] = SlotState::Queued;
            self.queue.push_back(Queued {
                slot,
                kind: Kind::Rebuild,
                cause: None,
            });
            self.shared
                .set_readiness(&mut state, slot, generation, Readiness::Stale, None);
        }
    }

    /// Writes `clean` and checkpoints (D11).
    fn mark_clean(&mut self, clean: bool) {
        let Some(mark) = self.mark else { return };
        let Some(file) = self.file.as_mut() else { return };
        let written = file.begin().and_then(|tx| {
            tx.set_stamp(&Stamp { mark, clean })?;
            tx.commit()
        });
        if let Err(error) = written {
            tracing::warn!(%error, "writing the derived stamp");
        } else if clean && let Err(error) = file.checkpoint() {
            tracing::warn!(%error, "checkpointing the derived store");
        }
    }

    fn close(&mut self) {
        self.requeue_running();
        self.mark_clean(true);
        if let Some(file) = self.file.take()
            && let Err(error) = file.close()
        {
            tracing::warn!(%error, "closing the derived store");
        }
        self.collection = None;
        self.queue.clear();
        self.stop = true;
        // A suspend still waiting gets its answer: nothing runs any more.
        for ack in self.suspend_acks.drain(..) {
            let _ = ack.send(());
        }
        let mut state = self.shared.lock();
        state.bound = None;
        state.collection = None;
        state.closed = true;
        self.shared.changed.notify_all();
    }

    fn publish_idle(&self) {
        let idle = self.running.is_none()
            && self.builders.is_empty()
            && (self.queue.is_empty() || self.suspended || self.file.is_none() || self.stop);
        let mut state = self.shared.lock();
        if state.idle != idle {
            state.idle = idle;
            self.shared.changed.notify_all();
        }
    }
}

/// D5.1 and D5.2 on the file: syncs the components, assesses each, stamps
/// the collection's mark with `clean` 0 and the bind's seq `through`, and
/// clears `ready` of every component that is not fresh.
fn bind_file(
    file: &mut DerivedFile,
    registry: &Registry,
    mark: &CollectionMark,
    through: Seq,
) -> Result<Vec<Verdict>, EngineError> {
    let tx = file.begin()?;
    let synced = tx.sync_components(registry)?;
    let stamp = tx.stamp()?;
    let verdicts: Vec<Verdict> = synced
        .iter()
        .map(|s| derived::assess(stamp.as_ref(), &s.record, s.reset, mark))
        .collect();
    for (synced, verdict) in synced.iter().zip(&verdicts) {
        if *verdict != Verdict::Fresh {
            tx.set_ready(&synced.record.name, false)?;
        }
    }
    tx.set_stamp(&Stamp {
        mark: *mark,
        clean: false,
    })?;
    tx.set_seq(stored_seq(through))?;
    tx.commit()?;
    Ok(verdicts)
}

/// A seq as the stamp stores it (D3).
fn stored_seq(seq: Seq) -> i64 {
    i64::try_from(seq.0).unwrap_or(i64::MAX)
}

/// Whether a builder may read now.
enum Usability {
    Usable,
    /// Opening or in an exclusive phase: wait.
    Wait,
    /// Closed, failed or replaced.
    Gone,
}

fn usability(state: &crate::lifecycle::LcState, core: &Arc<CollectionCore>, generation: Generation) -> Usability {
    let same = state
        .current
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.core, core));
    match &state.state {
        LifecycleState::Opening | LifecycleState::Upgrading { .. } => Usability::Wait,
        LifecycleState::ExclusiveBusy { .. } if same => Usability::Wait,
        LifecycleState::Ready { generation: g } if same && *g == generation => Usability::Usable,
        _ => Usability::Gone,
    }
}

/// One refresh, on its own thread (D7).
struct Builder {
    shared: Arc<Shared>,
    sender: Sender<Msg>,
    collection: Collection,
    slot: usize,
    kind: Kind,
    cause: Option<Seq>,
    refresh: u64,
    abort: Arc<AtomicBool>,
}

impl Builder {
    /// Always ends with `Done`, which the writer waits for before a suspend
    /// returns: the snapshot is released by then.
    fn run(self, activity: &ThreadActivity) {
        let (snapshot, outcome) = panic::catch_unwind(AssertUnwindSafe(|| self.refresh(activity)))
            .unwrap_or_else(|payload| (None, Outcome::Failed(EngineError::from_panic(payload.as_ref()))));
        // A commit the snapshot sees may not have run the observers yet
        // (they run after the commit lock is released). Once they all have,
        // each message the store was sent for them is ahead of `Done`, so the
        // writer has handled them, and its `through`, the stamp's `seq` the
        // refresh writes, covers every commit the rows hold (D8).
        if let (Some(seq), Outcome::Built | Outcome::CaughtUp) = (snapshot, &outcome) {
            let _busy = activity.busy();
            self.shared.stream.wait_settled(seq);
        }
        let _ = self.sender.send(Msg::Done {
            refresh: self.refresh,
            snapshot,
            outcome,
        });
    }

    /// Waits while the collection opens or is in an exclusive phase; false
    /// once it is gone or the refresh was stopped.
    fn wait_usable(&self) -> bool {
        let Some(engine) = self.shared.engine() else {
            return false;
        };
        let mut state = engine.lock_state();
        loop {
            if self.abort.load(Ordering::SeqCst) {
                return false;
            }
            match usability(&state, &self.collection.core, self.collection.generation()) {
                Usability::Usable => return true,
                Usability::Gone => return false,
                Usability::Wait => {
                    state = engine.inner.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
    }

    fn refresh(&self, activity: &ThreadActivity) -> (Option<Seq>, Outcome) {
        if !self.wait_usable() {
            return (None, Outcome::Cancelled);
        }
        let _busy = activity.busy();
        let lease = match self.collection.reader(ReaderRole::Maintenance) {
            Ok(lease) => lease,
            Err(EngineError::StaleGeneration { .. } | EngineError::CollectionUnavailable { .. }) => {
                return (None, Outcome::Cancelled);
            }
            Err(error) => return (None, Outcome::Failed(error)),
        };
        let snapshot = match lease.snapshot() {
            Ok(snapshot) => snapshot,
            Err(EngineError::StaleGeneration { .. }) => return (None, Outcome::Cancelled),
            Err(error) => return (None, Outcome::Failed(error)),
        };
        let seq = snapshot.seq();
        let mut sink = BuildSink {
            shared: Arc::clone(&self.shared),
            sender: self.sender.clone(),
            core: Arc::clone(&self.collection.core),
            generation: self.collection.generation(),
            slot: self.slot,
            refresh: self.refresh,
            cause: self.cause,
            abort: Arc::clone(&self.abort),
            deferred: false,
        };
        let component = Arc::clone(&self.shared.components[self.slot]);
        let result = panic::catch_unwind(AssertUnwindSafe(|| match self.kind {
            Kind::Rebuild => component.build(&snapshot, &mut sink).map(|()| Outcome::Built),
            Kind::CatchUp { since } => component
                .catch_up(&snapshot, EpochMillis(since), &mut sink)
                .map(|caught| match caught {
                    CatchUp::Done => Outcome::CaughtUp,
                    CatchUp::Rebuild => Outcome::NeedsRebuild,
                }),
        }));
        let outcome = match result {
            Ok(Ok(outcome)) => {
                // The last batch's check: a refresh stopped after its last
                // write does not count.
                match sink.check() {
                    Ok(()) => outcome,
                    Err(_) if sink.deferred => Outcome::Deferred,
                    Err(_) => Outcome::Cancelled,
                }
            }
            Ok(Err(_)) if sink.deferred => Outcome::Deferred,
            Ok(Err(_)) if self.abort.load(Ordering::SeqCst) => Outcome::Cancelled,
            Ok(Err(EngineError::Cancelled)) => Outcome::Cancelled,
            Ok(Err(error)) => Outcome::Failed(error),
            Err(payload) => Outcome::Failed(EngineError::from_panic(payload.as_ref())),
        };
        (Some(seq), outcome)
    }
}

/// Where a refresh writes its batches and reports progress (D7).
pub struct BuildSink {
    shared: Arc<Shared>,
    sender: Sender<Msg>,
    core: Arc<CollectionCore>,
    generation: Generation,
    slot: usize,
    refresh: u64,
    /// The refresh's cause, for its progress events (D8).
    cause: Option<Seq>,
    abort: Arc<AtomicBool>,
    /// Stopped because the collection entered an exclusive phase.
    deferred: bool,
}

impl fmt::Debug for BuildSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildSink")
            .field("refresh", &self.refresh)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl BuildSink {
    /// `Cancelled` once the refresh must stop: stopped by the store, the
    /// collection entered an exclusive phase (D7), closed or was replaced.
    pub fn check(&mut self) -> Result<(), EngineError> {
        if self.abort.load(Ordering::SeqCst) {
            return Err(EngineError::Cancelled);
        }
        let Some(engine) = self.shared.engine() else {
            return Err(EngineError::Cancelled);
        };
        let state = engine.lock_state();
        match usability(&state, &self.core, self.generation) {
            Usability::Usable => Ok(()),
            Usability::Wait => {
                self.deferred = true;
                Err(EngineError::Cancelled)
            }
            Usability::Gone => Err(EngineError::Cancelled),
        }
    }

    /// Runs `batch` in its own derived transaction on the tables being
    /// refreshed, after [`BuildSink::check`].
    pub fn write(
        &mut self,
        batch: impl FnOnce(&ComponentTables<'_>) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<(), EngineError> {
        self.check()?;
        let (ack, done) = mpsc::sync_channel(1);
        self.sender
            .send(Msg::Write {
                refresh: self.refresh,
                batch: Box::new(batch),
                ack,
            })
            .map_err(|_| EngineError::Cancelled)?;
        done.recv().unwrap_or(Err(EngineError::Cancelled))
    }

    /// Reports `completed` of `total` rows; published at most every 100 ms.
    pub fn progress(&mut self, completed: u64, total: Option<u64>) {
        let now = monotonic::now();
        let mut state = self.shared.lock();
        let view = &mut state.slots[self.slot];
        if view.refresh != Some(self.refresh) {
            return;
        }
        if view
            .last_progress
            .is_some_and(|last| now.saturating_sub(last) < PROGRESS_INTERVAL)
        {
            return;
        }
        view.last_progress = Some(now);
        self.shared.set_readiness(
            &mut state,
            self.slot,
            self.generation,
            Readiness::Building { completed, total },
            self.cause,
        );
    }
}
