//! Change Note Type (`specs/notetypes/change-notetype.md` M1 to M11): the
//! info query (M5) with its default maps (M2, M3) and loss preview (M6), and
//! the change itself (M8, M9) as one transaction and one "Change Note Type"
//! undo step.
//!
//! With the schema stamp, the change carries the `LayoutToken` that the
//! info step returned. A change made on another layout is
//! `schema_changed`, and its maps are checked against the layout shown.
//! It plans on a reader snapshot: checks 1 to 4 of M8 and the confirmed
//! losses counted on that snapshot, so a stale screen or an
//! out-of-date confirmation costs no safety backup. The safety backup of a
//! structural edit (`package/backup-triggers` T6.1, T6.2) is taken between
//! planning and the write phase (`commit.rs`), whose checks on the live rows
//! of its own transaction all come before its first write: the schema stamp
//! and the layout (check 3), the selection (check 4, a note storing more
//! values than the old note type has fields included), then the confirmed losses counted
//! again (M8 as changed 2026-11-20, `notetypes/draft-save` D8). A larger
//! live count refuses with `LossesChanged` and a fresh M6 preview; the
//! staleness refusals come first and win.

mod commit;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use meguri_model::changes::OpSource;
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, Table};

use super::NotetypeKind;
use super::structure::{Losses, StructureRefusal};
use crate::executor::catalog;
use crate::undo::entry::StepKind;
use crate::undo::names::CHANGE_NOTE_TYPE;
use crate::{CancelToken, Collection, EngineError, OpResult, OpSpec, Plan, ProgressSink, SchemaReason};

/// What the user confirmed would be lost, or the live counts of the same
/// categories (M8, `notetypes/draft-save` D8): one count per category, each
/// compared on its own. A category absent here was confirmed as 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeLosses {
    /// Per discarded old field ordinal: the selected notes whose stored text
    /// for that field is not the empty string (M5's count, so `<br>` and a
    /// single space count; not `text/field-emptiness` E1).
    pub fields: BTreeMap<u32, u64>,
    /// Per dropped old template ordinal: the selected notes' cards of it.
    pub templates: BTreeMap<u32, u64>,
    /// The cards M9.1 deletes without a template at a renumbering target,
    /// plus the cards M9.2 deletes (cloze to standard): M6's removed cards
    /// less the dropped templates' cards.
    pub other_cards: u64,
}

impl ChangeLosses {
    /// D8: whether any live count is larger than its own confirmed count.
    /// Never a total; a count that shrinks or stays equal does not refuse.
    pub fn exceeds(&self, confirmed: &ChangeLosses) -> bool {
        let over = |live: &BTreeMap<u32, u64>, shown: &BTreeMap<u32, u64>| {
            live.iter().any(|(ord, count)| *count > shown.get(ord).copied().unwrap_or(0))
        };
        over(&self.fields, &confirmed.fields)
            || over(&self.templates, &confirmed.templates)
            || self.other_cards > confirmed.other_cards
    }
}

/// M6: what the selected notes would lose under one field map and template
/// map, with the counts behind it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangePreview {
    /// The old fields no field-map entry names, ascending.
    pub discarded_fields: Vec<u32>,
    /// With a template map, the old templates no entry names, ascending;
    /// `None` without a template map.
    pub dropped_templates: Option<Vec<u32>>,
    /// How many cards of the selected notes M9 deletes.
    pub removed_cards: u64,
    /// The confirmed-losses categories of M8, counted: what a caller passes
    /// back as `ChangeNotetype::confirmed` once the user accepted this.
    pub losses: ChangeLosses,
}

/// One note type of a change as M5 reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotetypeSide {
    pub id: NotetypeId,
    /// The stored name.
    pub name: String,
    pub kind: NotetypeKind,
    /// Field names in ordinal order.
    pub fields: Vec<String>,
    /// Template names in ordinal order (a cloze note type has its one).
    pub templates: Vec<String>,
}

impl NotetypeSide {
    fn of(notetype: &Notetype) -> NotetypeSide {
        NotetypeSide {
            id: notetype.id,
            name: notetype.name.clone(),
            kind: NotetypeKind::of(notetype),
            fields: notetype.fields.iter().map(|f| f.name.clone()).collect(),
            templates: notetype.templates.iter().map(|t| t.name.clone()).collect(),
        }
    }
}

/// M5's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeInfo {
    pub old: NotetypeSide,
    pub new: NotetypeSide,
    /// M2's default field map: per new field, an old field ordinal or none.
    pub fields: Vec<Option<u32>>,
    /// M3's default template map, `None` when either note type is cloze
    /// (M1).
    pub templates: Option<Vec<Option<u32>>>,
    /// M6 for the two default maps.
    pub preview: ChangePreview,
    /// Per old field ordinal: the selected notes whose stored text for it is
    /// not the empty string.
    pub field_content: Vec<u64>,
    /// Per ordinal at least one card of the selected notes has, ascending:
    /// the number of those cards (ordinals without a template included).
    pub card_counts: BTreeMap<u16, u64>,
    /// `col.scm` as read: M8 check 3's stamp.
    pub schema: i64,
    /// The two note types' layouts as read, which the change takes back with
    /// the stamp: a change made on another layout is `schema_changed`.
    pub layout: LayoutToken,
}

impl ChangeInfo {
    /// M6 for any mapping the user edits, from this one result (R947): the
    /// counts do not depend on the mapping (M5). Entries the note types do
    /// not have are ignored here; the change refuses them (M8 check 1).
    pub fn preview(&self, fields: &[Option<u32>], templates: Option<&[Option<u32>]>) -> ChangePreview {
        let shape = Shape {
            old_fields: self.old.fields.len(),
            old_templates: self.old.templates.len(),
            old_cloze: self.old.kind == NotetypeKind::Cloze,
            new_fields: self.new.fields.len(),
            new_templates: self.new.templates.len(),
            new_cloze: self.new.kind == NotetypeKind::Cloze,
        };
        let census = Census {
            field_content: self.field_content.clone(),
            card_counts: self.card_counts.clone(),
        };
        // Keep only the entries the new note type has, naming parts the old
        // one has: the preview of any other mapping is that of its valid part.
        let within = |map: &[Option<u32>], len: usize, old: usize| -> Vec<Option<u32>> {
            map.iter()
                .take(len)
                .map(|o| o.filter(|o| (*o as usize) < old))
                .collect()
        };
        let fields = within(fields, shape.new_fields, shape.old_fields);
        let templates = templates.map(|map| within(map, shape.new_templates, shape.old_templates));
        preview(&shape, &census, &fields, templates.as_deref())
    }
}

/// M8's input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeNotetype {
    /// The selection (M4): a note listed twice counts once, at its first
    /// position.
    pub notes: Vec<NoteId>,
    /// The old note type the info query reported.
    pub old_notetype: NotetypeId,
    pub new_notetype: NotetypeId,
    /// Per new field, an old field ordinal or none (M1).
    pub fields: Vec<Option<u32>>,
    /// Per new template, an old template ordinal or none; present exactly
    /// when both note types are standard (M1).
    pub templates: Option<Vec<Option<u32>>>,
    /// The info query's `col.scm` (M8 check 3).
    pub schema: i64,
    /// The info query's `layout`: the maps are checked against it.
    pub layout: LayoutToken,
    /// What the M6 preview showed and the user accepted; a caller with no
    /// sheet passes the counts it reads at the call (M8, D8).
    pub confirmed: ChangeLosses,
}

/// M9's result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotetypeChanged {
    /// The notes of the selection, duplicates counted once.
    pub notes: u64,
}

/// The two note types' shapes, which every rule of M1, M6 and M9 reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub old_fields: usize,
    pub old_templates: usize,
    pub old_cloze: bool,
    pub new_fields: usize,
    pub new_templates: usize,
    pub new_cloze: bool,
}

impl Shape {
    fn of(old: &Notetype, new: &Notetype) -> Shape {
        Shape {
            old_fields: old.fields.len(),
            old_templates: old.templates.len(),
            old_cloze: NotetypeKind::of(old) == NotetypeKind::Cloze,
            new_fields: new.fields.len(),
            new_templates: new.templates.len(),
            new_cloze: NotetypeKind::of(new) == NotetypeKind::Cloze,
        }
    }

    /// M1: a template map exactly when both note types are standard.
    fn has_template_map(&self) -> bool {
        !self.old_cloze && !self.new_cloze
    }
}

/// What M9 step 2 does to a card, by its ordinal before the change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardEffect {
    Keep,
    /// Renumbered to this ordinal and stamped (M9.1).
    Move(u16),
    /// Deleted with its card grave: `Some(ord)` for a dropped template's
    /// card, `None` for the other category of M8 (M9.1's card without a
    /// template at a renumbering target, M9.2's card).
    Remove(Option<u32>),
}

/// M9 step 2 for one card ordinal.
pub(crate) fn card_effect(shape: &Shape, templates: Option<&[Option<u32>]>, ord: u16) -> CardEffect {
    let ordinal = u32::from(ord);
    if let Some(map) = templates {
        if usize::from(ord) < shape.old_templates {
            // M9.1: a dropped template's card goes; a named one moves to the
            // position of the entry that names it.
            return match map.iter().position(|entry| *entry == Some(ordinal)) {
                None => CardEffect::Remove(Some(ordinal)),
                Some(at) if at == usize::from(ord) => CardEffect::Keep,
                Some(at) => CardEffect::Move(u16::try_from(at).unwrap_or(u16::MAX)),
            };
        }
        // A card without a template: deleted when the entry at its ordinal
        // names an old template, since a renumbered card may take it
        // (decided 2026-10-27, data safety); kept untouched otherwise.
        return match map.get(usize::from(ord)) {
            Some(Some(_)) => CardEffect::Remove(None),
            _ => CardEffect::Keep,
        };
    }
    if shape.old_cloze && !shape.new_cloze && usize::from(ord) >= shape.new_templates {
        // M9.2.
        return CardEffect::Remove(None);
    }
    // M9.3, and cloze to cloze: cards keep their ordinals.
    CardEffect::Keep
}

/// M5's counts for a selection, on one set of rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Census {
    /// Per stored field position: notes whose text there is not empty.
    pub field_content: Vec<u64>,
    pub card_counts: BTreeMap<u16, u64>,
}

/// M6 from M5's counts.
pub(crate) fn preview(
    shape: &Shape,
    census: &Census,
    fields: &[Option<u32>],
    templates: Option<&[Option<u32>]>,
) -> ChangePreview {
    let discarded_fields: Vec<u32> = (0..shape.old_fields)
        .map(ordinal)
        .filter(|o| !fields.contains(&Some(*o)))
        .collect();
    let dropped_templates: Option<Vec<u32>> = templates.map(|map| {
        (0..shape.old_templates)
            .map(ordinal)
            .filter(|o| !map.contains(&Some(*o)))
            .collect()
    });
    let mut losses = ChangeLosses::default();
    for &field in &discarded_fields {
        let count = census.field_content.get(field as usize).copied().unwrap_or(0);
        losses.fields.insert(field, count);
    }
    for &template in dropped_templates.iter().flatten() {
        losses.templates.insert(template, 0);
    }
    let mut removed_cards = 0;
    for (&ord, &count) in &census.card_counts {
        match card_effect(shape, templates, ord) {
            CardEffect::Remove(Some(template)) => {
                *losses.templates.entry(template).or_default() += count;
                removed_cards += count;
            }
            CardEffect::Remove(None) => {
                losses.other_cards += count;
                removed_cards += count;
            }
            CardEffect::Keep | CardEffect::Move(_) => {}
        }
    }
    ChangePreview {
        discarded_fields,
        dropped_templates,
        removed_cards,
        losses,
    }
}

fn ordinal(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

/// M2 (and M3 on template names): exact names first, then the unused old
/// ones in ascending ordinal to the new ones still without one, in new
/// order; the rest get none.
pub(crate) fn default_map(old: &[String], new: &[String]) -> Vec<Option<u32>> {
    let mut used = vec![false; old.len()];
    let mut map: Vec<Option<u32>> = new
        .iter()
        .map(|name| {
            let at = old.iter().enumerate().position(|(at, o)| o == name && !used[at])?;
            used[at] = true;
            Some(ordinal(at))
        })
        .collect();
    let mut unused = (0..old.len()).filter(|at| !used[*at]);
    for entry in map.iter_mut().filter(|entry| entry.is_none()) {
        match unused.next() {
            Some(at) => *entry = Some(ordinal(at)),
            None => break,
        }
    }
    map
}

/// The selection rules a census enforces: M4's at the info query, M8
/// check 4's at the change.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Rules {
    /// M4: the first listed note must exist (`not_found`); every other one
    /// must exist and share its note type (`multiple_notetypes`).
    Info,
    /// M8 check 4: every listed note exists (`not_found`, wherever it is
    /// listed), then every one uses `old` (`multiple_notetypes`), then none
    /// stores more values than `old` has fields (`fields`): the change would
    /// drop those values uncounted (`NoteFieldCountMismatch`, as the draft
    /// save's S1.2 item 4).
    Change { old: NotetypeId, fields: usize },
}

pub(crate) fn note_not_found(id: NoteId) -> EngineError {
    EngineError::NotFound {
        table: Table::Notes,
        key: id.0.to_string(),
    }
}

pub(crate) fn notetype_not_found(id: NotetypeId) -> EngineError {
    EngineError::NotFound {
        table: Table::Notetypes,
        key: id.0.to_string(),
    }
}

/// One pass over the selection on `reader`'s rows: the selection rules,
/// then M5's counts. Returns the first note's note type.
pub(crate) fn census(
    reader: &impl ReadRows,
    notes: &[NoteId],
    rules: Rules,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<(NotetypeId, Census), EngineError> {
    let mut census = Census::default();
    let mut first = None;
    let mut other_notetype = false;
    let mut too_many_values = None;
    let total = notes.len() as u64;
    progress.report(0, Some(total));
    for (at, &id) in notes.iter().enumerate() {
        if at % 256 == 0 {
            cancel.check()?;
        }
        let Some(note) = reader.note(id)? else {
            return Err(match rules {
                Rules::Info if at > 0 => StructureRefusal::MultipleNotetypes.into(),
                _ => note_not_found(id),
            });
        };
        let expected = match rules {
            Rules::Info => *first.get_or_insert(note.notetype_id),
            Rules::Change { old, .. } => {
                first.get_or_insert(note.notetype_id);
                old
            }
        };
        if let Rules::Change { fields, .. } = rules
            && note.notetype_id == expected
            && too_many_values.is_none()
            && note.field_values().count() > fields
        {
            too_many_values = Some(note.id);
        }
        if note.notetype_id != expected {
            match rules {
                Rules::Info => return Err(StructureRefusal::MultipleNotetypes.into()),
                // Existence is checked for every note first (M8 check 4).
                Rules::Change { .. } => other_notetype = true,
            }
        }
        for (position, value) in note.field_values().enumerate() {
            if census.field_content.len() <= position {
                census.field_content.resize(position + 1, 0);
            }
            if !value.is_empty() {
                census.field_content[position] += 1;
            }
        }
        for card in reader.cards_of_note(id)? {
            *census.card_counts.entry(card.ord).or_default() += 1;
        }
        progress.report(at as u64 + 1, Some(total));
    }
    if other_notetype {
        return Err(StructureRefusal::MultipleNotetypes.into());
    }
    if let Some(note) = too_many_values {
        return Err(StructureRefusal::NoteFieldCountMismatch { note }.into());
    }
    let first = first.ok_or_else(|| invalid("the selection is empty"))?;
    Ok((first, census))
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument {
        message: message.into(),
    }
}

/// M4: a note listed twice counts once, at its first position.
fn deduplicated(notes: &[NoteId]) -> Vec<NoteId> {
    let mut seen = std::collections::HashSet::new();
    notes.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// The info query (M5) on one snapshot of the query reader.
pub fn info(
    collection: &Collection,
    notes: &[NoteId],
    new_notetype: NotetypeId,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<ChangeInfo, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    info_on(&*snapshot, notes, new_notetype, cancel, progress)
}

/// M5 on `reader`'s rows.
pub fn info_on(
    reader: &impl ReadRows,
    notes: &[NoteId],
    new_notetype: NotetypeId,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<ChangeInfo, EngineError> {
    let notes = deduplicated(notes);
    if notes.is_empty() {
        return Err(invalid("Change Note Type needs at least one note (M4)"));
    }
    let schema = reader.col()?.schema_modified.0;
    let (old_id, mut census) = census(reader, &notes, Rules::Info, cancel, progress)?;
    let old = reader.notetype(old_id)?.ok_or_else(|| notetype_not_found(old_id))?;
    let new = reader
        .notetype(new_notetype)?
        .ok_or_else(|| notetype_not_found(new_notetype))?;
    census.field_content.resize(old.fields.len(), 0);
    let shape = Shape::of(&old, &new);
    let old_side = NotetypeSide::of(&old);
    let new_side = NotetypeSide::of(&new);
    let fields = default_map(&old_side.fields, &new_side.fields);
    let templates = shape
        .has_template_map()
        .then(|| default_map(&old_side.templates, &new_side.templates));
    let preview = preview(&shape, &census, &fields, templates.as_deref());
    Ok(ChangeInfo {
        old: old_side,
        new: new_side,
        fields,
        templates,
        preview,
        field_content: census.field_content,
        card_counts: census.card_counts,
        schema,
        layout: LayoutToken::of(&old, &new),
    })
}

/// M8 check 1 after the note types are found: the maps' shape.
fn check_maps(shape: &Shape, request: &ChangeNotetype) -> Result<(), EngineError> {
    if request.fields.len() != shape.new_fields {
        return Err(invalid(format!(
            "the field map has {} entries for {} new fields",
            request.fields.len(),
            shape.new_fields
        )));
    }
    if let Some(origin) = request.fields.iter().flatten().find(|o| **o as usize >= shape.old_fields) {
        return Err(invalid(format!("the field map names old field {origin}, which does not exist")));
    }
    match (&request.templates, shape.has_template_map()) {
        (None, false) => Ok(()),
        (Some(_), false) => Err(invalid("a change to or from a cloze note type has no template map (M1)")),
        (None, true) => Err(invalid("a change between two standard note types needs a template map (M1)")),
        (Some(map), true) => {
            if map.len() != shape.new_templates {
                return Err(invalid(format!(
                    "the template map has {} entries for {} new templates",
                    map.len(),
                    shape.new_templates
                )));
            }
            let mut seen = std::collections::HashSet::new();
            for &origin in map.iter().flatten() {
                if origin as usize >= shape.old_templates {
                    return Err(invalid(format!(
                        "the template map names old template {origin}, which does not exist"
                    )));
                }
                if !seen.insert(origin) {
                    return Err(invalid(format!("the template map names old template {origin} twice")));
                }
            }
            Ok(())
        }
    }
}

/// M7: the same note type with identity maps changes nothing.
fn changes_nothing(request: &ChangeNotetype) -> bool {
    let identity = |map: &[Option<u32>]| map.iter().enumerate().all(|(at, o)| *o == Some(ordinal(at)));
    request.old_notetype == request.new_notetype
        && identity(&request.fields)
        && request.templates.as_deref().is_none_or(identity)
}

pub(crate) fn losses_changed(preview: ChangePreview) -> EngineError {
    StructureRefusal::LossesChanged(Box::new(Losses::Change(preview))).into()
}

/// What planning hands the write phase.
#[derive(Clone, Debug)]
pub(crate) struct Planned {
    pub request: Arc<ChangeNotetype>,
    pub shape: Shape,
}

/// The two note types' layouts as one token: their kinds, sort fields and
/// parts by position (each field's and template's id, or its name for a
/// legacy part without one). Plain saves (CSS, options, renames of parts
/// with ids) leave it alone; a reorder, add or remove changes it. Compared
/// with the stamp because `col.scm` is the clock's millisecond: a second
/// schema change in the same millisecond leaves the stamp equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutToken(pub [u8; 32]);

impl LayoutToken {
    pub fn of(old: &Notetype, new: &Notetype) -> LayoutToken {
        use meguri_storage::Cell;
        let mut cells = Vec::new();
        for notetype in [old, new] {
            cells.push(Cell::Integer(notetype.id.0));
            cells.push(Cell::Integer(i64::from(notetype.config.kind)));
            cells.push(Cell::Integer(i64::from(notetype.config.sort_field_idx)));
            for parts in [
                notetype
                    .fields
                    .iter()
                    .map(|f| (f.config.id, f.name.as_str()))
                    .collect::<Vec<_>>(),
                notetype
                    .templates
                    .iter()
                    .map(|t| (t.config.id, t.name.as_str()))
                    .collect(),
            ] {
                cells.push(Cell::Integer(i64::try_from(parts.len()).unwrap_or(i64::MAX)));
                for (id, name) in parts {
                    cells.push(id.map_or_else(|| Cell::text(name.to_owned()), Cell::Integer));
                }
            }
        }
        LayoutToken(meguri_storage::journal::digest(&cells))
    }
}

/// M8 checks 1 to 4 and the first count of the confirmed losses, on the
/// planning snapshot, in M8's order.
fn plan(
    reader: &impl ReadRows,
    request: &ChangeNotetype,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Shape, EngineError> {
    cancel.check()?;
    // Check 1.
    let old = reader
        .notetype(request.old_notetype)?
        .ok_or_else(|| notetype_not_found(request.old_notetype))?;
    let new = reader
        .notetype(request.new_notetype)?
        .ok_or_else(|| notetype_not_found(request.new_notetype))?;
    // The maps are the user's on the layout the info query read: a layout
    // that changed since is `schema_changed`, never a caller error (as
    // `notetypes/structure-changes` S1.2 orders `Stale` before the ranges).
    if LayoutToken::of(&old, &new) != request.layout {
        return Err(StructureRefusal::SchemaChanged.into());
    }
    let shape = Shape::of(&old, &new);
    check_maps(&shape, request)?;
    // R945's repeated field map between two note types is not built: the
    // structure report refuses a repeated origin in a `type_changes` map.
    // Refused here, before any backup or write.
    if request.old_notetype != request.new_notetype {
        let mut seen = std::collections::HashSet::new();
        if request.fields.iter().flatten().any(|o| !seen.insert(*o)) {
            return Err(EngineError::Unsupported {
                feature: "change_notetype.repeated_field_between_note_types".into(),
            });
        }
    }
    // Check 2.
    if changes_nothing(request) {
        return Err(StructureRefusal::NoChanges.into());
    }
    // Checks 3 and 4, first run: a stale screen costs no backup.
    if reader.col()?.schema_modified.0 != request.schema {
        return Err(StructureRefusal::SchemaChanged.into());
    }
    let rules = Rules::Change {
        old: old.id,
        fields: shape.old_fields,
    };
    let (_, census) = census(reader, &request.notes, rules, cancel, progress)?;
    // The confirmed losses, first count: an out-of-date confirmation costs
    // no backup either (D8).
    let preview = preview(&shape, &census, &request.fields, request.templates.as_deref());
    if preview.losses.exceeds(&request.confirmed) {
        return Err(losses_changed(preview));
    }
    Ok(shape)
}

/// The change (M8, M9): plans on a maintenance snapshot, takes the safety
/// backup of a structural edit (M8 check 5), then writes in one transaction
/// and one "Change Note Type" step. Never planned again: every refusal is
/// returned to the caller.
pub fn change(
    collection: &Collection,
    request: ChangeNotetype,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<NotetypeChanged>, EngineError> {
    let mut request = request;
    request.notes = deduplicated(&request.notes);
    // Check 1's first part: a caller error.
    if request.notes.is_empty() {
        return Err(invalid("Change Note Type needs at least one note (M4)"));
    }
    let request = Arc::new(request);
    let spec = OpSpec::new(catalog::CHANGE_NOTETYPE, OpSource::UserUndoable)
        .undo_step(CHANGE_NOTE_TYPE, StepKind::Other)
        .cancel(cancel.clone())
        .progress(progress.clone());
    crate::plan_and_commit_with_replans(
        collection,
        ReaderRole::Maintenance,
        spec,
        0,
        |context| {
            let shape = plan(&**context.snapshot(), &request, &cancel, &progress)?;
            Ok(Plan {
                value: Planned {
                    request: request.clone(),
                    shape,
                },
                // Checks 3 and 4 run again on the live rows inside the write
                // transaction, and only that run decides.
                preconditions: Vec::new(),
            })
        },
        // G2 `notes_notetype_changed`: always structural (G3), so the safety
        // backup is taken (T6.1); every change rewrites the selected notes.
        |spec, _| {
            spec.clone()
                .schema_changes([SchemaReason::NotesNotetypeChanged])
                .declare_long_write(Duration::from_secs(1))
        },
        |context, planned| commit::commit(context, &planned),
    )
}
