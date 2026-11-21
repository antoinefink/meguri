//! Removing a note type (`notetypes/structure-changes` S12): its notes with
//! their cards and graves, its `_nt_<id>_` config keys and its rows, in one
//! transaction and one "Remove Note Type" step, after a safety backup of
//! its own (T6.1, no coalescing). The counts the delete question showed are
//! a precondition (S12 and `notetypes/draft-save` D8, changed 2026-11-20):
//! counted again on the planning snapshot, so an out-of-date question costs
//! no backup, and inside the write transaction before its first write;
//! when either live count is larger the removal writes nothing and returns
//! `LossesChanged { notes, cards }`.

use std::sync::Arc;

use meguri_model::changes::OpSource;
use meguri_model::ids::NotetypeId;
use meguri_storage::journal::{StructureReason, StructureReport};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};

use super::CURRENT_NOTETYPE_KEY;
use super::structure::{Losses, StructureRefusal, write_spec};
use crate::executor::catalog;
use crate::undo::entry::StepKind;
use crate::undo::names::REMOVE_NOTE_TYPE;
use crate::{CancelToken, Collection, EngineError, OpResult, OpSpec, Plan, ProgressSink, SchemaReason};

/// S12's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoveNotetype {
    pub notetype: NotetypeId,
    /// The note type's notes the delete question showed (W12); callers with
    /// no question pass the count they read.
    pub confirmed_notes: u64,
    /// The note type's cards the question showed.
    pub confirmed_cards: u64,
}

/// What a removal did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub id: NotetypeId,
    /// False when no note type had the id: nothing changed.
    pub removed: bool,
    pub notes_removed: u64,
    pub cards_removed: u64,
}

/// The note type's notes and cards on `reader`'s rows, and whether it is the
/// collection's only note type; `None` when it does not exist.
fn census(
    reader: &impl ReadRows,
    id: NotetypeId,
    cancel: &CancelToken,
) -> Result<Option<(u64, u64, bool)>, EngineError> {
    if reader.notetype(id)?.is_none() {
        return Ok(None);
    }
    let only = super::read::ids(reader)?.len() == 1;
    let notes = reader.notetype_note_count(id)?;
    let mut cards = 0;
    let mut after = None;
    loop {
        cancel.check()?;
        let page = reader.card_ordinals_of_notetype_page(id, after)?;
        let Some(last) = page.last() else { break };
        after = Some(last.0);
        cards += page.len() as u64;
    }
    Ok(Some((notes, cards, only)))
}

/// S12's refusals on one set of rows: `LastNotetype`, then the confirmed
/// counts, each compared on its own (D8).
fn check(request: &RemoveNotetype, notes: u64, cards: u64, only: bool) -> Result<(), EngineError> {
    if only {
        return Err(StructureRefusal::LastNotetype.into());
    }
    if notes > request.confirmed_notes || cards > request.confirmed_cards {
        return Err(StructureRefusal::LossesChanged(Box::new(Losses::Removal { notes, cards })).into());
    }
    Ok(())
}

/// The removal op. A note type that no longer exists is a success that
/// changes nothing and takes no backup.
pub fn remove(
    collection: &Collection,
    request: RemoveNotetype,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<Removed>, EngineError> {
    let spec = OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
        .undo_step(REMOVE_NOTE_TYPE, StepKind::Other)
        .cancel(cancel.clone())
        .progress(progress.clone());
    crate::plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        spec,
        |context| {
            let found = census(&**context.snapshot(), request.notetype, &cancel)?;
            if let Some((notes, cards, only)) = found {
                // Refused on the snapshot already: no backup for a removal
                // whose question has to show again.
                check(&request, notes, cards, only)?;
            }
            Ok(Plan {
                value: found.map(|(notes, ..)| notes),
                preconditions: Vec::new(),
            })
        },
        |spec, planned| match planned {
            Some(notes) => write_spec(spec, &[SchemaReason::NotetypeRemoved], *notes > 0),
            None => spec.clone(),
        },
        move |context, planned| {
            let id = request.notetype;
            let mut removed = Removed {
                id,
                ..Removed::default()
            };
            if planned.is_none() {
                return Ok(removed);
            }
            let scope = context.scope();
            // Again on the live rows of this transaction, before any write:
            // this count alone guarantees the commit.
            let Some((notes, cards, only)) = census(scope, id, &CancelToken::new())? else {
                return Ok(removed);
            };
            check(&request, notes, cards, only)?;
            crate::mark_schema_change(scope, &[SchemaReason::NotetypeRemoved])?;
            let mut report = StructureReport::default();
            report.reasons.insert(StructureReason::NotetypeRemoved);
            report.removed.insert(id.0);
            let token = scope.start_structure_report(report)?;
            // Step 1: in ascending id order, each note's cards with card
            // graves, then the note with its note grave; no deleted-notes log.
            let mut after = None;
            loop {
                let page = scope.notetype_notes_after(id, after, 500)?;
                let Some(last) = page.last() else { break };
                after = Some(last.id);
                for note in page {
                    removed.cards_removed += scope.cards_of_note(note.id)?.len() as u64;
                    scope.remove_note_and_cards(note.id)?;
                    removed.notes_removed += 1;
                }
            }
            // Step 3: the note type's config keys (C10).
            scope.remove_config_family(&format!("_nt_{}_", id.0))?;
            // Step 4: its rows, no grave.
            scope.remove_notetype(id)?;
            scope.complete_structure_report(token)?;
            // Step 5: the remaining note type whose name sorts first.
            if let Some(first) = scope.first_notetype_id_by_name()? {
                scope.set_config(CURRENT_NOTETYPE_KEY, &ConfigValue::from(first.0))?;
            }
            removed.removed = true;
            Ok(removed)
        },
    )
}
