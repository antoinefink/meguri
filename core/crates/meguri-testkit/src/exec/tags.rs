//! The tags area's adapters (`specs/conformance/tag-ops.md` G1 to G9): the
//! eight tag and flag ops and the `tag_tree` observation, run with
//! meguri-engine's tag and flag operations (U30) inside a write scope of the
//! session's collection, as one operation each. Hosted operations preserve their engine undo metadata and observer hooks.

use meguri_engine::flags::{self, InvalidFlag};
use meguri_engine::tags::{self, InvalidName, TagNode};
use meguri_model::ids::{CardId, NoteId};

use serde_json::{Value, json};

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{LogicalRef, ObservationRequest, Step};

/// Registers the tag ops and `tag_tree`.
pub(super) fn register(registry: &mut Registry) {
    registry.op("add_note_tags", add_note_tags);
    registry.op("remove_note_tags", remove_note_tags);
    registry.op("delete_tags", delete_tags);
    registry.op_rejecting("rename_tag", &[InvalidName::KIND], rename_tag);
    registry.op("reparent_tags", reparent_tags);
    registry.op("clear_unused_tags", clear_unused_tags);
    registry.op("set_tag_collapsed", set_tag_collapsed);
    registry.op_rejecting("set_flag", &[InvalidFlag::KIND], set_flag);
    registry.observation("tag_tree", tag_tree);
}

fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn wrong_op(name: &str) -> ExecError {
    ExecError::new("internal_error", format!("{name} adapter called for another op"))
}

fn resolve(ids: &[i64], r: LogicalRef) -> Result<i64, ExecError> {
    ids.get(r.index())
        .copied()
        .ok_or_else(|| ExecError::new("unknown_ref", format!("{r} was not created")))
}

fn notes(session: &Session<'_>, refs: impl IntoIterator<Item = LogicalRef>) -> Result<Vec<NoteId>, ExecError> {
    refs.into_iter()
        .map(|r| resolve(&session.notes, r).map(NoteId))
        .collect()
}

fn count(n: usize) -> OpResult {
    OpResult::Output(json!({ "count": n }))
}

/// G1: T10 Add.
fn add_note_tags(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::AddNoteTags(args) = step else {
        return Err(wrong_op("add_note_tags"));
    };
    let notes = notes(session, args.notes.iter().map(|r| r.0))?;
    let collection = session.engine_collection()?;
    tags::add_to_notes(&collection, &notes, &args.tags, &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G2: T10 Remove.
fn remove_note_tags(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::RemoveNoteTags(args) = step else {
        return Err(wrong_op("remove_note_tags"));
    };
    let notes = notes(session, args.notes.iter().map(|r| r.0))?;
    let collection = session.engine_collection()?;
    tags::remove_from_notes(&collection, &notes, &args.tags, &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G3: T9.
fn delete_tags(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::DeleteTags(args) = step else {
        return Err(wrong_op("delete_tags"));
    };
    let collection = session.engine_collection()?;
    tags::delete(&collection, &args.tags, &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G4: T7, refusing per T7 step 1.
fn rename_tag(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::RenameTag(args) = step else {
        return Err(wrong_op("rename_tag"));
    };
    if tags::check_new_name(&args.to).is_err() {
        return Ok(OpResult::Rejected(InvalidName::KIND));
    }
    let collection = session.engine_collection()?;
    tags::rename(&collection, &args.from, &args.to, &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G5: T8; an empty parent is the top level.
fn reparent_tags(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::ReparentTags(args) = step else {
        return Err(wrong_op("reparent_tags"));
    };
    let target = (!args.parent.is_empty()).then_some(args.parent.as_str());
    let collection = session.engine_collection()?;
    tags::reparent(&collection, &args.tags, target, &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G6: T11.
fn clear_unused_tags(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::ClearUnusedTags(_) = step else {
        return Err(wrong_op("clear_unused_tags"));
    };
    let collection = session.engine_collection()?;
    tags::clear_unused(&collection, &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G7: T12, the names in order as one operation.
fn set_tag_collapsed(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::SetTagCollapsed(args) = step else {
        return Err(wrong_op("set_tag_collapsed"));
    };
    let collection = session.engine_collection()?;
    tags::set_collapsed(&collection, &args.tags, args.collapsed).map_err(core)?;
    Ok(OpResult::Done)
}

/// G8: F2, refusing a flag above 7. A flag change keeps the card's `mod`.
fn set_flag(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::SetFlag(args) = step else {
        return Err(wrong_op("set_flag"));
    };
    let Ok(flag) = flags::check_flag(args.flag) else {
        return Ok(OpResult::Rejected(InvalidFlag::KIND));
    };
    let cards: Vec<CardId> = args
        .cards
        .iter()
        .map(|r| resolve(&session.cards, r.0).map(CardId))
        .collect::<Result<_, _>>()?;
    let collection = session.engine_collection()?;
    flags::set_flag(&collection, &cards, u64::from(flag), &tags::RunOptions::default())
        .map(|r| count(r.value))
        .map_err(core)
}

/// G9's value of a node list.
pub fn tag_tree_value(nodes: &[TagNode]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|node| {
                json!({
                    "name": node.name,
                    "collapsed": node.collapsed,
                    "children": tag_tree_value(&node.children),
                })
            })
            .collect(),
    )
}

/// G9: T13 on the session's collection.
fn tag_tree(session: &mut Session<'_>, _request: &ObservationRequest) -> Result<Value, ExecError> {
    let reader = session
        .storage
        .collection()
        .ok_or_else(|| core("no collection"))?
        .reader()?;
    let tree = tags::read_tree(&*reader).map_err(core)?;
    Ok(tag_tree_value(&tree))
}
