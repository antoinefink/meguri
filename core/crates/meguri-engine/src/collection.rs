//! The collection handle: how callers submit ops, read through role readers
//! and take snapshots. A handle belongs to one generation (`engine/lifecycle`
//! L3); through an older one every op, reader and snapshot fails with
//! `StaleGeneration`.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use meguri_storage::{ReaderRole, Writer};

use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::executor::ExecShared;
use crate::executor::catalog::OpDef;
use crate::executor::exclusive::Privilege;
use crate::executor::op::{OpContext, OpResult, OpSpec, Pending, WriteDelay};
use crate::lifecycle::Generation;
use crate::readers::{ReaderLease, Readers};

/// What every handle of one open collection shares.
pub(crate) struct CollectionCore {
    pub(crate) exec: Arc<ExecShared>,
    pub(crate) readers: Readers,
}

/// A handle to the open collection, for one generation. Cheap to clone.
#[derive(Clone)]
pub struct Collection {
    pub(crate) core: Arc<CollectionCore>,
    pub(crate) generation: Generation,
    pub(crate) privilege: Option<Privilege>,
}

impl fmt::Debug for Collection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Collection")
            .field("path", &self.core.exec.path)
            .field("generation", &self.generation)
            .field("privileged", &self.privilege.is_some())
            .finish()
    }
}

impl Collection {
    /// The generation this handle belongs to.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// The engine's current generation.
    pub fn current_generation(&self) -> Generation {
        self.core.exec.generation()
    }

    /// Whether this handle is of the current generation.
    pub fn is_current(&self) -> bool {
        self.generation == self.current_generation()
    }

    pub fn path(&self) -> &Path {
        &self.core.exec.path
    }

    /// The seq of the last committed op.
    pub fn last_seq(&self) -> Seq {
        self.core.exec.last_seq()
    }

    /// The declared long write or maintenance task the writer runs now
    /// (`engine/op-model` O8): U43 keeps answers in its overlay meanwhile.
    pub fn write_delay(&self) -> Option<WriteDelay> {
        self.core.exec.write_delay()
    }

    /// Submits an op and returns at once (`engine/op-model` O2).
    pub fn submit<T, F>(&self, spec: OpSpec, op: F) -> Pending<OpResult<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
    {
        self.core.exec.submit_op(spec, self.generation, self.privilege, op)
    }

    /// Submits an op and waits for its result.
    pub fn run<T, F>(&self, spec: OpSpec, op: F) -> Result<OpResult<T>, EngineError>
    where
        T: Send + 'static,
        F: FnOnce(&mut OpContext<'_, '_>) -> Result<T, EngineError> + Send + 'static,
    {
        self.submit(spec, op).wait()
    }

    /// Submits a maintenance task of the op catalog (`engine/op-model` O8):
    /// `work` gets the writer connection outside any transaction.
    pub fn submit_maintenance<T, F>(&self, def: OpDef, declared: Duration, work: F) -> Pending<T>
    where
        T: Send + 'static,
        F: FnOnce(&Writer) -> Result<T, EngineError> + Send + 'static,
    {
        self.core
            .exec
            .submit_maintenance(def, declared, self.generation, self.privilege, work)
    }

    /// The reader of `role` (the first of its kind for search and server
    /// readers), held until the lease drops.
    pub fn reader(&self, role: ReaderRole) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_at(role, 0)
    }

    /// The reader of search worker `worker`.
    pub fn search_reader(&self, worker: usize) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_at(ReaderRole::Search, worker)
    }

    /// Server reader `index` (macOS, while the local servers run).
    pub fn server_reader(&self, index: usize) -> Result<ReaderLease<'_>, EngineError> {
        self.reader_at(ReaderRole::Server, index)
    }

    fn reader_at(&self, role: ReaderRole, index: usize) -> Result<ReaderLease<'_>, EngineError> {
        self.check_current()?;
        let guard = self.core.readers.lease(role, index)?;
        Ok(ReaderLease::new(guard, &self.core.exec, self.generation))
    }

    fn check_current(&self) -> Result<(), EngineError> {
        let current = self.current_generation();
        if current == self.generation {
            Ok(())
        } else {
            Err(EngineError::StaleGeneration {
                planned: self.generation,
                current,
            })
        }
    }
}
