//! Production field rename prerequisite (`notetypes/field-rename` F1–F5).
//! The request cannot carry unrelated draft edits. Planning uses U60 source
//! spans and G9; the writer validates the captured aggregate and identities
//! again before saving the type and committing G9 in the same journal.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use meguri_model::changes::OpSource;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::{Notetype, NotetypeField};
use meguri_storage::{ReadRows, ReaderRole, Table, WriteScope};

use super::rewrite::{Map, Side, Target, rewrite};
use super::{AggregateDigest, InvalidNotetype, NotetypeKind, Saved, names, write};
use crate::cardgen::regenerate::{self, RegenerationPlan};
use crate::executor::catalog;
use crate::undo::{entry::StepKind, names::UPDATE_NOTE_TYPE};
use crate::{CancelToken, Collection, EngineError, OpResult, OpSpec, Plan, ProgressSink};

/// Both values as captured when the field was read. Ordinals disambiguate
/// legacy missing/repeated IDs; optional IDs alone never identify a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldIdentity {
    pub ord: u32,
    pub id: Option<i64>,
}

impl FieldIdentity {
    pub fn of(field: &NotetypeField) -> Self {
        Self {
            ord: field.ord,
            id: field.config.id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldRename {
    pub field: FieldIdentity,
    pub name: String,
}

/// Rename-only input: it cannot add, remove or move fields/templates, edit
/// templates, options, styling, kind or sort field. Typed-name checking for
/// editor callers remains the separate V5 API; save normalization is N4.
#[derive(Clone, Debug)]
pub struct RenameFields {
    pub notetype: NotetypeId,
    pub base: AggregateDigest,
    pub fields: Vec<FieldRename>,
}

/// A composable plan with private contents: only the typed rename planner
/// can construct one. Its captured base is rechecked by `commit_in`.
#[derive(Clone, Debug)]
pub struct FieldRenamePlan {
    request: RenameFields,
    renamed: Notetype,
    changed: bool,
    regeneration: Option<RegenerationPlan>,
}

impl FieldRenamePlan {
    /// Whether the commit generates cards (G9): its write is then declared.
    pub(crate) fn regenerates(&self) -> bool {
        self.regeneration.is_some()
    }

    /// The note type the commit stores.
    pub(crate) fn renamed(&self) -> &Notetype {
        &self.renamed
    }
}

fn invalid(message: &str) -> EngineError {
    EngineError::InvalidArgument {
        message: message.to_owned(),
    }
}

fn validate(reader: &impl ReadRows, request: &RenameFields) -> Result<Notetype, EngineError> {
    let stored = reader
        .notetype(request.notetype)?
        .ok_or_else(|| invalid("unknown note type"))?;
    if AggregateDigest::of(&stored) != request.base {
        return Err(EngineError::Stale {
            table: Table::Notetypes,
            key: request.notetype.0.to_string(),
        });
    }
    if request.fields.is_empty() {
        return Err(invalid("a field rename requires at least one field"));
    }
    let mut seen = HashSet::new();
    for rename in &request.fields {
        if !seen.insert(rename.field.ord) {
            return Err(invalid("duplicate field identity"));
        }
        // Use the stored ordinal, not a name or the optional ID as a key.
        let field = stored
            .fields
            .iter()
            .find(|field| field.ord == rename.field.ord)
            .ok_or_else(|| invalid("field ordinal is out of range"))?;
        if field.config.id != rename.field.id {
            return Err(invalid("field ordinal no longer identifies the captured field ID"));
        }
    }
    Ok(stored)
}

/// Plans off the writer, without writes. Each original reference is looked
/// up once in the complete normalized old-to-final map, so swaps cannot
/// cascade. Field-editor saves skip V3 (V1), including malformed texts.
pub fn plan(
    reader: &impl ReadRows,
    request: RenameFields,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<FieldRenamePlan, EngineError> {
    cancel.check()?;
    let stored = validate(reader, &request)?;
    let mut renamed = stored.clone();
    for rename in &request.fields {
        let field = renamed
            .fields
            .iter_mut()
            .find(|field| field.ord == rename.field.ord)
            .expect("validated field identity");
        field.name.clone_from(&rename.name);
    }
    if renamed.templates.is_empty() {
        return Err(InvalidNotetype::NoTemplate.into());
    }
    names::normalize_field_names(&mut renamed.fields)?;
    let map: Map<'_> = stored
        .fields
        .iter()
        .zip(&renamed.fields)
        .filter(|(before, after)| before.name != after.name)
        .map(|(before, after)| (before.name.as_str(), Target::Renamed(after.name.as_str())))
        .collect();
    let changed = !map.is_empty();
    if !changed {
        // No fallback, req repair, row normalization or write on a no-op.
        drop(map);
        return Ok(FieldRenamePlan {
            request,
            renamed: stored,
            changed: false,
            regeneration: None,
        });
    }
    let cloze = NotetypeKind::of(&renamed) == NotetypeKind::Cloze;
    let first = renamed.fields[0].name.as_str();
    for template in &mut renamed.templates {
        cancel.check()?;
        let config = &mut template.config;
        config.q_format = rewrite(&config.q_format, &map, first, cloze, Side::Front);
        config.a_format = rewrite(&config.a_format, &map, first, cloze, Side::Back);
        config.q_format_browser = rewrite(&config.q_format_browser, &map, first, cloze, Side::BrowserFront);
        config.a_format_browser = rewrite(&config.a_format_browser, &map, first, cloze, Side::BrowserBack);
    }
    drop(map);
    // Preserve unknown members of retained requirement records as well as
    // those of the containing config. Only known requirement values change.
    let reqs = write::card_requirements(&renamed)
        .into_iter()
        .map(|computed| {
            let mut requirement = renamed
                .config
                .reqs
                .iter()
                .find(|old| old.card_ord == computed.card_ord)
                .cloned()
                .unwrap_or_default();
            requirement.card_ord = computed.card_ord;
            requirement.kind = computed.kind;
            requirement.field_ords = computed.field_ords;
            requirement
        })
        .collect();
    renamed.config.reqs = reqs;
    let fronts_changed = stored
        .templates
        .iter()
        .zip(&renamed.templates)
        .any(|(before, after)| before.config.q_format != after.config.q_format);
    let regeneration = if fronts_changed {
        Some(regenerate::plan(reader, &renamed, cancel, progress)?)
    } else {
        None
    };
    cancel.check()?;
    Ok(FieldRenamePlan {
        request,
        renamed,
        changed,
        regeneration,
    })
}

/// Apply inside the caller's executor transaction. Validate before any
/// write, including normalized no-ops. G9 re-derives live note/card
/// membership here; it never runs as a second operation or undo step.
pub fn commit_in(scope: &mut WriteScope<'_>, planned: &FieldRenamePlan) -> Result<Saved, EngineError> {
    commit_counted(scope, planned).map(|(saved, _)| saved)
}

/// [`commit_in`], with the number of cards G9 generated.
pub(crate) fn commit_counted(
    scope: &mut WriteScope<'_>,
    planned: &FieldRenamePlan,
) -> Result<(Saved, usize), EngineError> {
    let stored = validate(scope, &planned.request)?;
    if !planned.changed {
        let saved = Saved {
            id: planned.renamed.id,
            name: planned.renamed.name.clone(),
            written: false,
        };
        return Ok((saved, 0));
    }
    let mut contribution = meguri_storage::journal::StructureReport::default();
    contribution.layouts.insert(stored.id.0, meguri_storage::journal::LayoutReport::identity(&stored)?);
    let report = scope.start_structure_report(contribution)?;
    let written = scope.put_notetype(&planned.renamed)?;
    scope.complete_structure_report(report)?;
    let cards = match &planned.regeneration {
        Some(regeneration) => regenerate::commit_in(scope, regeneration)?.len(),
        None => 0,
    };
    let saved = Saved {
        id: planned.renamed.id,
        name: planned.renamed.name.clone(),
        written: written.changed(),
    };
    Ok((saved, cards))
}

/// Blocking worker entry point, with G9's progress/cancellation and declared
/// long-write contract. The write phase completes past the cancel point.
/// A stale caller base is never replaced with a newly captured digest.
pub fn rename_fields(
    collection: &Collection,
    request: RenameFields,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<Saved>, EngineError> {
    let spec = OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
        .undo_step(UPDATE_NOTE_TYPE, StepKind::Other)
        .cancel(cancel.clone())
        .progress(progress.clone());
    crate::plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        spec,
        |context| {
            Ok(Plan {
                value: plan(&**context.snapshot(), request.clone(), &cancel, &progress)?,
                // The aggregate/identities are checked under the writer; G9
                // reselects notes/cards and resolves placement on live rows.
                preconditions: Vec::new(),
            })
        },
        |spec, planned| {
            if planned.regeneration.is_some() {
                spec.clone().declare_long_write(Duration::from_secs(1))
            } else {
                // In particular a normalized no-op publishes no write-delay
                // events and takes no journal or undo step.
                spec.clone()
            }
        },
        |context, planned| commit_in(context.scope(), &planned),
    )
}
