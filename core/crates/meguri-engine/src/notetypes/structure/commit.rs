//! The write phase of a structure change (S3 step 5, S5 to S9, S10): in
//! one transaction, after the safety backup, the base token and the notes'
//! value counts are checked again on the live rows, then the notes are
//! rewritten, cards removed and renumbered, the note type rows written and
//! cards generated. Every check comes before the first write, so a refusal
//! writes nothing; any later error rolls the whole transaction back.

use meguri_model::grave::GraveKind;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, WriteScope};

use super::plan::{self, CardOutcome, Layout, Planned};
use super::{StructureSaved, stale};
use crate::cardgen::regenerate;
use crate::executor::op::OpContext;
use crate::notetypes::{AggregateDigest, names, random_id, read, rename};
use crate::{CancelToken, EngineError, ProgressSink};

/// Notes written per page.
const PAGE: usize = 500;

pub(crate) fn commit(ctx: &mut OpContext<'_, '_>, planned: &Planned) -> Result<StructureSaved, EngineError> {
    match planned {
        Planned::Rename(plan) => {
            let (saved, cards) = rename::commit_counted(ctx.scope(), plan)?;
            Ok(StructureSaved {
                id: saved.id,
                name: saved.name,
                written: saved.written,
                cards_added: cards as u64,
                ..StructureSaved::default()
            })
        }
        Planned::Layout(layout) => commit_layout(ctx, layout),
    }
}

/// The checks of the write phase, on the rows of its own transaction and
/// before its first write: the base token (S1.2 item 1, `engine/op-model`
/// O7), then the notes' value counts (item 4: a note added since planning
/// counts). A precondition on the live rows that must hold for the save to
/// write anything goes after the base token check, here.
pub(crate) fn check_live(scope: &WriteScope<'_>, layout: &Layout) -> Result<(), EngineError> {
    let id = layout.base.id;
    let live = scope.notetype(id)?.ok_or_else(|| stale(id))?;
    if AggregateDigest::of(&live) != layout.digest {
        return Err(stale(id));
    }
    plan::check_note_values(scope, layout, &CancelToken::new(), &ProgressSink::new())
}

pub(crate) fn commit_layout(ctx: &mut OpContext<'_, '_>, layout: &Layout) -> Result<StructureSaved, EngineError> {
    check_live(ctx.scope(), layout)?;
    let mut result = StructureSaved {
        id: layout.saved.id,
        name: layout.saved.name.clone(),
        reasons: layout.reasons.clone(),
        ..StructureSaved::default()
    };
    if !layout.changed {
        result.reasons.clear();
        return Ok(result);
    }
    let (scope, random) = ctx.scope_and_random();
    // G5: `scm` inside the transaction, after admission, before structure.
    crate::mark_schema_change(scope, &layout.reasons)?;
    let report = match layout.report()? {
        Some(report) => Some(scope.start_structure_report(report)?),
        None => None,
    };
    if layout.rewrite_notes {
        result.notes_rewritten = rewrite_notes(scope, layout)?;
    }
    if !layout.cards.is_identity() {
        let (removed, renumbered, without) = rewrite_cards(scope, layout)?;
        result.cards_removed = removed;
        result.cards_renumbered = renumbered;
        result.notes_without_cards = without;
    }
    meguri_model::faultpoint!("notetypes.structure.write_phase")
        .map_err(|error| EngineError::internal(format!("injected write-phase failure: {error}")))?;
    let mut saved = layout.saved.clone();
    // N4 step 7 re-derived on the live rows (O7) from the requested name:
    // another note type may have taken it, or freed it, since planning.
    let others: Vec<_> = read::names(scope)?.into_iter().filter(|(other, _)| *other != saved.id).collect();
    saved.name = names::unique_among(&layout.requested_name, saved.id, &others);
    result.name.clone_from(&saved.name);
    for (field, fresh) in saved.fields.iter_mut().zip(&layout.fresh_fields) {
        if *fresh {
            field.config.id = Some(random_id(random));
        }
    }
    for (template, fresh) in saved.templates.iter_mut().zip(&layout.fresh_templates) {
        if *fresh {
            template.config.id = Some(random_id(random));
        }
    }
    let written = scope.put_notetype(&saved)?;
    if let Some(report) = report {
        scope.complete_structure_report(report)?;
    }
    if let Some(generation) = &layout.generation {
        result.cards_added = regenerate::commit_in(scope, generation)?.len() as u64;
    }
    result.written = written.changed()
        || result.notes_rewritten > 0
        || result.cards_removed > 0
        || result.cards_renumbered > 0
        || result.cards_added > 0;
    Ok(result)
}

/// S5 on the live notes of the type, selected in this transaction: every
/// one is written and stamped.
fn rewrite_notes(scope: &mut WriteScope<'_>, layout: &Layout) -> Result<u64, EngineError> {
    let normalize_text = crate::notes::fields::normalize_enabled(scope)?;
    let mut written = 0;
    let mut after = None;
    loop {
        let page = scope.notetype_notes_after(layout.base.id, after, PAGE)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in page {
            let note = plan::remap_note(layout, &note, normalize_text)?;
            scope.put_note_forced(&note)?;
            written += 1;
        }
    }
    Ok(written)
}

/// S6 (and D4.1) on the live cards of the type: removed cards go with a
/// card grave, moved ones are renumbered and stamped. Returns the cards
/// removed, the cards renumbered and the notes left with no card.
fn rewrite_cards(scope: &mut WriteScope<'_>, layout: &Layout) -> Result<(u64, u64, u64), EngineError> {
    let (mut removed, mut renumbered, mut without) = (0, 0, 0);
    let mut after = None;
    loop {
        let page = scope.notetype_notes_after(layout.base.id, after, PAGE)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in page {
            let cards = scope.cards_of_note(note.id)?;
            let mut gone = 0;
            // Removal comes before renumbering (S6 step 3).
            for card in &cards {
                if layout.cards.outcome_of(card.ord) == CardOutcome::Remove {
                    scope.remove_card(card.id)?;
                    scope.add_grave(card.id.0, GraveKind::Card, Usn::PENDING)?;
                    // Its id, grave and review log stay its own: G9 below
                    // never gives the id to a new card.
                    scope.retire_card_id(card.id);
                    gone += 1;
                }
            }
            for card in &cards {
                if let CardOutcome::Move(to) = layout.cards.outcome_of(card.ord) {
                    // Only `ord` and the stamps change (S6 step 2): the stored
                    // image is edited, never rebuilt from the typed card.
                    scope.put_card_ord(card.id, to)?;
                    renumbered += 1;
                }
            }
            removed += gone;
            if !cards.is_empty() && gone == cards.len() as u64 {
                without += 1;
            }
        }
    }
    Ok((removed, renumbered, without))
}
