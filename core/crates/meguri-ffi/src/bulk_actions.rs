//! Browse's bulk actions (U87, `browser/bulk-actions`, `browser/workspace`
//! V8.1) as jobs (`browser/bulk-framework` K2): Change Deck, Flag, Suspend,
//! Unsuspend, Bury, Unbury, Set Due Date and Forget on card ids, Add Tags,
//! Remove Tags and Delete Notes on note ids. A start call returns the job's
//! handle at once; the result object holds the counts (B16), Delete Notes'
//! cards removed, and the commit's summary once the job ends, before its
//! `JobFinished`. A refusal is the job's `Failed` outcome (`result` then
//! returns it as its error).
//!
//! Every action runs admitted by the open collection's review service
//! (`ReviewService::admit_card_action`): the answers it accepted are written
//! first, so none fails on a card row the action rewrote or removed, and an
//! active session sees the change. Each action takes ids, or a Browse
//! selection (`browser/bulk-framework` K1) that the job resolves in the
//! core, after admission, so a large selection never crosses the FFI as
//! ids. Find and Replace's start call (`find_replace`) runs the same way.
use crate::browse_search::BrowseSelection;
use crate::{CoreError, callback, jobs::JobHandle, mutations::MutationSummary, runtime};
use meguri_engine::bulk::cards::{self, CardAction};
use meguri_engine::bulk::change_deck::{self, DeckTarget};
use meguri_engine::bulk::notes::{self, NoteAction};
use meguri_engine::bulk::{self, BulkJob, BulkOutcome, CardTarget, NoteTarget};
use meguri_model::ids::{CardId, DeckId, NoteId};
use std::sync::Arc;

/// A card action of V8.1.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BulkCardAction {
    /// Change Deck to an existing deck (B7.2).
    ChangeDeck { deck: i64 },
    /// Change Deck to a typed name: that deck, or a new one made in the
    /// same step (B7.2 step 0).
    ChangeDeckToName { name: String },
    /// Flag 1 to 7, or 0 for No Flag; it sets, it does not toggle.
    SetFlag { flag: u8 },
    Suspend,
    /// The suspended target cards only.
    Unsuspend,
    Bury,
    /// The buried target cards only.
    Unbury,
    /// The text as typed (`setDueBrowser`).
    SetDueDate { days: String },
    /// Reset with its two choices (`restorePositionBrowser`,
    /// `resetCountsBrowser`).
    Forget { restore_position: bool, reset_counts: bool },
}

/// A note action of V8.1.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BulkNoteAction {
    /// The tag entry's text; TAB, LF and VT separate tags too (B9).
    AddTags { text: String },
    RemoveTags { text: String },
    DeleteNotes,
}

/// What an action did (B16): the rows written and the ids it was given,
/// Delete Notes' cards removed, and the commit.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BulkActionResult {
    pub affected: u64,
    pub considered: u64,
    /// Delete Notes only (B11).
    pub cards_removed: Option<u64>,
    pub summary: MutationSummary,
}

impl From<BulkOutcome> for BulkActionResult {
    fn from(outcome: BulkOutcome) -> Self {
        Self {
            affected: outcome.counts.affected,
            considered: outcome.counts.considered,
            cards_removed: outcome.cards_removed,
            summary: MutationSummary {
                seq: outcome.seq.0,
                generation: outcome.generation.0,
                undo_name: outcome.undo.map(|undo| undo.name.to_owned()),
                changes: outcome.changes.as_ref().into(),
                past_cancel_point: outcome.past_cancel_point,
            },
        }
    }
}

/// A started bulk action's result.
#[derive(Debug, uniffi::Object)]
pub struct BulkActionJob {
    job: BulkJob,
}

#[uniffi::export]
impl BulkActionJob {
    /// `None` while the job runs; the result once it ended, or its refusal.
    pub fn result(&self) -> Result<Option<BulkActionResult>, CoreError> {
        callback::enter("BulkActionJob::result");
        match self.job.outcome() {
            None => Ok(None),
            Some(Ok(outcome)) => Ok(Some(outcome.into())),
            Some(Err(error)) => Err(error.into()),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct BulkActionJobStart {
    pub handle: JobHandle,
    pub result: Arc<BulkActionJob>,
}

fn started(job: BulkJob) -> BulkActionJobStart {
    BulkActionJobStart {
        handle: crate::jobs::wrap(job.handle().clone()),
        result: Arc::new(BulkActionJob { job }),
    }
}

/// A card action in the engine's terms.
#[derive(Debug, PartialEq, Eq)]
enum EngineAction {
    ChangeDeck(DeckTarget),
    Card(CardAction),
}

impl From<BulkCardAction> for EngineAction {
    fn from(action: BulkCardAction) -> EngineAction {
        let card = EngineAction::Card;
        match action {
            BulkCardAction::ChangeDeck { deck } => EngineAction::ChangeDeck(DeckTarget::Deck(DeckId(deck))),
            BulkCardAction::ChangeDeckToName { name } => EngineAction::ChangeDeck(DeckTarget::NewDeck(name)),
            BulkCardAction::SetFlag { flag } => card(CardAction::SetFlag(flag)),
            BulkCardAction::Suspend => card(CardAction::Suspend),
            BulkCardAction::Unsuspend => card(CardAction::Unsuspend),
            BulkCardAction::Bury => card(CardAction::Bury),
            BulkCardAction::Unbury => card(CardAction::Unbury),
            BulkCardAction::SetDueDate { days } => card(CardAction::SetDueDate { days }),
            BulkCardAction::Forget {
                restore_position,
                reset_counts,
            } => card(CardAction::Forget {
                restore_position,
                reset_counts,
            }),
        }
    }
}

impl From<BulkNoteAction> for NoteAction {
    fn from(action: BulkNoteAction) -> NoteAction {
        match action {
            BulkNoteAction::AddTags { text } => NoteAction::AddTags(text),
            BulkNoteAction::RemoveTags { text } => NoteAction::RemoveTags(text),
            BulkNoteAction::DeleteNotes => NoteAction::DeleteNotes,
        }
    }
}

/// Runs `run` as a bulk job admitted by the host's review service.
pub(crate) fn admitted(
    run: impl FnOnce(&meguri_engine::Collection, Arc<meguri_engine::CancelToken>, meguri_engine::ProgressSink)
        -> Result<BulkOutcome, meguri_engine::EngineError>
        + Send
        + 'static,
) -> Result<BulkActionJobStart, CoreError> {
    let host = runtime::host()?;
    // The ids name rows of the collection open now: a job that starts after
    // a replacement refuses with `StaleGeneration`.
    let generation = host.collection()?.generation();
    let job = bulk::start(runtime::engine()?, move |context| {
        let (cancel, progress) = (Arc::clone(&context.cancel), context.progress.clone());
        host.review().admit_card_action(
            &context.cancel,
            Some(generation),
            |collection| run(collection, cancel, progress),
            |outcome: &BulkOutcome| outcome.seq,
        )
    })?;
    Ok(started(job))
}

/// Starts a card action on `cards` (in table order, B1).
#[uniffi::export]
pub fn start_bulk_card_action(cards: Vec<i64>, action: BulkCardAction) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_bulk_card_action");
    card_action(CardTarget::Cards(cards.into_iter().map(CardId).collect()), action)
}

/// Starts a card action on the target cards of `selection` (B1, H12: every
/// card of each selected note in Notes mode), resolved by the job. A
/// selection that cannot be resolved (released, superseded, of another
/// revision or collection) is the job's `InvalidArgument` (K1).
#[uniffi::export]
pub fn start_bulk_card_action_on_selection(
    selection: Arc<BrowseSelection>,
    action: BulkCardAction,
) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_bulk_card_action_on_selection");
    card_action(CardTarget::Selection(selection.engine_selection().clone()), action)
}

/// Starts a note action on `notes` (in table order, B1).
#[uniffi::export]
pub fn start_bulk_note_action(notes: Vec<i64>, action: BulkNoteAction) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_bulk_note_action");
    note_action(NoteTarget::Notes(notes.into_iter().map(NoteId).collect()), action)
}

/// Starts a note action on the target notes of `selection` (B1, H12),
/// resolved by the job, refused as `start_bulk_card_action_on_selection`
/// says.
#[uniffi::export]
pub fn start_bulk_note_action_on_selection(
    selection: Arc<BrowseSelection>,
    action: BulkNoteAction,
) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_bulk_note_action_on_selection");
    note_action(NoteTarget::Selection(selection.engine_selection().clone()), action)
}

fn card_action(target: CardTarget, action: BulkCardAction) -> Result<BulkActionJobStart, CoreError> {
    match EngineAction::from(action) {
        EngineAction::ChangeDeck(deck) => admitted(move |collection, cancel, progress| {
            change_deck::run(collection, &target, &deck, cancel, progress).map(BulkOutcome::of)
        }),
        EngineAction::Card(action) => admitted(move |collection, cancel, progress| {
            cards::run_admitted(collection, &target, &action, cancel, progress).map(BulkOutcome::of)
        }),
    }
}

fn note_action(target: NoteTarget, action: BulkNoteAction) -> Result<BulkActionJobStart, CoreError> {
    let action = NoteAction::from(action);
    admitted(move |collection, cancel, progress| {
        notes::run(collection, &target, &action, cancel, progress)
            .map(|result| BulkOutcome::split(result, |value| (value.counts, value.cards_removed)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_keep_their_targets_and_texts() {
        let engine = |action: BulkCardAction| EngineAction::from(action);
        assert_eq!(
            engine(BulkCardAction::ChangeDeck { deck: 7 }),
            EngineAction::ChangeDeck(DeckTarget::Deck(DeckId(7)))
        );
        assert_eq!(
            engine(BulkCardAction::ChangeDeckToName { name: " a::b ".into() }),
            EngineAction::ChangeDeck(DeckTarget::NewDeck(" a::b ".into()))
        );
        for (action, expected) in [
            (BulkCardAction::SetFlag { flag: 0 }, CardAction::SetFlag(0)),
            (BulkCardAction::Suspend, CardAction::Suspend),
            (BulkCardAction::Unsuspend, CardAction::Unsuspend),
            (BulkCardAction::Bury, CardAction::Bury),
            (BulkCardAction::Unbury, CardAction::Unbury),
            (
                BulkCardAction::SetDueDate { days: "3-7!".into() },
                CardAction::SetDueDate { days: "3-7!".into() },
            ),
            (
                BulkCardAction::Forget {
                    restore_position: false,
                    reset_counts: true,
                },
                CardAction::Forget {
                    restore_position: false,
                    reset_counts: true,
                },
            ),
        ] {
            assert_eq!(engine(action), EngineAction::Card(expected));
        }
        assert_eq!(
            NoteAction::from(BulkNoteAction::AddTags { text: "a\tb".into() }),
            NoteAction::AddTags("a\tb".into())
        );
        assert_eq!(NoteAction::from(BulkNoteAction::DeleteNotes), NoteAction::DeleteNotes);
    }
}
