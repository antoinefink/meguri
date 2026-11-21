//! The note type area's adapters (`specs/conformance/notetype-ops.md` T10):
//! `setup.notetypes` (dsl.md D6.4, each entry added as `add_notetype` adds
//! it), the ops `add_notetype`, `add_stock_notetype`, `clone_notetype`,
//! `ensure_io_notetype` and `update_notetype` (T5: a plain save for the
//! plain edits of T10.1, U34's draft apply op for the others), and the
//! observations `notetype_structure` (T8) and `notetype_templates`
//! (`notetype-structure-ops.md` M3). Each op uses the public hosted engine
//! API, including undo and stale-draft checks.

use meguri_engine::notetypes::structure::{self, DraftField, DraftTemplate, FieldOptions, StructureDraft};
use meguri_engine::notetypes::{
    self, AggregateDigest, Draft, NewNotetype, NewTemplate, NotetypeKind, SaveOptions, StockKind,
};
use meguri_engine::{CancelToken, EngineError, ProgressSink};
use meguri_model::deck::{human_name, native_name};
use meguri_model::ids::DeckId;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, StorageError, Table};
use serde_json::{Map, Value, json};

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{
    self, NotetypeEdit, NotetypeSpec, ObservationArgs, ObservationRequest, Setup, Step, TemplateEdit,
};

/// T7: what the four saving ops may record.
const REJECTIONS: &[&str] = &[
    "invalid_notetype",
    "template_parse_error",
    "duplicate_front",
    "no_front_field",
    "unknown_field",
    "missing_cloze",
];

/// T10.1: the `update_notetype` edits a plain save applies (U29) and those
/// of U34's draft apply op.
const EDITS: [&str; 14] = [
    "update_notetype.rename",
    "update_notetype.css",
    "update_notetype.field_options",
    "update_notetype.rename_template",
    "update_notetype.template",
    "update_notetype.sort_field",
    "update_notetype.add_field",
    "update_notetype.remove_field",
    "update_notetype.rename_field",
    "update_notetype.move_field",
    "update_notetype.add_template",
    "update_notetype.remove_template",
    "update_notetype.move_template",
    "update_notetype.template_text",
];

/// Registers `setup.notetypes`, the ops and
/// the `notetype_structure` kind.
pub(super) fn register(registry: &mut Registry) {
    registry.setup("notetypes", setup_notetypes);
    registry.op_rejecting("add_notetype", REJECTIONS, add_notetype);
    registry.op_rejecting("add_stock_notetype", REJECTIONS, add_stock_notetype);
    registry.op_rejecting("clone_notetype", REJECTIONS, clone_notetype);
    registry.op_rejecting("update_notetype", REJECTIONS, update_notetype);
    for part in EDITS {
        registry.op_part(part);
    }
    registry.op("ensure_io_notetype", ensure_io_notetype);
    registry.observation("notetype_structure", notetype_structure);
    registry.observation("notetype_templates", notetype_templates);
}

fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn unknown_ref(detail: impl Into<String>) -> ExecError {
    ExecError::new("unknown_ref", detail)
}

/// Runs `f` in one local write scope on the session's writer and commits
/// it; a refused save rolls back and is recorded as T7's kind.
fn outcome<T>(value: Result<meguri_engine::OpResult<T>, EngineError>) -> Result<Result<T, &'static str>, ExecError> {
    match value {
        Ok(result) => Ok(Ok(result.value)),
        Err(EngineError::NotetypeRefused(refusal)) => Ok(Err(refusal.kind())),
        Err(other) => Err(core(other)),
    }
}

/// An op's output: `{name}` or the rejection.
fn named(result: Result<String, &'static str>) -> OpResult {
    match result {
        Ok(name) => OpResult::Output(json!({ "name": name })),
        Err(kind) => OpResult::Rejected(kind),
    }
}

/// D6.4 and T2: a `setup.notetypes` entry or an `add_notetype` argument.
fn new_notetype(spec: &NotetypeSpec) -> NewNotetype {
    NewNotetype {
        name: spec.name.clone(),
        kind: match spec.kind {
            scenario::NotetypeKind::Standard => NotetypeKind::Standard,
            scenario::NotetypeKind::Cloze => NotetypeKind::Cloze,
        },
        fields: spec.fields.clone(),
        templates: spec
            .templates
            .iter()
            .map(|t| NewTemplate {
                name: t.name.clone(),
                front: t.front.clone(),
                back: t.back.clone(),
                browser_front: t.browser_front.clone(),
                browser_back: t.browser_back.clone(),
            })
            .collect(),
        css: Some(spec.css.clone()),
    }
}

/// D6.4: each entry added as `add_notetype` adds it; a refusal makes the
/// scenario invalid.
fn setup_notetypes(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    for spec in setup.notetypes.iter().flatten() {
        let collection = session.engine_collection()?;
        if let Err(kind) = outcome(notetypes::add(&collection, new_notetype(spec)))? {
            return Err(invalid(format!("setup note type {:?} refused: {kind}", spec.name)));
        }
    }
    Ok(())
}

/// T2.
fn add_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::AddNotetype(spec) = step else {
        return Err(invalid("expected add_notetype"));
    };
    let collection = session.engine_collection()?;
    Ok(named(
        outcome(notetypes::add(&collection, new_notetype(spec)))?.map(|a| a.name),
    ))
}

/// T3.
fn add_stock_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::AddStockNotetype(args) = step else {
        return Err(ExecError::new(
            "internal_error",
            "add_stock_notetype adapter called for another op",
        ));
    };
    let kind = match args.kind {
        scenario::StockKind::Basic => StockKind::Basic,
        scenario::StockKind::BasicAndReversed => StockKind::BasicAndReversed,
        scenario::StockKind::BasicOptionalReversed => StockKind::BasicOptionalReversed,
        scenario::StockKind::BasicTyping => StockKind::BasicTyping,
        scenario::StockKind::Cloze => StockKind::Cloze,
        scenario::StockKind::ImageOcclusion => StockKind::ImageOcclusion,
    };
    let collection = session.engine_collection()?;
    Ok(named(
        outcome(notetypes::add_stock(&collection, kind, args.name.clone()))?.map(|a| a.name),
    ))
}

/// The note type stored under exactly `name` (T1).
fn find(scope: &impl ReadRows, name: &str) -> Result<Result<Notetype, String>, StorageError> {
    Ok(notetypes::read::by_name(scope, name)?.ok_or_else(|| format!("no note type is named {name:?} (T1)")))
}

/// T4.
fn clone_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::CloneNotetype(args) = step else {
        return Err(invalid("expected clone_notetype"));
    };
    let collection = session.engine_collection()?;
    let source = {
        let reader = collection.reader(meguri_storage::ReaderRole::Query).map_err(core)?;
        find(&*reader, &args.notetype).map_err(core)?.map_err(unknown_ref)?
    };
    Ok(named(
        outcome(notetypes::clone_notetype(&collection, source.id, args.name.clone()))?.map(|a| a.name),
    ))
}

/// T6.
fn ensure_io_notetype(session: &mut Session<'_>, _step: &Step) -> Result<OpResult, ExecError> {
    let collection = session.engine_collection()?;
    match outcome(notetypes::ensure_image_occlusion(&collection))? {
        Ok(value) => Ok(OpResult::Output(json!({ "name": value.name }))),
        Err(kind) => Err(core(format!("ensure_io_notetype refused: {kind}"))),
    }
}

/// T5: the edits applied in order to a draft of the stored note type and
/// saved once (checked unless `skip_checks`): a plain save when they are
/// only T10.1's plain edits (U29), U34's draft apply op otherwise, with the
/// base token loaded and no confirmation (T9).
fn update_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::UpdateNotetype(args) = step else {
        return Err(invalid("expected update_notetype"));
    };
    let collection = session.engine_collection()?;
    let plain = args.edits.iter().all(is_plain);
    if plain {
        let draft = {
            let reader = collection.reader(meguri_storage::ReaderRole::Query).map_err(core)?;
            let stored = find(&*reader, &args.notetype).map_err(core)?.map_err(unknown_ref)?;
            let base = AggregateDigest::of(&stored);
            Draft {
                notetype: apply_edits(&*reader, stored, &args.edits)?,
                base,
            }
        };
        return Ok(named(
            outcome(notetypes::save(
                &collection,
                draft,
                SaveOptions {
                    skip_checks: args.skip_checks,
                },
            ))?
            .map(|s| s.name),
        ));
    }
    let draft = {
        let reader = collection.reader(meguri_storage::ReaderRole::Query).map_err(core)?;
        let stored = find(&*reader, &args.notetype).map_err(core)?.map_err(unknown_ref)?;
        let mut draft = structure_draft(&*reader, &stored, &args.edits)?;
        draft.skip_checks = args.skip_checks;
        draft
    };
    let saved = structure::apply(&collection, draft, CancelToken::new(), ProgressSink::new());
    Ok(named(outcome(saved)?.map(|s| s.name)))
}

/// T10.1's plain edits.
fn is_plain(edit: &NotetypeEdit) -> bool {
    match edit {
        NotetypeEdit::Rename(_)
        | NotetypeEdit::Css(_)
        | NotetypeEdit::FieldOptions(_)
        | NotetypeEdit::RenameTemplate(_) => true,
        NotetypeEdit::Template(edit) => edit.front.is_none() && edit.back.is_none(),
        _ => false,
    }
}

/// T5's edits on a draft of `stored` for the draft apply op: fields and
/// templates keep their stored positions as origins, wherever they move.
fn structure_draft(reader: &impl ReadRows, stored: &Notetype, edits: &[NotetypeEdit]) -> Result<StructureDraft, ExecError> {
    let mut draft = StructureDraft::of(stored);
    // The sort field is a field of the stored note type (T5), followed by
    // its origin through later moves.
    let mut sort_origin = None;
    let field_at = |draft: &StructureDraft, name: &str| {
        draft
            .fields
            .iter()
            .position(|f| f.name == name)
            .ok_or_else(|| unknown_ref(format!("no field {name:?} in the draft (T5)")))
    };
    let template_at = |draft: &StructureDraft, name: &str| {
        draft
            .templates
            .iter()
            .position(|t| t.name == name)
            .ok_or_else(|| unknown_ref(format!("no template {name:?} in the draft (T5)")))
    };
    for edit in edits {
        match edit {
            NotetypeEdit::Rename(name) => name.clone_into(&mut draft.name),
            NotetypeEdit::Css(css) => css.clone_into(&mut draft.css),
            NotetypeEdit::SortField(name) => {
                let at = field_at(&draft, name)?;
                let origin = draft.fields[at]
                    .origin
                    .ok_or_else(|| invalid(format!("sort field {name:?} was added by this update (T5)")))?;
                sort_origin = Some(origin);
            }
            NotetypeEdit::AddField(name) => draft.fields.push(DraftField {
                origin: None,
                name: name.clone(),
                options: FieldOptions::new_field(),
            }),
            NotetypeEdit::RemoveField(name) => {
                let at = field_at(&draft, name)?;
                draft.fields.remove(at);
            }
            NotetypeEdit::RenameField(pair) => {
                let at = field_at(&draft, &pair.from)?;
                pair.to.clone_into(&mut draft.fields[at].name);
            }
            NotetypeEdit::MoveField(m) => {
                let at = field_at(&draft, &m.field)?;
                let to = usize::try_from(m.to).ok().filter(|to| *to < draft.fields.len());
                let to = to.ok_or_else(|| invalid(format!("move_field to {} is not a position (T5)", m.to)))?;
                let field = draft.fields.remove(at);
                draft.fields.insert(to, field);
            }
            NotetypeEdit::FieldOptions(options)
                if options.tag != scenario::Presence::Missing || options.prevent_deletion.is_some() =>
            {
                return Err(ExecError::new(
                    "unsupported_op",
                    "update_notetype.field_options.io is pending",
                ));
            }
            NotetypeEdit::FieldOptions(options) => {
                let at = field_at(&draft, &options.field)?;
                let o = &mut draft.fields[at].options;
                if let Some(v) = options.sticky {
                    o.sticky = v;
                }
                if let Some(v) = options.rtl {
                    o.rtl = v;
                }
                if let Some(v) = &options.font_name {
                    v.clone_into(&mut o.font_name);
                }
                if let Some(v) = options.font_size {
                    o.font_size =
                        u32::try_from(v).map_err(|_| invalid(format!("font_size {v} is not a 32-bit size")))?;
                }
                if let Some(v) = &options.description {
                    v.clone_into(&mut o.description);
                }
                if let Some(v) = options.plain_text {
                    o.plain_text = v;
                }
                if let Some(v) = options.collapsed {
                    o.collapsed = v;
                }
                if let Some(v) = options.exclude_from_search {
                    o.exclude_from_search = v;
                }
            }
            NotetypeEdit::AddTemplate(t) => draft.templates.push(DraftTemplate {
                origin: None,
                name: t.name.clone(),
                front: t.front.clone(),
                back: t.back.clone(),
                browser_front: String::new(),
                browser_back: String::new(),
                deck: 0,
                browser_font_name: String::new(),
                browser_font_size: 0,
            }),
            NotetypeEdit::RemoveTemplate(name) => {
                let at = template_at(&draft, name)?;
                draft.templates.remove(at);
            }
            NotetypeEdit::RenameTemplate(pair) => {
                let at = template_at(&draft, &pair.from)?;
                pair.to.clone_into(&mut draft.templates[at].name);
            }
            NotetypeEdit::MoveTemplate(m) => {
                let at = template_at(&draft, &m.template)?;
                let to = usize::try_from(m.to).ok().filter(|to| *to < draft.templates.len());
                let to = to.ok_or_else(|| invalid(format!("move_template to {} is not a position (T5)", m.to)))?;
                let template = draft.templates.remove(at);
                draft.templates.insert(to, template);
            }
            NotetypeEdit::Template(edit) => {
                let deck = match &edit.deck {
                    None => None,
                    Some(None) => Some(DeckId(0)),
                    Some(Some(name)) => Some(deck_id(reader, name)?),
                };
                let at = template_at(&draft, &edit.name)?;
                let t = &mut draft.templates[at];
                if let Some(v) = &edit.front {
                    v.clone_into(&mut t.front);
                }
                if let Some(v) = &edit.back {
                    v.clone_into(&mut t.back);
                }
                if let Some(v) = &edit.browser_front {
                    v.clone_into(&mut t.browser_front);
                }
                if let Some(v) = &edit.browser_back {
                    v.clone_into(&mut t.browser_back);
                }
                if let Some(v) = &edit.browser_font_name {
                    v.clone_into(&mut t.browser_font_name);
                }
                if let Some(v) = edit.browser_font_size {
                    t.browser_font_size =
                        u32::try_from(v).map_err(|_| invalid(format!("browser_font_size {v} is not a 32-bit size")))?;
                }
                if let Some(deck) = deck {
                    t.deck = deck.0;
                }
            }
        }
    }
    if let Some(origin) = sort_origin {
        let at = draft
            .fields
            .iter()
            .position(|f| f.origin == Some(origin))
            .ok_or_else(|| invalid("the sort field is removed by the same update (T5)"))?;
        draft.sort_field = Some(u32::try_from(at).map_err(|_| invalid("too many fields"))?);
    }
    Ok(draft)
}

/// T5's plain edits on a draft (U29's plain save).
fn apply_edits(scope: &impl ReadRows, mut draft: Notetype, edits: &[NotetypeEdit]) -> Result<Notetype, ExecError> {
    for edit in edits {
        match edit {
            NotetypeEdit::Rename(name) => name.clone_into(&mut draft.name),
            NotetypeEdit::Css(css) => css.clone_into(&mut draft.config.css),
            NotetypeEdit::FieldOptions(options)
                if options.tag != scenario::Presence::Missing || options.prevent_deletion.is_some() =>
            {
                return Err(ExecError::new(
                    "unsupported_op",
                    "update_notetype.field_options.io is pending",
                ));
            }
            NotetypeEdit::FieldOptions(options) => {
                let field = draft
                    .fields
                    .iter_mut()
                    .find(|f| f.name == options.field)
                    .ok_or_else(|| unknown_ref(format!("no field {:?} in the draft (T5)", options.field)))?;
                let config = &mut field.config;
                if let Some(v) = options.sticky {
                    config.sticky = v;
                }
                if let Some(v) = options.rtl {
                    config.rtl = v;
                }
                if let Some(v) = &options.font_name {
                    v.clone_into(&mut config.font_name);
                }
                if let Some(v) = options.font_size {
                    config.font_size =
                        u32::try_from(v).map_err(|_| invalid(format!("font_size {v} is not a 32-bit size")))?;
                }
                if let Some(v) = &options.description {
                    v.clone_into(&mut config.description);
                }
                if let Some(v) = options.plain_text {
                    config.plain_text = v;
                }
                if let Some(v) = options.collapsed {
                    config.collapsed = v;
                }
                if let Some(v) = options.exclude_from_search {
                    config.exclude_from_search = v;
                }
            }
            NotetypeEdit::RenameTemplate(pair) => {
                let template = draft
                    .templates
                    .iter_mut()
                    .find(|t| t.name == pair.from)
                    .ok_or_else(|| unknown_ref(format!("no template {:?} in the draft (T5)", pair.from)))?;
                pair.to.clone_into(&mut template.name);
            }
            NotetypeEdit::Template(edit) if edit.front.is_none() && edit.back.is_none() => {
                apply_template_edit(scope, &mut draft, edit)?;
            }
            other => {
                return Err(ExecError::new(
                    "internal_error",
                    format!("{} is not a plain edit", other.capability()),
                ));
            }
        }
    }
    Ok(draft)
}

/// A `template` edit without `front` or `back`: browser appearance and the
/// deck override.
fn apply_template_edit(scope: &impl ReadRows, draft: &mut Notetype, edit: &TemplateEdit) -> Result<(), ExecError> {
    let deck = match &edit.deck {
        None => None,
        Some(None) => Some(DeckId(0)),
        Some(Some(name)) => Some(deck_id(scope, name)?),
    };
    let template = draft
        .templates
        .iter_mut()
        .find(|t| t.name == edit.name)
        .ok_or_else(|| unknown_ref(format!("no template {:?} in the draft (T5)", edit.name)))?;
    let config = &mut template.config;
    if let Some(v) = &edit.browser_front {
        v.clone_into(&mut config.q_format_browser);
    }
    if let Some(v) = &edit.browser_back {
        v.clone_into(&mut config.a_format_browser);
    }
    if let Some(v) = &edit.browser_font_name {
        v.clone_into(&mut config.browser_font_name);
    }
    if let Some(v) = edit.browser_font_size {
        config.browser_font_size =
            u32::try_from(v).map_err(|_| invalid(format!("browser_font_size {v} is not a 32-bit size")))?;
    }
    if let Some(deck) = deck {
        config.target_deck_id = deck.0;
    }
    Ok(())
}

/// The id of the deck with this human name.
fn deck_id(scope: &impl ReadRows, name: &str) -> Result<DeckId, ExecError> {
    let native = native_name(name);
    let mut after = None;
    loop {
        let page = scope.ids_after(Table::Decks, after, 1_000).map_err(core)?;
        let Some(last) = page.last().copied() else {
            return Err(unknown_ref(format!("no deck {name:?}")));
        };
        after = Some(last);
        for id in page {
            if scope.deck(DeckId(id)).map_err(core)?.is_some_and(|d| d.name == native) {
                return Ok(DeckId(id));
            }
        }
    }
}

/// T8: the structure of the requested note types, or of every one.
fn notetype_structure(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let writer = collection.reader()?;
    let all = notetypes::read::all(&*writer).map_err(core)?;
    let chosen: Vec<&Notetype> = match &request.args {
        ObservationArgs::None => all.iter().collect(),
        ObservationArgs::Mapping(map) => map
            .get("notetypes")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("notetype_structure takes {notetypes}"))?
            .iter()
            .map(|name| {
                let name = name.as_str().unwrap_or_default();
                all.iter()
                    .find(|n| n.name == name)
                    .ok_or_else(|| unknown_ref(format!("no note type is named {name:?} (T1)")))
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(invalid("notetype_structure takes no references")),
    };
    let mut out = Map::new();
    for notetype in chosen {
        out.insert(notetype.name.clone(), structure(&*writer, notetype)?);
    }
    Ok(Value::Object(out))
}

/// `notetype-structure-ops.md` M3: the stored template texts of the named
/// note types.
fn notetype_templates(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let writer = collection.reader()?;
    let all = notetypes::read::all(&*writer).map_err(core)?;
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("notetype_templates takes {notetypes}"));
    };
    let names = map
        .get("notetypes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("notetype_templates takes {notetypes}"))?;
    let mut out = Map::new();
    for name in names {
        let name = name.as_str().unwrap_or_default();
        let notetype = all
            .iter()
            .find(|n| n.name == name)
            .ok_or_else(|| unknown_ref(format!("no note type is named {name:?} (T1)")))?;
        let templates: Vec<Value> = notetype
            .templates
            .iter()
            .map(|t| {
                let c = &t.config;
                json!({
                    "name": t.name,
                    "front": c.q_format,
                    "back": c.a_format,
                    "browser_front": c.q_format_browser,
                    "browser_back": c.a_format_browser,
                })
            })
            .collect();
        out.insert(notetype.name.clone(), Value::Array(templates));
    }
    Ok(Value::Object(out))
}

/// One note type's T8 value.
fn structure(reader: &impl ReadRows, notetype: &Notetype) -> Result<Value, ExecError> {
    let config = &notetype.config;
    let fields: Vec<Value> = notetype
        .fields
        .iter()
        .map(|field| {
            let c = &field.config;
            json!({
                "name": field.name,
                "sticky": c.sticky,
                "rtl": c.rtl,
                "font_name": c.font_name,
                "font_size": c.font_size,
                "description": c.description,
                "plain_text": c.plain_text,
                "collapsed": c.collapsed,
                "exclude_from_search": c.exclude_from_search,
                "tag": c.tag,
                "prevent_deletion": c.prevent_deletion,
            })
        })
        .collect();
    let mut templates = Vec::with_capacity(notetype.templates.len());
    for (position, template) in notetype.templates.iter().enumerate() {
        let target = template.config.target_deck_id;
        let deck = if target == 0 {
            Value::Null
        } else {
            match reader.deck(DeckId(target)).map_err(core)? {
                Some(deck) => Value::String(human_name(&deck.name)),
                None => Value::from(target),
            }
        };
        let requirement = config
            .reqs
            .iter()
            .find(|r| usize::try_from(r.card_ord).is_ok_and(|ord| ord == position))
            .map(|r| {
                let kind = match r.kind {
                    0 => "none",
                    1 => "any",
                    2 => "all",
                    other => return Err(core(format!("a card requirement of kind {other}"))),
                };
                Ok(json!({ "kind": kind, "fields": r.field_ords }))
            })
            .transpose()?;
        templates.push(json!({ "name": template.name, "deck": deck, "requirement": requirement }));
    }
    Ok(json!({
        "kind": if NotetypeKind::of(notetype) == NotetypeKind::Cloze { "cloze" } else { "standard" },
        "original_stock_kind": config.original_stock_kind,
        "original_id": config.original_id,
        "sort_field": config.sort_field_idx,
        "fields": fields,
        "templates": templates,
    }))
}
