//! The note type structure ops over FFI (`notetypes/structure-changes` S1
//! to S11, U34): the draft apply op as a job with progress and cancel, its
//! refusals, and the template removal preview. A draft carries the base
//! token it was loaded with; the core checks it again inside its write
//! transaction.

use std::collections::{BTreeMap, BTreeSet};

use std::sync::{Arc, Mutex};

use meguri_engine::notetypes::structure::{
    self, DraftField, DraftTemplate, FieldOptions, StructureDraft, StructureRefusal, StructureSaved,
};
use meguri_engine::notetypes::AggregateDigest;
use meguri_model::ids::{NoteId, NotetypeId};

use crate::error_types::SchemaChangeReason;
use crate::jobs::JobHandle;
use crate::mutations::MutationSummary;
use crate::{CoreError, callback, runtime};

/// A count for one ordinal: the cards of a template proposed for removal.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OrdinalCount {
    pub ord: u32,
    pub count: u64,
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
}

impl TryFrom<NotetypeStructureDraft> for StructureDraft {
    type Error = CoreError;

    fn try_from(d: NotetypeStructureDraft) -> Result<StructureDraft, CoreError> {
        let base: [u8; 32] = d.base.try_into().map_err(|_| CoreError::InvalidArgument {
            message: "aggregate digest must be 32 bytes".into(),
        })?;
        Ok(StructureDraft {
            notetype: NotetypeId(d.notetype_id),
            base: AggregateDigest(base),
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

/// The structure save's own refusals; nothing was written.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeStructureRefusal {
    ProtectedField { field: u32 },
    NoteFieldCountMismatch { note: i64 },
}

impl From<StructureRefusal> for NotetypeStructureRefusal {
    fn from(r: StructureRefusal) -> Self {
        match r {
            StructureRefusal::ProtectedField { field } => Self::ProtectedField { field },
            StructureRefusal::NoteFieldCountMismatch { note } => Self::NoteFieldCountMismatch { note: note.0 },
        }
    }
}

impl From<NotetypeStructureRefusal> for StructureRefusal {
    fn from(r: NotetypeStructureRefusal) -> Self {
        match r {
            NotetypeStructureRefusal::ProtectedField { field } => Self::ProtectedField { field },
            NotetypeStructureRefusal::NoteFieldCountMismatch { note } => {
                Self::NoteFieldCountMismatch { note: NoteId(note) }
            }
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
        for refusal in [
            NotetypeStructureRefusal::ProtectedField { field: 2 },
            NotetypeStructureRefusal::NoteFieldCountMismatch { note: 7 },
        ] {
            let error = CoreError::NotetypeStructureRefused { refusal };
            assert_eq!(CoreError::from(EngineError::from(error.clone())), error);
        }
    }

    #[test]
    fn a_draft_with_a_short_base_is_refused() {
        let draft = NotetypeStructureDraft {
            notetype_id: 1,
            base: vec![0; 31],
            name: "x".into(),
            css: String::new(),
            fields: Vec::new(),
            templates: Vec::new(),
            sort_field: None,
            skip_checks: false,
        };
        assert!(matches!(
            StructureDraft::try_from(draft),
            Err(CoreError::InvalidArgument { .. })
        ));
    }
}
