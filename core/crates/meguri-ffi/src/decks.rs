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
pub fn rename_deck(id: i64, name: String) -> Result<DeckMutation, CoreError> {
    callback::enter("rename_deck");
    Ok(decks::rename(&runtime::host()?.collection()?, DeckId(id), name)?.into())
}
#[uniffi::export]
pub fn move_decks(ids: Vec<i64>, parent: Option<i64>) -> Result<CountMutation, CoreError> {
    callback::enter("move_decks");
    Ok(decks::move_decks(
        &runtime::host()?.collection()?,
        ids.into_iter().map(DeckId).collect(),
        parent.map(DeckId),
    )?
    .into())
}
#[uniffi::export]
pub fn collapse_decks(ids: Vec<i64>, scope: DeckCollapseScope, collapsed: bool) -> Result<MutationSummary, CoreError> {
    callback::enter("collapse_decks");
    let scope = match scope {
        DeckCollapseScope::Study => decks::CollapseScope::Study,
        DeckCollapseScope::Browser => decks::CollapseScope::Browser,
    };
    Ok(MutationSummary::of(&decks::collapse(
        &runtime::host()?.collection()?,
        ids.into_iter().map(DeckId).collect(),
        scope,
        collapsed,
    )?))
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
/// The engine's deletion orchestrator owns backup/preconditions and no-op
/// handling. Its current API accepts no token after invocation; cancellation
/// is checked before entry, with the remaining contract reported in handoff.
#[uniffi::export]
pub fn start_delete_decks(ids: Vec<i64>) -> Result<DeckDeletionJobStart, CoreError> {
    callback::enter("start_delete_decks");
    let host = runtime::host()?;
    let c = host.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let r = meguri_engine::decks::delete(&c, ids.into_iter().map(DeckId).collect(), host.backup())?;
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
