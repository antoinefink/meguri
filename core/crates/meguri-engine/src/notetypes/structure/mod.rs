//! The draft apply op (`specs/notetypes/structure-changes.md` S1 to S10,
//! S13): one save of a whole draft of one note type that adds, removes,
//! moves or renames fields and templates, changes the sort field or a front,
//! and rewrites the type's notes and cards to match, as one transaction and
//! one "Update Note Type" undo step. The removal preview (S11) is
//! [`preview`].
//!
//! The op plans on a reader snapshot (`plan.rs`: the diff, the rewritten
//! templates, the card map and the generation plan, the refusals of S1.2)
//! and writes in one short phase (`commit.rs`) whose checks on the live rows
//! of its own transaction all come before its first write. The safety
//! backup of a schema-classified save is taken between the two
//! (`package/backup-triggers` T6.1, T6.2), so a failed backup stops the op
//! before anything is written.
//!
//! The losses the user confirmed (S1.2 as changed 2026-11-20,
//! `notetypes/draft-save` D8) are counted again twice: on the planning
//! snapshot, so an out-of-date confirmation costs no backup, and inside the
//! write transaction right after the staleness checks. A live count larger
//! than its confirmed count, compared per category, refuses with
//! `LossesChanged`, the op's own read as payload, and writes nothing.

pub(crate) mod commit;
pub(crate) mod plan;
pub mod preview;
mod summary;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use meguri_model::changes::OpSource;
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_model::proto::notetype::FieldConfig;
use meguri_storage::{ReaderRole, Table};

pub use preview::{RemovalPreview, preview_removal, removal_preview};
pub use summary::{DraftChange, DraftSummary, RestoreSummary};


use super::AggregateDigest;
use crate::executor::catalog;
use crate::undo::entry::StepKind;
use crate::undo::names::UPDATE_NOTE_TYPE;
use crate::{CancelToken, Collection, EngineError, OpResult, OpSpec, Plan, ProgressSink, SchemaReason};

/// The options of a field the draft holds (`model/protobuf-records` P6.2).
/// A field with an origin keeps everything else of its base field: id, tag,
/// protection, `other` bytes and unknown fields (S1.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldOptions {
    pub sticky: bool,
    pub rtl: bool,
    pub font_name: String,
    pub font_size: u32,
    pub description: String,
    pub plain_text: bool,
    pub collapsed: bool,
    pub exclude_from_search: bool,
}

impl FieldOptions {
    /// A stored field's options.
    pub fn of(config: &FieldConfig) -> FieldOptions {
        FieldOptions {
            sticky: config.sticky,
            rtl: config.rtl,
            font_name: config.font_name.clone(),
            font_size: config.font_size,
            description: config.description.clone(),
            plain_text: config.plain_text,
            collapsed: config.collapsed,
            exclude_from_search: config.exclude_from_search,
        }
    }

    /// A new field's options (`notetypes/records` N5.2).
    pub fn new_field() -> FieldOptions {
        FieldOptions {
            sticky: false,
            rtl: false,
            font_name: "Arial".to_owned(),
            font_size: 20,
            description: String::new(),
            plain_text: false,
            collapsed: false,
            exclude_from_search: false,
        }
    }

    pub(crate) fn apply(&self, config: &mut FieldConfig) {
        config.sticky = self.sticky;
        config.rtl = self.rtl;
        self.font_name.clone_into(&mut config.font_name);
        config.font_size = self.font_size;
        self.description.clone_into(&mut config.description);
        config.plain_text = self.plain_text;
        config.collapsed = self.collapsed;
        config.exclude_from_search = self.exclude_from_search;
    }
}

/// One field of a draft, in the draft's order (S1.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftField {
    /// The base ordinal (position in the base) the field comes from; `None`
    /// for a field the draft adds.
    pub origin: Option<u32>,
    pub name: String,
    pub options: FieldOptions,
}

/// One template of a draft, in the draft's order (S1.1). A template with an
/// origin keeps its base template's id, `other` bytes, unknown fields and
/// row stamps (`notetypes/records` N3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftTemplate {
    /// The base ordinal the template comes from; `None` for a new one.
    pub origin: Option<u32>,
    pub name: String,
    pub front: String,
    pub back: String,
    pub browser_front: String,
    pub browser_back: String,
    /// The deck override's deck id, 0 for none.
    pub deck: i64,
    pub browser_font_name: String,
    pub browser_font_size: u32,
}

/// What the user confirmed would be lost (`notetypes/draft-save` D8): one
/// count per loss category, each compared on its own with its live count.
/// A category absent here was confirmed as 0. A precondition, not a
/// confirmation input (`notetypes/schema-changes` G4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfirmedLosses {
    /// Per removed base field ordinal: the notes with content (E1).
    pub fields: BTreeMap<u32, u64>,
    /// Per removed base template ordinal: its cards.
    pub templates: BTreeMap<u32, u64>,
    /// `restore-to-default` D4.1 and D7.1: the cards a cloze note type
    /// restored as a standard kind loses past the stock template count.
    pub kind_change_cards: u64,
}

impl ConfirmedLosses {
    /// Whether every count is 0 (`restore-to-default` D6).
    pub fn is_zero(&self) -> bool {
        self.kind_change_cards == 0
            && self.fields.values().all(|n| *n == 0)
            && self.templates.values().all(|n| *n == 0)
    }
}

/// A whole draft of one note type (S1.1).
#[derive(Clone, Debug)]
pub struct StructureDraft {
    pub notetype: NotetypeId,
    /// The `AggregateDigest` of the base, taken when the draft was loaded.
    pub base: AggregateDigest,
    pub name: String,
    pub css: String,
    pub fields: Vec<DraftField>,
    pub templates: Vec<DraftTemplate>,
    /// A position in `fields`, or `None` to keep the base's sort field
    /// (S3 step 4).
    pub sort_field: Option<u32>,
    /// `notetypes/validation` V1: true for the field editor's saves.
    pub skip_checks: bool,
    /// The losses the save sheet showed (S1.2; zero for each when none was
    /// shown). Callers with no question pass the counts they read.
    pub confirmed: ConfirmedLosses,
}

impl StructureDraft {
    /// An unchanged draft of a note type as read: every field and template
    /// has its position as origin, and the base is the note type's digest.
    pub fn of(notetype: &Notetype) -> StructureDraft {
        StructureDraft {
            notetype: notetype.id,
            base: AggregateDigest::of(notetype),
            name: notetype.name.clone(),
            css: notetype.config.css.clone(),
            fields: notetype
                .fields
                .iter()
                .enumerate()
                .map(|(at, field)| DraftField {
                    origin: Some(position(at)),
                    name: field.name.clone(),
                    options: FieldOptions::of(&field.config),
                })
                .collect(),
            templates: notetype
                .templates
                .iter()
                .enumerate()
                .map(|(at, template)| {
                    let c = &template.config;
                    DraftTemplate {
                        origin: Some(position(at)),
                        name: template.name.clone(),
                        front: c.q_format.clone(),
                        back: c.a_format.clone(),
                        browser_front: c.q_format_browser.clone(),
                        browser_back: c.a_format_browser.clone(),
                        deck: c.target_deck_id,
                        browser_font_name: c.browser_font_name.clone(),
                        browser_font_size: c.browser_font_size,
                    }
                })
                .collect(),
            sort_field: None,
            skip_checks: false,
            confirmed: ConfirmedLosses::default(),
        }
    }
}

/// What a structure save did (S1.3). A save that changes nothing writes
/// nothing and reports zeros.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructureSaved {
    pub id: NotetypeId,
    /// The stored name after the save (N4).
    pub name: String,
    pub written: bool,
    pub notes_rewritten: u64,
    pub cards_added: u64,
    pub cards_removed: u64,
    pub cards_renumbered: u64,
    /// Notes of the type that had a card before the save and have none
    /// after it (R933).
    pub notes_without_cards: u64,
    /// The schema reasons the save carried (G2), in G2's order.
    pub reasons: Vec<SchemaReason>,
}

/// A refusal of a structure save beyond `notetypes/validation`'s: nothing
/// was written and no undo step was recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructureRefusal {
    /// S1.2 item 3: the draft removes a protected field (its base ordinal).
    ProtectedField { field: u32 },
    /// S1.2 item 4: a note of the type stores more values than the base
    /// has fields.
    NoteFieldCountMismatch { note: NoteId },
    /// `notetypes/draft-save` D8: a live loss count is larger than the
    /// confirmed one; nothing was written.
    LossesChanged(Box<Losses>),
    /// S12: the collection's only note type.
    LastNotetype,
    /// `restore-to-default` D2: no usable stock kind.
    NeedsStockKind,
}

/// `LossesChanged`'s payload: the op's own read, taken in the refusing
/// transaction (D8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Losses {
    /// The draft save: D2's summary.
    Draft(DraftSummary),
    /// A note type removal (S12): the live counts.
    Removal { notes: u64, cards: u64 },
    /// Restore to Default: a new `restore_summary` (D7.1).
    Restore(RestoreSummary),
}

impl StructureRefusal {
    /// A stable name, as the conformance runner records refusals.
    pub fn kind(&self) -> &'static str {
        match self {
            StructureRefusal::ProtectedField { .. } => "protected_field",
            StructureRefusal::NoteFieldCountMismatch { .. } => "note_field_count_mismatch",
            StructureRefusal::LossesChanged(_) => "losses_changed",
            StructureRefusal::LastNotetype => "last_notetype",
            StructureRefusal::NeedsStockKind => "needs_stock_kind",
        }
    }
}

impl fmt::Display for StructureRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructureRefusal::ProtectedField { field } => write!(f, "protected_field (base field {field})"),
            StructureRefusal::NoteFieldCountMismatch { note } => write!(f, "note_field_count_mismatch (note {note})"),
            StructureRefusal::LossesChanged(losses) => write!(f, "losses_changed ({losses:?})"),
            other => f.write_str(other.kind()),
        }
    }
}

impl From<StructureRefusal> for EngineError {
    fn from(refusal: StructureRefusal) -> EngineError {
        EngineError::NotetypeStructure(refusal)
    }
}

pub(crate) fn position(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

pub(crate) fn stale(id: NotetypeId) -> EngineError {
    EngineError::Stale {
        table: Table::Notetypes,
        key: id.0.to_string(),
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument {
        message: message.into(),
    }
}

/// The write phase's spec for a plan: its schema reasons, which decide the
/// safety backup (G3, T6.1), and a declared long write when it rewrites
/// notes or cards (S10, O8).
pub(crate) fn write_spec(spec: &OpSpec, reasons: &[SchemaReason], touches_notes: bool) -> OpSpec {
    let spec = spec.clone().schema_changes(reasons.iter().copied());
    if touches_notes {
        spec.declare_long_write(Duration::from_secs(1))
    } else {
        spec
    }
}

/// The draft apply op: plans on a maintenance snapshot (cancellable, with
/// progress), takes the safety backup of a schema-classified save, then
/// writes in one transaction and one "Update Note Type" step. A `Stale`
/// draft is never planned again (S1.2 item 1): the caller reloads.
pub fn apply(
    collection: &Collection,
    draft: StructureDraft,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<StructureSaved>, EngineError> {
    let spec = OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
        .undo_step(UPDATE_NOTE_TYPE, StepKind::Other)
        .cancel(cancel.clone())
        .progress(progress.clone());
    // S1.2 item 1: `Stale` is returned, never planned again.
    crate::plan_and_commit_with_replans(
        collection,
        ReaderRole::Maintenance,
        spec,
        0,
        |context| {
            Ok(Plan {
                value: plan::plan_draft(&**context.snapshot(), &draft, &cancel, &progress)?,
                // The base token is checked again on the live rows inside the
                // write transaction (S1.2 item 1).
                preconditions: Vec::new(),
            })
        },
        |spec, planned| planned.write_spec(spec),
        |context, planned| commit::commit(context, &planned),
    )
}
