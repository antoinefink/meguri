//! Restore to Default (`specs/notetypes/restore-to-default.md` D1 to D7.2):
//! a note type's fields, templates and styling replaced with Meguri's stock
//! version of its kind, its notes and cards mapped by position (D4) through
//! the structure op's planner and write phase (`notetypes/structure-changes`
//! S5 to S7), in one transaction and one "Update Note Type" step.
//!
//! There is no base token from the caller. The op checks the digest of the
//! note type as its plan read it inside the write transaction (D6). With no
//! confirmed losses a mismatch replans once from the live note type; with
//! confirmed losses it never replans silently: a digest other than the one
//! the caller's `restore_summary` read, at planning or in the transaction,
//! returns `LossesChanged` with a new summary (D6, D7.1). The confirmed
//! losses are counted again on the planning snapshot and in the write
//! transaction (D7, `notetypes/draft-save` D8).

use std::sync::Arc;

use meguri_model::changes::OpSource;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_storage::journal::StructureReason;
use meguri_storage::{ReadRows, ReaderRole, Table};

use super::structure::plan::{self, CardMap, Layout, NoRandom, Purpose, Shape};
use super::structure::{
    ConfirmedLosses, Losses, RestoreSummary, StructureRefusal, StructureSaved, commit, position, stale, write_spec,
};
use super::{AggregateDigest, NotetypeKind, StockKind, write};
use crate::executor::catalog;
use crate::undo::entry::StepKind;
use crate::undo::names::UPDATE_NOTE_TYPE;
use crate::{CancelToken, Collection, EngineError, OpResult, OpSpec, Plan, ProgressSink, SchemaReason};

/// D1's input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreNotetype {
    pub notetype: NotetypeId,
    /// The stock kind to restore; `None` lets the stored kind decide (D2).
    pub kind: Option<StockKind>,
    /// The losses the restore question showed (D7.1:
    /// [`RestoreSummary::confirmed`]); all zero when none was shown.
    pub confirmed: ConfirmedLosses,
    /// The digest the `restore_summary` behind `confirmed` read (D7.1).
    pub digest: Option<AggregateDigest>,
}

impl RestoreNotetype {
    /// Whether the caller passed what a question showed (D7.2).
    fn confirms(&self) -> bool {
        self.digest.is_some() || !self.confirmed.is_zero()
    }
}

/// D2: the stock kind of a restore.
fn stock_kind(notetype: &Notetype, given: Option<StockKind>) -> Result<StockKind, EngineError> {
    if let Some(kind) = given {
        return Ok(kind);
    }
    StockKind::ALL
        .into_iter()
        .find(|kind| kind.stored() == notetype.config.original_stock_kind)
        // Deviation `restore-kind-mismatch`: a stock kind of the other kind
        // is refused too.
        .filter(|kind| kind.kind() == NotetypeKind::of(notetype))
        .ok_or_else(|| StructureRefusal::NeedsStockKind.into())
}

fn not_found(id: NotetypeId) -> EngineError {
    EngineError::NotFound {
        table: Table::Notetypes,
        key: id.0.to_string(),
    }
}

/// D2 to D5 on `base`: the restored note type, by-position origins and the
/// schema classification, before any read of notes and cards.
fn layout_of_restore(base: Notetype, kind: Option<StockKind>, confirmed: ConfirmedLosses) -> Result<Layout, EngineError> {
    let stock_kind = stock_kind(&base, kind)?;
    let stock = stock_kind.notetype(&NoRandom);
    // D3: the stock fields and templates, fresh ids at commit, row stamps 0.
    let mut saved = base.clone();
    saved.fields = stock.fields;
    saved.templates = stock.templates;
    for (at, field) in saved.fields.iter_mut().enumerate() {
        field.ord = position(at);
        field.prior_ord = None;
        field.config.id = None;
    }
    for (at, template) in saved.templates.iter_mut().enumerate() {
        template.ord = position(at);
        template.prior_ord = None;
        template.config.id = None;
    }
    saved.config.css.clone_from(&stock.config.css);
    let kind_changed = kind.is_some() && stock_kind.kind() != NotetypeKind::of(&base);
    if kind.is_some() {
        saved.config.kind = stock.config.kind;
        saved.config.original_stock_kind = stock.config.original_stock_kind;
    }
    saved.config.sort_field_idx = base
        .config
        .sort_field_idx
        .min(position(saved.fields.len().saturating_sub(1)));
    saved.config.reqs = write::card_requirements(&saved);
    // D4: origins by position.
    let by_position = |count: usize, base: usize| -> Vec<Option<u32>> {
        (0..count).map(|at| (at < base).then(|| position(at))).collect()
    };
    let field_origins = by_position(saved.fields.len(), base.fields.len());
    let template_origins = by_position(saved.templates.len(), base.templates.len());
    let text_or_styling_changed = saved.config.css != base.config.css
        || saved.templates.iter().zip(&base.templates).any(|(a, b)| {
            a.config.q_format != b.config.q_format
                || a.config.a_format != b.config.a_format
                || a.config.q_format_browser != b.config.q_format_browser
                || a.config.a_format_browser != b.config.a_format_browser
        });
    let shape = Shape {
        fresh_fields: vec![true; field_origins.len()],
        fresh_templates: vec![true; template_origins.len()],
        field_origins,
        template_origins,
        confirmed,
        text_or_styling_changed,
        purpose: Purpose::Restore,
    };
    let base_cloze = NotetypeKind::of(&base) == NotetypeKind::Cloze;
    let saved_cloze = NotetypeKind::of(&saved) == NotetypeKind::Cloze;
    let saved_templates = saved.templates.len();
    let mut layout = plan::classify(base, saved, shape);
    // D5: classified by the counts and a change of kind, not by G3.
    let counts_changed = layout.saved.fields.len() != layout.base.fields.len()
        || layout.saved.templates.len() != layout.base.templates.len();
    layout.reasons.clear();
    layout.structure_reasons.clear();
    if counts_changed || kind_changed {
        layout.reasons.push(SchemaReason::NotetypeRestored);
        layout.structure_reasons.insert(StructureReason::NotetypeRestored);
    }
    layout.report_layout |= kind_changed;
    // D4.1: a cloze note type restored as a standard kind keeps the cards
    // whose ordinal has a stock template and removes the others; its card
    // ordinals map to the stock templates at the same positions.
    if base_cloze && !saved_cloze {
        let templates = u16::try_from(saved_templates).unwrap_or(u16::MAX);
        layout.cards = CardMap {
            remove_from: Some(templates),
            ..layout.cards
        };
        layout.report_template_map = Some((0..saved_templates).map(|at| at as i32).collect());
    }
    Ok(layout)
}

/// D7.1 on `reader`'s rows: the summary of the by-position draft, the
/// kind-change cards and the digest read. Writes nothing.
fn summary_on(
    reader: &impl ReadRows,
    id: NotetypeId,
    kind: Option<StockKind>,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<RestoreSummary, EngineError> {
    let base = reader.notetype(id)?.ok_or_else(|| not_found(id))?;
    let layout = layout_of_restore(base, kind, ConfirmedLosses::default())?;
    let counts = plan::count(reader, &layout, cancel, progress)?;
    let generation = plan::plan_generation(reader, &layout, cancel, progress)?;
    match plan::losses_read(&layout, &counts, generation.as_ref()) {
        Losses::Restore(summary) => Ok(summary),
        other => Err(EngineError::internal(format!("a restore read {other:?}"))),
    }
}

/// `restore_summary(id, kind?)` (D7.1) on one snapshot: what the restore
/// question shows and the caller confirms. Refuses as the op would, with
/// `NotFound` and `NeedsStockKind`.
pub fn restore_summary(
    collection: &Collection,
    id: NotetypeId,
    kind: Option<StockKind>,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<RestoreSummary, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    summary_on(&*snapshot, id, kind, cancel, progress)
}

fn losses_changed(summary: RestoreSummary) -> EngineError {
    StructureRefusal::LossesChanged(Box::new(Losses::Restore(summary))).into()
}

/// The plan of a restore on `reader`'s rows (D2 to D7): the refusals of
/// D7.2 and the confirmed losses on the snapshot.
fn plan_restore(
    reader: &impl ReadRows,
    request: &RestoreNotetype,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Layout, EngineError> {
    cancel.check()?;
    let id = request.notetype;
    let Some(base) = reader.notetype(id)? else {
        // D6, D7.2: deleted after the summary was read is `Stale`.
        return Err(if request.confirms() { stale(id) } else { not_found(id) });
    };
    if !request.confirmed.is_zero()
        && let Some(digest) = request.digest
        && AggregateDigest::of(&base) != digest
    {
        // D6: the note type changed since the question was read; a reorder
        // can move another field past the stock count while every count
        // stays equal, so the question shows again.
        return Err(losses_changed(summary_on(reader, id, request.kind, cancel, progress)?));
    }
    let mut layout = layout_of_restore(base, request.kind, request.confirmed.clone())?;
    plan::finish(reader, &mut layout, true, cancel, progress).map_err(|error| match error {
        // D3: the stock text always passes V3; a failure is a bug.
        EngineError::NotetypeRefused(refusal) => EngineError::internal(format!("stock text refused: {refusal}")),
        other => other,
    })?;
    Ok(layout)
}

/// Restore to Default as an op (D6): one transaction and one "Update Note
/// Type" step after the safety backup of a schema-classified restore.
pub fn restore(
    collection: &Collection,
    request: RestoreNotetype,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<StructureSaved>, EngineError> {
    let spec = OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
        .undo_step(UPDATE_NOTE_TYPE, StepKind::Other)
        .cancel(cancel.clone())
        .progress(progress.clone());
    let committed = request.clone();
    crate::plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        spec,
        |context| {
            // D6: one replan (a restore with no confirmed losses), then
            // `Stale`.
            if context.attempt() > 1 {
                return Err(stale(request.notetype));
            }
            let layout = plan_restore(&**context.snapshot(), &request, &cancel, &progress)?;
            Ok(Plan {
                value: layout,
                preconditions: Vec::new(),
            })
        },
        |spec, layout| write_spec(spec, &layout.reasons, layout.touches_notes()),
        move |context, layout| {
            let id = committed.notetype;
            let scope = context.scope();
            let live = scope.notetype(id)?.ok_or_else(|| stale(id))?;
            if AggregateDigest::of(&live) != layout.digest {
                if committed.confirmed.is_zero() {
                    // D6: replanned once from the live note type.
                    return Err(stale(id));
                }
                // D6 (round 9 review): no silent replan of a confirmed
                // restore; the question shows again with this transaction's
                // summary.
                let none = CancelToken::new();
                let summary = summary_on(scope, id, committed.kind, &none, &ProgressSink::new())?;
                return Err(losses_changed(summary));
            }
            commit::commit_layout(context, &layout)
        },
    )
}
