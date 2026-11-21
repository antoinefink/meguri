//! The write phase of Change Note Type (M8's second run of checks 3 and 4,
//! M9): in one transaction, after the safety backup, the schema stamp, the
//! selection and the confirmed losses are checked again on the live rows;
//! only then does anything write. Every value written is re-derived from
//! the live rows (`engine/op-model` O7). Any error after the first write
//! rolls the whole transaction back.

use meguri_model::grave::GraveKind;
use meguri_model::notetype::Notetype;
use meguri_model::usn::Usn;
use meguri_storage::ReadRows;
use meguri_storage::journal::{LayoutShape, StructureReason, StructureReport, TypeChangeReport};

use super::{
    ChangeNotetype, CardEffect, NotetypeChanged, Planned, Rules, Shape, census, card_effect, losses_changed, note_not_found,
    notetype_not_found, preview,
};
use crate::executor::op::OpContext;
use crate::notetypes::NotetypeKind;
use crate::notetypes::structure::StructureRefusal;
use crate::{CancelToken, EngineError, ProgressSink, SchemaReason};

/// The checks of the write phase, on the rows of its own transaction and
/// before its first write, in M8's order: check 3 (the stamp), check 4 (the
/// selection), then the confirmed losses counted again (D8). Staleness wins
/// over `LossesChanged`, whose preview is read in this transaction.
/// Returns the two note types as the transaction reads them.
fn check_live(ctx: &mut OpContext<'_, '_>, planned: &Planned) -> Result<(Notetype, Notetype), EngineError> {
    let request = &planned.request;
    let scope = ctx.scope();
    if scope.col()?.schema_modified.0 != request.schema {
        return Err(StructureRefusal::SchemaChanged.into());
    }
    // Check 3 passing guarantees the note type records checks 1 and 2 read
    // (a field, template or kind change and a deletion set `scm`, G2); they
    // are read again so every value written comes from the live rows.
    let old = scope
        .notetype(request.old_notetype)?
        .ok_or_else(|| notetype_not_found(request.old_notetype))?;
    let new = scope
        .notetype(request.new_notetype)?
        .ok_or_else(|| notetype_not_found(request.new_notetype))?;
    if Shape::of(&old, &new) != planned.shape {
        // Only a write that changed a note type without setting `scm` could
        // get here; the maps were checked against other shapes.
        return Err(StructureRefusal::SchemaChanged.into());
    }
    let (_, census) = census(
        scope,
        &request.notes,
        Rules::Change { old: old.id },
        &CancelToken::new(),
        &ProgressSink::new(),
    )?;
    let live = preview(&planned.shape, &census, &request.fields, request.templates.as_deref());
    if live.losses.exceeds(&request.confirmed) {
        return Err(losses_changed(live));
    }
    Ok((old, new))
}

/// M10's structure report: reason 6, and between two note types the
/// `type_changes` entry with M1's maps (-1 for none) or K19.2's prefix map.
fn report(old: &Notetype, new: &Notetype, request: &ChangeNotetype) -> Result<StructureReport, EngineError> {
    let mut report = StructureReport::default();
    report.reasons.insert(StructureReason::NotesNotetypeChanged);
    // A change to the same note type moves no note's `mid`. M10 asks for an
    // entry with old id = new id, which the storage report refuses (one
    // entry per pair of different note types, selected by the notes whose
    // `mid` changed): it reports the reason alone until that is decided.
    if old.id != new.id {
        let map = |entries: &[Option<u32>]| -> Vec<i32> {
            entries
                .iter()
                .map(|e| e.map_or(-1, |o| i32::try_from(o).unwrap_or(-1)))
                .collect()
        };
        let template_map = match &request.templates {
            Some(templates) => map(templates),
            // K19.2 "No template map": empty from a cloze note type, the
            // identity of the old template count from a standard one.
            None if NotetypeKind::of(old) == NotetypeKind::Cloze => Vec::new(),
            None => (0..old.templates.len())
                .map(|at| i32::try_from(at).unwrap_or(i32::MAX))
                .collect(),
        };
        report.type_changes.insert(
            (old.id.0, new.id.0),
            TypeChangeReport {
                before: LayoutShape::of(old)?,
                after: LayoutShape::of(new)?,
                field_map: map(&request.fields),
                template_map,
            },
        );
    }
    Ok(report)
}

pub(super) fn commit(ctx: &mut OpContext<'_, '_>, planned: &Planned) -> Result<NotetypeChanged, EngineError> {
    let (old, new) = check_live(ctx, planned)?;
    let request = &planned.request;
    let progress = ctx.progress().cloned().unwrap_or_else(ProgressSink::new);
    let scope = ctx.scope();
    // Step 1: G5.
    crate::mark_schema_change(scope, &[SchemaReason::NotesNotetypeChanged])?;
    let token = scope.start_structure_report(report(&old, &new, request)?)?;
    let templates = request.templates.as_deref();
    // Step 2: the selected notes' cards. Deletions come before renumbering,
    // so a renumbered card never meets the card it replaces.
    for &id in &request.notes {
        let cards = scope.cards_of_note(id)?;
        for card in &cards {
            if let CardEffect::Remove(_) = card_effect(&planned.shape, templates, card.ord) {
                scope.remove_card(card.id)?;
                scope.add_grave(card.id.0, GraveKind::Card, Usn::PENDING)?;
                // Its id, grave and review log stay its own: step 3 never
                // gives the id to a new card.
                scope.retire_card_id(card.id);
            }
        }
        for card in &cards {
            if let CardEffect::Move(to) = card_effect(&planned.shape, templates, card.ord) {
                // Only `ord` and the stamps change; every other column,
                // deck and scheduling included, stays as stored.
                scope.put_card_ord(card.id, to)?;
            }
        }
    }
    // Step 3: each note in selection order.
    let total = request.notes.len() as u64;
    for (done, &id) in request.notes.iter().enumerate() {
        let mut note = scope.note(id)?.ok_or_else(|| note_not_found(id))?;
        let values: Vec<&str> = note.field_values().collect();
        let fields: Vec<String> = request
            .fields
            .iter()
            .map(|origin| {
                let value = origin.and_then(|o| values.get(o as usize).copied()).unwrap_or("");
                // Control characters other than TAB and LF go; no NFC,
                // whatever `normalize_note_text` holds (M9 step 3).
                meguri_text::normalize::field_as_saved(value, false).into_owned()
            })
            .collect();
        note.fields = fields.join("\u{1f}");
        note.notetype_id = new.id;
        note.sort_field =
            meguri_text::checksum::sort_text_of(&fields, new.config.sort_field_idx as usize).into_owned();
        note.checksum = meguri_text::checksum::note_checksum(fields.first().map_or("", String::as_str));
        // Stamped and written even when its row reads as before.
        scope.put_note_forced(&note)?;
        // G7 over the live cards, G6 for a note left with none; D3 item 3 is
        // the new note type's last deck; P2 draws per note.
        crate::cardgen::generate_in(scope, &new, &note, None)?;
        progress.report(done as u64 + 1, Some(total));
    }
    meguri_model::faultpoint!("notetypes.change.write_phase")
        .map_err(|error| EngineError::internal(format!("injected write-phase failure: {error}")))?;
    scope.complete_structure_report(token)?;
    Ok(NotetypeChanged { notes: total })
}
