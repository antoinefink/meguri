//! U28 adapters. The host's real backup port is mandatory for card deletion.
use crate::{
    CoreError, callback,
    mutations::{CountMutation, MutationSummary},
    runtime,
};
use meguri_engine::decks;
use meguri_model::ids::DeckId;
use meguri_storage::ReaderRole;
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
#[uniffi::export]
pub fn start_delete_decks(ids: Vec<i64>) -> Result<DeckDeletionJobStart, CoreError> {
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
