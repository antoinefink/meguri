//! Readers by role (KTD3, `engine/lifecycle` L11): one connection per role
//! (and per search worker and server reader), opened when first asked for and
//! kept until the collection closes or is replaced. One user holds a reader
//! at a time. Long-lived snapshots use the sync or the maintenance reader,
//! never the interactive, query or server ones.

use std::ops::Deref;
use std::sync::{Mutex, MutexGuard, PoisonError};

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
    reader: Mutex<Option<Reader>>,
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
                    reader: Mutex::new(None),
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
    ) -> Result<MutexGuard<'_, Option<Reader>>, EngineError> {
        let slot = self
            .slots
            .iter()
            .find(|s| s.role == role && s.index == index)
            .ok_or_else(|| EngineError::InvalidArgument {
                message: format!("there is no {role:?} reader {index}"),
            })?;
        let mut reader = slot.reader.lock().unwrap_or_else(PoisonError::into_inner);
        check()?;
        if reader.is_none() {
            let factory = self
                .factory
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or_else(|| EngineError::unavailable("the collection's readers are closed"))?;
            *reader = Some(factory.open(role)?);
        }
        Ok(reader)
    }

    /// Closes every reader, waiting for those in use; no reader opens again
    /// until a factory is set.
    pub(crate) fn close_all(&self) {
        *self.factory.lock().unwrap_or_else(PoisonError::into_inner) = None;
        for slot in &self.slots {
            slot.reader.lock().unwrap_or_else(PoisonError::into_inner).take();
        }
    }

    /// Readers open on the collection `factory` names from now on.
    pub(crate) fn set_factory(&self, factory: ReaderFactory) {
        *self.factory.lock().unwrap_or_else(PoisonError::into_inner) = Some(factory);
    }
}

/// A reader held by one user until dropped.
pub struct ReaderLease<'a> {
    guard: MutexGuard<'a, Option<Reader>>,
    exec: &'a ExecShared,
    generation: Generation,
}

impl<'a> ReaderLease<'a> {
    pub(crate) fn new(
        guard: MutexGuard<'a, Option<Reader>>,
        exec: &'a ExecShared,
        generation: Generation,
    ) -> ReaderLease<'a> {
        ReaderLease {
            guard,
            exec,
            generation,
        }
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
        })
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

/// A read snapshot with the seq of the last commit it sees.
pub struct Snapshot<'a> {
    inner: ReadSnapshot<'a>,
    seq: Seq,
    generation: Generation,
}

impl Snapshot<'_> {
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
