//! The bulk-action adapters (`specs/conformance/bulk-ops.md` X1, X3): the
//! `find_replace` op runs meguri-engine's Find and Replace (U86,
//! `browser/find-replace` F11) through the hosted collection, as one
//! operation with its undo step; the `find_replace_fields` observation reads
//! `selection_summary`'s field names (F3).

use meguri_engine::bulk::NoteTarget;
use meguri_engine::bulk::find_replace::{self, FindIn, FindReplace, Refusal};
use meguri_engine::bulk::summary::selection_summary;
use meguri_engine::{CancelToken, EngineError, ProgressSink};
use meguri_model::ids::NoteId;
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
    let request = FindReplace {
        find: args.find.clone(),
        replace: args.replace.clone(),
        regex: args.regex.unwrap_or(false),
        ignore_case: args.ignore_case.unwrap_or(true),
        within,
    };
    let collection = session.engine_collection()?;
    match find_replace::run(&collection, &target, &request, CancelToken::new(), ProgressSink::new()) {
        Ok(done) => Ok(OpResult::Output(json!({ "count": done.value.affected }))),
        Err(EngineError::FindReplace(refusal)) => match refusal {
            Refusal::InvalidRegex { .. } => Ok(OpResult::Rejected("invalid_regex")),
            Refusal::SeparatorInReplacement => Ok(OpResult::Rejected("space_in_replacement")),
            Refusal::FieldCountMismatch { .. } => Ok(OpResult::Rejected("field_count")),
            // X1: no scenario lists a note of a missing note type; it would
            // need its own rejection kind first.
            Refusal::NotetypeMissing { .. } => Err(core(EngineError::FindReplace(refusal))),
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
