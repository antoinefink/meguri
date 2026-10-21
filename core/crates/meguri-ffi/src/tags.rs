//! U30 typed exports; scans and bulk selections use the engine job pool.
use crate::{
    CoreError, callback,
    jobs::JobHandle,
    mutations::{BoolMutation, CountMutation},
    runtime,
};
use meguri_engine::{flags, marked, tags};
use meguri_model::ids::{CardId, NoteId};
use meguri_storage::ReaderRole;
use std::sync::{Arc, Mutex};
#[derive(Clone, Debug, uniffi::Record)]
pub struct TagMutationJobStart {
    pub handle: JobHandle,
    pub result: Arc<TagMutationJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct TagMutationJob {
    result: Arc<Mutex<Option<CountMutation>>>,
}
#[uniffi::export]
impl TagMutationJob {
    /// Read after JobFinished(Succeeded). No blocking wait or process registry.
    pub fn result(&self) -> Result<Option<CountMutation>, CoreError> {
        callback::enter("TagMutationJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
fn start(
    work: impl FnOnce(
        &meguri_engine::Collection,
        &tags::RunOptions,
    ) -> Result<meguri_engine::OpResult<usize>, meguri_engine::EngineError>
    + Send
    + 'static,
) -> Result<TagMutationJobStart, CoreError> {
    let c = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let options = tags::RunOptions {
            cancel: Some(j.cancel.clone()),
            progress: Some(j.progress.clone()),
        };
        let r = work(&c, &options)?;
        *out.lock().unwrap() = Some(r.into());
        Ok(())
    })?;
    Ok(TagMutationJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(TagMutationJob { result }),
    })
}
fn short(len: usize) -> Result<(), CoreError> {
    if len > tags::LONG_WRITE_ROWS {
        return Err(CoreError::InvalidArgument {
            message: "use the cancellable bulk job for this selection".into(),
        });
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TagNode {
    pub name: String,
    pub full_name: String,
    pub collapsed: bool,
    pub implicit: bool,
    pub children: Vec<TagNode>,
}
impl From<tags::TagNode> for TagNode {
    fn from(n: tags::TagNode) -> Self {
        Self {
            name: n.name,
            full_name: n.full_name,
            collapsed: n.collapsed,
            implicit: n.implicit,
            children: n.children.into_iter().map(Into::into).collect(),
        }
    }
}
#[uniffi::export]
pub fn tag_tree() -> Result<Vec<TagNode>, CoreError> {
    callback::enter("tag_tree");
    Ok(tags::tag_tree(&runtime::host()?.collection()?)?
        .into_iter()
        .map(Into::into)
        .collect())
}
#[uniffi::export]
pub fn flag_names() -> Result<Vec<String>, CoreError> {
    callback::enter("flag_names");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    Ok(flags::flag_names(&*r)?.to_vec())
}
#[uniffi::export]
pub fn add_note_tags(notes: Vec<i64>, text: String) -> Result<CountMutation, CoreError> {
    callback::enter("add_note_tags");
    short(notes.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(tags::add_to_notes(
        c,
        &notes.iter().copied().map(NoteId).collect::<Vec<_>>(),
        &text,
        options,
    )?
    .into())
}
#[uniffi::export]
pub fn start_add_note_tags(notes: Vec<i64>, text: String) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_add_note_tags");
    start(move |c, options| {
        tags::add_to_notes(
            c,
            &notes.iter().copied().map(NoteId).collect::<Vec<_>>(),
            &text,
            options,
        )
    })
}
#[uniffi::export]
pub fn remove_note_tags(notes: Vec<i64>, text: String) -> Result<CountMutation, CoreError> {
    callback::enter("remove_note_tags");
    short(notes.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(tags::remove_from_notes(
        c,
        &notes.iter().copied().map(NoteId).collect::<Vec<_>>(),
        &text,
        options,
    )?
    .into())
}
#[uniffi::export]
pub fn start_remove_note_tags(notes: Vec<i64>, text: String) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_remove_note_tags");
    start(move |c, options| {
        tags::remove_from_notes(
            c,
            &notes.iter().copied().map(NoteId).collect::<Vec<_>>(),
            &text,
            options,
        )
    })
}
#[uniffi::export]
pub fn set_card_flag(cards: Vec<i64>, flag: u64) -> Result<CountMutation, CoreError> {
    callback::enter("set_card_flag");
    short(cards.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(flags::set_flag(c, &cards.iter().copied().map(CardId).collect::<Vec<_>>(), flag, options)?.into())
}
#[uniffi::export]
pub fn start_set_card_flag(cards: Vec<i64>, flag: u64) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_set_card_flag");
    start(move |c, options| flags::set_flag(c, &cards.iter().copied().map(CardId).collect::<Vec<_>>(), flag, options))
}
#[uniffi::export]
pub fn toggle_card_flag(current: i64, cards: Vec<i64>, chosen: u8) -> Result<CountMutation, CoreError> {
    callback::enter("toggle_card_flag");
    short(cards.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(flags::toggle_flag(
        c,
        CardId(current),
        &cards.iter().copied().map(CardId).collect::<Vec<_>>(),
        chosen,
        options,
    )?
    .into())
}
#[uniffi::export]
pub fn start_toggle_card_flag(current: i64, cards: Vec<i64>, chosen: u8) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_toggle_card_flag");
    start(move |c, options| {
        flags::toggle_flag(
            c,
            CardId(current),
            &cards.iter().copied().map(CardId).collect::<Vec<_>>(),
            chosen,
            options,
        )
    })
}
#[uniffi::export]
pub fn mark_notes(notes: Vec<i64>) -> Result<CountMutation, CoreError> {
    callback::enter("mark_notes");
    short(notes.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(marked::mark(c, &notes.iter().copied().map(NoteId).collect::<Vec<_>>(), options)?.into())
}
#[uniffi::export]
pub fn start_mark_notes(notes: Vec<i64>) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_mark_notes");
    start(move |c, options| marked::mark(c, &notes.iter().copied().map(NoteId).collect::<Vec<_>>(), options))
}
#[uniffi::export]
pub fn unmark_notes(notes: Vec<i64>) -> Result<CountMutation, CoreError> {
    callback::enter("unmark_notes");
    short(notes.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(marked::unmark(c, &notes.iter().copied().map(NoteId).collect::<Vec<_>>(), options)?.into())
}
#[uniffi::export]
pub fn start_unmark_notes(notes: Vec<i64>) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_unmark_notes");
    start(move |c, options| marked::unmark(c, &notes.iter().copied().map(NoteId).collect::<Vec<_>>(), options))
}
#[uniffi::export]
pub fn toggle_note_mark(current: i64, notes: Vec<i64>) -> Result<CountMutation, CoreError> {
    callback::enter("toggle_note_mark");
    short(notes.len())?;
    let c = runtime::host()?.collection()?;
    let options = &tags::RunOptions::default();
    let c = &c;
    Ok(marked::toggle_mark(
        c,
        NoteId(current),
        &notes.iter().copied().map(NoteId).collect::<Vec<_>>(),
        options,
    )?
    .into())
}
#[uniffi::export]
pub fn start_toggle_note_mark(current: i64, notes: Vec<i64>) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_toggle_note_mark");
    start(move |c, options| {
        marked::toggle_mark(
            c,
            NoteId(current),
            &notes.iter().copied().map(NoteId).collect::<Vec<_>>(),
            options,
        )
    })
}
#[uniffi::export]
pub fn start_rename_tag(from: String, to: String) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_rename_tag");
    start(move |c, options| tags::rename(c, &from, &to, options))
}
#[uniffi::export]
pub fn start_reparent_tags(sources: Vec<String>, target: Option<String>) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_reparent_tags");
    start(move |c, options| tags::reparent(c, &sources, target.as_deref(), options))
}
#[uniffi::export]
pub fn start_delete_tags(text: String) -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_delete_tags");
    start(move |c, options| tags::delete(c, &text, options))
}
#[uniffi::export]
pub fn start_clear_unused_tags() -> Result<TagMutationJobStart, CoreError> {
    callback::enter("start_clear_unused_tags");
    start(move |c, options| tags::clear_unused(c, options))
}
#[uniffi::export]
pub fn set_tags_collapsed(names: Vec<String>, collapsed: bool) -> Result<CountMutation, CoreError> {
    callback::enter("set_tags_collapsed");
    short(names.len())?;
    Ok(tags::set_collapsed(&runtime::host()?.collection()?, &names, collapsed)?.into())
}
#[uniffi::export]
pub fn rename_flag(flag: u8, name: String) -> Result<BoolMutation, CoreError> {
    callback::enter("rename_flag");
    Ok(flags::rename_flag(&runtime::host()?.collection()?, flag, &name)?.into())
}
#[uniffi::export]
pub fn restore_flag_name(flag: u8) -> Result<BoolMutation, CoreError> {
    callback::enter("restore_flag_name");
    Ok(flags::restore_flag_name(&runtime::host()?.collection()?, flag)?.into())
}
