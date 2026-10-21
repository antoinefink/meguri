//! Conformance undo operations through the hosted collection (V1–V3).
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{ObservationRequest, Step};
use meguri_engine::{DisabledReason, EngineError, UndoScope, UndoStatus};
use serde_json::{Value, json};

pub(super) fn register(registry: &mut Registry) {
    registry.op_rejecting("undo", &["nothing_to_undo", "changed_externally"], restore);
    registry.op_rejecting("redo", &["nothing_to_redo", "changed_externally"], restore);
    registry.observation("undo_status", status);
}
fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn restore(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let undo = match step {
        Step::Undo(_) => true,
        Step::Redo(_) => false,
        _ => return Err(core("expected undo or redo")),
    };
    let collection = session.engine_collection()?;
    session.held_queue = None;
    session.day.clear();
    let empty = if undo { "nothing_to_undo" } else { "nothing_to_redo" };
    let peek = if undo {
        collection.peek_undo(UndoScope::Global)
    } else {
        collection.peek_redo(UndoScope::Global)
    }
    .map_err(core)?;
    if peek.name().is_none() {
        return Ok(OpResult::Rejected(empty));
    }
    let result = if undo {
        collection.undo(UndoScope::Global)
    } else {
        collection.redo(UndoScope::Global)
    };
    match result {
        Ok(_) => Ok(OpResult::Done),
        Err(EngineError::UndoUnavailable {
            reason: DisabledReason::ChangedExternally,
        }) => Ok(OpResult::Rejected("changed_externally")),
        Err(EngineError::UndoUnavailable { .. }) => Ok(OpResult::Rejected(empty)),
        Err(error) => Err(core(error)),
    }
}

fn id(status: UndoStatus) -> Result<Value, ExecError> {
    match status.name() {
        None => Ok(Value::Null),
        Some(name) if meguri_engine::undo::names::ALL.contains(&name) => {
            Ok(json!(name.to_ascii_lowercase().replace([' ', '/'], "_")))
        }
        Some(_) => Err(core("undo step has no conformance V4 id")),
    }
}
fn status(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let collection = session.engine_collection()?;
    Ok(
        json!({"undo": id(collection.peek_undo(UndoScope::Global).map_err(core)?)?, "redo": id(collection.peek_redo(UndoScope::Global).map_err(core)?)?}),
    )
}
