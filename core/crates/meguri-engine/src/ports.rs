//! The ports this crate declares (KTD2): traits whose implementations live
//! in crates above the engine and are wired only by meguri-host (U38). The
//! engine calls them; they never call back into the writer synchronously.

use std::fmt;
use std::sync::Arc;

use crate::changes::observers::ObservedCommit;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::lifecycle::Generation;

/// Sees every committed op with a non-empty change set, on the writer
/// thread, after the commit and before publication (`engine/change-stream`
/// S2). Consumers: U26, U41, U95, U100. Keep it short and never call Swift.
pub trait ChangeObserver: Send + Sync {
    fn name(&self) -> &str;
    /// An error is logged; the commit stands and later observers still run.
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError>;
}

/// Told after every generation change (`engine/lifecycle` L8): U25 clears
/// undo, U26 invalidates derived data.
pub trait GenerationListener: Send + Sync {
    fn name(&self) -> &str;
    fn collection_replaced(&self, old: Generation, new: Generation);
}

/// Work outside the executor that must reach a quiet point: before an
/// exclusive phase (`engine/lifecycle` L7) and in U32's suspend protocol.
pub trait SuspendParticipant: Send + Sync {
    fn name(&self) -> &str;
    /// Commits or hands over pending work now (U43 flushes its answer
    /// overlay into ops). Called while the collection still admits ops.
    fn flush(&self) -> Result<(), EngineError>;
    /// Bounded flush for suspension; long implementations override this.
    fn flush_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        if crate::monotonic::now() >= deadline {
            return Err(EngineError::SuspendDeadline {
                phase: self.name().into(),
            });
        }
        self.flush()
    }
    /// Quiesces before suspension (U32).
    fn suspend(&self) -> Result<(), EngineError> {
        Ok(())
    }
    /// Deadline-aware participants override this for bounded quiescence.
    fn suspend_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        if crate::monotonic::now() >= deadline {
            return Err(EngineError::SuspendDeadline {
                phase: self.name().into(),
            });
        }
        self.suspend()
    }
    /// Releases participant-owned connections after all workers have quiesced
    /// and the engine's readers (including attached databases) have closed.
    fn release_connections_until(&self, _deadline: std::time::Duration) -> Result<(), EngineError> {
        Ok(())
    }
    /// Resumes after suspension (U32).
    fn resume(&self) -> Result<(), EngineError> {
        Ok(())
    }
    /// Rebind using the current, privileged collection before public admission.
    /// Existing participants need only their connection-level resume.
    fn resume_with_collection(&self, _collection: &Collection) -> Result<(), EngineError> {
        self.resume()
    }
}

/// Runs while the collection opens, before it is `Ready` (`engine/lifecycle`
/// L2): U43's pending-answer replay. The engine's day clock check runs after
/// every registered step. The handle it gets is the only one whose ops are
/// admitted then. An error fails the open.
pub trait OpenStep: Send + Sync {
    fn name(&self) -> &str;
    fn run(&self, collection: &Collection) -> Result<(), EngineError>;
}

/// The engine's port catalog: U38's `ports.rs` checks each has its provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortKind {
    ChangeObserver,
    GenerationListener,
    SuspendParticipant,
    OpenStep,
}

impl PortKind {
    pub const ALL: [PortKind; 4] = [
        PortKind::ChangeObserver,
        PortKind::GenerationListener,
        PortKind::SuspendParticipant,
        PortKind::OpenStep,
    ];
}

/// The port implementations of one open collection, in registration order.
#[derive(Clone, Default)]
pub struct Ports {
    pub observers: Vec<Arc<dyn ChangeObserver>>,
    pub generation_listeners: Vec<Arc<dyn GenerationListener>>,
    pub suspend_participants: Vec<Arc<dyn SuspendParticipant>>,
    pub open_steps: Vec<Arc<dyn OpenStep>>,
}

impl fmt::Debug for Ports {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = |list: Vec<&str>| list.join(", ");
        f.debug_struct("Ports")
            .field("observers", &names(self.observers.iter().map(|o| o.name()).collect()))
            .field(
                "generation_listeners",
                &names(self.generation_listeners.iter().map(|o| o.name()).collect()),
            )
            .field(
                "suspend_participants",
                &names(self.suspend_participants.iter().map(|o| o.name()).collect()),
            )
            .field("open_steps", &names(self.open_steps.iter().map(|o| o.name()).collect()))
            .finish()
    }
}

impl Ports {
    pub fn new() -> Ports {
        Ports::default()
    }

    pub fn observer(mut self, observer: Arc<dyn ChangeObserver>) -> Ports {
        self.observers.push(observer);
        self
    }

    pub fn generation_listener(mut self, listener: Arc<dyn GenerationListener>) -> Ports {
        self.generation_listeners.push(listener);
        self
    }

    pub fn suspend_participant(mut self, participant: Arc<dyn SuspendParticipant>) -> Ports {
        self.suspend_participants.push(participant);
        self
    }

    pub fn open_step(mut self, step: Arc<dyn OpenStep>) -> Ports {
        self.open_steps.push(step);
        self
    }

    /// How many providers a port has.
    pub fn providers(&self, kind: PortKind) -> usize {
        match kind {
            PortKind::ChangeObserver => self.observers.len(),
            PortKind::GenerationListener => self.generation_listeners.len(),
            PortKind::SuspendParticipant => self.suspend_participants.len(),
            PortKind::OpenStep => self.open_steps.len(),
        }
    }
}

/// Swift supplies important-usage capacity; Linux uses statvfs (U32).
pub use meguri_storage::space::SpaceProvider as DiskSpaceProvider;

/// U127's pre-operation backup hook. The caller waits off the writer;
/// failure prevents the deletion. meguri-host supplies the package service.
pub trait BackupService: Send + Sync {
    fn before_deck_delete(&self) -> Result<(), EngineError>;
}

/// Semantic bindings owned by the composition root. Several bindings may
/// implement the same trait; counting `PortKind` alone cannot detect omissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServicePort {
    DerivedStore,
    Media,
    Backup,
    DeckCounts,
    Review,
    Sync,
    Statistics,
    Widgets,
}

impl ServicePort {
    pub const ALL: [Self; 8] = [
        Self::DerivedStore,
        Self::Media,
        Self::Backup,
        Self::DeckCounts,
        Self::Review,
        Self::Sync,
        Self::Statistics,
        Self::Widgets,
    ];
}

/// A future provider is explicit and refuses calls; it never acknowledges work.
#[derive(Clone, Copy, Debug)]
pub struct Unavailable {
    pub feature: &'static str,
    pub provider_unit: &'static str,
}

impl Unavailable {
    pub fn require(&self) -> Result<(), EngineError> {
        Err(EngineError::CollectionUnavailable {
            reason: format!("{} requires provider {}", self.feature, self.provider_unit),
        })
    }
}

impl BackupService for Unavailable {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        self.require()
    }
}
