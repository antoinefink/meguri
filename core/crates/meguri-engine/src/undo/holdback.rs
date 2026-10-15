//! The held-back run and the sync's side of undo (`specs/engine/undo.md` M8,
//! M9; KTD6, KTD7): what a normal sync keeps local, which steps it flags
//! upload-pending, and what its merge reports.

use std::collections::BTreeSet;

use meguri_storage::{RowImage, RowKey, Table};

use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::executor::preconditions::TouchKey;
use crate::undo::entry::{Entry, StepInfo};

/// The held-back run (M8) as a value: its steps and the rows they touched,
/// each with the image it had before the run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeldBackRun {
    /// Newest first.
    pub steps: Vec<StepInfo>,
    /// One per key, in no set order: the image from the oldest step of the
    /// run that touched it; `None` when the run created the row.
    pub rows: Vec<HeldRow>,
}

impl HeldBackRun {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Whether the run touched the row (`tags` keys compare with `unicase`).
    pub fn touches(&self, table: Table, key: &RowKey) -> bool {
        let probe = TouchKey::new(table, key.clone());
        self.rows
            .iter()
            .any(|row| TouchKey::new(row.table, row.key.clone()) == probe)
    }
}

/// A row of the held-back run and its image before the run.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldRow {
    pub table: Table,
    pub key: RowKey,
    pub before: Option<RowImage>,
}

/// What a normal sync's barrier gets (M9.1).
#[derive(Clone, Debug, PartialEq)]
pub struct SyncHold {
    /// The seq of the last op the snapshot sees.
    pub snapshot: Seq,
    pub run: HeldBackRun,
}

/// What a `SyncIncoming` merge reports (M9.4): its sync's snapshot and every
/// incoming row or grave it staged, applied or not. Give a grave as the key
/// of the card, note or deck it deletes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncMerge {
    pub snapshot: Seq,
    pub staged: Vec<(Table, RowKey)>,
}

/// The run's value from its steps, newest first (M8).
pub(crate) fn run_value(run: &[Entry]) -> Result<HeldBackRun, EngineError> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for entry in run.iter().rev() {
        for row in entry.journal.entries() {
            if row.table() == Table::Col {
                continue;
            }
            if seen.insert(TouchKey::new(row.table(), row.key().clone())) {
                rows.push(HeldRow {
                    table: row.table(),
                    key: row.key().clone(),
                    before: entry.before(row)?,
                });
            }
        }
    }
    Ok(HeldBackRun {
        steps: run.iter().map(|entry| entry.info.clone()).collect(),
        rows,
    })
}
