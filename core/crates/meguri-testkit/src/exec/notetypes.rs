//! The note type area's adapters (`specs/conformance/notetype-ops.md` T10):
//! the stand-in new collection of T10.2 (U28's bootstrap replaces it),
//! `setup.notetypes` (dsl.md D6.4, each entry added as `add_notetype` adds
//! it), the ops `add_notetype`, `add_stock_notetype`, `clone_notetype`,
//! `ensure_io_notetype` and the plain-save part of `update_notetype`
//! (T10.1), and the `notetype_structure` observation (T8). Each op runs
//! meguri-engine's `notetypes` code in one local write scope on the
//! session's writer, until the session runs the engine (U38).

use meguri_engine::EngineError;
use meguri_engine::notetypes::{
    self, AggregateDigest, Draft, NewNotetype, NewTemplate, NotetypeKind, SaveOptions, StockKind,
};
use meguri_model::clock::Clock;
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::deck::{human_name, native_name};
use meguri_model::ids::DeckId;
use meguri_model::notetype::Notetype;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_sched::timing::new_collection_timing;
use meguri_storage::{ReadRows, StampMode, StorageError, Table, WriteScope};
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

/// T10.1: the `update_notetype` edits a plain save applies (U29); U34 adds
/// the others.
const PLAIN_EDITS: [&str; 5] = [
    "update_notetype.rename",
    "update_notetype.css",
    "update_notetype.field_options",
    "update_notetype.rename_template",
    "update_notetype.template",
];

/// Registers the stand-in new collection, `setup.notetypes`, the ops and
/// the `notetype_structure` kind.
pub(super) fn register(registry: &mut Registry) {
    registry.new_collection(new_collection);
    registry.setup("notetypes", setup_notetypes);
    registry.op_rejecting("add_notetype", REJECTIONS, add_notetype);
    registry.op_rejecting("add_stock_notetype", REJECTIONS, add_stock_notetype);
    registry.op_rejecting("clone_notetype", REJECTIONS, clone_notetype);
    registry.op_rejecting("update_notetype", REJECTIONS, update_notetype);
    for part in PLAIN_EDITS {
        registry.op_part(part);
    }
    registry.op("ensure_io_notetype", ensure_io_notetype);
    registry.observation("notetype_structure", notetype_structure);
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

/// T10.2: the new collection at the clock's instant, made with the storage
/// area's create path: a schema-18 `col` row with `crt` and the
/// `creationOffset` key of `sched/day-clock` D11 (`col.mod` 0, `scm` now),
/// `schedVer` 2, the two browser keys a new collection holds
/// (`browser/config-keys` K1: `sortType` `"noteFld"`, `sortBackwards`
/// false), and the six stock kinds and `curModel` as `notetypes/stock-kinds`
/// K2 says, written by meguri-engine's stock-kind code. The default deck,
/// the default preset and the other new-collection config keys come with
/// U28.
fn new_collection(session: &mut Session<'_>) -> Result<(), ExecError> {
    let now_ms = session.clock.now_ms();
    let timing = new_collection_timing(now_ms.div_euclid(1000), session.clock.as_ref());
    let col = Col {
        version: Col::SCHEMA_VERSION,
        created: EpochSecs(timing.crt),
        modified: EpochMillis(0),
        schema_modified: EpochMillis(now_ms),
        last_sync: EpochMillis(0),
        usn: Usn(0),
        dirty: 0,
        legacy: Default::default(),
    };
    let entry = |key: &str, value: String| ConfigEntry {
        key: key.to_owned(),
        usn: Usn(0),
        mtime: EpochSecs(0),
        value: value.into_bytes(),
    };
    let config = [
        entry("creationOffset", timing.creation_offset.to_string()),
        entry("schedVer", "2".to_owned()),
        entry("sortBackwards", "false".to_owned()),
        entry("sortType", "\"noteFld\"".to_owned()),
    ];
    let random = session.random.clone();
    super::storage::create_collection(session, &col, &config, |scope| {
        notetypes::stock::add_to_new_collection(scope, random.as_ref())
            .map(|_| ())
            .map_err(core)
    })
}

/// Runs `f` in one local write scope on the session's writer and commits
/// it; a refused save rolls back and is recorded as T7's kind.
fn in_scope<T>(
    session: &mut Session<'_>,
    f: impl FnOnce(&mut WriteScope<'_>, &Session<'_>) -> Result<T, EngineError>,
) -> Result<Result<T, &'static str>, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, session.clock.as_ref(), None)
        .map_err(core)?;
    match f(&mut scope, session) {
        Ok(value) => {
            scope.commit().map_err(core)?;
            Ok(Ok(value))
        }
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
            })
            .collect(),
        css: Some(spec.css.clone()),
    }
}

/// D6.4: each entry added as `add_notetype` adds it; a refusal makes the
/// scenario invalid.
fn setup_notetypes(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    for spec in setup.notetypes.iter().flatten() {
        let new = new_notetype(spec);
        let added = in_scope(session, |scope, session| {
            notetypes::add_in(scope, session.random.as_ref(), &new)
        })?;
        if let Err(kind) = added {
            return Err(invalid(format!("setup note type {:?} refused: {kind}", spec.name)));
        }
    }
    Ok(())
}

/// T2.
fn add_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::AddNotetype(spec) = step else {
        return Err(ExecError::new(
            "internal_error",
            "add_notetype adapter called for another op",
        ));
    };
    let new = new_notetype(spec);
    let added = in_scope(session, |scope, session| {
        notetypes::add_in(scope, session.random.as_ref(), &new).map(|a| a.name)
    })?;
    Ok(named(added))
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
    let added = in_scope(session, |scope, session| {
        notetypes::add_stock_in(scope, session.random.as_ref(), kind, args.name.as_deref()).map(|a| a.name)
    })?;
    Ok(named(added))
}

/// The note type stored under exactly `name` (T1).
fn find(scope: &WriteScope<'_>, name: &str) -> Result<Result<Notetype, String>, StorageError> {
    Ok(notetypes::read::by_name(scope, name)?.ok_or_else(|| format!("no note type is named {name:?} (T1)")))
}

/// T4.
fn clone_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::CloneNotetype(args) = step else {
        return Err(ExecError::new(
            "internal_error",
            "clone_notetype adapter called for another op",
        ));
    };
    let mut missing = None;
    let added = in_scope(session, |scope, _| {
        let source = match find(scope, &args.notetype)? {
            Ok(source) => source,
            Err(detail) => {
                missing = Some(detail);
                return Ok(String::new());
            }
        };
        notetypes::clone_in(scope, source.id, args.name.as_deref()).map(|a| a.name)
    })?;
    match missing {
        Some(detail) => Err(unknown_ref(detail)),
        None => Ok(named(added)),
    }
}

/// T6.
fn ensure_io_notetype(session: &mut Session<'_>, _step: &Step) -> Result<OpResult, ExecError> {
    let ensured = in_scope(session, |scope, session| {
        notetypes::ensure_image_occlusion_in(scope, session.random.as_ref()).map(|a| a.name)
    })?;
    match ensured {
        Ok(name) => Ok(OpResult::Output(json!({ "name": name }))),
        Err(kind) => Err(core(format!("ensure_io_notetype refused: {kind}"))),
    }
}

/// T5's plain save: the edits applied in order to a draft of the stored
/// note type, saved once (checked unless `skip_checks`).
fn update_notetype(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::UpdateNotetype(args) = step else {
        return Err(ExecError::new(
            "internal_error",
            "update_notetype adapter called for another op",
        ));
    };
    let mut failed = None;
    let saved = in_scope(session, |scope, _| {
        let draft = match find(scope, &args.notetype)? {
            Ok(stored) => {
                let base = AggregateDigest::of(&stored);
                apply_edits(scope, stored, &args.edits).map(|notetype| Draft { notetype, base })
            }
            Err(detail) => Err(unknown_ref(detail)),
        };
        match draft {
            Ok(draft) => {
                let options = SaveOptions {
                    skip_checks: args.skip_checks,
                };
                notetypes::save_in(scope, &draft, options).map(|s| s.name)
            }
            Err(error) => {
                failed = Some(error);
                Ok(String::new())
            }
        }
    })?;
    match failed {
        Some(error) => Err(error),
        None => Ok(named(saved)),
    }
}

/// T5's plain edits on a draft; any other edit is U34's.
fn apply_edits(scope: &WriteScope<'_>, mut draft: Notetype, edits: &[NotetypeEdit]) -> Result<Notetype, ExecError> {
    for edit in edits {
        match edit {
            NotetypeEdit::Rename(name) => name.clone_into(&mut draft.name),
            NotetypeEdit::Css(css) => css.clone_into(&mut draft.config.css),
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
                    "unsupported_op",
                    format!("{} needs U34's draft apply", other.capability()),
                ));
            }
        }
    }
    Ok(draft)
}

/// A `template` edit without `front` or `back`: browser appearance and the
/// deck override.
fn apply_template_edit(scope: &WriteScope<'_>, draft: &mut Notetype, edit: &TemplateEdit) -> Result<(), ExecError> {
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
fn deck_id(scope: &WriteScope<'_>, name: &str) -> Result<DeckId, ExecError> {
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
    let writer = collection.writer();
    let all = notetypes::read::all(writer).map_err(core)?;
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
        out.insert(notetype.name.clone(), structure(writer, notetype)?);
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
