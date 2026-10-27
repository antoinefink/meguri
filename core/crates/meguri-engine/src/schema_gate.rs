//! Schema classification, stamping and the storage guard (G2–G6).
//! Plans declare their effect before safety admission; stamping stays in the op.

use std::collections::BTreeMap;

use meguri_model::col::Col;
use meguri_model::ids::NotetypeId;
use meguri_model::time::EpochMillis;
use meguri_storage::journal::Journal;
use meguri_storage::rows::col::COL_KEY;
use meguri_storage::{ReadRows, Record, RowKey, StorageError, Table, WriteScope};

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
    /// Classification only; incoming CloudKit execution belongs to U114.
    CloudkitStructureApplied,
}

impl SchemaReason {
    /// Every reason, in G2's order.
    pub const ALL: [SchemaReason; 14] = [
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
        SchemaReason::CloudkitStructureApplied,
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
            SchemaReason::CloudkitStructureApplied => "cloudkit_structure_applied",
        }
    }

    /// G2's structural-edit classification, independent of `scm` and `ls`.
    pub const fn is_structural(self) -> bool {
        matches!(
            self,
            Self::FieldsChanged
                | Self::SortFieldChanged
                | Self::TemplatesChanged
                | Self::NotetypeRemoved
                | Self::NotetypeRestored
                | Self::NotesNotetypeChanged
                | Self::PresetRemoved
                | Self::NotetypesMerged
        )
    }
}

/// G1: the schema is modified since the last sync when `scm > ls`.
pub fn is_modified(col: &Col) -> bool {
    col.schema_modified.0 > col.last_sync.0
}

/// G5: stamp inside the transaction, after safety admission and before writing
/// structure. The stamp participates in the normal change set and undo journal.
pub fn mark_schema_change(scope: &mut WriteScope<'_>, reasons: &[SchemaReason]) -> Result<(), EngineError> {
    if reasons.is_empty() {
        return Ok(());
    }
    let mut col = scope.col()?;
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
    let (Some(before), Some(after)) = (journal.before(entry)?, journal.after(entry)?) else {
        return Ok(true);
    };
    let before = Col::from_cells(&before)?;
    let after = Col::from_cells(&after)?;
    Ok(before.schema_modified != after.schema_modified)
}
