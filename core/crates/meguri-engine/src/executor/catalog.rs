//! The op catalog (`engine/op-model` O9): the one list of ops that may
//! declare a long write phase, and of the maintenance tasks that run on the
//! writer outside a transaction. Add an entry here and in the spec together.
//! Any other op is an [`OpDef::new`] and may not declare a long write.

/// What an op is, as the executor sees it: its name and what it may declare.
/// Only this module can make a definition that may declare a long write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpDef {
    name: &'static str,
    long_write: bool,
    cancellable_write: bool,
    maintenance_task: bool,
}

impl OpDef {
    /// A plain op: its write phase stays short (O6) and completes once
    /// started (O5).
    pub const fn new(name: &'static str) -> OpDef {
        OpDef {
            name,
            long_write: false,
            cancellable_write: false,
            maintenance_task: false,
        }
    }

    const fn long_write(name: &'static str, cancellable_write: bool) -> OpDef {
        OpDef {
            name,
            long_write: true,
            cancellable_write,
            maintenance_task: false,
        }
    }

    const fn maintenance(name: &'static str) -> OpDef {
        OpDef {
            name,
            long_write: true,
            cancellable_write: false,
            maintenance_task: true,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Whether the op may declare a write phase longer than about 200 ms
    /// (O8).
    pub fn may_declare_long_write(&self) -> bool {
        self.long_write && !self.maintenance_task
    }

    /// Whether the op's write phase checks its cancel token between batches
    /// and rolls back on cancel (O5).
    pub fn cancellable_write(&self) -> bool {
        self.cancellable_write
    }

    /// Whether this is a maintenance task: writer work outside any
    /// transaction (O8).
    pub fn is_maintenance_task(&self) -> bool {
        self.maintenance_task
    }
}

/// Check Database's vacuum and repairs (U128).
pub const CHECK_DATABASE: OpDef = OpDef::long_write("check_database", false);
/// A large `.apkg` apply (U120).
pub const APKG_APPLY: OpDef = OpDef::long_write("apkg_apply", false);
/// A large `SyncIncoming` merge (U107); cancellable between batches.
pub const SYNC_INCOMING_MERGE: OpDef = OpDef::long_write("sync_incoming_merge", true);
/// A note type structure change on a large note type (U34).
pub const NOTETYPE_STRUCTURE_CHANGE: OpDef = OpDef::long_write("notetype_structure_change", false);
/// A bulk op over a large selection (U86).
pub const BULK_OP: OpDef = OpDef::long_write("bulk_op", false);

/// `VACUUM` (U128, U132).
pub const VACUUM: OpDef = OpDef::maintenance("vacuum");
/// `REINDEX` (U128).
pub const REINDEX: OpDef = OpDef::maintenance("reindex");
/// `ANALYZE` (U132).
pub const ANALYZE: OpDef = OpDef::maintenance("analyze");
/// `wal_checkpoint` (U109, U132).
pub const WAL_CHECKPOINT: OpDef = OpDef::maintenance("wal_checkpoint");

/// Every catalog entry, in the spec's order.
pub const CATALOG: [OpDef; 9] = [
    CHECK_DATABASE,
    APKG_APPLY,
    SYNC_INCOMING_MERGE,
    NOTETYPE_STRUCTURE_CHANGE,
    BULK_OP,
    VACUUM,
    REINDEX,
    ANALYZE,
    WAL_CHECKPOINT,
];

/// The catalog entry named `name`.
pub fn lookup(name: &str) -> Option<OpDef> {
    CATALOG.iter().copied().find(|def| def.name == name)
}
