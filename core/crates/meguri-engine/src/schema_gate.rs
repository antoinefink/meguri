//! Schema changes and the confirmation gate (`specs/notetypes/schema-changes.md`):
//! the classification table every op that can force a one-way sync declares
//! its reasons from (G2), the gate that asks first after a sync (G4), the
//! stamp that marks the schema modified (G5), and the storage guard under
//! them (G6).
//!
//! An op calls [`mark_schema_change`] inside its write scope once it knows
//! its effect and before it writes anything: the gate refuses with
//! [`EngineError::NeedsSchemaConfirmation`] or sets `col.scm` to now. The
//! guard runs at every op's commit on the writer (`executor/writer.rs`).

use std::collections::BTreeMap;

use meguri_model::col::Col;
use meguri_model::ids::NotetypeId;
use meguri_model::time::EpochMillis;
use meguri_storage::journal::{After, Journal};
use meguri_storage::rows::col::COL_KEY;
use meguri_storage::{ReadRows, ReaderRole, Record, RowKey, StorageError, Table, WriteScope};

use crate::collection::Collection;
use crate::error::EngineError;

/// Why an op changes the schema (G2). The table is the whole list: no other
/// op ever sets `scm` (R1590).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SchemaReason {
    /// A note type save whose fields are added, removed or reordered (U34).
    FieldsChanged,
    /// A note type save that makes another field the sort field (U34, R926).
    SortFieldChanged,
    /// A note type save whose templates are added, removed or reordered
    /// (U34).
    TemplatesChanged,
    /// Deleting a note type (U34, R918).
    NotetypeRemoved,
    /// Restore to Default when it changes the field or template structure
    /// (U34, R914).
    NotetypeRestored,
    /// Change Note Type (U35, R948).
    NotesNotetypeChanged,
    /// Deleting a deck preset (U28, U232).
    PresetRemoved,
    /// An `.apkg` import that merges incoming note types into local ones
    /// (U121).
    NotetypesMerged,
    /// A `.colpkg` import or a backup restore (U124, U127), set on the
    /// incoming collection: the replace confirmation is the caller's.
    CollectionReplaced,
    /// The Check Database repairs U128 and U129 list as schema changes.
    CheckDatabaseRepair,
    /// A normal sync whose sanity check failed (U107, R1533).
    SyncSanityFailed,
    /// A one-way upload (U109, R1541), which then sets `ls` to `scm`.
    FullUpload,
    /// The "Force a one-way sync on next sync" setting (U113/U160, R1547):
    /// the setting is the user's request.
    ForceOneWay,
}

impl SchemaReason {
    /// Every reason, in G2's order.
    pub const ALL: [SchemaReason; 13] = [
        SchemaReason::FieldsChanged,
        SchemaReason::SortFieldChanged,
        SchemaReason::TemplatesChanged,
        SchemaReason::NotetypeRemoved,
        SchemaReason::NotetypeRestored,
        SchemaReason::NotesNotetypeChanged,
        SchemaReason::PresetRemoved,
        SchemaReason::NotetypesMerged,
        SchemaReason::CollectionReplaced,
        SchemaReason::CheckDatabaseRepair,
        SchemaReason::SyncSanityFailed,
        SchemaReason::FullUpload,
        SchemaReason::ForceOneWay,
    ];

    /// G2's name of the reason.
    pub const fn name(self) -> &'static str {
        match self {
            SchemaReason::FieldsChanged => "fields_changed",
            SchemaReason::SortFieldChanged => "sort_field_changed",
            SchemaReason::TemplatesChanged => "templates_changed",
            SchemaReason::NotetypeRemoved => "notetype_removed",
            SchemaReason::NotetypeRestored => "notetype_restored",
            SchemaReason::NotesNotetypeChanged => "notes_notetype_changed",
            SchemaReason::PresetRemoved => "preset_removed",
            SchemaReason::NotetypesMerged => "notetypes_merged",
            SchemaReason::CollectionReplaced => "collection_replaced",
            SchemaReason::CheckDatabaseRepair => "check_database_repair",
            SchemaReason::SyncSanityFailed => "sync_sanity_failed",
            SchemaReason::FullUpload => "full_upload",
            SchemaReason::ForceOneWay => "force_one_way",
        }
    }

    /// G2's last column: whether an op for this reason asks the user first
    /// when the schema is not modified since the last sync.
    pub const fn asks_first(self) -> bool {
        match self {
            SchemaReason::FieldsChanged
            | SchemaReason::SortFieldChanged
            | SchemaReason::TemplatesChanged
            | SchemaReason::NotetypeRemoved
            | SchemaReason::NotetypeRestored
            | SchemaReason::NotesNotetypeChanged
            | SchemaReason::PresetRemoved
            | SchemaReason::NotetypesMerged => true,
            SchemaReason::CollectionReplaced
            | SchemaReason::CheckDatabaseRepair
            | SchemaReason::SyncSanityFailed
            | SchemaReason::FullUpload
            | SchemaReason::ForceOneWay => false,
        }
    }
}

/// G1: the schema is modified since the last sync when `scm > ls`.
pub fn is_modified(col: &Col) -> bool {
    col.schema_modified.0 > col.last_sync.0
}

/// G4, for the UI: whether a schema change that asks first would ask now,
/// read from the live `col` row. The op's own check is the one that counts:
/// a sync can finish in between.
pub fn would_ask(collection: &Collection) -> Result<bool, EngineError> {
    let reader = collection.reader(ReaderRole::Query)?;
    Ok(!is_modified(&reader.col()?))
}

/// G4 and G5 inside an op's write scope, after the op has computed its effect
/// and before it writes anything. With no reason it does nothing. When a
/// reason asks first, `confirmed` is false and the live `col` row has
/// `scm <= ls`, it refuses with [`EngineError::NeedsSchemaConfirmation`]
/// (the op then writes nothing). Otherwise it sets `scm` to now in the
/// scope, confirmed or not and also when `scm > ls` already: the write moves
/// `col.mod` with the op, is in its change set and in its undo step.
pub fn mark_schema_change(
    scope: &mut WriteScope<'_>,
    reasons: &[SchemaReason],
    confirmed: bool,
) -> Result<(), EngineError> {
    if reasons.is_empty() {
        return Ok(());
    }
    let mut col = scope.col()?;
    if !confirmed && !is_modified(&col) && reasons.iter().any(|r| r.asks_first()) {
        let mut asked: Vec<SchemaReason> = reasons.to_vec();
        asked.sort();
        asked.dedup();
        return Err(EngineError::NeedsSchemaConfirmation { reasons: asked });
    }
    col.schema_modified = EpochMillis(scope.now().0);
    scope.put_col(&col)?;
    Ok(())
}

/// G6 at the commit of a write transaction on the open collection: every
/// note type that exists before and after it and whose number of `fields`
/// or `templates` rows changed requires `scm > ls` in the state it leaves.
/// The one exception is the undo (`undoing`) of a step that changed `scm`:
/// it restores the step's `scm` together with the counts. Note types added
/// or removed in the transaction do not count.
pub(crate) fn guard(scope: &WriteScope<'_>, undoing: bool) -> Result<(), EngineError> {
    let Some(journal) = scope.journal() else {
        return Ok(());
    };
    let Some(changed) = count_changed(journal) else {
        return Ok(());
    };
    if is_modified(&scope.col()?) || (undoing && scm_changed(journal)?) {
        return Ok(());
    }
    Err(EngineError::SchemaNotMarked { notetype_id: changed })
}

/// The first note type, by id, that exists before and after the journal's
/// transaction and whose field or template rows changed in number.
fn count_changed(journal: &Journal) -> Option<NotetypeId> {
    // Net rows added per note type and table.
    let mut net: BTreeMap<(i64, Table), i64> = BTreeMap::new();
    let mut heads: BTreeMap<i64, bool> = BTreeMap::new();
    for entry in journal.entries() {
        match (entry.table(), entry.key()) {
            (table @ (Table::Fields | Table::Templates), RowKey::Pair(ntid, _)) => {
                let delta = i64::from(entry.exists_after()) - i64::from(entry.existed_before());
                *net.entry((*ntid, table)).or_default() += delta;
            }
            (Table::Notetypes, RowKey::Id(id)) => {
                heads.insert(*id, entry.existed_before() && entry.exists_after());
            }
            _ => {}
        }
    }
    net.into_iter()
        .filter(|(_, delta)| *delta != 0)
        .map(|((ntid, _), _)| ntid)
        // A note type with no journal entry of its own row existed before and
        // after: only its fields or templates were written.
        .find(|ntid| heads.get(ntid).copied().unwrap_or(true))
        .map(NotetypeId)
}

/// Whether the transaction changed `col.scm`.
fn scm_changed(journal: &Journal) -> Result<bool, StorageError> {
    let Some(entry) = journal.entry(Table::Col, &COL_KEY) else {
        return Ok(false);
    };
    let (Some(before), After::Row(after)) = (journal.before(entry)?, entry.after()) else {
        return Ok(true);
    };
    let before = Col::from_cells(&before)?;
    let after = Col::from_cells(after)?;
    Ok(before.schema_modified != after.schema_modified)
}
