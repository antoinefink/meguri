//! Note types (`specs/notetypes/`): the record as an aggregate of its
//! `notetypes`, `fields` and `templates` rows (`records` N1, N2), what a
//! local save writes (N3), the normalization every save applies (N4, in
//! [`names`]), the template checks (`validation`, in [`validate`]), the card
//! requirements (N6), adding from scratch, from a stock kind or by cloning
//! (N5, N7, `stock-kinds` K8), and the on-demand Image Occlusion note type
//! (N9).
//!
//! A plain save takes a [`Draft`]: the note type as read, edited, with the
//! digest of what it was read as. The save checks that digest against the
//! live rows inside its transaction (`engine/op-model` O7), so a draft made
//! before another save of the same note type fails with `Stale` instead of
//! writing back what that save changed.
//!
//! Each operation has two forms: an engine op on a [`Collection`], one
//! undoable step named as `specs/conformance/undo-ops.md` V4 names it, and an
//! `_in` form that does the same inside a write scope the caller holds (the
//! conformance adapters before U38, a new collection's bootstrap, composite
//! ops). Structure changes that rewrite notes and cards (fields and
//! templates added, removed, reordered or renamed, template text, the sort
//! field) are [`structure`]'s draft apply op; [`save_in`] refuses them.

pub mod change;
pub mod clone;
pub mod names;
pub mod read;
pub mod remove;
pub mod rename;
pub mod restore;
pub(crate) mod rewrite;
pub mod stock;
pub mod structure;
pub mod template_formats;
pub mod validate;
pub mod write;

use std::fmt;
use std::ops::{Deref, DerefMut};

use meguri_model::changes::OpSource;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::rng::RandomSource;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{ConfigValue, ReadRows, Table, WriteScope};

use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpResult, OpSpec};
use crate::undo::entry::StepKind;
use crate::undo::names as step_names;

pub use names::{FieldNameCheck, FieldNameRefusal, InvalidNotetype, check_typed_field_name};
pub use rename::{FieldIdentity, FieldRename, RenameFields, rename_fields};
pub use stock::StockKind;
pub use template_formats::{TemplateFormatsSaved, TemplateFormatsUpdate, TemplateIdentity, update_template_formats};
pub use validate::{CheckKind, TemplateFailure, TemplateSide, template_failures};
pub use write::AggregateDigest;

/// The config key of the current note type (`storage/config-table`).
pub const CURRENT_NOTETYPE_KEY: &str = "curModel";

/// `records` N5.4: the LaTeX header every new note type stores.
pub const DEFAULT_LATEX_PRE: &str = "\\documentclass[12pt]{article}\n\\special{papersize=3in,5in}\n\\usepackage[utf8]{inputenc}\n\\usepackage{amssymb,amsmath}\n\\pagestyle{empty}\n\\setlength{\\parindent}{0in}\n\\begin{document}\n";

/// `records` N5.4: the LaTeX footer, with no line end.
pub const DEFAULT_LATEX_POST: &str = "\\end{document}";

/// Meguri's base card CSS (`stock-kinds` K6): every stock kind's styling,
/// and a note type's made from scratch when the caller gives none.
pub const BASE_CARD_CSS: &str = include_str!("stock/templates/card.css");

/// Standard or cloze (`records` N2, `kind` 0 or 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NotetypeKind {
    #[default]
    Standard,
    Cloze,
}

impl NotetypeKind {
    /// The stored `kind`.
    pub const fn stored(self) -> i32 {
        match self {
            NotetypeKind::Standard => 0,
            NotetypeKind::Cloze => 1,
        }
    }

    /// A stored note type's kind: only 1 is cloze.
    pub fn of(notetype: &Notetype) -> NotetypeKind {
        if notetype.config.kind == 1 {
            NotetypeKind::Cloze
        } else {
            NotetypeKind::Standard
        }
    }
}

/// Why a save refused (`notetypes/validation` V2, V3), with the kind
/// `specs/conformance/notetype-ops.md` T7 records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotetypeRefusal {
    /// V2: `invalid_notetype`.
    Invalid(InvalidNotetype),
    /// V3's first failure.
    Template(TemplateFailure),
}

impl NotetypeRefusal {
    /// T7's kind.
    pub fn kind(&self) -> &'static str {
        match self {
            NotetypeRefusal::Invalid(_) => "invalid_notetype",
            NotetypeRefusal::Template(failure) => failure.kind.name(),
        }
    }
}

impl fmt::Display for NotetypeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotetypeRefusal::Invalid(reason) => write!(f, "invalid_notetype ({reason:?})"),
            NotetypeRefusal::Template(failure) => write!(
                f,
                "{} (template {}{})",
                failure.kind.name(),
                failure.template,
                failure.side.map(|s| format!(", {s:?}")).unwrap_or_default()
            ),
        }
    }
}

impl From<InvalidNotetype> for EngineError {
    fn from(reason: InvalidNotetype) -> EngineError {
        EngineError::NotetypeRefused(NotetypeRefusal::Invalid(reason))
    }
}

impl From<TemplateFailure> for EngineError {
    fn from(failure: TemplateFailure) -> EngineError {
        EngineError::NotetypeRefused(NotetypeRefusal::Template(failure))
    }
}

/// A note type made from scratch (`records` N5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewNotetype {
    pub name: String,
    pub kind: NotetypeKind,
    /// Field names, in order.
    pub fields: Vec<String>,
    pub templates: Vec<NewTemplate>,
    /// The styling; Meguri's base card CSS when `None`.
    pub css: Option<String>,
}

/// A new template: its name, review formats and optional browser formats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewTemplate {
    pub name: String,
    pub front: String,
    pub back: String,
    pub browser_front: String,
    pub browser_back: String,
}

/// A note type an op added, or found (the Image Occlusion note type).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: NotetypeId,
    /// The stored name, which may differ from the requested one (N4).
    pub name: String,
    /// Whether the op wrote it (always for an add; for the on-demand Image
    /// Occlusion note type, false when one existed).
    pub written: bool,
}

/// What a save stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: NotetypeId,
    /// The stored name after the save.
    pub name: String,
    /// False for a save that changed nothing (N3.1): nothing was written.
    pub written: bool,
}

/// A note type being edited: the note type as read, with the editor's
/// changes, and the digest of the aggregate it was read as. It reads and
/// edits as the note type it holds.
#[derive(Clone, Debug)]
pub struct Draft {
    pub notetype: Notetype,
    pub base: AggregateDigest,
}

impl Draft {
    /// A draft of a note type as read: its base is the note type's digest.
    pub fn of(notetype: Notetype) -> Draft {
        Draft {
            base: AggregateDigest::of(&notetype),
            notetype,
        }
    }
}

impl Deref for Draft {
    type Target = Notetype;

    fn deref(&self) -> &Notetype {
        &self.notetype
    }
}

impl DerefMut for Draft {
    fn deref_mut(&mut self) -> &mut Notetype {
        &mut self.notetype
    }
}

/// How a save checks the note type (`notetypes/validation` V1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// An unchecked save (the field editor's): no template checks (V3).
    pub skip_checks: bool,
}

/// A new field (`records` N5.2) with a fresh random id.
pub fn new_field(name: &str, random: &dyn RandomSource) -> NotetypeField {
    let mut config = FieldConfig::default();
    config.font_name = "Arial".to_owned();
    config.font_size = 20;
    config.id = Some(random_id(random));
    NotetypeField {
        ord: 0,
        name: name.to_owned(),
        config: ProtoBlob::new(config),
        prior_ord: None,
    }
}

/// A new template (`records` N5.3) with a fresh random id, its row's
/// `mtime_secs` and `usn` 0.
pub fn new_template(name: &str, front: &str, back: &str, random: &dyn RandomSource) -> NotetypeTemplate {
    let mut config = TemplateConfig::default();
    config.q_format = front.to_owned();
    config.a_format = back.to_owned();
    config.id = Some(random_id(random));
    NotetypeTemplate {
        ord: 0,
        name: name.to_owned(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        config: ProtoBlob::new(config),
        prior_ord: None,
    }
}

/// R905: a uniformly random signed 64-bit id.
pub(crate) fn random_id(random: &dyn RandomSource) -> i64 {
    i64::from_ne_bytes(random.next_u64().to_ne_bytes())
}

/// The note type config of a new note type (`records` N5.1, `stock-kinds`
/// K3): sort field 0, the default LaTeX values, `original_stock_kind` as
/// given, no original id.
pub(crate) fn new_config(kind: NotetypeKind, css: String, original_stock_kind: i32) -> NotetypeConfig {
    let mut config = NotetypeConfig::default();
    config.kind = kind.stored();
    config.css = css;
    config.latex_pre = DEFAULT_LATEX_PRE.to_owned();
    config.latex_post = DEFAULT_LATEX_POST.to_owned();
    config.original_stock_kind = original_stock_kind;
    config
}

/// The note type N5.1 makes from scratch, not yet normalized; its row
/// stamps are the scope's to set.
pub fn from_scratch(new: &NewNotetype, random: &dyn RandomSource) -> Notetype {
    let css = new.css.clone().unwrap_or_else(|| BASE_CARD_CSS.to_owned());
    Notetype {
        id: NotetypeId(0),
        name: new.name.clone(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        // Anki builds every new note type from its Basic kind and keeps
        // that value (N5.1).
        config: ProtoBlob::new(new_config(new.kind, css, StockKind::Basic.stored())),
        fields: new.fields.iter().map(|name| new_field(name, random)).collect(),
        templates: new
            .templates
            .iter()
            .map(|t| {
                let mut template = new_template(&t.name, &t.front, &t.back, random);
                template.config.q_format_browser = t.browser_front.clone();
                template.config.a_format_browser = t.browser_back.clone();
                template
            })
            .collect(),
    }
}

/// How a new note type's names are normalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Normalize {
    /// Every step of N4.
    All,
    /// N4 steps 1, 2 and 7 on the note type name only (a clone, N7).
    NameOnly,
}

/// N5: a checked add under a new id. Normalization (N4), the structural
/// rules and template checks (V2, V3), the card requirements (N6), the rows
/// written in the scope's stamping mode, and `curModel` set to the new id
/// when `select`.
pub(crate) fn insert(
    scope: &mut WriteScope<'_>,
    mut notetype: Notetype,
    normalize: Normalize,
    select: bool,
) -> Result<Added, EngineError> {
    let others = read::names(scope)?;
    match normalize {
        Normalize::All => names::normalize(&mut notetype, &others)?,
        Normalize::NameOnly => names::normalize_name(&mut notetype, &others)?,
    }
    if let Some(failure) = validate::first_failure(&notetype) {
        return Err(failure.into());
    }
    notetype.config.reqs = write::card_requirements(&notetype);
    let id = scope.add_notetype(&notetype)?;
    if select {
        scope.set_config(CURRENT_NOTETYPE_KEY, &ConfigValue::from(id.0))?;
    }
    Ok(Added {
        id,
        name: notetype.name,
        written: true,
    })
}

/// N5.1 inside a write scope: adds a note type from scratch and makes it the
/// current one.
pub fn add_in(scope: &mut WriteScope<'_>, random: &dyn RandomSource, new: &NewNotetype) -> Result<Added, EngineError> {
    insert(scope, from_scratch(new, random), Normalize::All, true)
}

/// `stock-kinds` K8 inside a write scope: adds a stock kind under `name`
/// (the kind's name when `None`) and makes it the current note type.
pub fn add_stock_in(
    scope: &mut WriteScope<'_>,
    random: &dyn RandomSource,
    kind: StockKind,
    name: Option<&str>,
) -> Result<Added, EngineError> {
    let mut notetype = kind.notetype(random);
    if let Some(name) = name {
        name.clone_into(&mut notetype.name);
    }
    insert(scope, notetype, Normalize::All, true)
}

/// N7 inside a write scope: adds a copy of `source` named `name` (`<source
/// name> copy` when `None`) and makes it the current note type.
pub fn clone_in(scope: &mut WriteScope<'_>, source: NotetypeId, name: Option<&str>) -> Result<Added, EngineError> {
    let source = scope.notetype(source)?.ok_or_else(|| unknown(source))?;
    insert(scope, clone::cloned(&source, name), Normalize::NameOnly, true)
}

/// N9 inside a write scope: the collection's Image Occlusion note type, the
/// stock kind added under `Image Occlusion` (N4 step 7) when there is none.
/// `curModel` keeps its value.
pub fn ensure_image_occlusion_in(scope: &mut WriteScope<'_>, random: &dyn RandomSource) -> Result<Added, EngineError> {
    if let Some(existing) = read::image_occlusion(scope)? {
        return Ok(Added {
            id: existing.id,
            name: existing.name,
            written: false,
        });
    }
    insert(scope, StockKind::ImageOcclusion.notetype(random), Normalize::All, false)
}

/// A plain save inside a write scope (`records` N3, N8): the draft, a
/// note type as read with its name, styling, field options, template names,
/// browser appearance or deck overrides changed, is normalized (N4),
/// checked unless `skip_checks` (V1, V3) and written row by row; a draft
/// equal to the stored note type writes nothing (N3.1). When the stored
/// aggregate is no longer the one the draft was made from (its base), the
/// save writes nothing and fails with `Stale` (`engine/op-model` O7): the
/// editor reads the note type again. Changes of the field or template lists,
/// field names, template text, the sort field or the kind are U34's
/// structure changes and are refused with `InvalidArgument`.
pub fn save_in(scope: &mut WriteScope<'_>, draft: &Draft, options: SaveOptions) -> Result<Saved, EngineError> {
    let id = draft.notetype.id;
    let stored = scope.notetype(id)?.ok_or_else(|| unknown(id))?;
    if AggregateDigest::of(&stored) != draft.base {
        return Err(EngineError::Stale {
            table: Table::Notetypes,
            key: id.0.to_string(),
        });
    }
    if let Some(change) = write::structure_change(&stored, &draft.notetype) {
        return Err(EngineError::InvalidArgument {
            message: format!("{change} is a note type structure change (U34), not a plain save"),
        });
    }
    let mut notetype = draft.notetype.clone();
    let others: Vec<(NotetypeId, String)> = read::names(scope)?
        .into_iter()
        .filter(|(id, _)| *id != notetype.id)
        .collect();
    names::normalize(&mut notetype, &others)?;
    if !options.skip_checks
        && let Some(failure) = validate::first_failure(&notetype)
    {
        return Err(failure.into());
    }
    if write::requirements_stale(&stored, &notetype) {
        notetype.config.reqs = write::card_requirements(&notetype);
    }
    let mut contribution = meguri_storage::journal::StructureReport::default();
    contribution.layouts.insert(notetype.id.0, meguri_storage::journal::LayoutReport::identity(&stored)?);
    let report = scope.start_structure_report(contribution)?;
    let written = scope.put_notetype(&notetype)?;
    scope.complete_structure_report(report)?;
    Ok(Saved {
        id: notetype.id,
        name: notetype.name,
        written: written.changed(),
    })
}

fn unknown(id: NotetypeId) -> EngineError {
    EngineError::InvalidArgument {
        message: format!("no note type {id}"),
    }
}

/// The spec of a note type op: a user op that records the undo step `step`.
fn op_spec(name: &'static str, step: &'static str) -> OpSpec {
    OpSpec::named(name, OpSource::UserUndoable).undo_step(step, StepKind::Other)
}

/// N5.1 as an op: "Add Note Type".
pub fn add(collection: &Collection, new: NewNotetype) -> Result<OpResult<Added>, EngineError> {
    collection.run(op_spec("add_notetype", step_names::ADD_NOTE_TYPE), move |ctx| {
        let (scope, random) = ctx.scope_and_random();
        add_in(scope, random, &new)
    })
}

/// K8 as an op: "Add Note Type".
pub fn add_stock(
    collection: &Collection,
    kind: StockKind,
    name: Option<String>,
) -> Result<OpResult<Added>, EngineError> {
    collection.run(op_spec("add_stock_notetype", step_names::ADD_NOTE_TYPE), move |ctx| {
        let (scope, random) = ctx.scope_and_random();
        add_stock_in(scope, random, kind, name.as_deref())
    })
}

/// N7 as an op: "Add Note Type".
pub fn clone_notetype(
    collection: &Collection,
    source: NotetypeId,
    name: Option<String>,
) -> Result<OpResult<Added>, EngineError> {
    collection.run(op_spec("clone_notetype", step_names::ADD_NOTE_TYPE), move |ctx| {
        clone_in(ctx.scope(), source, name.as_deref())
    })
}

/// N9 as an op: "Add Note Type" when it adds one.
pub fn ensure_image_occlusion(collection: &Collection) -> Result<OpResult<Added>, EngineError> {
    collection.run(op_spec("ensure_io_notetype", step_names::ADD_NOTE_TYPE), move |ctx| {
        let (scope, random) = ctx.scope_and_random();
        ensure_image_occlusion_in(scope, random)
    })
}

/// A plain save as an op: "Update Note Type", recorded only when it wrote.
/// A stale draft is not planned again: its edits are the caller's.
pub fn save(collection: &Collection, draft: Draft, options: SaveOptions) -> Result<OpResult<Saved>, EngineError> {
    collection.run(op_spec("update_notetype", step_names::UPDATE_NOTE_TYPE), move |ctx| {
        save_in(ctx.scope(), &draft, options)
    })
}
