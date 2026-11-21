//! The reads the Note Types screens make (U267): the list's order and note
//! counts (`notetypes/editor-actions` A2), the notes with content of each
//! field (`notetypes/workspace` W3.1), the near name of an unknown field
//! (W7.1), the notes a draft's fronts give no card (W8) and the delete
//! question's counts (W12). The draft save's summary (`notetypes/draft-save`
//! D2) is [`super::structure::draft_summary`].
//!
//! Each read takes one snapshot and writes nothing. The passes over a
//! type's notes check their cancel token and report progress (notes read
//! out of the type's notes). "Notes with content" is
//! `text/field-emptiness` E1 on the stored value, the test the draft save
//! counts its losses with (`plan::has_content`): `<br>` alone is empty.

use std::collections::{BTreeMap, BTreeSet};

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_template::ParsedTemplate;

use super::NotetypeKind;
use super::structure::StructureDraft;
use super::structure::plan::{self, Layout};
use crate::{CancelToken, Collection, EngineError, ProgressSink};

/// Notes read per page.
const PAGE: usize = 500;

fn not_found(id: NotetypeId) -> EngineError {
    EngineError::NotFound {
        table: Table::Notetypes,
        key: id.0.to_string(),
    }
}

/// A2's ordered list: every note type's id and stored name, by name under
/// the `unicase` comparison of the `notetypes.name` column, ascending id
/// breaking a tie.
pub fn ordered_names(reader: &impl ReadRows) -> Result<Vec<(NotetypeId, String)>, EngineError> {
    Ok(reader.notetype_names_by_name()?)
}

/// A2's note counts: every note type's id with the number of notes whose
/// `mid` is that id (0 for a note type without notes). A note whose `mid`
/// names no note type is counted nowhere.
pub fn note_counts(reader: &impl ReadRows) -> Result<BTreeMap<NotetypeId, u64>, EngineError> {
    let mut counts: BTreeMap<NotetypeId, u64> = super::read::ids(reader)?.into_iter().map(|id| (id, 0)).collect();
    for (id, notes) in reader.note_counts_by_notetype()? {
        if let Some(count) = counts.get_mut(&id) {
            *count = notes;
        }
    }
    Ok(counts)
}

/// [`note_counts`] on one snapshot of the collection.
pub fn list_note_counts(collection: &Collection) -> Result<BTreeMap<NotetypeId, u64>, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    note_counts(&*snapshot)
}

/// W3.1's result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldContentCounts {
    /// The type's notes.
    pub notes: u64,
    /// For each stored field ordinal, in order: the notes of the type whose
    /// value at that ordinal is present and not empty (E1).
    pub fields: Vec<u64>,
}

/// W3.1 on `reader`'s rows. A note with fewer values than the type has
/// fields counts for none of the missing ones; values past the type's
/// fields (a damaged note) count for nothing.
pub fn field_content_counts_on(
    reader: &impl ReadRows,
    id: NotetypeId,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<FieldContentCounts, EngineError> {
    cancel.check()?;
    let notetype = reader.notetype(id)?.ok_or_else(|| not_found(id))?;
    let total = reader.notetype_note_count(id)?;
    progress.report(0, Some(total));
    let mut counts = FieldContentCounts {
        notes: 0,
        fields: vec![0; notetype.fields.len()],
    };
    let mut after = None;
    loop {
        cancel.check()?;
        let page = reader.notetype_notes_after(id, after, PAGE)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in page {
            let values: Vec<&str> = note.field_values().collect();
            for (ord, count) in counts.fields.iter_mut().enumerate() {
                *count += u64::from(plan::has_content(&values, super::structure::position(ord)));
            }
            counts.notes += 1;
        }
        progress.report(counts.notes, Some(total));
    }
    Ok(counts)
}

/// W3.1's `field_content_counts(notetype id)` on one snapshot.
pub fn field_content_counts(
    collection: &Collection,
    id: NotetypeId,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<FieldContentCounts, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    field_content_counts_on(&*snapshot, id, cancel, progress)
}

/// W12's counts: the note type's notes and their cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemovalCounts {
    pub notes: u64,
    pub cards: u64,
}

/// W12's question on one snapshot: the counts the removal compares its
/// confirmed counts with (`notetypes/structure-changes` S12), read by the
/// removal's own census. `NotFound` when no note type has the id.
pub fn removal_counts(
    collection: &Collection,
    id: NotetypeId,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<RemovalCounts, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let (notes, cards, _) = super::remove::census(&*snapshot, id, cancel, progress)?.ok_or_else(|| not_found(id))?;
    Ok(RemovalCounts { notes, cards })
}

/// W7.1: the position in `fields` of the name suggested for the unknown
/// field name `unknown`: the first field whose name equals it under simple
/// case folding, else the first at the smallest Levenshtein distance of at
/// most 2, counted in characters after both are simply case folded (so
/// `fornt` suggests `Front`); none otherwise. The caller shows the field's
/// stored spelling.
pub fn near_field_name(unknown: &str, fields: &[&str]) -> Option<usize> {
    let mapper = icu_casemap::CaseMapper::new();
    let fold = |text: &str| -> Vec<char> { text.chars().map(|ch| mapper.simple_fold(ch)).collect() };
    let folded = fold(unknown);
    if let Some(at) = fields.iter().position(|name| fold(name) == folded) {
        return Some(at);
    }
    let mut best: Option<(usize, usize)> = None;
    for (at, name) in fields.iter().enumerate() {
        let distance = levenshtein(&folded, &fold(name), 2);
        if distance <= 2 && best.is_none_or(|(_, shortest)| distance < shortest) {
            best = Some((at, distance));
        }
    }
    best.map(|(at, _)| at)
}

/// The edit distance between `a` and `b`, or a value above `bound` once
/// it is known to exceed it.
fn levenshtein(a: &[char], b: &[char], bound: usize) -> usize {
    if a.len().abs_diff(b.len()) > bound {
        return bound + 1;
    }
    let mut costs: Vec<usize> = (0..=b.len()).collect();
    for (i, left) in a.iter().enumerate() {
        let mut corner = costs[0];
        costs[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let replaced = corner + usize::from(left != right);
            corner = costs[j + 1];
            costs[j + 1] = replaced.min(costs[j] + 1).min(corner + 1);
        }
    }
    costs[b.len()]
}

/// W8 for one card type of the draft: the notes its front gives no card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlankFront {
    /// Every such note, ascending.
    pub notes: Vec<NoteId>,
    /// The position in the draft's fields of the one field that decides it,
    /// for W8's reason line: the front refers to that field and to no other
    /// name that can be non-empty (W1's Used On references, T11.1), and it
    /// is empty in every counted note. None otherwise, and when no note is
    /// counted.
    pub field: Option<u32>,
}

/// The one field of `saved` that `front` needs alone: its T11.1 references,
/// leaving out names that are never non-empty for a note (`FrontSide` and
/// names that are no field or special name, `notes/card-generation` G2),
/// are exactly that field, and the front renders something when that
/// field alone is non-empty (so filling it would give the card).
fn sole_field(front: &ParsedTemplate, saved: &Notetype) -> Option<u32> {
    if front.structure_error().is_some() {
        return None;
    }
    let field = |name: &str| saved.fields.iter().position(|f| f.name == name);
    let mut needed: BTreeSet<Result<usize, &str>> = BTreeSet::new();
    for name in front.field_references() {
        match field(name) {
            Some(at) => {
                needed.insert(Ok(at));
            }
            None if name == "Tags" || crate::cardgen::normal::ALWAYS_NON_EMPTY.contains(&name) => {
                needed.insert(Err(name));
            }
            None => {}
        }
    }
    let [Ok(at)] = needed.into_iter().collect::<Vec<_>>()[..] else {
        return None;
    };
    let name = saved.fields[at].name.as_str();
    front
        .renders_something(|key| key == name, meguri_template::Form::Strict)
        .then(|| super::structure::position(at))
}

/// W8 on `reader`'s rows: for each card type of the draft in its order, the
/// notes of the type for which its front, as the save would store it, does
/// not render something by the strict analysis (`render/template-syntax`
/// T11.2) with the note's non-empty names (`notes/card-generation` G2),
/// the note's values as the save would leave them: G3's test, which the
/// save's card generation applies (S7). A front that does not parse gives
/// no note a card. Empty for a cloze note type. Refuses as the draft's
/// planning does before it counts (`InvalidArgument`, `Stale`,
/// `invalid_notetype`, `NoteFieldCountMismatch`, `Cancelled`).
pub fn notes_without_card_on(
    reader: &impl ReadRows,
    draft: &StructureDraft,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Vec<BlankFront>, EngineError> {
    cancel.check()?;
    let base = plan::checked_base(reader, draft)?;
    let layout: Layout = plan::layout_of_draft(reader, base, draft)?;
    let saved = &layout.saved;
    if NotetypeKind::of(saved) == NotetypeKind::Cloze {
        return Ok(Vec::new());
    }
    let normalize_text = crate::notes::fields::normalize_enabled(reader)?;
    let fronts: Vec<ParsedTemplate> = crate::cardgen::normal::fronts(saved);
    let deciding: Vec<Option<u32>> = fronts.iter().map(|front| sole_field(front, saved)).collect();
    let mut blank: Vec<BlankFront> = vec![BlankFront::default(); fronts.len()];
    // Whether every counted note so far has the deciding field empty.
    let mut decided = vec![true; fronts.len()];
    let total = reader.notetype_note_count(layout.base.id)?;
    progress.report(0, Some(total));
    let mut done = 0;
    let mut after = None;
    loop {
        cancel.check()?;
        let page = reader.notetype_notes_after(layout.base.id, after, PAGE)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in page {
            // S5: the values the save would store, as its generation reads them.
            let note = if layout.rewrite_notes {
                plan::remap_note(&layout, &note, normalize_text)?
            } else {
                note
            };
            let required = crate::cardgen::normal::required_by(saved, &fronts, &note);
            let values: Vec<&str> = note.field_values().collect();
            for (at, front) in blank.iter_mut().enumerate() {
                let ord = u16::try_from(at).unwrap_or(u16::MAX);
                if required.contains(&ord) {
                    continue;
                }
                front.notes.push(note.id);
                if let Some(field) = deciding[at]
                    && plan::has_content(&values, field)
                {
                    decided[at] = false;
                }
            }
            done += 1;
        }
        progress.report(done, Some(total));
    }
    for ((front, field), decided) in blank.iter_mut().zip(deciding).zip(decided) {
        front.field = field.filter(|_| decided && !front.notes.is_empty());
    }
    Ok(blank)
}

/// W8's `notes_without_card(notetype, draft templates)` on one snapshot.
pub fn notes_without_card(
    collection: &Collection,
    draft: &StructureDraft,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Vec<BlankFront>, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    notes_without_card_on(&*snapshot, draft, cancel, progress)
}

#[cfg(test)]
mod tests {
    use super::{levenshtein, near_field_name};

    #[test]
    fn near_names_fold_case_first_then_take_the_nearest_within_two() {
        let fields = ["Front", "Back", "Extra"];
        assert_eq!(near_field_name("front", &fields), Some(0));
        assert_eq!(near_field_name("BACK", &fields), Some(1));
        assert_eq!(near_field_name("Fornt", &fields), Some(0));
        assert_eq!(near_field_name("Extr", &fields), Some(2));
        assert_eq!(near_field_name("Bak", &fields), Some(1));
        // Distance 3: none.
        assert_eq!(near_field_name("Xyz", &fields), None);
        assert_eq!(near_field_name("Frontal", &fields), Some(0));
        assert_eq!(near_field_name("Frontally", &fields), None);
        // Ties go to the first in field order.
        assert_eq!(near_field_name("Ab", &["Ac", "Ad"]), Some(0));
        // Equal under folding comes before any distance.
        assert_eq!(near_field_name("ab", &["ac", "AB"]), Some(1));
        // The distance is taken on folded text.
        assert_eq!(near_field_name("fornt", &fields), Some(0));
        assert_eq!(near_field_name("BAK", &fields), Some(1));
        assert_eq!(near_field_name("EXTRAS", &fields), Some(2));
        // Simple folding is per character: ß is not ss, so the names are
        // two edits apart, not equal.
        assert_eq!(near_field_name("STRASSE", &["Straße"]), Some(0));
        assert_eq!(near_field_name("STRASSEN", &["Straße"]), None);
        assert_eq!(near_field_name("\u{212A}ELVIN", &["kelvin"]), Some(0));
        assert_eq!(near_field_name("x", &[]), None);
    }

    #[test]
    fn levenshtein_counts_characters_not_bytes() {
        assert_eq!(levenshtein(&['é'], &['e'], 2), 1);
        assert_eq!(levenshtein(&[], &['a', 'b'], 2), 2);
        assert!(levenshtein(&['a'; 5], &[], 2) > 2);
    }
}
