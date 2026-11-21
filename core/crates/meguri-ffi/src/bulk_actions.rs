//! Browse's bulk actions (U87, `browser/bulk-actions`, `browser/workspace`
//! V8.1) as jobs (`browser/bulk-framework` K2): Change Deck, Flag, Suspend,
//! Unsuspend, Bury, Unbury, Set Due Date and Forget on card ids, Add Tags,
//! Remove Tags and Delete Notes on note ids. A start call returns the job's
//! handle at once; the result object holds the counts (B16), Delete Notes'
//! cards removed, and the commit's summary once the job ends, before its
//! `JobFinished`. A refusal is the job's `Failed` outcome (`result` then
//! returns it as its error).
//!
//! The scheduling actions run admitted by the open collection's review
//! service, so answers it accepted are written first and an active session
//! sees the change (`ReviewService::admit_card_action`); the others run as
//! their engine ops do. Selections of a search handle arrive with the Browse
//! FFI (U264); these exports take the ids.
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

/// Starts a card action on `cards` (in table order, B1).
#[uniffi::export]
pub fn start_bulk_card_action(cards: Vec<i64>, action: BulkCardAction) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_bulk_card_action");
    let host = runtime::host()?;
    let engine = runtime::engine()?;
    let target = CardTarget::Cards(cards.into_iter().map(CardId).collect());
    let job = match EngineAction::from(action) {
        EngineAction::ChangeDeck(deck) => change_deck::start(engine, host.collection()?, target, deck)?,
        EngineAction::Card(action) if action.writes_scheduling() => bulk::start(engine, move |context| {
            let cancel = Arc::clone(&context.cancel);
            let progress = context.progress.clone();
            host.review()
                .admit_card_action(&context.cancel, |collection| {
                    cards::run_admitted(collection, &target, &action, cancel, progress)
                })
                .map(BulkOutcome::of)
        })?,
        EngineAction::Card(action) => cards::start(engine, host.collection()?, target, action)?,
    };
    Ok(started(job))
}

/// Starts a note action on `notes` (in table order, B1).
#[uniffi::export]
pub fn start_bulk_note_action(notes: Vec<i64>, action: BulkNoteAction) -> Result<BulkActionJobStart, CoreError> {
    callback::enter("start_bulk_note_action");
    let host = runtime::host()?;
    let target = NoteTarget::Notes(notes.into_iter().map(NoteId).collect());
    let job = notes::start(runtime::engine()?, host.collection()?, target, action.into())?;
    Ok(started(job))
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
