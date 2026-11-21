//! Change Note Type over FFI (`notetypes/change-notetype` M1 to M11, U35):
//! the info query as a job (one snapshot, progress and cancel), M6's preview
//! for any mapping the screen edits, and the change as a job. The change
//! carries the losses its sheet showed (M8, `notetypes/draft-save` D8); the
//! core counts them again inside its write transaction and refuses with
//! `ChangeLossesChanged` when one grew.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use meguri_engine::notetypes::NotetypeKind;
use meguri_engine::notetypes::change::{
    self, ChangeInfo, ChangeLosses, ChangeNotetype, ChangePreview, NotetypeSide,
};
use meguri_model::ids::{NoteId, NotetypeId};

use crate::jobs::JobHandle;
use crate::mutations::MutationSummary;
use crate::notetype_structure::OrdinalCount;
use crate::{CoreError, callback, runtime};

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

fn from_map(map: &BTreeMap<u32, u64>) -> Vec<OrdinalCount> {
    map.iter().map(|(ord, count)| OrdinalCount { ord: *ord, count: *count }).collect()
}

/// M8's loss categories: per discarded old field, the notes with stored
/// text there (`<br>` counts); per dropped old template, its cards; and the
/// other cards M9 deletes. A category left out was confirmed as 0.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChangeLosses {
    pub fields: Vec<OrdinalCount>,
    pub templates: Vec<OrdinalCount>,
    pub other_cards: u64,
}

impl TryFrom<NotetypeChangeLosses> for ChangeLosses {
    type Error = CoreError;

    fn try_from(l: NotetypeChangeLosses) -> Result<ChangeLosses, CoreError> {
        Ok(ChangeLosses {
            fields: to_map(l.fields)?,
            templates: to_map(l.templates)?,
            other_cards: l.other_cards,
        })
    }
}

impl From<ChangeLosses> for NotetypeChangeLosses {
    fn from(l: ChangeLosses) -> Self {
        Self {
            fields: from_map(&l.fields),
            templates: from_map(&l.templates),
            other_cards: l.other_cards,
        }
    }
}

/// M6.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChangePreview {
    pub discarded_fields: Vec<u32>,
    /// None without a template map.
    pub dropped_templates: Option<Vec<u32>>,
    pub removed_cards: u64,
    /// What `NotetypeChangeRequest.confirmed` takes once the user accepted this.
    pub losses: NotetypeChangeLosses,
}

impl From<ChangePreview> for NotetypeChangePreview {
    fn from(p: ChangePreview) -> Self {
        Self {
            discarded_fields: p.discarded_fields,
            dropped_templates: p.dropped_templates,
            removed_cards: p.removed_cards,
            losses: p.losses.into(),
        }
    }
}

/// Only the refusal's way back into the engine (`CoreError` to
/// `EngineError` inside a job); repeated ordinals cannot occur there.
impl From<NotetypeChangePreview> for ChangePreview {
    fn from(p: NotetypeChangePreview) -> Self {
        Self {
            discarded_fields: p.discarded_fields,
            dropped_templates: p.dropped_templates,
            removed_cards: p.removed_cards,
            losses: ChangeLosses {
                fields: p.losses.fields.iter().map(|c| (c.ord, c.count)).collect(),
                templates: p.losses.templates.iter().map(|c| (c.ord, c.count)).collect(),
                other_cards: p.losses.other_cards,
            },
        }
    }
}

/// One note type of a change.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChangeSide {
    pub id: i64,
    pub name: String,
    pub cloze: bool,
    pub fields: Vec<String>,
    pub templates: Vec<String>,
}

impl From<NotetypeSide> for NotetypeChangeSide {
    fn from(s: NotetypeSide) -> Self {
        Self {
            id: s.id.0,
            name: s.name,
            cloze: s.kind == NotetypeKind::Cloze,
            fields: s.fields,
            templates: s.templates,
        }
    }
}

impl From<NotetypeChangeSide> for NotetypeSide {
    fn from(s: NotetypeChangeSide) -> Self {
        Self {
            id: NotetypeId(s.id),
            name: s.name,
            kind: if s.cloze { NotetypeKind::Cloze } else { NotetypeKind::Standard },
            fields: s.fields,
            templates: s.templates,
        }
    }
}

/// M5's result.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChangeInfo {
    pub old: NotetypeChangeSide,
    pub new: NotetypeChangeSide,
    /// M2: per new field, an old field ordinal or none.
    pub fields: Vec<Option<u32>>,
    /// M3, none when either note type is cloze.
    pub templates: Option<Vec<Option<u32>>>,
    pub preview: NotetypeChangePreview,
    /// Per old field ordinal.
    pub field_content: Vec<u64>,
    /// Per card ordinal present, ascending.
    pub card_counts: Vec<OrdinalCount>,
    /// `col.scm` as read, which the change takes back.
    pub schema: i64,
}

impl From<ChangeInfo> for NotetypeChangeInfo {
    fn from(i: ChangeInfo) -> Self {
        Self {
            old: i.old.into(),
            new: i.new.into(),
            fields: i.fields,
            templates: i.templates,
            preview: i.preview.into(),
            field_content: i.field_content,
            card_counts: i
                .card_counts
                .iter()
                .map(|(ord, count)| OrdinalCount {
                    ord: u32::from(*ord),
                    count: *count,
                })
                .collect(),
            schema: i.schema,
        }
    }
}

impl TryFrom<NotetypeChangeInfo> for ChangeInfo {
    type Error = CoreError;

    fn try_from(i: NotetypeChangeInfo) -> Result<ChangeInfo, CoreError> {
        let mut card_counts = BTreeMap::new();
        for (ord, count) in to_map(i.card_counts)? {
            let ord = u16::try_from(ord).map_err(|_| CoreError::InvalidArgument {
                message: format!("card ordinal {ord} is out of range"),
            })?;
            card_counts.insert(ord, count);
        }
        Ok(ChangeInfo {
            old: i.old.into(),
            new: i.new.into(),
            fields: i.fields,
            templates: i.templates,
            preview: i.preview.into(),
            field_content: i.field_content,
            card_counts,
            schema: i.schema,
        })
    }
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeChangeInfoJob {
    result: Arc<Mutex<Option<NotetypeChangeInfo>>>,
}

#[uniffi::export]
impl NotetypeChangeInfoJob {
    pub fn result(&self) -> Result<Option<NotetypeChangeInfo>, CoreError> {
        callback::enter("NotetypeChangeInfoJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeChangeInfoJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeChangeInfoJob>,
}

/// M5 on one snapshot on the job pool: one pass over the selected notes and
/// their cards, with progress and cancel.
#[uniffi::export]
pub fn start_notetype_change_info(
    note_ids: Vec<i64>,
    new_notetype_id: i64,
) -> Result<NotetypeChangeInfoJobStart, CoreError> {
    callback::enter("start_notetype_change_info");
    let collection = runtime::host()?.collection()?;
    let notes: Vec<NoteId> = note_ids.into_iter().map(NoteId).collect();
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let read = change::info(&collection, &notes, NotetypeId(new_notetype_id), &job.cancel, &job.progress)?;
        *out.lock().unwrap() = Some(read.into());
        Ok(())
    })?;
    Ok(NotetypeChangeInfoJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeChangeInfoJob { result }),
    })
}

/// M6 for a mapping the screen edits, from one info result (R947). Pure:
/// it reads no collection.
#[uniffi::export]
pub fn notetype_change_preview(
    info: NotetypeChangeInfo,
    fields: Vec<Option<u32>>,
    templates: Option<Vec<Option<u32>>>,
) -> Result<NotetypeChangePreview, CoreError> {
    callback::enter("notetype_change_preview");
    let info = ChangeInfo::try_from(info)?;
    Ok(info.preview(&fields, templates.as_deref()).into())
}

/// M8's input.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChangeRequest {
    pub note_ids: Vec<i64>,
    pub old_notetype_id: i64,
    pub new_notetype_id: i64,
    pub fields: Vec<Option<u32>>,
    /// Present exactly when both note types are standard.
    pub templates: Option<Vec<Option<u32>>>,
    /// `NotetypeChangeInfo.schema`.
    pub schema: i64,
    /// `NotetypeChangePreview.losses` of the preview the sheet showed.
    pub confirmed: NotetypeChangeLosses,
}

impl TryFrom<NotetypeChangeRequest> for ChangeNotetype {
    type Error = CoreError;

    fn try_from(c: NotetypeChangeRequest) -> Result<ChangeNotetype, CoreError> {
        Ok(ChangeNotetype {
            notes: c.note_ids.into_iter().map(NoteId).collect(),
            old_notetype: NotetypeId(c.old_notetype_id),
            new_notetype: NotetypeId(c.new_notetype_id),
            fields: c.fields,
            templates: c.templates,
            schema: c.schema,
            confirmed: c.confirmed.try_into()?,
        })
    }
}

/// M9's result: "N notes updated." (R948).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeChanged {
    pub summary: MutationSummary,
    pub notes: u64,
}

#[derive(Debug, uniffi::Object)]
pub struct NotetypeChangeJob {
    result: Arc<Mutex<Option<NotetypeChanged>>>,
}

#[uniffi::export]
impl NotetypeChangeJob {
    pub fn result(&self) -> Result<Option<NotetypeChanged>, CoreError> {
        callback::enter("NotetypeChangeJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeChangeJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeChangeJob>,
}

/// Starts the change (M8, M9) on the job pool: planning reports progress
/// and can be cancelled, as can its safety backup; the job's outcome
/// carries the refusal.
#[uniffi::export]
pub fn start_notetype_change(request: NotetypeChangeRequest) -> Result<NotetypeChangeJobStart, CoreError> {
    callback::enter("start_notetype_change");
    let request = ChangeNotetype::try_from(request)?;
    // The collection the info query read: a handle of an older generation
    // refuses the op if a replacement lands while the job waits.
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let changed = change::change(&collection, request, job.cancel.clone(), job.progress.clone())?;
        *out.lock().unwrap() = Some(NotetypeChanged {
            summary: MutationSummary::of(&changed),
            notes: changed.value.notes,
        });
        Ok(())
    })?;
    Ok(NotetypeChangeJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeChangeJob { result }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> NotetypeChangeInfo {
        let side = |id, cloze| NotetypeChangeSide {
            id,
            name: format!("N{id}"),
            cloze,
            fields: vec!["Front".into(), "Back".into()],
            templates: vec!["Card 1".into(), "Card 2".into()],
        };
        NotetypeChangeInfo {
            old: side(1, false),
            new: side(2, false),
            fields: vec![Some(0), Some(1)],
            templates: Some(vec![Some(0), Some(1)]),
            preview: NotetypeChangePreview {
                discarded_fields: Vec::new(),
                dropped_templates: Some(Vec::new()),
                removed_cards: 0,
                losses: NotetypeChangeLosses::default(),
            },
            field_content: vec![3, 2],
            card_counts: vec![OrdinalCount { ord: 0, count: 3 }, OrdinalCount { ord: 1, count: 2 }],
            schema: 7,
        }
    }

    #[test]
    fn the_preview_of_an_edited_mapping_crosses_the_bridge() {
        let preview = notetype_change_preview(info(), vec![Some(0), None], Some(vec![Some(0), None])).unwrap();
        assert_eq!(preview.discarded_fields, [1]);
        assert_eq!(preview.dropped_templates, Some(vec![1]));
        assert_eq!(preview.removed_cards, 2);
        assert_eq!(preview.losses.fields, [OrdinalCount { ord: 1, count: 2 }]);
        assert_eq!(preview.losses.templates, [OrdinalCount { ord: 1, count: 2 }]);
    }

    #[test]
    fn repeated_counts_are_refused() {
        let mut bad = info();
        bad.card_counts.push(OrdinalCount { ord: 0, count: 1 });
        assert!(matches!(
            notetype_change_preview(bad, vec![Some(0), Some(1)], None),
            Err(CoreError::InvalidArgument { .. })
        ));
        let request = NotetypeChangeRequest {
            note_ids: vec![1],
            old_notetype_id: 1,
            new_notetype_id: 2,
            fields: vec![Some(0)],
            templates: None,
            schema: 0,
            confirmed: NotetypeChangeLosses {
                fields: vec![OrdinalCount { ord: 1, count: 1 }, OrdinalCount { ord: 1, count: 2 }],
                ..Default::default()
            },
        };
        assert!(matches!(ChangeNotetype::try_from(request), Err(CoreError::InvalidArgument { .. })));
    }
}
