//! G9's cancellable planning and composable commit. A U34 caller passes its
//! in-memory note type, then saves it and commits this plan in one scope.
use crate::executor::catalog;
use crate::{CancelToken, Collection, EngineError, OpResult, OpSpec, Plan, Precondition, ProgressSink};
use meguri_model::{
    changes::OpSource,
    ids::{CardId, NoteId, NotetypeId},
    notetype::Notetype,
};
use meguri_storage::{Cell, ReadRows, ReaderRole, WriteScope};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct NotePlan {
    pub note: NoteId,
    pub cards: Vec<super::MissingCard>,
    pub sibling_position: Option<u32>,
    content: [u8; 32],
}

// Only inputs that affect existence/placement/position, never mtimes or usns.
fn content(note: &meguri_model::note::Note, cards: &[meguri_model::card::Card]) -> [u8; 32] {
    let mut cells = vec![Cell::text(note.fields.clone()), Cell::text(note.tags.clone())];
    for card in cards {
        cells.extend(
            [
                card.id.0,
                i64::from(card.ord),
                card.deck_id.0,
                card.original_deck_id.0,
                card.card_type.stored(),
                i64::from(card.due),
                i64::from(card.original_due),
            ]
            .map(Cell::Integer),
        );
    }
    meguri_storage::journal::digest(&cells)
}

#[derive(Clone, Debug)]
pub struct RegenerationPlan {
    pub notetype: Notetype,
    pub notes: Vec<NotePlan>,
}

pub fn plan(
    reader: &impl ReadRows,
    nt: &Notetype,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<RegenerationPlan, EngineError> {
    plan_mapped(reader, nt, cancel, progress, |note, cards| Ok((note, cards)))
}

/// G9's plan for the notes as a structure change leaves them before it
/// generates (`notetypes/structure-changes` S5, S6, S7): `map` turns each
/// stored note and its cards into the rows the commit will have written by
/// then, so a note unchanged since planning passes `commit_in`'s content
/// check there. The mapped rows are never written.
pub fn plan_mapped<F>(
    reader: &impl ReadRows,
    nt: &Notetype,
    cancel: &CancelToken,
    progress: &ProgressSink,
    mut map: F,
) -> Result<RegenerationPlan, EngineError>
where
    F: FnMut(meguri_model::note::Note, Vec<meguri_model::card::Card>) -> Result<(meguri_model::note::Note, Vec<meguri_model::card::Card>), EngineError>,
{
    let mut notes = Vec::new();
    let mut after = None;
    let mut done = 0;
    let fronts = super::normal::fronts(nt);
    // One snapshot: deck usability and the last deck do not change.
    let memo = super::placement::Memo::default();
    let total = reader.notetype_note_count(nt.id)?;
    progress.report(0, Some(total));
    loop {
        cancel.check()?;
        let page = reader.notetype_notes_after(nt.id, after, 500)?;
        if page.is_empty() {
            break;
        }
        after = page.last().map(|note| note.id);
        for note in page {
            cancel.check()?;
            let cards = reader.cards_of_note(note.id)?;
            let (note, siblings) = map(note, cards)?;
            if !siblings.is_empty() && (nt.config.kind == 1 || siblings.len() != nt.templates.len()) {
                notes.push(NotePlan {
                    note: note.id,
                    cards: super::plan_note_with(reader, &memo, nt, &fronts, &note, &siblings, None, false)?,
                    sibling_position: super::position::sibling_position(&siblings),
                    content: content(&note, &siblings),
                });
            }
            done += 1;
            progress.report(done, Some(total));
        }
    }
    progress.report(done, Some(done));
    Ok(RegenerationPlan {
        notetype: nt.clone(),
        notes,
    })
}

/// Re-read the live rows. Existing cards can have moved, been deleted or
/// been created since planning; no stale row or counter is ever written.
pub fn commit_in(scope: &mut WriteScope<'_>, planned: &RegenerationPlan) -> Result<Vec<CardId>, EngineError> {
    let mut created = Vec::new();
    let mut after = None;
    // Re-select the predicate in the write transaction (op-model O7), so
    // notes added or moved to this type after planning are included too.
    loop {
        let page = scope.notetype_notes_after(planned.notetype.id, after, 500)?;
        if page.is_empty() {
            break;
        }
        after = page.last().map(|note| note.id);
        for note in page {
            let siblings = scope.cards_of_note(note.id)?;
            if siblings.is_empty()
                || (planned.notetype.config.kind != 1 && siblings.len() == planned.notetype.templates.len())
            {
                continue;
            }
            let entry = planned
                .notes
                .binary_search_by_key(&note.id, |entry| entry.note)
                .ok()
                .map(|index| &planned.notes[index]);
            let missing = if let Some(entry) = entry.filter(|entry| content(&note, &siblings) == entry.content) {
                // Existence was planned. Resolve decks again because their
                // rows and the last-deck config can change independently.
                entry
                    .cards
                    .iter()
                    .map(|card| {
                        Ok(super::MissingCard {
                            ord: card.ord,
                            deck: super::placement::deck(scope, &planned.notetype, card.ord, false, &siblings, None)?,
                        })
                    })
                    .collect::<Result<Vec<_>, EngineError>>()?
            } else {
                super::plan_note(scope, &planned.notetype, &note, &siblings, None, false)?
            };
            created.extend(super::apply_in(scope, &note, &siblings, &missing)?);
        }
    }
    Ok(created)
}

/// Blocking worker entry point. App-facing callers start it on the job pool.
pub fn regenerate(
    collection: &Collection,
    id: NotetypeId,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<Vec<CardId>>, EngineError> {
    let spec = OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
        .undo_step(
            crate::undo::names::UPDATE_NOTE_TYPE,
            crate::undo::entry::StepKind::Other,
        )
        .declare_long_write(Duration::from_secs(1))
        .cancel(cancel.clone())
        .progress(progress.clone());
    crate::plan_and_commit(
        collection,
        ReaderRole::Maintenance,
        spec,
        |context| {
            let nt = context
                .snapshot()
                .notetype(id)?
                .ok_or_else(|| crate::notes::invalid("unknown note type"))?;
            let planned = plan(&**context.snapshot(), &nt, &cancel, &progress)?;
            let preconditions = [
                meguri_storage::Table::Notetypes,
                meguri_storage::Table::Fields,
                meguri_storage::Table::Templates,
            ]
            .into_iter()
            .map(|table| Precondition::TableUntouched {
                table,
                since: context.seq(),
            })
            .collect();
            Ok(Plan {
                value: planned,
                preconditions,
            })
        },
        |context, planned| commit_in(context.scope(), &planned),
    )
}

/// Short app-facing start call; planning runs on the engine's bounded pool.
pub fn start(engine: &crate::Engine, collection: Collection, id: NotetypeId) -> Result<crate::JobHandle, EngineError> {
    engine.start_job(crate::JobKind::BulkOp, move |context| {
        regenerate(&collection, id, context.cancel.clone(), context.progress.clone()).map(|_| ())
    })
}
