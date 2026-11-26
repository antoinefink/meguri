//! The note type structure ops over FFI (`notetypes/structure-changes` S1
//! to S13, `notetypes/restore-to-default` D1 to D7.2, U34): the draft apply
//! op, note type removal and Restore to Default as jobs with progress and
//! cancel, their refusals, the template removal preview and the restore
//! summary. Each destructive op carries the losses its question showed
//! (`notetypes/draft-save` D8); the core counts them again inside its write
//! transaction and refuses with `LossesChanged` when one grew.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use meguri_engine::notetypes::remove::{self, RemoveNotetype};
use meguri_engine::notetypes::restore::{self, RestoreNotetype};
use meguri_engine::notetypes::structure::{
    self, ConfirmedLosses, DraftChange, DraftField, DraftSummary, DraftTemplate, FieldOptions, Losses,
    RestoreSummary, StructureDraft, StructureRefusal, StructureSaved,
};
use meguri_engine::notetypes::AggregateDigest;
use meguri_model::ids::{NoteId, NotetypeId};

use crate::error_types::SchemaChangeReason;
use crate::jobs::JobHandle;
use crate::mutations::MutationSummary;
use crate::{CoreError, callback, runtime};

/// A count for one base ordinal: a removed field's notes with content, or a
/// removed template's cards.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OrdinalCount {
    pub ord: u32,
    pub count: u64,
}

fn to_map(counts: Vec<OrdinalCount>) -> Result<BTreeMap<u32, u64>, CoreError> {
    let mut map = BTreeMap::new();
    for OrdinalCount { ord, count } in counts {
        if map.insert(ord, count).is_some() {
            return Err(CoreError::InvalidArgument {
                message: format!("ordinal {ord} is listed twice"),
            });
        }
    }
    Ok(map)
}

pub(crate) fn digest(bytes: Vec<u8>) -> Result<AggregateDigest, CoreError> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| CoreError::InvalidArgument {
        message: "aggregate digest must be 32 bytes".into(),
    })?;
    Ok(AggregateDigest(bytes))
}

/// The losses a question showed (D8): one count per category, compared
/// each on its own; a category left out was confirmed as 0.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeConfirmedLosses {
    /// Per removed base field ordinal: the notes with content.
    pub fields: Vec<OrdinalCount>,
    /// Per removed base template ordinal: its cards.
    pub templates: Vec<OrdinalCount>,
    /// Restore to Default's kind-change cards (D7.1).
    pub kind_change_cards: u64,
}

impl TryFrom<NotetypeConfirmedLosses> for ConfirmedLosses {
    type Error = CoreError;

    fn try_from(c: NotetypeConfirmedLosses) -> Result<ConfirmedLosses, CoreError> {
        Ok(ConfirmedLosses {
            fields: to_map(c.fields)?,
            templates: to_map(c.templates)?,
            kind_change_cards: c.kind_change_cards,
        })
    }
}

fn from_map(map: &BTreeMap<u32, u64>) -> Vec<OrdinalCount> {
    map.iter().map(|(ord, count)| OrdinalCount { ord: *ord, count: *count }).collect()
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StructureDraftField {
    /// The base ordinal the field comes from; none for a new field.
    pub origin: Option<u32>,
    pub name: String,
    pub sticky: bool,
    pub rtl: bool,
    pub font_name: String,
    pub font_size: u32,
    pub description: String,
    pub plain_text: bool,
    pub collapsed: bool,
    pub exclude_from_search: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StructureDraftTemplate {
    /// The base ordinal the template comes from; none for a new one.
    pub origin: Option<u32>,
    pub name: String,
    pub front: String,
    pub back: String,
    pub browser_front: String,
    pub browser_back: String,
    pub target_deck_id: i64,
    pub browser_font_name: String,
    pub browser_font_size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeStructureDraft {
    pub notetype_id: i64,
    /// `NotetypeRecord.base` as read.
    pub base: Vec<u8>,
    pub name: String,
    pub css: String,
    pub fields: Vec<StructureDraftField>,
    pub templates: Vec<StructureDraftTemplate>,
    /// A position in `fields`; none keeps the base's sort field.
    pub sort_field: Option<u32>,
    pub skip_checks: bool,
    /// What the save sheet showed (zero for each when none was shown).
    pub confirmed: NotetypeConfirmedLosses,
}

impl TryFrom<NotetypeStructureDraft> for StructureDraft {
    type Error = CoreError;

    fn try_from(d: NotetypeStructureDraft) -> Result<StructureDraft, CoreError> {
        Ok(StructureDraft {
            notetype: NotetypeId(d.notetype_id),
            base: digest(d.base)?,
            name: d.name,
            css: d.css,
            fields: d
                .fields
                .into_iter()
                .map(|f| DraftField {
                    origin: f.origin,
                    name: f.name,
                    options: FieldOptions {
                        sticky: f.sticky,
                        rtl: f.rtl,
                        font_name: f.font_name,
                        font_size: f.font_size,
                        description: f.description,
                        plain_text: f.plain_text,
                        collapsed: f.collapsed,
                        exclude_from_search: f.exclude_from_search,
                    },
                })
                .collect(),
            templates: d
                .templates
                .into_iter()
                .map(|t| DraftTemplate {
                    origin: t.origin,
                    name: t.name,
                    front: t.front,
                    back: t.back,
                    browser_front: t.browser_front,
                    browser_back: t.browser_back,
                    deck: t.target_deck_id,
                    browser_font_name: t.browser_font_name,
                    browser_font_size: t.browser_font_size,
                })
                .collect(),
            sort_field: d.sort_field,
            skip_checks: d.skip_checks,
            confirmed: d.confirmed.try_into()?,
        })
    }
}

/// S1.3.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeStructureSaved {
    pub summary: MutationSummary,
    pub id: i64,
    pub name: String,
    pub written: bool,
    pub notes_rewritten: u64,
    pub cards_added: u64,
    pub cards_removed: u64,
    pub cards_renumbered: u64,
    pub notes_without_cards: u64,
    pub reasons: Vec<SchemaChangeReason>,
}

impl From<meguri_engine::OpResult<StructureSaved>> for NotetypeStructureSaved {
    fn from(r: meguri_engine::OpResult<StructureSaved>) -> Self {
        let summary = MutationSummary::of(&r);
        let v = r.value;
        Self {
            summary,
            id: v.id.0,
            name: v.name,
            written: v.written,
            notes_rewritten: v.notes_rewritten,
            cards_added: v.cards_added,
            cards_removed: v.cards_removed,
            cards_renumbered: v.cards_renumbered,
            notes_without_cards: v.notes_without_cards,
            reasons: v.reasons.into_iter().map(Into::into).collect(),
        }
    }
}

/// `notetypes/draft-save` D2's changes.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeDraftChange {
    FieldAdded { name: String },
    FieldRemoved { ord: u32, name: String, notes_with_content: u64 },
    FieldRenamed { from: String, to: String },
    FieldsReordered,
    SortFieldChanged { name: String },
    TemplateAdded { name: String, cards_created: u64 },
    TemplateRemoved { ord: u32, name: String, cards_removed: u64, notes_without_cards: u64 },
    TemplateRenamed { from: String, to: String },
    TemplatesReordered,
    NotetypeRenamed { from: String, to: String },
    TextOrStylingChanged,
}

impl From<DraftChange> for NotetypeDraftChange {
    fn from(c: DraftChange) -> Self {
        use DraftChange as D;
        match c {
            D::FieldAdded { name } => Self::FieldAdded { name },
            D::FieldRemoved { ord, name, notes_with_content } => Self::FieldRemoved { ord, name, notes_with_content },
            D::FieldRenamed { from, to } => Self::FieldRenamed { from, to },
            D::FieldsReordered => Self::FieldsReordered,
            D::SortFieldChanged { name } => Self::SortFieldChanged { name },
            D::TemplateAdded { name, cards_created } => Self::TemplateAdded { name, cards_created },
            D::TemplateRemoved { ord, name, cards_removed, notes_without_cards } => {
                Self::TemplateRemoved { ord, name, cards_removed, notes_without_cards }
            }
            D::TemplateRenamed { from, to } => Self::TemplateRenamed { from, to },
            D::TemplatesReordered => Self::TemplatesReordered,
            D::NotetypeRenamed { from, to } => Self::NotetypeRenamed { from, to },
            D::TextOrStylingChanged => Self::TextOrStylingChanged,
        }
    }
}

impl From<NotetypeDraftChange> for DraftChange {
    fn from(c: NotetypeDraftChange) -> Self {
        use NotetypeDraftChange as C;
        match c {
            C::FieldAdded { name } => Self::FieldAdded { name },
            C::FieldRemoved { ord, name, notes_with_content } => Self::FieldRemoved { ord, name, notes_with_content },
            C::FieldRenamed { from, to } => Self::FieldRenamed { from, to },
            C::FieldsReordered => Self::FieldsReordered,
            C::SortFieldChanged { name } => Self::SortFieldChanged { name },
            C::TemplateAdded { name, cards_created } => Self::TemplateAdded { name, cards_created },
            C::TemplateRemoved { ord, name, cards_removed, notes_without_cards } => {
                Self::TemplateRemoved { ord, name, cards_removed, notes_without_cards }
            }
            C::TemplateRenamed { from, to } => Self::TemplateRenamed { from, to },
            C::TemplatesReordered => Self::TemplatesReordered,
            C::NotetypeRenamed { from, to } => Self::NotetypeRenamed { from, to },
            C::TextOrStylingChanged => Self::TextOrStylingChanged,
        }
    }
}

/// D2's summary.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeDraftSummary {
    pub reasons: Vec<SchemaChangeReason>,
    pub changes: Vec<NotetypeDraftChange>,
    pub loses_content: bool,
}

impl From<DraftSummary> for NotetypeDraftSummary {
    fn from(s: DraftSummary) -> Self {
        Self {
            reasons: s.reasons.into_iter().map(Into::into).collect(),
            changes: s.changes.into_iter().map(Into::into).collect(),
            loses_content: s.loses_content,
        }
    }
}

impl From<NotetypeDraftSummary> for DraftSummary {
    fn from(s: NotetypeDraftSummary) -> Self {
        Self {
            reasons: s.reasons.into_iter().map(Into::into).collect(),
            changes: s.changes.into_iter().map(Into::into).collect(),
            loses_content: s.loses_content,
        }
    }
}

/// `restore-to-default` D7.1.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeRestoreSummary {
    pub summary: NotetypeDraftSummary,
    pub kind_change_cards_removed: u64,
    /// The digest read, which the restore takes back with the confirmed
    /// losses: each `FieldRemoved` row's `notes_with_content` by its `ord`,
    /// each `TemplateRemoved` row's `cards_removed` by its `ord`, and
    /// `kind_change_cards_removed` (D7.1).
    pub digest: Vec<u8>,
}

impl From<RestoreSummary> for NotetypeRestoreSummary {
    fn from(s: RestoreSummary) -> Self {
        Self {
            kind_change_cards_removed: s.kind_change_cards_removed,
            digest: s.digest.0.to_vec(),
            summary: s.summary.into(),
        }
    }
}

/// Only the refusal's way back into the engine (`CoreError` to
/// `EngineError` inside a job): a request takes its digest through
/// [`digest`], which refuses a wrong length.
impl From<NotetypeRestoreSummary> for RestoreSummary {
    fn from(s: NotetypeRestoreSummary) -> Self {
        let mut digest = [0u8; 32];
        if s.digest.len() == 32 {
            digest.copy_from_slice(&s.digest);
        }
        Self {
            summary: s.summary.into(),
            kind_change_cards_removed: s.kind_change_cards_removed,
            digest: AggregateDigest(digest),
        }
    }
}

/// The structure ops' own refusals; nothing was written.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeStructureRefusal {
    ProtectedField { field: u32 },
    NoteFieldCountMismatch { note: i64 },
    /// The draft save's losses grew: show the sheet again with this summary.
    LossesChanged { summary: NotetypeDraftSummary },
    /// A removal's live counts grew past the confirmed ones.
    RemovalLossesChanged { notes: u64, cards: u64 },
    /// A restore's losses or note type changed: ask again with this summary.
    RestoreLossesChanged { summary: NotetypeRestoreSummary },
    LastNotetype,
    NeedsStockKind,
    /// Change Note Type M7: the change would change nothing.
    NoChanges,
    /// Change Note Type M8 check 3: another schema change since the info
    /// query; open the screen again.
    SchemaChanged,
    /// Change Note Type M4, M8 check 4: the notes do not share one note type.
    MultipleNotetypes,
    /// Change Note Type's losses grew: show the sheet again with these
    /// counts (the fresh M6 preview's: the discarded fields and dropped
    /// templates are their keys) and pass them back once accepted. Kept
    /// small: `CoreError` carries this refusal inline.
    ChangeLossesChanged {
        losses: crate::notetype_change::NotetypeChangeLosses,
        removed_cards: u64,
        /// Whether the change has a template map (M1): with one, the
        /// dropped templates are the `templates` keys, possibly none.
        template_map: bool,
    },
}

impl From<StructureRefusal> for NotetypeStructureRefusal {
    fn from(r: StructureRefusal) -> Self {
        match r {
            StructureRefusal::ProtectedField { field } => Self::ProtectedField { field },
            StructureRefusal::NoteFieldCountMismatch { note } => Self::NoteFieldCountMismatch { note: note.0 },
            StructureRefusal::LossesChanged(losses) => match *losses {
                Losses::Draft(summary) => Self::LossesChanged {
                    summary: summary.into(),
                },
                Losses::Removal { notes, cards } => Self::RemovalLossesChanged { notes, cards },
                Losses::Restore(summary) => Self::RestoreLossesChanged {
                    summary: summary.into(),
                },
                Losses::Change(preview) => Self::ChangeLossesChanged {
                    template_map: preview.dropped_templates.is_some(),
                    losses: preview.losses.into(),
                    removed_cards: preview.removed_cards,
                },
            },
            StructureRefusal::LastNotetype => Self::LastNotetype,
            StructureRefusal::NeedsStockKind => Self::NeedsStockKind,
            StructureRefusal::NoChanges => Self::NoChanges,
            StructureRefusal::SchemaChanged => Self::SchemaChanged,
            StructureRefusal::MultipleNotetypes => Self::MultipleNotetypes,
        }
    }
}

impl From<NotetypeStructureRefusal> for StructureRefusal {
    fn from(r: NotetypeStructureRefusal) -> Self {
        use NotetypeStructureRefusal as R;
        match r {
            R::ProtectedField { field } => Self::ProtectedField { field },
            R::NoteFieldCountMismatch { note } => Self::NoteFieldCountMismatch { note: NoteId(note) },
            R::LossesChanged { summary } => Self::LossesChanged(Box::new(Losses::Draft(summary.into()))),
            R::RemovalLossesChanged { notes, cards } => Self::LossesChanged(Box::new(Losses::Removal { notes, cards })),
            R::RestoreLossesChanged { summary } => Self::LossesChanged(Box::new(Losses::Restore(summary.into()))),
            R::LastNotetype => Self::LastNotetype,
            R::NeedsStockKind => Self::NeedsStockKind,
            R::NoChanges => Self::NoChanges,
            R::SchemaChanged => Self::SchemaChanged,
            R::MultipleNotetypes => Self::MultipleNotetypes,
            R::ChangeLossesChanged {
                losses,
                removed_cards,
                template_map,
            } => Self::LossesChanged(Box::new(Losses::Change(crate::notetype_change::preview_of_losses(
                losses,
                removed_cards,
                template_map,
            )))),
        }
    }
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeStructureJob {
    result: Arc<Mutex<Option<NotetypeStructureSaved>>>,
}

#[uniffi::export]
impl NotetypeStructureJob {
    /// The op's result once the job finished successfully.
    pub fn result(&self) -> Result<Option<NotetypeStructureSaved>, CoreError> {
        callback::enter("NotetypeStructureJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeStructureJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeStructureJob>,
}

/// Starts the draft apply op (S1) on the job pool: planning reports
/// progress and can be cancelled; the job's outcome carries the refusal.
#[uniffi::export]
pub fn start_notetype_structure_save(draft: NotetypeStructureDraft) -> Result<NotetypeStructureJobStart, CoreError> {
    callback::enter("start_notetype_structure_save");
    let draft = StructureDraft::try_from(draft)?;
    // The collection the draft was made on: a handle of an older generation
    // refuses the op (`StaleGeneration`) if a replacement lands while the
    // job waits, so the draft never reaches another collection.
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let saved = structure::apply(&collection, draft, job.cancel.clone(), job.progress.clone())?;
        *out.lock().unwrap() = Some(saved.into());
        Ok(())
    })?;
    Ok(NotetypeStructureJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeStructureJob { result }),
    })
}

/// S12's input.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeRemoval {
    pub notetype_id: i64,
    /// The notes and cards W12's question showed.
    pub confirmed_notes: u64,
    pub confirmed_cards: u64,
}

/// What a removal did.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeRemoved {
    pub summary: MutationSummary,
    pub id: i64,
    /// False when no note type had the id: nothing changed.
    pub removed: bool,
    pub notes_removed: u64,
    pub cards_removed: u64,
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeRemovalJob {
    result: Arc<Mutex<Option<NotetypeRemoved>>>,
}

#[uniffi::export]
impl NotetypeRemovalJob {
    pub fn result(&self) -> Result<Option<NotetypeRemoved>, CoreError> {
        callback::enter("NotetypeRemovalJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeRemovalJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeRemovalJob>,
}

/// Starts S12's removal on the job pool.
#[uniffi::export]
pub fn start_notetype_removal(request: NotetypeRemoval) -> Result<NotetypeRemovalJobStart, CoreError> {
    callback::enter("start_notetype_removal");
    let host = runtime::host()?;
    let generation = host.collection()?.generation().0;
    remove_notetype_on(host, request, generation)
}
/// `generation` is the collection the removal question described.
#[uniffi::export]
pub fn start_notetype_removal_for_generation(request: NotetypeRemoval, generation: u64)
    -> Result<NotetypeRemovalJobStart, CoreError> {
    callback::enter("start_notetype_removal_for_generation");
    remove_notetype_on(runtime::host()?, request, generation)
}
pub(crate) fn remove_notetype_on(host: Arc<meguri_host::CoreHost>, request: NotetypeRemoval, generation: u64)
    -> Result<NotetypeRemovalJobStart, CoreError> {
    let collection = host.collection_for_generation(meguri_engine::Generation(generation))?;
    let input = RemoveNotetype {
        notetype: NotetypeId(request.notetype_id),
        confirmed_notes: request.confirmed_notes,
        confirmed_cards: request.confirmed_cards,
    };
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let engine = host.engine().clone();
    let handle = engine.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let _host = host;
        let removed = remove::remove(&collection, input, job.cancel.clone(), job.progress.clone())?;
        *out.lock().unwrap() = Some(NotetypeRemoved {
            summary: MutationSummary::of(&removed),
            id: removed.value.id.0,
            removed: removed.value.removed,
            notes_removed: removed.value.notes_removed,
            cards_removed: removed.value.cards_removed,
        });
        Ok(())
    })?;
    Ok(NotetypeRemovalJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeRemovalJob { result }),
    })
}

/// D1's input.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeRestore {
    pub notetype_id: i64,
    /// The stock kind; none lets the stored kind decide (D2).
    pub kind: Option<crate::notetypes::StockNotetype>,
    /// The counts of the `NotetypeRestoreSummary` the question showed, or
    /// all zero with no question.
    pub confirmed: NotetypeConfirmedLosses,
    /// `NotetypeRestoreSummary.digest`, or none with no question.
    pub digest: Option<Vec<u8>>,
}

/// Starts Restore to Default on the job pool; its result is S1.3's.
#[uniffi::export]
pub fn start_notetype_restore(request: NotetypeRestore) -> Result<NotetypeStructureJobStart, CoreError> {
    callback::enter("start_notetype_restore");
    let collection = runtime::host()?.collection()?;
    let input = RestoreNotetype {
        notetype: NotetypeId(request.notetype_id),
        kind: request.kind.map(Into::into),
        confirmed: request.confirmed.try_into()?,
        digest: request.digest.map(digest).transpose()?,
    };
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let saved = restore::restore(&collection, input, job.cancel.clone(), job.progress.clone())?;
        *out.lock().unwrap() = Some(saved.into());
        Ok(())
    })?;
    Ok(NotetypeStructureJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeStructureJob { result }),
    })
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeRestoreSummaryJob {
    result: Arc<Mutex<Option<NotetypeRestoreSummary>>>,
}

#[uniffi::export]
impl NotetypeRestoreSummaryJob {
    pub fn result(&self) -> Result<Option<NotetypeRestoreSummary>, CoreError> {
        callback::enter("NotetypeRestoreSummaryJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeRestoreSummaryJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeRestoreSummaryJob>,
}

/// D7.1's `restore_summary`, read on one snapshot on the job pool: it scans
/// every note and card of the type, with progress and cancel.
#[uniffi::export]
pub fn start_notetype_restore_summary(
    notetype_id: i64,
    kind: Option<crate::notetypes::StockNotetype>,
) -> Result<NotetypeRestoreSummaryJobStart, CoreError> {
    callback::enter("start_notetype_restore_summary");
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let summary = restore::restore_summary(
            &collection,
            NotetypeId(notetype_id),
            kind.map(Into::into),
            &job.cancel,
            &job.progress,
        )?;
        *out.lock().unwrap() = Some(summary.into());
        Ok(())
    })?;
    Ok(NotetypeRestoreSummaryJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeRestoreSummaryJob { result }),
    })
}

/// S11.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TemplateRemovalPreview {
    pub cards: Vec<OrdinalCount>,
    pub notes_without_cards: u64,
}

/// S11: the cards of each template proposed for removal and the notes left
/// with no card, read on one snapshot.
#[uniffi::export]
pub fn notetype_template_removal_preview(
    notetype_id: i64,
    ords: Vec<u32>,
) -> Result<TemplateRemovalPreview, CoreError> {
    callback::enter("notetype_template_removal_preview");
    let ords: BTreeSet<u32> = ords.into_iter().collect();
    let collection = runtime::host()?.collection()?;
    let preview = structure::preview_removal(&collection, NotetypeId(notetype_id), &ords)?;
    Ok(TemplateRemovalPreview {
        cards: from_map(&preview.cards),
        notes_without_cards: preview.notes_without_cards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_engine::EngineError;

    #[test]
    fn structure_refusals_cross_the_bridge_unchanged() {
        let summary = NotetypeDraftSummary {
            reasons: vec![SchemaChangeReason::FieldsChanged],
            changes: vec![
                NotetypeDraftChange::FieldRemoved {
                    ord: 1,
                    name: "Back".into(),
                    notes_with_content: 3,
                },
                NotetypeDraftChange::TemplateRemoved {
                    ord: 1,
                    name: "Card 2".into(),
                    cards_removed: 4,
                    notes_without_cards: 1,
                },
            ],
            loses_content: true,
        };
        let restore = NotetypeRestoreSummary {
            summary: summary.clone(),
            kind_change_cards_removed: 2,
            digest: vec![7; 32],
        };
        for refusal in [
            NotetypeStructureRefusal::ProtectedField { field: 2 },
            NotetypeStructureRefusal::NoteFieldCountMismatch { note: 7 },
            NotetypeStructureRefusal::LossesChanged { summary },
            NotetypeStructureRefusal::RemovalLossesChanged { notes: 1, cards: 2 },
            NotetypeStructureRefusal::RestoreLossesChanged { summary: restore },
            NotetypeStructureRefusal::LastNotetype,
            NotetypeStructureRefusal::NeedsStockKind,
            NotetypeStructureRefusal::NoChanges,
            NotetypeStructureRefusal::SchemaChanged,
            NotetypeStructureRefusal::MultipleNotetypes,
            NotetypeStructureRefusal::ChangeLossesChanged {
                losses: crate::notetype_change::NotetypeChangeLosses {
                    fields: vec![OrdinalCount { ord: 1, count: 2 }],
                    templates: vec![OrdinalCount { ord: 0, count: 2 }],
                    other_cards: 1,
                },
                removed_cards: 3,
                template_map: true,
            },
            NotetypeStructureRefusal::ChangeLossesChanged {
                losses: crate::notetype_change::NotetypeChangeLosses::default(),
                removed_cards: 0,
                template_map: true,
            },
        ] {
            let error = CoreError::NotetypeStructureRefused { refusal };
            assert_eq!(CoreError::from(EngineError::from(error.clone())), error);
        }
    }

    #[test]
    fn a_draft_with_a_short_base_or_a_repeated_count_is_refused() {
        let draft = NotetypeStructureDraft {
            notetype_id: 1,
            base: vec![0; 31],
            name: "x".into(),
            css: String::new(),
            fields: Vec::new(),
            templates: Vec::new(),
            sort_field: None,
            skip_checks: false,
            confirmed: NotetypeConfirmedLosses::default(),
        };
        assert!(matches!(
            StructureDraft::try_from(draft.clone()),
            Err(CoreError::InvalidArgument { .. })
        ));
        let repeated = NotetypeStructureDraft {
            base: vec![0; 32],
            confirmed: NotetypeConfirmedLosses {
                templates: vec![OrdinalCount { ord: 1, count: 2 }, OrdinalCount { ord: 1, count: 3 }],
                ..Default::default()
            },
            ..draft
        };
        assert!(matches!(
            StructureDraft::try_from(repeated),
            Err(CoreError::InvalidArgument { .. })
        ));
    }
}
