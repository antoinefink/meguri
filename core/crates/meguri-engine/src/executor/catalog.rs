//! The op catalog (`engine/op-model` O9): the named engine ops with their
//! undo class (`engine/undo` M11, R2020), whether each may declare a long
//! write phase, and the maintenance tasks that run on the writer outside a
//! transaction. Add an entry here and in the spec together. Any other op is
//! an [`OpDef::new`]: unregistered, and it may not declare a long write.

/// What undo does with an op (`engine/undo` M11, R2020).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UndoClass {
    /// Recorded as an undo step.
    Undoable,
    /// Not undoable and destroys data: the UI confirms it first and U127's
    /// pre-op backup runs before its write.
    DestructiveNonUndoable,
    /// Not undoable, and destroys nothing.
    NonDestructive,
}

impl UndoClass {
    /// Whether the op must be confirmed before it runs.
    pub fn requires_confirmation(self) -> bool {
        self == UndoClass::DestructiveNonUndoable
    }

    /// Whether U127's pre-op backup must run before its write.
    pub fn requires_backup(self) -> bool {
        self == UndoClass::DestructiveNonUndoable
    }
}

/// What an op is, as the executor sees it: its name, its undo class when it
/// is a catalog op, and what it may declare. Only this module can make a
/// definition that may declare a long write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpDef {
    name: &'static str,
    undo: Option<UndoClass>,
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
            undo: None,
            long_write: false,
            cancellable_write: false,
            maintenance_task: false,
        }
    }

    const fn long_write(name: &'static str, undo: UndoClass, cancellable_write: bool) -> OpDef {
        OpDef {
            name,
            undo: Some(undo),
            long_write: true,
            cancellable_write,
            maintenance_task: false,
        }
    }

    /// A catalog op whose write phase stays short.
    const fn short(name: &'static str, undo: UndoClass) -> OpDef {
        OpDef {
            name,
            undo: Some(undo),
            long_write: false,
            cancellable_write: false,
            maintenance_task: false,
        }
    }

    const fn maintenance(name: &'static str) -> OpDef {
        OpDef {
            name,
            undo: Some(UndoClass::NonDestructive),
            long_write: true,
            cancellable_write: false,
            maintenance_task: true,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The undo class of a catalog op; `None` for a plain op.
    pub fn undo_class(&self) -> Option<UndoClass> {
        self.undo
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

/// Check Database's vacuum and repairs (U128): it may delete broken rows.
pub const CHECK_DATABASE: OpDef = OpDef::long_write("check_database", UndoClass::DestructiveNonUndoable, false);
/// A large `.apkg` apply (U120): one undo step named "Import".
pub const APKG_APPLY: OpDef = OpDef::long_write("apkg_apply", UndoClass::Undoable, false);
/// A large `SyncIncoming` merge (U107); cancellable between batches.
pub const SYNC_INCOMING_MERGE: OpDef = OpDef::long_write("sync_incoming_merge", UndoClass::NonDestructive, true);
/// A note type structure change on a large note type (U34): undoable.
pub const NOTETYPE_STRUCTURE_CHANGE: OpDef = OpDef::long_write("notetype_structure_change", UndoClass::Undoable, false);
/// A bulk op over a large selection (U86): one undo step named after it.
pub const BULK_OP: OpDef = OpDef::long_write("bulk_op", UndoClass::Undoable, false);
/// Undo of a step (`engine/undo` M4).
pub const UNDO: OpDef = OpDef::short("undo", UndoClass::Undoable);
/// Redo of a step (`engine/undo` M5).
pub const REDO: OpDef = OpDef::short("redo", UndoClass::Undoable);
/// Undo of a step whose op declared a long write: it declares the same.
pub const UNDO_LONG_WRITE: OpDef = OpDef::long_write("undo_long_write", UndoClass::Undoable, false);
/// Redo of a step whose op declared a long write.
pub const REDO_LONG_WRITE: OpDef = OpDef::long_write("redo_long_write", UndoClass::Undoable, false);

/// `VACUUM` (U128, U132).
pub const VACUUM: OpDef = OpDef::maintenance("vacuum");
/// `REINDEX` (U128).
pub const REINDEX: OpDef = OpDef::maintenance("reindex");
/// `ANALYZE` (U132).
pub const ANALYZE: OpDef = OpDef::maintenance("analyze");
/// `wal_checkpoint` (U109, U132).
pub const WAL_CHECKPOINT: OpDef = OpDef::maintenance("wal_checkpoint");

/// Every catalog entry, in the spec's order.
pub const CATALOG: [OpDef; 13] = [
    CHECK_DATABASE,
    APKG_APPLY,
    SYNC_INCOMING_MERGE,
    NOTETYPE_STRUCTURE_CHANGE,
    BULK_OP,
    UNDO,
    REDO,
    UNDO_LONG_WRITE,
    REDO_LONG_WRITE,
    VACUUM,
    REINDEX,
    ANALYZE,
    WAL_CHECKPOINT,
];

/// The catalog entry named `name`.
pub fn lookup(name: &str) -> Option<OpDef> {
    CATALOG.iter().copied().find(|def| def.name == name)
}
