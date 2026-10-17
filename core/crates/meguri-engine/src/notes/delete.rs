use crate::undo::names::DELETE_NOTE;
use crate::{Collection, EngineError, OpResult, Precondition};
use meguri_model::{changes::OpSource, ids::NoteId};
use meguri_storage::{ReadRows, RowKey, StorageError, Table, WriteScope};
use std::{collections::HashSet, path::Path};

/// `None` means no requested note exists: no op runs and history stays.
pub fn delete(
    collection: &Collection,
    ids: Vec<NoteId>,
    source: OpSource,
) -> Result<Option<OpResult<usize>>, EngineError> {
    for attempt in 0..=crate::executor::plan_commit::REPLANS {
        let candidate = {
            let reader = collection.reader(meguri_storage::ReaderRole::Query)?;
            let mut candidate = None;
            for &id in &ids {
                if let Some(digest) = reader.row_digest(Table::Notes, &RowKey::Id(id.0))? {
                    candidate = Some((id, digest));
                    break;
                }
            }
            candidate
        };
        let Some((id, digest)) = candidate else { return Ok(None) };
        // A concurrent deletion must not turn this into a committed no-op
        // that clears redo (record-ops W3). Retry before any history effect.
        let spec = super::op_spec(DELETE_NOTE, source, ids.len()).precondition(Precondition::RowDigest {
            table: Table::Notes,
            key: RowKey::Id(id.0),
            digest: Some(digest),
        });
        let path = collection.path().to_owned();
        let requested = ids.clone();
        match collection.run(spec, move |context| delete_in(context.scope(), &path, &requested)) {
            Err(EngineError::Stale { .. }) if attempt < crate::executor::plan_commit::REPLANS => continue,
            result => return result.map(Some),
        }
    }
    unreachable!("the final attempt returns its result")
}

pub fn delete_in(scope: &mut WriteScope<'_>, collection: &Path, ids: &[NoteId]) -> Result<usize, EngineError> {
    let mut seen = HashSet::new();
    let mut notes = Vec::new();
    for &id in ids {
        if seen.insert(id)
            && let Some(note) = scope.note(id)?
        {
            notes.push(note);
        }
    }
    super::deleted_log::append(collection, &notes)
        .map_err(|e| EngineError::Storage(StorageError::Io { detail: e.to_string() }))?;
    for note in &notes {
        scope.remove_note_and_cards(note.id)?;
    }
    Ok(notes.len())
}
