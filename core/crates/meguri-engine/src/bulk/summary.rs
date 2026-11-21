//! `selection_summary` (`specs/browser/bulk-framework.md`;
//! `browser/find-replace` F3): the field names Find and Replace's In list
//! offers for a target, with the target's note types and how many of its
//! notes exist. A read: it never writes.

use std::collections::BTreeSet;

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, StorageError, Table};
use meguri_text::fold::{compare_folded, folded};

use super::{NoteTarget, ResolvedNotes};
use crate::collection::Collection;
use crate::error::EngineError;

/// Notes read per statement.
const PAGE: usize = 2_000;

/// What a target holds, for the sheets' field lists and enablement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionSummary {
    /// F3: one entry per name under `unicase`, spelled as the note type with
    /// the lowest id spells it, in `unicase` order (code points among names
    /// `unicase` finds equal).
    pub field_names: Vec<String>,
    /// The note types of the notes that exist, ascending; every note type
    /// for the all-notes target.
    pub notetypes: Vec<NotetypeId>,
    /// The target's notes that exist.
    pub notes: u64,
}

/// F3 for `target`, from one read transaction on the query reader. For the
/// all-notes target the field names are those of every note type of the
/// collection (Meguri's listing, F3, F13).
pub fn selection_summary(collection: &Collection, target: &NoteTarget) -> Result<SelectionSummary, EngineError> {
    let target = target.resolve(collection)?;
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    Ok(summary_in(&*snapshot, &target)?)
}

/// [`selection_summary`] on a reader the caller holds.
pub fn summary_in(reader: &(impl ReadRows + ?Sized), target: &ResolvedNotes) -> Result<SelectionSummary, StorageError> {
    let (ids, notes) = match target {
        ResolvedNotes::AllNotes => (
            crate::notetypes::read::ids(reader)?,
            reader.row_count(Table::Notes)?,
        ),
        ResolvedNotes::Notes(listed) => {
            let mut unique = listed.clone();
            unique.sort_unstable();
            unique.dedup();
            let mut ids = BTreeSet::new();
            let mut notes = 0;
            for chunk in unique.chunks(PAGE) {
                for note in reader.notes_with_ids(chunk)? {
                    notes += 1;
                    ids.insert(note.notetype_id);
                }
            }
            (ids.into_iter().collect(), notes)
        }
    };
    // Step 1: a note whose note type does not exist adds nothing.
    let mut notetypes = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(notetype) = reader.notetype(id)? {
            notetypes.push(notetype);
        }
    }
    Ok(SelectionSummary {
        field_names: field_names(&notetypes),
        notetypes: notetypes.iter().map(|n| n.id).collect(),
        notes,
    })
}

/// F3 steps 2 to 4 over note types listed by ascending id.
pub fn field_names(notetypes: &[Notetype]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut names: Vec<String> = Vec::new();
    for notetype in notetypes {
        for field in &notetype.fields {
            if seen.insert(folded(&field.name)) {
                names.push(field.name.clone());
            }
        }
    }
    names.sort_by(|a, b| compare_folded(a, b).then_with(|| a.cmp(b)));
    names
}
