//! The held-back run and the sync's side of undo (`specs/engine/undo.md` M8,
//! M9; KTD6, KTD7): what a normal sync keeps local, which steps it flags
//! upload-pending, and what its merge reports.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use meguri_storage::journal::{Entry as JournalEntry, Journal};
use meguri_storage::{RowImage, RowKey, Table};

use crate::changes::stream::Seq;
use crate::error::EngineError;
use crate::undo::entry::{Entry, EntryId, StepInfo};

/// The held-back run (M8): its steps and the rows they touched. It keeps the
/// steps' journals and reads a row's image from before the run only when
/// asked ([`HeldBackRun::before`], [`HeldBackRun::rows`]), from memory or
/// from the journal's spill file, so a sync's barrier reads no image and a
/// large spilled step costs no more memory than its journal does (R1851).
#[derive(Clone, Default)]
pub struct HeldBackRun {
    /// Newest first.
    pub steps: Vec<StepInfo>,
    /// The steps' journals, oldest first.
    journals: Vec<Arc<Journal>>,
}

impl HeldBackRun {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Whether the run touched the row (`tags` keys compare with `unicase`).
    /// The `col` row is never in the run.
    pub fn touches(&self, table: Table, key: &RowKey) -> bool {
        self.first_change(table, key).is_some()
    }

    /// The row with its image from before the run, read now: the image the
    /// oldest step of the run that touched it found, `None` inside when the
    /// run created the row. `None` when the run did not touch the row.
    pub fn before(&self, table: Table, key: &RowKey) -> Result<Option<HeldRow>, EngineError> {
        self.first_change(table, key)
            .map(|(journal, change)| held_row(journal, change))
            .transpose()
    }

    /// Every row the run touched, once each, oldest step first; each image is
    /// read only when the iteration reaches its row.
    pub fn rows(&self) -> impl Iterator<Item = Result<HeldRow, EngineError>> + '_ {
        self.journals.iter().enumerate().flat_map(move |(at, journal)| {
            let older = &self.journals[..at];
            journal
                .entries()
                .filter(move |change| {
                    change.table() != Table::Col
                        && older
                            .iter()
                            .all(|earlier| earlier.entry(change.table(), change.key()).is_none())
                })
                .map(move |change| held_row(journal, change))
        })
    }

    /// Journal keys only, with no image access (including spilled journals).
    pub fn keys(&self) -> impl Iterator<Item = (Table, &RowKey)> + '_ {
        self.journals
            .iter()
            .flat_map(|journal| journal.entries())
            .filter(|change| change.table() != Table::Col)
            .map(|change| (change.table(), change.key()))
    }

    /// Seq immediately before the oldest step that changed this row.
    pub fn image_seq(&self, table: Table, key: &RowKey) -> Option<Seq> {
        if table == Table::Col {
            return None;
        }
        self.journals
            .iter()
            .zip(self.steps.iter().rev())
            .find(|(journal, _)| journal.entry(table, key).is_some())
            .map(|(_, step)| Seq(step.id.0.saturating_sub(1)))
    }

    /// The oldest step's change of the row, with that step's journal.
    fn first_change(&self, table: Table, key: &RowKey) -> Option<(&Journal, &JournalEntry)> {
        if table == Table::Col {
            return None;
        }
        self.journals
            .iter()
            .find_map(|journal| journal.entry(table, key).map(|change| (&**journal, change)))
    }
}

impl fmt::Debug for HeldBackRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeldBackRun")
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

fn held_row(journal: &Journal, change: &JournalEntry) -> Result<HeldRow, EngineError> {
    Ok(HeldRow {
        table: change.table(),
        key: change.key().clone(),
        before: journal.before(change)?,
    })
}

/// A row of the held-back run and its image before the run.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldRow {
    pub table: Table,
    pub key: RowKey,
    pub before: Option<RowImage>,
}

/// What a normal sync's barrier gets (M9.1).
#[derive(Clone, Debug)]
pub struct SyncHold {
    /// The seq of the last op the snapshot sees.
    pub snapshot: Seq,
    pub run: HeldBackRun,
}

/// A CloudKit barrier's snapshot, lazy held run, and newly flagged keys (M9.6).
/// The map is a copy; the history retains its own keys with each owning step.
#[derive(Clone, Debug)]
pub struct CloudHold {
    pub snapshot: Seq,
    pub run: HeldBackRun,
    pub flagged: BTreeMap<EntryId, BTreeSet<(Table, RowKey)>>,
}

/// What a `SyncIncoming` merge reports (M9.4): its sync's snapshot and every
/// incoming row or grave it staged, applied or not. Give a grave as the key
/// of the card, note or deck it deletes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncMerge {
    pub snapshot: Seq,
    pub staged: Vec<(Table, RowKey)>,
}

/// The run of these steps, newest first (M8): their journals, no image read.
pub(crate) fn run_value(run: &[Entry]) -> HeldBackRun {
    HeldBackRun {
        steps: run.iter().map(|entry| entry.info.clone()).collect(),
        journals: run.iter().rev().map(|entry| Arc::clone(&entry.journal)).collect(),
    }
}
