//! U28 adapters. The host's real backup port is mandatory for card deletion.
use crate::{
    CoreError, callback,
    mutations::{CountMutation, MutationSummary},
    runtime,
};
use meguri_engine::decks;
use meguri_model::ids::DeckId;
use meguri_storage::{ReadRows, ReaderRole};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckNameEntry {
    pub id: i64,
    pub full_name: String,
    pub filtered: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckNameSnapshot {
    pub generation: u64,
    pub decks: Vec<DeckNameEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckNameMatch {
    pub generation: u64,
    pub deck: Option<DeckNameEntry>,
}

/// D18: canonical lookup over actual persisted rows, including hidden decks.
#[uniffi::export]
pub fn match_deck_name(name: String) -> Result<DeckNameMatch, CoreError> {
    callback::enter("match_deck_name");
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let rows = snapshot.deck_rows().map_err(meguri_engine::EngineError::from)?;
    let deck = decks::names::find(&rows, &decks::names::native_name(&name)).map(|deck| DeckNameEntry {
        id: deck.id.0,
        full_name: decks::names::human_name(&deck.name),
        filtered: decks::is_filtered(deck),
    });
    Ok(DeckNameMatch {
        generation: snapshot.generation().0,
        deck,
    })
}

fn name_entries(mut rows: Vec<meguri_model::deck::Deck>) -> Vec<DeckNameEntry> {
    rows.sort_by(|a, b| meguri_text::fold::compare_folded(&a.name, &b.name).then_with(|| a.id.cmp(&b.id)));
    rows.into_iter()
        .map(|deck| DeckNameEntry {
            id: deck.id.0,
            full_name: decks::names::human_name(&deck.name),
            filtered: decks::is_filtered(&deck),
        })
        .collect()
}

/// D17: persisted records, independent of display-tree visibility and counts.
#[uniffi::export]
pub fn all_deck_names() -> Result<DeckNameSnapshot, CoreError> {
    callback::enter("all_deck_names");
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    Ok(DeckNameSnapshot {
        generation: snapshot.generation().0,
        decks: name_entries(snapshot.deck_rows().map_err(meguri_engine::EngineError::from)?),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckMutation {
    pub summary: MutationSummary,
    pub id: i64,
    pub name: String,
}
impl From<meguri_engine::OpResult<meguri_model::deck::Deck>> for DeckMutation {
    fn from(r: meguri_engine::OpResult<meguri_model::deck::Deck>) -> Self {
        Self {
            summary: MutationSummary::of(&r),
            id: r.value.id.0,
            name: decks::names::human_name(&r.value.name),
        }
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum DeckCollapseScope {
    Study,
    Browser,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckDeletionSummary {
    pub decks: u64,
    pub cards: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckDeletion {
    pub summary: MutationSummary,
    pub deleted: DeckDeletionSummary,
}
#[uniffi::export]
pub fn add_deck(name: String) -> Result<DeckMutation, CoreError> {
    callback::enter("add_deck");
    Ok(decks::add(&runtime::host()?.collection()?, name)?.into())
}
#[uniffi::export]
pub fn select_deck(id: i64) -> Result<MutationSummary, CoreError> {
    callback::enter("select_deck");
    Ok(MutationSummary::of(&decks::select(
        &runtime::host()?.collection()?,
        DeckId(id),
    )?))
}
#[uniffi::export]
pub fn deck_deletion_summary(ids: Vec<i64>) -> Result<DeckDeletionSummary, CoreError> {
    callback::enter("deck_deletion_summary");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let s = decks::deletion_summary(&*r, &ids.into_iter().map(DeckId).collect::<Vec<_>>())?;
    Ok(DeckDeletionSummary {
        decks: s.decks as u64,
        cards: s.cards as u64,
    })
}

use std::sync::{Arc, Mutex};
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckDeletionJobStart {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<DeckDeletionJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct DeckDeletionJob {
    result: Arc<Mutex<Option<DeckDeletion>>>,
}
#[uniffi::export]
impl DeckDeletionJob {
    pub fn result(&self) -> Result<Option<DeckDeletion>, CoreError> {
        callback::enter("DeckDeletionJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
/// `decks/decks` D8. `confirmed` is the last summary read before the delete
/// (the question's counts, or the zero-card read's); a live count above it
/// refuses with `DeckRefusal::LossesChanged` and writes nothing (D8 step 6).
#[uniffi::export]
pub fn start_delete_decks(ids: Vec<i64>, confirmed: DeckDeletionSummary) -> Result<DeckDeletionJobStart, CoreError> {
    callback::enter("start_delete_decks");
    let host = runtime::host()?;
    let c = host.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let r = meguri_engine::decks::delete_controlled(
            &c,
            ids.into_iter().map(DeckId).collect(),
            decks::ConfirmedLosses::Counts(decks::DeletionSummary {
                decks: usize::try_from(confirmed.decks).unwrap_or(usize::MAX),
                cards: usize::try_from(confirmed.cards).unwrap_or(usize::MAX),
            }),
            host.backup(),
            j.cancel.clone(),
            j.progress.clone(),
        )?;
        *out.lock().unwrap() = Some(DeckDeletion {
            summary: MutationSummary::of(&r),
            deleted: DeckDeletionSummary {
                decks: r.value.decks as u64,
                cards: r.value.cards as u64,
            },
        });
        Ok(())
    })?;
    Ok(DeckDeletionJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(DeckDeletionJob { result }),
    })
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckRenameJobStart {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<DeckRenameJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct DeckRenameJob {
    result: Arc<Mutex<Option<DeckMutation>>>,
}
#[uniffi::export]
impl DeckRenameJob {
    pub fn result(&self) -> Result<Option<DeckMutation>, CoreError> {
        callback::enter("DeckRenameJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[uniffi::export]
pub fn start_rename_deck(id: i64, name: String) -> Result<DeckRenameJobStart, CoreError> {
    callback::enter("start_rename_deck");
    let c = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        let value = decks::rename_controlled(&c, DeckId(id), name, j.cancel.clone(), j.progress.clone())?.into();
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(DeckRenameJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(DeckRenameJob { result }),
    })
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckMoveJobStart {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<DeckMoveJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct DeckMoveJob {
    result: Arc<Mutex<Option<CountMutation>>>,
}
#[uniffi::export]
impl DeckMoveJob {
    pub fn result(&self) -> Result<Option<CountMutation>, CoreError> {
        callback::enter("DeckMoveJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[uniffi::export]
pub fn start_move_decks(ids: Vec<i64>, parent: Option<i64>) -> Result<DeckMoveJobStart, CoreError> {
    callback::enter("start_move_decks");
    let c = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        let value = decks::move_decks_controlled(
            &c,
            ids.into_iter().map(DeckId).collect(),
            parent.map(DeckId),
            j.cancel.clone(),
            j.progress.clone(),
        )?
        .into();
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(DeckMoveJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(DeckMoveJob { result }),
    })
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckCollapseJobStart {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<DeckCollapseJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct DeckCollapseJob {
    result: Arc<Mutex<Option<MutationSummary>>>,
}
#[uniffi::export]
impl DeckCollapseJob {
    pub fn result(&self) -> Result<Option<MutationSummary>, CoreError> {
        callback::enter("DeckCollapseJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[uniffi::export]
pub fn start_collapse_decks(
    ids: Vec<i64>,
    scope: DeckCollapseScope,
    collapsed: bool,
) -> Result<DeckCollapseJobStart, CoreError> {
    callback::enter("start_collapse_decks");
    let c = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        let value = MutationSummary::of(&decks::collapse_controlled(
            &c,
            ids.into_iter().map(DeckId).collect(),
            match scope {
                DeckCollapseScope::Study => decks::CollapseScope::Study,
                DeckCollapseScope::Browser => decks::CollapseScope::Browser,
            },
            collapsed,
            j.cancel.clone(),
            j.progress.clone(),
        )?);
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(DeckCollapseJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(DeckCollapseJob { result }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_use_stored_unicode_order_and_id_ties_before_human_conversion() {
        let rows = [(9, "Straße"), (3, "STRASSE"), (7, "a!"), (6, "a\u{1f}子"), (5, "a")]
            .into_iter()
            .map(|(id, name)| {
                let mut deck = decks::new_deck(name.into());
                deck.id = DeckId(id);
                deck
            })
            .collect();
        assert_eq!(
            name_entries(rows)
                .into_iter()
                .map(|entry| (entry.id, entry.full_name))
                .collect::<Vec<_>>(),
            [(5, "a"), (6, "a::子"), (7, "a!"), (3, "STRASSE"), (9, "Straße")]
                .into_iter()
                .map(|(id, name)| (id, name.to_owned()))
                .collect::<Vec<_>>()
        );
    }
}
