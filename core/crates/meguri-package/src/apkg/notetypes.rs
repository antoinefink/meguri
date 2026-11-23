//! Note types of an import: what both paths share, and the pass without a
//! merge (`package/apkg-import` I7). Everything here is planning: records
//! and decisions computed from the plan's snapshot, written by `commit`.
use std::collections::{BTreeMap, HashMap, HashSet};

use meguri_engine::cardgen::regenerate::{self, RegenerationPlan};
use meguri_engine::notetypes::import as saves;
use meguri_engine::{CancelToken, ProgressSink, SchemaReason};
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_model::time::EpochSecs;
use meguri_storage::ReadRows;
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason, StructureReport};

use super::{ImportOptions, ImportOutcome, MergePreview, UpdateCondition, corrupt};

/// Where the notes of one package note type go.
#[derive(Clone, Debug)]
pub(crate) struct TypeRoute {
    pub target: TypeTarget,
    /// The target note type as the import's note type writes leave it.
    pub record: Notetype,
    /// For each field position of `record`, the incoming position that
    /// fills it; `None` for the identity (no merge).
    pub field_map: Option<Vec<Option<usize>>>,
    /// Incoming template ordinal to target ordinal; `None` for the identity
    /// on every ordinal (no merge, and cloze note types).
    pub template_map: Option<Vec<u16>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeTarget {
    /// A note type id known at planning.
    Id(i64),
    /// The n-th copy this import adds, whose id the write phase draws.
    Copy(usize),
}

/// One planned note type write, in write order.
#[derive(Clone, Debug)]
pub(crate) enum TypeWrite {
    /// Writes `record` under its id: an add under the package's id, an
    /// update, or a merged record. `stamp_now` sets `mtime_secs` to now.
    Put { record: Notetype, stamp_now: bool },
    /// Adds a copy under a new id and makes it the current note type.
    AddCopy { record: Notetype, stamp_now: bool },
    /// Rewrites every live note of `notetype` (S5): each field of `record`
    /// takes the note's value at its origin, or the empty string.
    RewriteNotes {
        notetype: i64,
        record: Notetype,
        origins: Vec<Option<usize>>,
    },
    /// G9's commit half for a note type whose templates or fronts changed.
    Regenerate(Box<RegenerationPlan>),
    /// Moves notes to `record`'s note type with Change Note Type's effects
    /// (M9 steps 2 and 3): every live note of `from`, or only `notes`.
    MoveNotes {
        from: i64,
        record: Notetype,
        fields: Vec<Option<u32>>,
        /// For each target template position, the old ordinal; `None` when
        /// either side is a cloze note type (cards keep their ordinals).
        templates: Option<Vec<Option<u32>>>,
        notes: Option<Vec<NoteId>>,
    },
}

/// What the note type pass of a plan produced.
#[derive(Clone, Debug, Default)]
pub(crate) struct TypeStage {
    pub writes: Vec<TypeWrite>,
    /// Note types to remove once the notes are written, when no note uses
    /// them (J8), in ascending id.
    pub removals: Vec<i64>,
    pub routes: HashMap<i64, TypeRoute>,
    pub reasons: Vec<SchemaReason>,
    pub report: StructureReport,
    pub preview: MergePreview,
    /// Target note types (by the id the snapshot holds) whose notes this
    /// pass stamps before I9 compares: their `mod` reads as now.
    pub stamped: HashSet<i64>,
    /// Target notes that end on another note type: by id, the new `mid`.
    pub moved: HashMap<NoteId, i64>,
    /// Whole note types whose notes all move: old id to new id.
    pub merged_into: HashMap<i64, i64>,
    pub added: u64,
    pub updated: u64,
    pub copied: u64,
}

impl TypeStage {
    /// The `mid` a target note has once the note type writes ran.
    pub fn mid_after(&self, note: NoteId, snapshot_mid: i64) -> i64 {
        if let Some(mid) = self.moved.get(&note) {
            return *mid;
        }
        self.merged_into.get(&snapshot_mid).copied().unwrap_or(snapshot_mid)
    }
}

/// I7's "same schema": equal counts, and at each position ids when both
/// have one, else names byte for byte.
pub(crate) fn same_schema(a: &Notetype, b: &Notetype) -> bool {
    fn part(a_id: Option<i64>, b_id: Option<i64>, a_name: &str, b_name: &str) -> bool {
        match (a_id, b_id) {
            (Some(a), Some(b)) => a == b,
            _ => a_name == b_name,
        }
    }
    a.fields.len() == b.fields.len()
        && a.templates.len() == b.templates.len()
        && a.fields
            .iter()
            .zip(&b.fields)
            .all(|(a, b)| part(a.config.id, b.config.id, &a.name, &b.name))
        && a.templates
            .iter()
            .zip(&b.templates)
            .all(|(a, b)| part(a.config.id, b.config.id, &a.name, &b.name))
}

pub(crate) fn fronts(notetype: &Notetype) -> Vec<&str> {
    notetype.templates.iter().map(|t| t.config.q_format.as_str()).collect()
}

/// Whether a field or template name differs at some position.
pub(crate) fn names_differ(a: &Notetype, b: &Notetype) -> bool {
    a.fields.iter().zip(&b.fields).any(|(a, b)| a.name != b.name)
        || a.templates.iter().zip(&b.templates).any(|(a, b)| a.name != b.name)
}

/// A refused record ends the import (I7 "A refused record").
pub(crate) fn refused(id: i64, reason: meguri_engine::notetypes::InvalidNotetype) -> super::ApkgImportError {
    corrupt(format!("note type {id} is refused: {reason:?}"))
}

/// N4 as an unchecked save on an incoming record against the names taken so
/// far; returns whether step 7 changed the name (which stamps the row now).
pub(crate) fn normalized(
    record: &mut Notetype,
    package_id: i64,
    others: &[(NotetypeId, String)],
) -> ImportOutcome<bool> {
    let mut alone = record.clone();
    saves::normalize_incoming(&mut alone, &[]).map_err(|reason| refused(package_id, reason))?;
    saves::normalize_incoming(record, others).map_err(|reason| refused(package_id, reason))?;
    Ok(alone.name != record.name)
}

fn holds(rule: UpdateCondition, target: EpochSecs, incoming: EpochSecs) -> bool {
    match rule {
        UpdateCondition::IfNewer => target.0 < incoming.0,
        UpdateCondition::Always => target.0 != incoming.0,
        UpdateCondition::Never => false,
    }
}

/// An identity layout between two records of one note type.
pub(crate) fn identity_layout(before: &Notetype, after: &Notetype) -> ImportOutcome<LayoutReport> {
    let mut layout = LayoutReport::identity(before)?;
    layout.after = LayoutShape::of(after)?;
    Ok(layout)
}

/// I7: every taken note type, in ascending id, against the target's note
/// types as the plan's snapshot holds them.
pub(crate) fn plan_without_merge(
    taken: &[Notetype],
    targets: &BTreeMap<i64, Notetype>,
    options: &ImportOptions,
    reader: &impl ReadRows,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> ImportOutcome<TypeStage> {
    let mut stage = TypeStage::default();
    let mut others: Vec<(NotetypeId, String)> = targets.values().map(|nt| (nt.id, nt.name.clone())).collect();
    let mut copies = 0_usize;
    for incoming in taken {
        cancel.check()?;
        let id = incoming.id.0;
        let Some(same_id) = targets.get(&id) else {
            let mut record = incoming.clone();
            let stamp_now = normalized(&mut record, id, &others)?;
            others.push((record.id, record.name.clone()));
            stage.routes.insert(
                id,
                TypeRoute {
                    target: TypeTarget::Id(id),
                    record: record.clone(),
                    field_map: None,
                    template_map: None,
                },
            );
            stage.writes.push(TypeWrite::Put { record, stamp_now });
            stage.added += 1;
            continue;
        };
        let existing = if same_schema(same_id, incoming) {
            Some(same_id)
        } else {
            targets
                .values()
                .find(|other| other.id.0 != id && other.config.original_id == Some(id) && same_schema(other, incoming))
        };
        let Some(target) = existing else {
            let mut record = incoming.clone();
            record.id = NotetypeId(0);
            let stamp_now = normalized(&mut record, id, &others)?;
            // A placeholder id no real note type has: later names differ.
            others.push((NotetypeId(-1 - copies as i64), record.name.clone()));
            stage.routes.insert(
                id,
                TypeRoute {
                    target: TypeTarget::Copy(copies),
                    record: record.clone(),
                    field_map: None,
                    template_map: None,
                },
            );
            stage.writes.push(TypeWrite::AddCopy { record, stamp_now });
            stage.copied += 1;
            copies += 1;
            continue;
        };
        if !holds(options.update_notetypes, target.mtime, incoming.mtime) {
            stage.routes.insert(
                id,
                TypeRoute {
                    target: TypeTarget::Id(target.id.0),
                    record: target.clone(),
                    field_map: None,
                    template_map: None,
                },
            );
            continue;
        }
        let mut record = incoming.clone();
        record.id = target.id;
        let stamp_now = normalized(&mut record, id, &others)?;
        for entry in &mut others {
            if entry.0 == target.id {
                entry.1.clone_from(&record.name);
            }
        }
        let sort_changed = record.config.sort_field_idx != target.config.sort_field_idx;
        if sort_changed {
            stage.reasons.push(SchemaReason::SortFieldChanged);
            stage.report.reasons.insert(StructureReason::SortFieldChanged);
            stage.stamped.insert(target.id.0);
            stage.writes.push(TypeWrite::RewriteNotes {
                notetype: target.id.0,
                record: record.clone(),
                origins: (0..record.fields.len()).map(Some).collect(),
            });
        }
        if sort_changed || names_differ(target, &record) || target.config.kind != record.config.kind {
            stage.report.layouts.insert(target.id.0, identity_layout(target, &record)?);
        }
        stage.writes.push(TypeWrite::Put {
            record: record.clone(),
            stamp_now,
        });
        if fronts(target) != fronts(&record) {
            let planned = regenerate::plan(reader, &record, cancel, progress)?;
            stage.writes.push(TypeWrite::Regenerate(Box::new(planned)));
        }
        stage.updated += 1;
        stage.routes.insert(
            id,
            TypeRoute {
                target: TypeTarget::Id(target.id.0),
                record,
                field_map: None,
                template_map: None,
            },
        );
    }
    Ok(stage)
}
