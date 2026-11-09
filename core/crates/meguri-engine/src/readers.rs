//! Readers by role (KTD3, `engine/lifecycle` L11): one connection per role
//! (and per search worker and server reader), opened when first asked for and
//! kept until the collection closes or is replaced. One user holds a reader
//! at a time. Long-lived snapshots use the sync or the maintenance reader,
//! never the interactive, query or server ones.

use std::ops::{Deref, DerefMut};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use meguri_storage::{ReadSnapshot, Reader, ReaderFactory, ReaderRole};

use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::executor::ExecShared;
use crate::lifecycle::Generation;

/// Search workers, each with its own reader (U85 sets how many it uses).
pub const SEARCH_READERS: usize = 4;
/// Server readers, for the local servers' read handlers (R1790).
pub const SERVER_READERS: usize = 2;

pub(crate) struct Readers {
    factory: Mutex<Option<ReaderFactory>>,
    slots: Vec<Slot>,
}

struct Slot {
    role: ReaderRole,
    index: usize,
    reader: Mutex<SlotState>,
    returned: Condvar,
}

#[derive(Default)]
struct SlotState {
    reader: Option<Reader>,
    // None alone also means unopened or captured. Only the pin may return
    // ownership while this is set; neither lease nor close may take it.
    transferred: bool,
}

pub(crate) struct ReaderGuard<'a> {
    slot: &'a Slot,
    state: MutexGuard<'a, SlotState>,
}

impl Deref for ReaderGuard<'_> {
    type Target = Option<Reader>;

    fn deref(&self) -> &Self::Target {
        &self.state.reader
    }
}

impl DerefMut for ReaderGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state.reader
    }
}

impl Readers {
    pub(crate) fn new(factory: ReaderFactory) -> Readers {
        let mut slots = Vec::new();
        for role in ReaderRole::ALL {
            let count = match role {
                ReaderRole::Search => SEARCH_READERS,
                ReaderRole::Server => SERVER_READERS,
                _ => 1,
            };
            for index in 0..count {
                slots.push(Slot {
                    role,
                    index,
                    reader: Mutex::new(SlotState::default()),
                    returned: Condvar::new(),
                });
            }
        }
        Readers {
            factory: Mutex::new(Some(factory)),
            slots,
        }
    }

    /// The reader of `role` number `index`, opened if it is not yet, once
    /// `check` passed. `check` runs while the reader is held: a replacement
    /// closes every reader before it changes the generation (L8), so what
    /// `check` saw holds until the lease drops.
    pub(crate) fn lease(
        &self,
        role: ReaderRole,
        index: usize,
        check: impl FnOnce() -> Result<(), EngineError>,
    ) -> Result<ReaderGuard<'_>, EngineError> {
        let slot = self
            .slots
            .iter()
            .find(|s| s.role == role && s.index == index)
            .ok_or_else(|| EngineError::InvalidArgument {
                message: format!("there is no {role:?} reader {index}"),
            })?;
        let mut reader = slot.reader.lock().unwrap_or_else(PoisonError::into_inner);
        while reader.transferred {
            #[cfg(test)]
            tests::waiting();
            reader = slot.returned.wait(reader).unwrap_or_else(PoisonError::into_inner);
            #[cfg(test)]
            if let Some(hook) = tests::take_woken() {
                // Test scheduler: pause a woken requester outside its actual
                // slot, allowing a real replacement to overtake it.
                drop(reader);
                hook();
                reader = slot.reader.lock().unwrap_or_else(PoisonError::into_inner);
            }
        }
        check()?;
        if reader.reader.is_none() {
            let factory = self
                .factory
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or_else(|| EngineError::unavailable("the collection's readers are closed"))?;
            reader.reader = Some(factory.open(role)?);
        }
        Ok(ReaderGuard { slot, state: reader })
    }

    /// Closes every reader, waiting for those in use; no reader opens again
    /// until a factory is set.
    pub(crate) fn close_all(&self) {
        *self.factory.lock().unwrap_or_else(PoisonError::into_inner) = None;
        for slot in &self.slots {
            let mut reader = slot.reader.lock().unwrap_or_else(PoisonError::into_inner);
            while reader.transferred {
                #[cfg(test)]
                tests::waiting();
                reader = slot.returned.wait(reader).unwrap_or_else(PoisonError::into_inner);
            }
            reader.reader.take();
        }
    }

    pub(crate) fn close_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        *self.factory.lock().unwrap_or_else(PoisonError::into_inner) = None;
        for slot in &self.slots {
            loop {
                let mut reader = match slot.reader.try_lock() {
                    Ok(reader) => reader,
                    Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        if crate::monotonic::now() >= deadline {
                            return Err(EngineError::SuspendDeadline {
                                phase: "reader leases".into(),
                            });
                        }
                        // Ordinary leases retain their mutex guard. Keep their
                        // existing deadline polling; transferred ownership can
                        // instead wait on its return notification below.
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                };
                while reader.transferred {
                    let now = crate::monotonic::now();
                    if now >= deadline {
                        return Err(EngineError::SuspendDeadline {
                            phase: "reader leases".into(),
                        });
                    }
                    (reader, _) = slot
                        .returned
                        .wait_timeout(reader, deadline - now)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                reader.reader.take();
                break;
            }
        }
        Ok(())
    }

    /// Readers open on the collection `factory` names from now on.
    pub(crate) fn set_factory(&self, factory: ReaderFactory) {
        *self.factory.lock().unwrap_or_else(PoisonError::into_inner) = Some(factory);
    }
}

/// A reader held by one user until dropped.
pub struct ReaderLease<'a> {
    guard: ReaderGuard<'a>,
    exec: &'a ExecShared,
    generation: Generation,
}

impl<'a> ReaderLease<'a> {
    pub(crate) fn new(guard: ReaderGuard<'a>, exec: &'a ExecShared, generation: Generation) -> ReaderLease<'a> {
        ReaderLease {
            guard,
            exec,
            generation,
        }
    }

    /// Transfers an uncaptured Cloud reader's existing lifetime protection,
    /// releasing its slot mutex. Refusal returns this lease and guard intact.
    pub fn into_lifetime_pin(self) -> Result<ReaderLifetimePin<'a>, (Self, EngineError)> {
        let error = match self.guard.as_ref() {
            None => Some("a captured reader cannot become a lifetime pin"),
            Some(reader) if reader.role() != ReaderRole::Cloud => Some("a lifetime pin requires the Cloud reader"),
            Some(_) => None,
        };
        if let Some(message) = error {
            return Err((
                self,
                EngineError::InvalidArgument {
                    message: message.into(),
                },
            ));
        }
        let ReaderLease {
            mut guard,
            exec,
            generation,
        } = self;
        let reader = guard.take().expect("validated uncaptured Cloud reader");
        guard.state.transferred = true;
        let pin = ReaderLifetimePin {
            slot: guard.slot,
            reader: Some(reader),
            _exec: exec,
            generation,
            thread_bound: std::marker::PhantomData,
        };
        drop(guard);
        Ok(pin)
    }

    /// Begins a snapshot (`engine/op-model` O6): a read transaction and the
    /// seq of the last commit it sees, taken under the commit lock.
    pub fn snapshot(&self) -> Result<Snapshot<'_>, EngineError> {
        let current = self.exec.generation();
        if current != self.generation {
            return Err(EngineError::StaleGeneration {
                planned: self.generation,
                current,
            });
        }
        let last = self.exec.lock_commit();
        let inner = self.begin()?;
        let seq = *last;
        drop(last);
        Ok(Snapshot {
            inner,
            seq,
            generation: self.generation,
            wal_identity: crate::WalIdentity::new(self.generation, self.reader().native_identity()),
        })
    }

    /// H4: the lease stays on the job thread, excluding replacement until the
    /// owned snapshot is dropped. Pinning and undo capture run on the writer.
    pub fn capture_sync(
        &mut self,
        collection: &crate::Collection,
    ) -> Result<(meguri_storage::snapshot::SyncSnapshot, crate::SyncHold), EngineError> {
        if self.generation != collection.generation() || !std::ptr::eq(self.exec, &*collection.core.exec) {
            return Err(EngineError::internal("sync reader belongs to another collection"));
        }
        let reader = self
            .guard
            .take()
            .ok_or_else(|| EngineError::internal("sync reader already captured"))?;
        let exec = collection.core.exec.clone();
        let result = collection.run(
            crate::OpSpec::named("sync_snapshot_barrier", crate::OpSource::Rollover),
            move |ctx| {
                let snapshot = reader.pin_sync()?;
                let session = *exec.active_session.lock().unwrap();
                let hold = ctx.begin_sync_hold(session)?;
                Ok((snapshot, hold))
            },
        )?;
        Ok(result.value)
    }

    /// P7: acquire this lease off the writer; pin and flag atomically between
    /// ops. The returned value borrows the lease, keeping replacement excluded.
    pub fn capture_cloud(
        &mut self,
        collection: &crate::Collection,
        keys: Vec<(meguri_storage::Table, meguri_storage::RowKey)>,
    ) -> Result<(CloudSnapshot<'_>, crate::CloudHold), EngineError> {
        if self.generation != collection.generation() || !std::ptr::eq(self.exec, &*collection.core.exec) {
            return Err(EngineError::internal("cloud reader belongs to another collection"));
        }
        let reader = self
            .guard
            .as_ref()
            .ok_or_else(|| EngineError::internal("cloud reader already captured"))?;
        if reader.role() != ReaderRole::Cloud {
            return Err(EngineError::InvalidArgument {
                message: "Cloud capture requires the Cloud reader".into(),
            });
        }
        let identity = crate::WalIdentity::new(self.generation, self.reader().native_identity());
        let reader = self
            .guard
            .take()
            .ok_or_else(|| EngineError::internal("cloud reader already captured"))?;
        let exec = collection.core.exec.clone();
        let result = collection.run(
            crate::OpSpec::named("cloud_snapshot_barrier", crate::OpSource::CloudIncoming),
            move |ctx| {
                let snapshot = reader.pin_cloud()?;
                let session = *exec.active_session.lock().unwrap();
                let hold = ctx.begin_cloud_hold(session, &keys)?;
                Ok((snapshot, hold, session))
            },
        )?;
        let (inner, hold, session) = result.value;
        Ok((
            CloudSnapshot {
                inner,
                seq: hold.snapshot,
                session,
                identity,
                lease: std::marker::PhantomData,
            },
            hold,
        ))
    }

    fn begin(&self) -> Result<ReadSnapshot<'_>, EngineError> {
        Ok(self.reader().begin_snapshot()?)
    }

    fn reader(&self) -> &Reader {
        self.guard.as_ref().expect("a lease always holds an open reader")
    }
}

impl Deref for ReaderLease<'_> {
    type Target = Reader;

    fn deref(&self) -> &Reader {
        self.reader()
    }
}

/// Thread-bound ownership of the same Cloud reader, with no held slot mutex.
/// Dropping it returns the connection before any lease or close may proceed.
/// It permits identity checks only, not SQLite work or new admission.
pub struct ReaderLifetimePin<'a> {
    slot: &'a Slot,
    reader: Option<Reader>,
    _exec: &'a ExecShared,
    generation: Generation,
    // Reader is Send; the original lease's MutexGuard is not. Retain that
    // restriction after releasing the actual guard. Reader also remains !Sync.
    thread_bound: std::marker::PhantomData<MutexGuard<'a, ()>>,
}

impl ReaderLifetimePin<'_> {
    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub fn wal_identity(&self) -> crate::WalIdentity {
        crate::WalIdentity::new(
            self.generation,
            self.reader
                .as_ref()
                .expect("a live pin owns its reader")
                .native_identity(),
        )
    }
}

impl Drop for ReaderLifetimePin<'_> {
    fn drop(&mut self) {
        let mut state = self.slot.reader.lock().unwrap_or_else(PoisonError::into_inner);
        state.reader = self.reader.take();
        state.transferred = false;
        self.slot.returned.notify_all();
    }
}

/// A read snapshot with the seq of the last commit it sees.
pub struct Snapshot<'a> {
    inner: ReadSnapshot<'a>,
    seq: Seq,
    generation: Generation,
    wal_identity: crate::WalIdentity,
}

impl Snapshot<'_> {
    /// Physical file checked when this role's connection opened, paired with
    /// the generation protected by the lease. This is not a durability receipt.
    pub fn wal_identity(&self) -> crate::WalIdentity {
        self.wal_identity
    }

    /// The seq of the last commit the snapshot sees: the `since` of its
    /// preconditions (O7).
    pub fn seq(&self) -> Seq {
        self.seq
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }
}

impl<'a> Deref for Snapshot<'a> {
    type Target = ReadSnapshot<'a>;

    fn deref(&self) -> &ReadSnapshot<'a> {
        &self.inner
    }
}

/// A Cloud snapshot cannot outlive its role lease. It carries the real
/// pre-barrier seq and physical identity; neither is a durability receipt.
pub struct CloudSnapshot<'a> {
    inner: meguri_storage::snapshot::CloudSnapshot,
    seq: Seq,
    identity: crate::WalIdentity,
    session: Option<crate::SessionId>,
    lease: std::marker::PhantomData<&'a mut ()>,
}
impl CloudSnapshot<'_> {
    pub fn active_session(&self) -> Option<crate::SessionId> {
        self.session
    }
    pub fn seq(&self) -> Seq {
        self.seq
    }
    pub fn generation(&self) -> Generation {
        self.identity.generation()
    }
    pub fn wal_identity(&self) -> crate::WalIdentity {
        self.identity
    }
}
impl Deref for CloudSnapshot<'_> {
    type Target = meguri_storage::snapshot::CloudSnapshot;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use meguri_model::clock::{StepClock, UtcOffset};
    use meguri_model::rng::SeededRandom;
    use meguri_storage::ReadRows;
    use meguri_storage::raw::SqlValue;

    thread_local! {
        static WAITING: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
        static WOKEN: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    pub(crate) fn on_waiting(hook: impl FnOnce() + 'static) {
        WAITING.with_borrow_mut(|slot| *slot = Some(Box::new(hook)));
    }

    pub(crate) fn on_woken(hook: impl FnOnce() + 'static) {
        WOKEN.with_borrow_mut(|slot| *slot = Some(Box::new(hook)));
    }

    pub(super) fn waiting() {
        let hook = WAITING.with_borrow_mut(Option::take);
        if let Some(hook) = hook {
            hook();
        }
    }

    pub(super) fn take_woken() -> Option<Box<dyn FnOnce()>> {
        WOKEN.with_borrow_mut(Option::take)
    }

    pub(crate) fn open() -> (tempfile::TempDir, crate::Engine, crate::Collection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../conformance/fixtures/storage/schema-18.anki2"),
            &path,
        )
        .unwrap();
        let engine = crate::Engine::new(crate::EngineConfig::default()).unwrap();
        let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
        let collection = engine
            .open_raw(&path, crate::OpenConfig::new(clock, Arc::new(SeededRandom::new(7))))
            .unwrap();
        (dir, engine, collection)
    }

    #[test]
    fn cloud_lifetime_pin_releases_actual_slot_mutex_and_excludes_a_second_reader() {
        let (_dir, engine, collection) = open();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        // Connection-local state distinguishes the same SQLite connection from
        // another connection to the same native file.
        lease.execute_raw("PRAGMA cache_size = -137", &[]).unwrap();
        let identity = lease.native_identity();
        let generation = collection.generation();
        let pin = lease
            .into_lifetime_pin()
            .unwrap_or_else(|(_, error)| panic!("{error:?}"));
        assert_eq!(pin.wal_identity(), crate::WalIdentity::new(generation, identity));
        assert_eq!(pin.generation(), generation);
        let slot = collection
            .core
            .readers
            .slots
            .iter()
            .find(|s| s.role == ReaderRole::Cloud)
            .unwrap();
        // This is the actual Cloud slot, as would be observed by reentrant
        // callback code, not an unrelated test lock.
        let state = slot.reader.try_lock().expect("pin must release its actual guard");
        assert!(state.transferred);
        assert!(state.reader.is_none());
        drop(state);
        for role in ReaderRole::ALL.into_iter().filter(|role| *role != ReaderRole::Cloud) {
            drop(collection.reader(role).unwrap());
        }
        let (waiting, reached) = mpsc::channel();
        let (returned, result) = mpsc::channel();
        let current = collection.clone();
        let waiter = std::thread::spawn(move || {
            on_waiting(move || waiting.send(()).unwrap());
            let lease = current.reader(ReaderRole::Cloud).unwrap();
            returned
                .send((
                    lease.native_identity(),
                    lease.execute_raw("PRAGMA cache_size", &[]).unwrap().rows,
                ))
                .unwrap();
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let state = slot.reader.lock().unwrap();
        assert!(state.transferred);
        assert!(state.reader.is_none(), "no second Reader opened while transferred");
        assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
        drop(state);
        drop(pin);
        let (returned_identity, rows) = result.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(returned_identity, identity);
        assert_eq!(rows[0][0], SqlValue::Integer(-137));
        waiter.join().unwrap();
        engine.close().unwrap();
    }

    #[test]
    fn cloud_lifetime_pin_unwind_restores_exact_reader_once() {
        let (_dir, engine, collection) = open();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        lease.execute_raw("PRAGMA cache_size = -139", &[]).unwrap();
        let identity = lease.native_identity();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _pin = lease
                .into_lifetime_pin()
                .unwrap_or_else(|(_, error)| panic!("{error:?}"));
            panic!("enqueue unwound");
        }));
        assert!(result.is_err());
        let slot = collection
            .core
            .readers
            .slots
            .iter()
            .find(|s| s.role == ReaderRole::Cloud)
            .unwrap();
        let state = slot.reader.try_lock().unwrap();
        assert!(!state.transferred);
        assert_eq!(state.reader.as_ref().unwrap().native_identity(), identity);
        drop(state);
        for _ in 0..2 {
            let lease = collection.reader(ReaderRole::Cloud).unwrap();
            assert_eq!(lease.native_identity(), identity);
            assert_eq!(
                lease.execute_raw("PRAGMA cache_size", &[]).unwrap().rows[0][0],
                SqlValue::Integer(-139)
            );
        }
        engine.close().unwrap();
        let state = slot.reader.try_lock().unwrap();
        assert!(!state.transferred);
        assert!(state.reader.is_none());
        drop(state);
        assert!(
            collection.reader(ReaderRole::Cloud).is_err(),
            "closed factory stays closed"
        );
    }

    #[test]
    fn lifetime_pin_refusal_retains_the_actual_slot_guard() {
        let (_dir, engine, collection) = open();
        for role in [ReaderRole::Sync, ReaderRole::Cloud] {
            let mut lease = collection.reader(role).unwrap();
            let snapshot = if role == ReaderRole::Cloud {
                Some(lease.capture_sync(&collection).unwrap().0)
            } else {
                None
            };
            let slot = lease.guard.slot;
            let state_address = &*lease.guard.state as *const SlotState;
            let (lease, error) = lease.into_lifetime_pin().err().expect("unsupported transfer");
            assert!(matches!(error, EngineError::InvalidArgument { .. }));
            assert_eq!(&*lease.guard.state as *const SlotState, state_address);
            assert!(std::ptr::eq(lease.guard.slot, slot));
            assert!(!lease.guard.state.transferred);
            assert!(matches!(
                slot.reader.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            assert_eq!(lease.guard.is_none(), snapshot.is_some());
            if let Some(snapshot) = snapshot {
                assert!(snapshot.col().is_ok());
            }
            drop(lease);
            assert!(slot.reader.try_lock().is_ok());
        }
        engine.close().unwrap();
    }

    #[test]
    fn cloud_lifetime_pin_close_all_waits_without_holding_factory_or_slot_mutex() {
        let (_dir, engine, collection) = open();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let pin = lease
            .into_lifetime_pin()
            .unwrap_or_else(|(_, error)| panic!("{error:?}"));
        let (waiting, reached) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let current = collection.clone();
        let closer = std::thread::spawn(move || {
            on_waiting(move || waiting.send(()).unwrap());
            current.core.readers.close_all();
            done.send(()).unwrap();
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(collection.core.readers.factory.try_lock().unwrap().is_none());
        let slot = collection
            .core
            .readers
            .slots
            .iter()
            .find(|s| s.role == ReaderRole::Cloud)
            .unwrap();
        let state = slot.reader.lock().unwrap();
        assert!(state.transferred);
        assert!(state.reader.is_none());
        assert!(matches!(finished.try_recv(), Err(mpsc::TryRecvError::Empty)));
        drop(state);
        drop(pin);
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        closer.join().unwrap();
        let state = slot.reader.try_lock().unwrap();
        assert!(!state.transferred);
        assert!(state.reader.is_none());
        drop(state);
        assert!(collection.reader(ReaderRole::Cloud).is_err());
        engine.close().unwrap();
    }
}
