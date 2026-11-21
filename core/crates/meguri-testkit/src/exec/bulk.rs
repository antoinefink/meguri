//! The bulk-action adapters (`specs/conformance/bulk-ops.md` X1 to X3): the
//! `find_replace` op runs meguri-engine's Find and Replace (U86,
//! `browser/find-replace` F11) and `change_deck` its Change Deck (U87,
//! `browser/bulk-actions` B7.2) through the hosted collection, each as one
//! operation with its undo step; the `find_replace_fields` observation reads
//! `selection_summary`'s field names (F3).

use meguri_engine::bulk::change_deck::{self, DeckTarget};
use meguri_engine::bulk::find_replace::{self, FindIn, FindReplace, Refusal};
use meguri_engine::bulk::summary::selection_summary;
use meguri_engine::bulk::{CardTarget, NoteTarget};
use meguri_engine::decks::Refusal as DeckRefusal;
use meguri_engine::{CancelToken, EngineError, ProgressSink};
use meguri_model::ids::{CardId, NoteId};
use meguri_storage::ReadRows;
use serde_json::{Value, json};

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{FindReplaceIn, LogicalRef, ObservationArgs, ObservationRequest, Step};

/// Registers `find_replace` and `find_replace_fields`.
pub(super) fn register(registry: &mut Registry) {
    registry.op_rejecting(
        "find_replace",
        &["invalid_regex", "space_in_replacement", "field_count"],
        run_find_replace,
    );
    registry.op_rejecting("change_deck", &["filtered_deck"], run_change_deck);
    registry.observation("find_replace_fields", find_replace_fields);
}

fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

/// The real id of a note reference; a deleted note keeps its id (X1).
fn note_id(session: &Session<'_>, reference: LogicalRef) -> Result<NoteId, ExecError> {
    session
        .notes
        .get(reference.index())
        .copied()
        .map(NoteId)
        .ok_or_else(|| ExecError::new("unknown_ref", format!("{reference} was not created")))
}

/// The real id of a card reference; a deleted card keeps its id (X2).
fn card_id(session: &Session<'_>, reference: LogicalRef) -> Result<CardId, ExecError> {
    session
        .cards
        .get(reference.index())
        .copied()
        .map(CardId)
        .ok_or_else(|| ExecError::new("unknown_ref", format!("{reference} was not created")))
}

/// X2: the listed cards to the named deck; the output is the cards moved.
fn run_change_deck(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::ChangeDeck(args) = step else {
        return Err(invalid("expected change_deck"));
    };
    let cards = args
        .cards
        .iter()
        .map(|card| card_id(session, card.0))
        .collect::<Result<Vec<_>, _>>()?;
    let collection = session.engine_collection()?;
    // X2: the deck by its stored name in human form, whatever kind it is.
    let deck = collection
        .reader(meguri_storage::ReaderRole::Query)
        .map_err(core)?
        .deck_rows()
        .map_err(core)?
        .into_iter()
        .find(|deck| deck.human_name() == args.deck)
        .map(|deck| deck.id)
        .ok_or_else(|| ExecError::new("unknown_ref", format!("no deck {:?}", args.deck)))?;
    match change_deck::run(
        &collection,
        &CardTarget::Cards(cards),
        &DeckTarget::Deck(deck),
        CancelToken::new(),
        ProgressSink::new(),
    ) {
        Ok(done) => Ok(OpResult::Output(json!({ "count": done.value.affected }))),
        Err(EngineError::DeckRefused(DeckRefusal::FilteredDeck)) => Ok(OpResult::Rejected("filtered_deck")),
        Err(error) => Err(core(error)),
    }
}

/// X1: the listed notes or all notes; the output is Meguri's `affected`.
fn run_find_replace(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::FindReplace(args) = step else {
        return Err(invalid("expected find_replace"));
    };
    let target = match &args.notes {
        Some(notes) => NoteTarget::Notes(
            notes
                .iter()
                .map(|note| note_id(session, note.0))
                .collect::<Result<_, _>>()?,
        ),
        None => NoteTarget::AllNotes,
    };
    let within = match &args.within {
        None | Some(FindReplaceIn::All) => FindIn::AllFields,
        Some(FindReplaceIn::Tags) => FindIn::Tags,
        Some(FindReplaceIn::Field(name)) => FindIn::Field(name.clone()),
    };
    let mut request = FindReplace {
        find: args.find.clone(),
        replace: args.replace.clone(),
        regex: args.regex.unwrap_or(false),
        ignore_case: args.ignore_case.unwrap_or(true),
        whole_words: false,
        within,
        confirmed: 0,
    };
    let collection = session.engine_collection()?;
    // The confirmed count (the pattern of `notetypes/draft-save` D8): a
    // caller with no question passes the count it reads at the step. A
    // refused preview leaves the run to give the same refusal.
    request.confirmed = find_replace::preview(&collection, &target, &request, &CancelToken::new(), &ProgressSink::new())
        .map_or(u64::MAX, |preview| preview.changed);
    match find_replace::run(&collection, &target, &request, CancelToken::new(), ProgressSink::new()) {
        Ok(done) => Ok(OpResult::Output(json!({ "count": done.value.affected }))),
        Err(EngineError::FindReplace(refusal)) => match refusal {
            Refusal::InvalidRegex { .. } => Ok(OpResult::Rejected("invalid_regex")),
            Refusal::SeparatorInReplacement => Ok(OpResult::Rejected("space_in_replacement")),
            Refusal::FieldCountMismatch { .. } => Ok(OpResult::Rejected("field_count")),
            // X1: no scenario lists a note of a missing note type; it would
            // need its own rejection kind first.
            Refusal::NotetypeMissing { .. } | Refusal::CountChanged { .. } => {
                Err(core(EngineError::FindReplace(refusal)))
            }
        },
        Err(error) => Err(core(error)),
    }
}

/// X3: F3's field names for the listed notes.
fn find_replace_fields(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("find_replace_fields takes {notes}"));
    };
    let notes = map
        .get("notes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("find_replace_fields takes {notes}"))?
        .iter()
        .map(|item| {
            let reference = item
                .as_str()
                .and_then(|text| text.parse::<LogicalRef>().ok())
                .ok_or_else(|| invalid("a note reference"))?;
            note_id(session, reference)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let collection = session.engine_collection()?;
    let summary = selection_summary(&collection, &NoteTarget::Notes(notes)).map_err(core)?;
    Ok(json!(summary.field_names))
}
