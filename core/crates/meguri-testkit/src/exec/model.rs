//! Stored model observations and patches (model-ops L1, L2, L4).
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{ObservationArgs, ObservationRequest, Step};
use meguri_model::{
    BlobError, ModelError,
    proto::{
        BlobMessage, ProtoBlob,
        deck::{DeckCommon, DeckKindMessage},
        deck_config::DeckConfigMessage,
        notetype::{FieldConfig, NotetypeConfig, TemplateConfig},
    },
    schema11,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, StampMode};
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason, StructureReport};
use meguri_engine::{EngineError, SchemaReason, notetypes::{self, AggregateDigest}};
use meguri_model::notetype::Notetype;
use serde_json::{Map, Value};

pub(super) fn register(r: &mut Registry) {
    r.op_rejecting("legacy_update", &["invalid_json"], update);
    r.observation("legacy_json", observe);
    r.observation("blob_decode", observe_blobs);
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
fn unknown(s: &str) -> ExecError {
    ExecError::new("unknown_ref", format!("no record {s:?}"))
}
fn writer<'a>(s: &'a Session<'_>) -> Result<super::storage::SessionReader<'a>, ExecError> {
    s.storage.collection().ok_or_else(|| core("no collection"))?.reader()
}
fn clear_history(s: &Session<'_>) -> Result<(), ExecError> {
    if let Some(super::storage::SessionCollection::Hosted { collection, .. }) = s.storage.collection() {
        collection
            .clear_undo(meguri_engine::ClearCause::ExternalChange)
            .map_err(core)?;
    }
    Ok(())
}

fn object<R: ReadRows>(rows: &R, kind: &str, name: &str) -> Result<Value, ExecError> {
    let text = match kind {
        "decks" => schema11::deck_to_json(
            &rows
                .deck_rows()
                .map_err(core)?
                .into_iter()
                .find(|d| d.human_name() == name)
                .ok_or_else(|| unknown(name))?,
        ),
        "presets" => {
            let mut matches = rows
                .deck_config_rows()
                .map_err(core)?
                .into_iter()
                .filter(|p| p.name == name);
            let p = matches.next().ok_or_else(|| unknown(name))?;
            if matches.next().is_some() {
                return Err(ExecError::new("invalid_scenario", "ambiguous preset"));
            }
            schema11::deck_config_to_json(&p.for_use())
        }
        "notetypes" => schema11::notetype_to_json(
            &meguri_engine::notetypes::read::by_name(rows, name)
                .map_err(core)?
                .ok_or_else(|| unknown(name))?,
        ),
        _ => return Err(core("unknown legacy kind")),
    };
    crate::observe::parse_json(&text).map_err(core)
}
fn merge(value: &mut Value, patch: &Value) {
    if let Value::Object(patch) = patch {
        if !value.is_object() {
            *value = Value::Object(Map::new());
        }
        for (key, patch) in patch {
            if patch.is_null() {
                value.as_object_mut().expect("object").remove(key);
            } else {
                merge(&mut value[key], patch);
            }
        }
    } else {
        *value = patch.clone();
    }
}

// L2 is an unchecked legacy save, not a native editor draft or a transfer.
// Previous ordinals in the merged object author origins; optional IDs do not.
struct LegacyNotetypePlan {
    source: Notetype,
    destination: Option<Notetype>,
    names: Vec<(meguri_model::ids::NotetypeId, String)>,
    report: StructureReport,
    reasons: Vec<SchemaReason>,
    missing_card_authority: bool,
}
fn invalid_edit(message: &str) -> EngineError {
    EngineError::InvalidArgument { message: message.into() }
}
fn exact_base(expected: &Notetype, actual: &Notetype) -> bool {
    AggregateDigest::of(expected) == AggregateDigest::of(actual)
        && expected.mtime == actual.mtime && expected.usn == actual.usn
}
impl LegacyNotetypePlan {
    fn new(source: Notetype, destination: Option<Notetype>, next: &mut Notetype, names: &[(meguri_model::ids::NotetypeId, String)]) -> Result<Self, EngineError> {
        let origins = |ords: Vec<Option<u32>>, keys: Vec<u32>| -> Result<Vec<i32>, EngineError> {
            let positions: std::collections::BTreeMap<_, _> = keys.into_iter().enumerate()
                .map(|(position, key)| (key, position)).collect();
            let mut seen = std::collections::BTreeSet::new();
            ords.into_iter().map(|ord| match ord {
                None => Ok(-1),
                Some(ord) if positions.contains_key(&ord) && seen.insert(ord) =>
                    i32::try_from(positions[&ord]).map_err(|_| invalid_edit("legacy origin exceeds map representation")),
                _ => Err(invalid_edit("legacy save has an absent or repeated previous ordinal")),
            }).collect()
        };
        let field_origins: Vec<_> = next.fields.iter().map(|f| f.prior_ord).collect();
        let (field_map, physical_templates) = if let Some(base) = &destination {
            (origins(field_origins.clone(), base.fields.iter().map(|f| f.ord).collect())?,
                origins(next.templates.iter().map(|t| t.prior_ord).collect(), base.templates.iter().map(|t| t.ord).collect())?)
        } else { (Vec::new(), Vec::new()) };
        // sortf is a logical index. N4 must follow logical origins even when
        // the exact stored child keys have gaps. Retain the explicit metadata.
        if destination.is_some() {
            for (field, origin) in next.fields.iter_mut().zip(&field_map) {
                field.prior_ord = u32::try_from(*origin).ok();
            }
        }
        notetypes::names::normalize(next, names)?;
        for (field, origin) in next.fields.iter_mut().zip(field_origins) {
            field.prior_ord = origin;
        }
        next.config.reqs = notetypes::write::card_requirements(next);
        let mut plan = Self { source, destination, names: names.to_vec(),
            report: StructureReport::default(), reasons: Vec::new(), missing_card_authority: false };
        // Saving under an unused converted ID creates a new aggregate, not a layout edit
        // of the aggregate used to construct the merged JSON.
        let Some(base) = &plan.destination else { return Ok(plan) };
        let moved = |map: &[i32], old: usize| map.len() != old || map.iter().enumerate().any(|(i, n)| *n != i as i32);
        let fields_changed = moved(&field_map, base.fields.len());
        let templates_changed = moved(&physical_templates, base.templates.len());
        let sort_changed = field_map.get(next.config.sort_field_idx as usize).copied()
            != i32::try_from(base.config.sort_field_idx).ok();
        for (changed, schema, structure) in [
            (fields_changed, SchemaReason::FieldsChanged, StructureReason::FieldsChanged),
            (templates_changed, SchemaReason::TemplatesChanged, StructureReason::TemplatesChanged),
            (sort_changed, SchemaReason::SortFieldChanged, StructureReason::SortFieldChanged),
        ] {
            if changed { plan.reasons.push(schema); plan.report.reasons.insert(structure); }
        }
        let kind_changed = base.config.kind != next.config.kind;
        // L2 writes no card ordinals. For cloze->cloze this operation fact
        // authors an EMPTY identity prefix, independently of physical rows.
        // Mixed-kind layouts must also describe that actual card behavior.
        let template_map = if base.config.kind == 1 {
            Vec::new()
        } else if kind_changed {
            plan.missing_card_authority = base.templates.len() != next.templates.len();
            // The old standard domain has no implicit identity tail.
            (0..LayoutShape::of(base)?.templates).map(|i| i as i32).collect()
        } else { physical_templates };
        let renamed = base.fields.iter().map(|f| &f.name).ne(next.fields.iter().map(|f| &f.name))
            || base.templates.iter().map(|t| &t.name).ne(next.templates.iter().map(|t| &t.name));
        let renumbered = base.fields.iter().enumerate().any(|(i, f)| f.ord as usize != i)
            || base.templates.iter().enumerate().any(|(i, t)| t.ord as usize != i);
        if !plan.reasons.is_empty() || renamed || renumbered || kind_changed
            || base.config.sort_field_idx != next.config.sort_field_idx {
            plan.report.layouts.insert(base.id.0, LayoutReport {
                before: LayoutShape::of(base)?, after: LayoutShape::of(next)?, field_map, template_map,
            });
        }
        Ok(plan)
    }
    fn write(self, scope: &mut meguri_storage::WriteScope<'_>, next: &Notetype) -> Result<(), EngineError> {
        let stale = |id: meguri_model::ids::NotetypeId| EngineError::Stale {
            table: meguri_storage::Table::Notetypes, key: id.0.to_string(),
        };
        if !scope.notetype(self.source.id)?.is_some_and(|live| exact_base(&self.source, &live)) {
            return Err(stale(self.source.id));
        }
        let live = scope.notetype(next.id)?;
        if !match (&self.destination, &live) {
            (Some(base), Some(live)) => exact_base(base, live),
            (None, None) => true,
            _ => false,
        } { return Err(stale(next.id)); }
        // N4's namespace was part of planning. Refuse atomically instead of
        // renormalizing a second time (which could remap sortf again).
        if notetypes::read::names(scope)? != self.names { return Err(stale(next.id)); }
        let mut missing_card_authority = self.missing_card_authority;
        if self.destination.as_ref().is_some_and(|base| base.config.kind == 1 && next.config.kind == 0) {
            // An empty old-cloze prefix truncates at the standard endpoint.
            // L2 deletes no cards: prove that none would be lost, in-writer.
            let mut after = None;
            loop {
                let cards = scope.card_ordinals_of_notetype_page(next.id, after)?;
                if cards.is_empty() { break; }
                after = cards.last().map(|(id, _)| *id);
                if cards.iter().any(|(_, ord)| usize::from(*ord) >= next.templates.len()) {
                    missing_card_authority = true;
                }
            }
        }
        if missing_card_authority {
            // The current layout geometry cannot express this aggregate-only
            // save across note-type kinds. L2 supplies no additional policy.
            return Err(meguri_storage::StorageError::StructureNotReported { notetype_id: next.id }.into());
        }
        let token = scope.start_structure_report(self.report)?;
        meguri_engine::mark_schema_change(scope, &self.reasons)?;
        scope.put_notetype(next)?;
        scope.complete_structure_report(token)?;
        Ok(())
    }
}
fn update(s: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::LegacyUpdate(args) = step else {
        return Err(core("wrong legacy op"));
    };
    let (kind, name) = if let Some(n) = &args.deck {
        ("decks", n)
    } else if let Some(n) = &args.preset {
        ("presets", n)
    } else {
        (
            "notetypes",
            args.notetype.as_ref().ok_or_else(|| core("no legacy target"))?,
        )
    };
    clear_history(s)?;
    let hosted_base = if kind == "notetypes"
        && matches!(s.storage.collection(), Some(super::storage::SessionCollection::Hosted { .. })) {
        Some(notetypes::read::by_name(&*writer(s)?, name).map_err(core)?.ok_or_else(|| unknown(name))?)
    } else { None };
    let mut object = if let Some(base) = &hosted_base {
        crate::observe::parse_json(&schema11::notetype_to_json(base)).map_err(core)?
    } else {
        object(&*writer(s)?, kind, name)?
    };
    merge(&mut object, &Value::Object(args.patch.clone()));
    let text = object.to_string();
    let mode = if kind == "presets" {
        StampMode::Local
    } else {
        StampMode::CallerSupplied
    };
    enum Patch {
        Deck(meguri_model::deck::Deck),
        Preset(meguri_model::deck_config::DeckConfig),
        Notetype(meguri_model::notetype::Notetype),
    }
    let parsed = match kind {
        "decks" => schema11::deck_from_json(&text).map(|mut d| {
            d.name = meguri_model::deck::normalize_native_name(&d.name);
            Patch::Deck(d)
        }),
        "presets" => schema11::deck_config_from_json(&text).map(Patch::Preset),
        _ => schema11::notetype_from_json(&text).map(|mut n| {
            n.config.reqs = meguri_engine::notetypes::write::card_requirements(&n);
            Patch::Notetype(n)
        }),
    };
    let Ok(mut parsed) = parsed else {
        return Ok(OpResult::Rejected("invalid_json"));
    };
    if let Patch::Preset(p) = &mut parsed {
        if let Some(stored) = writer(s)?.deck_config(p.id).map_err(core)? {
            // Schema-11 cannot carry unknown protobuf fields. Keep those
            // opaque fields when saving its converted known values (P4).
            if !stored.config.unknown_fields().is_empty() {
                let mut bytes = p.config.encode();
                for field in stored.config.unknown_fields().iter() {
                    bytes.extend_from_slice(field.bytes());
                }
                p.config = ProtoBlob::new(DeckConfigMessage::decode(&bytes).map_err(core)?);
            }
            // P5 compares actual stored content, before read bounds: an L2
            // save must persist any replacements supplied by the L1 view.
            if p.config.same_content(&stored.config) {
                p.config = stored.config;
            }
        }
        // L2's local-save stamps precede storage's equal-row comparison.
        p.mtime = meguri_model::time::EpochSecs(s.clock.now_secs());
        p.usn = meguri_model::usn::Usn::PENDING;
    }
    let plan = if let (Some(base), Patch::Notetype(n)) = (hosted_base, &mut parsed) {
        let rows = writer(s)?;
        let destination = rows.notetype(n.id).map_err(core)?;
        Some(LegacyNotetypePlan::new(base, destination, n, &notetypes::read::names(&*rows).map_err(core)?).map_err(core)?)
    } else { None };
    match s.storage.collection().ok_or_else(|| core("no collection"))? {
        super::storage::SessionCollection::Raw(raw) => {
            let mut scope = raw.writer().begin_write(mode, s.clock.as_ref(), None).map_err(core)?;
            match parsed {
                Patch::Deck(d) => scope.put(&d),
                Patch::Preset(p) => scope.put(&p),
                Patch::Notetype(n) => scope.put_notetype(&n),
            }.map_err(core)?;
            scope.commit().map_err(core)?;
        }
        super::storage::SessionCollection::Hosted { collection, .. } => {
            collection
                .run(
                    meguri_engine::OpSpec::named("legacy_update", meguri_engine::OpSource::UserNonUndoable)
                        .stamp(mode).schema_changes(plan.as_ref().map(|p| p.reasons.clone()).unwrap_or_default()),
                    move |ctx| {
                        match parsed {
                            Patch::Deck(d) => { ctx.scope().put(&d)?; }
                            Patch::Preset(p) => { ctx.scope().put(&p)?; }
                            Patch::Notetype(n) => plan.expect("hosted note type plan").write(ctx.scope(), &n)?,
                        }
                        Ok(())
                    },
                )
                .map_err(core)?;
        }
    }
    Ok(OpResult::Done)
}

fn observe(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    observe_named(s, request, object::<meguri_storage::Reader>)
}
fn observe_blobs(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    observe_named(s, request, blob_status)
}
fn observe_named(
    s: &mut Session<'_>,
    request: &ObservationRequest,
    value: fn(&meguri_storage::Reader, &str, &str) -> Result<Value, ExecError>,
) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(args) = &request.args else {
        return Err(core("legacy_json expects mapping"));
    };
    clear_history(s)?;
    let mut out = Map::new();
    for (kind, names) in args {
        let mut values = Map::new();
        for name in names.as_array().ok_or_else(|| core("legacy names must be a list"))? {
            let name = name.as_str().ok_or_else(|| core("legacy name must be string"))?;
            values.insert(name.into(), value(&*writer(s)?, kind, name)?);
        }
        out.insert(kind.clone(), Value::Object(values));
    }
    Ok(out.into())
}

// Read names and raw columns first: one malformed record must not hide another.
fn blob_status(writer: &meguri_storage::Reader, kind: &str, name: &str) -> Result<Value, ExecError> {
    let (table, columns, stored_name) = match kind {
        "notetypes" => ("notetypes", "id, config", name.to_owned()),
        "decks" => ("decks", "id, common, kind", name.replace("::", "\u{1f}")),
        "presets" => ("deck_config", "id, config", name.to_owned()),
        _ => return Err(core("unknown blob kind")),
    };
    let rows = writer
        .execute_raw(
            &format!("SELECT {columns} FROM {table} WHERE name = ? COLLATE BINARY"),
            &[SqlValue::Text(stored_name)],
        )
        .map_err(core)?;
    let row = match rows.rows.as_slice() {
        [] => return Err(unknown(name)),
        [row] => row,
        _ => return Err(ExecError::new("invalid_scenario", "ambiguous record name")),
    };
    let Some(SqlValue::Integer(id)) = row.first() else {
        return Err(core("invalid record identity"));
    };
    let valid = match kind {
        "presets" => decodes::<DeckConfigMessage>(&row[1])?,
        "decks" => {
            let common = decodes::<DeckCommon>(&row[1])?;
            let kind = decodes::<DeckKindMessage>(&row[2])?;
            common && kind
        }
        _ => {
            let mut valid = decodes::<NotetypeConfig>(&row[1])?;
            for table in ["fields", "templates"] {
                let children = writer
                    .execute_raw(
                        &format!("SELECT config FROM {table} WHERE ntid = ? ORDER BY ord"),
                        &[SqlValue::Integer(*id)],
                    )
                    .map_err(core)?;
                for child in &children.rows {
                    valid &= if table == "fields" {
                        decodes::<FieldConfig>(&child[0])?
                    } else {
                        decodes::<TemplateConfig>(&child[0])?
                    };
                }
            }
            valid
        }
    };
    Ok(Value::String(if valid { "ok" } else { "invalid_blob" }.into()))
}
fn decodes<M: BlobMessage>(value: &SqlValue) -> Result<bool, ExecError> {
    let SqlValue::Blob(bytes) = value else {
        return Err(core("a blob column is not stored as BLOB"));
    };
    match M::decode(bytes) {
        Ok(_) => Ok(true),
        Err(
            e @ ModelError::Blob {
                reason: BlobError::NoDeckKind,
                ..
            },
        ) => Err(core(e)),
        Err(ModelError::Blob { .. }) => Ok(false),
        Err(e) => Err(core(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use meguri_engine::{Collection, OpSource, OpSpec};
    use meguri_model::{ids::NotetypeId, rng::SeededRandom, time::EpochSecs, usn::Usn};
    use meguri_storage::{Cell, Record, RowImage, RowKey, Table};

    fn hosted(f: impl FnOnce(&Collection, Notetype, &super::super::BackupWitness)) {
        let dir = tempfile::tempdir().unwrap();
        let collection_dir = dir.path().join("collection");
        std::fs::create_dir(&collection_dir).unwrap();
        let clock = Arc::new(super::super::clock::ScenarioClock::new("UTC", "2030-01-15T10:00:00").unwrap());
        let random = Arc::new(SeededRandom::new(81));
        meguri_engine::bootstrap::create(&collection_dir.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
        let backup = Arc::new(super::super::BackupWitness::default());
        let host = meguri_host::CoreHost::open(meguri_host::HostConfig::new(
            collection_dir, dir.path().join("backups"), dir.path().join("widget.snapshot"),
            clock, random, Arc::new(meguri_storage::space::StatvfsSpace),
        ).with_backup_for_testing(backup.clone())).unwrap();
        let collection = host.collection().unwrap();
        let id = notetypes::add(&collection, notetypes::NewNotetype {
            name: "Legacy authored origins".into(), kind: notetypes::NotetypeKind::Standard,
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![notetypes::NewTemplate {
                name: "View".into(), front: "{{Front}}".into(), back: "{{Back}}".into(), ..Default::default()
            }], css: Some(String::new()),
        }).unwrap().value.id;
        let base = read(&collection, id);
        f(&collection, base, &backup);
        drop(collection);
        host.close().unwrap();
    }
    fn read(c: &Collection, id: NotetypeId) -> Notetype {
        c.reader(meguri_storage::ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap()
    }
    fn decoded(base: &Notetype, patch: impl FnOnce(&mut Value)) -> Notetype {
        let mut object = crate::observe::parse_json(&schema11::notetype_to_json(base)).unwrap();
        patch(&mut object);
        schema11::notetype_from_json(&object.to_string()).unwrap()
    }
    fn prepare(c: &Collection, source: Notetype, next: &mut Notetype) -> Result<LegacyNotetypePlan, EngineError> {
        let rows = c.reader(meguri_storage::ReaderRole::Query)?;
        let destination = rows.notetype(next.id)?;
        LegacyNotetypePlan::new(source, destination, next, &notetypes::read::names(&*rows)?)
    }
    fn execute(c: &Collection, plan: LegacyNotetypePlan, next: Notetype) -> Result<Arc<StructureReport>, EngineError> {
        c.run(OpSpec::named("legacy_update", OpSource::UserNonUndoable)
            .stamp(StampMode::CallerSupplied).schema_changes(plan.reasons.clone()), move |ctx| {
                plan.write(ctx.scope(), &next)?;
                Ok(ctx.scope().journal().unwrap().structure_report_owner())
            }).map(|result| result.value)
    }
    fn save(c: &Collection, source: Notetype, mut next: Notetype) -> Result<Arc<StructureReport>, EngineError> {
        let plan = prepare(c, source, &mut next)?;
        execute(c, plan, next)
    }
    fn insert_new_destination(c: &Collection, next: &Notetype) {
        assert!(c.reader(meguri_storage::ReaderRole::Query).unwrap().notetype(next.id).unwrap().is_none());
        let next = next.clone();
        c.run(OpSpec::named("legacy fixture new destination", OpSource::UserNonUndoable)
            .stamp(StampMode::CallerSupplied), move |ctx| { ctx.scope().put_notetype(&next)?; Ok(()) }).unwrap();
    }
    fn scm(c: &Collection) -> i64 {
        c.reader(meguri_storage::ReaderRole::Query).unwrap().col().unwrap().schema_modified.0
    }
    fn cloze(c: &Collection, text: &str) -> (Notetype, meguri_model::ids::NoteId) {
        let id = notetypes::add(c, notetypes::NewNotetype {
            name: "Legacy cloze card domains".into(), kind: notetypes::NotetypeKind::Cloze,
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![notetypes::NewTemplate {
                name: "Cloze view".into(), front: "{{cloze:Front}}".into(),
                back: "{{cloze:Front}}<hr>{{Back}}".into(), ..Default::default()
            }], css: Some(String::new()),
        }).unwrap().value.id;
        let note = meguri_engine::notes::add(c, meguri_engine::notes::AddNote {
            notetype_id: id, fields: vec![text.into(), "Answer context".into()], tags: Vec::new(),
            deck_id: meguri_model::ids::DeckId(1), guid: None, update_add_defaults: false,
        }, OpSource::UserNonUndoable).unwrap().value.id;
        (read(c, id), note)
    }
    fn cards(c: &Collection, note: meguri_model::ids::NoteId) -> (Vec<u16>, String) {
        let cards = c.reader(meguri_storage::ReaderRole::Query).unwrap().cards_of_note(note).unwrap();
        (cards.iter().map(|card| card.ord).collect(), format!("{cards:?}"))
    }
    fn card_images(c: &Collection, note: meguri_model::ids::NoteId) -> Vec<RowImage> {
        let rows = c.reader(meguri_storage::ReaderRole::Query).unwrap();
        rows.cards_of_note(note).unwrap().iter().map(|card| {
            let image = rows.row_image(Table::Cards, &RowKey::Id(card.id.0)).unwrap().unwrap();
            assert_eq!(image.len(), Table::Cards.columns().len());
            image
        }).collect()
    }
    fn seed_noncanonical_cards(c: &Collection, note: meguri_model::ids::NoteId) {
        c.run(OpSpec::named("legacy raw card retention fixture", OpSource::UserNonUndoable)
            .stamp(StampMode::CallerSupplied), move |ctx| {
                let scope = ctx.scope();
                for card in scope.cards_of_note(note)? {
                    let key = RowKey::Id(card.id.0);
                    let mut image = scope.row_image(Table::Cards, &key)?.unwrap();
                    image[8] = Cell::Integer(1_i64 << 40);
                    image[17] = Cell::text(" { \"retention_probe\" : 1.00 } \n");
                    scope.restore(Table::Cards, &key, Some(&image))?;
                    assert_eq!(scope.row_image(Table::Cards, &key)?, Some(image.clone()));
                    let decoded = scope.card(card.id)?.unwrap();
                    assert_eq!(decoded.due, 0, "the tolerated scalar must lose its raw value on decode");
                    assert_ne!(decoded.to_cells()[8], image[8]);
                    assert_ne!(decoded.to_cells()[17], image[17], "a whole-card write would normalize data text");
                }
                Ok(())
            }).unwrap();
    }
    fn unreadable_note_control(c: &Collection, notetype: NotetypeId) -> (meguri_model::ids::NoteId, RowImage) {
        let id = meguri_model::ids::NoteId(900_000_009);
        let mut image = meguri_model::note::Note {
            id, notetype_id: notetype, ..Default::default()
        }.to_cells();
        image[1] = Cell::Text(vec![0xff]);
        let stored = image.clone();
        c.run(OpSpec::named("legacy unrelated unreadable note fixture", OpSource::UserNonUndoable)
            .stamp(StampMode::CallerSupplied), move |ctx| {
                ctx.scope().restore(Table::Notes, &RowKey::Id(id.0), Some(&stored))?;
                Ok(())
            }).unwrap();
        assert!(c.reader(meguri_storage::ReaderRole::Query).unwrap().note(id).is_err());
        (id, image)
    }

    #[test]
    fn hosted_legacy_save_retains_previous_ordinals_and_new_template_with_cloned_id() {
        hosted(|c, base, backup| {
            let id = base.id;
            let cloned_id = base.templates[0].config.id;
            assert!(cloned_id.is_some());
            let next = decoded(&base, |v| {
                let mut added = v["tmpls"][0].clone();
                added["ord"] = Value::Null;
                added["name"] = Value::String("Added".into());
                // L2 deliberately skips validation of template references.
                added["qfmt"] = Value::String("{{Missing legacy field}}".into());
                v["tmpls"].as_array_mut().unwrap().push(added);
                v["flds"].as_array_mut().unwrap().swap(0, 1);
                v["mod"] = serde_json::json!(123);
                v["usn"] = serde_json::json!(17);
            });
            let report = save(c, base, next).unwrap();
            assert_eq!(report.layouts[&id.0].field_map, [1, 0]);
            assert_eq!(report.layouts[&id.0].template_map, [0, -1]);
            assert_eq!(report.reasons, [StructureReason::FieldsChanged, StructureReason::TemplatesChanged].into_iter().collect());
            assert_eq!(backup.calls(), 1, "schema plan must enter safety admission");
            let stored = read(c, id);
            assert_eq!((stored.mtime, stored.usn), (EpochSecs(123), Usn(17)));
            assert_eq!(stored.templates[1].config.id, cloned_id);
            assert_eq!(stored.config.reqs.len(), 2);
            assert_eq!(stored.config.reqs[0].card_ord, 0);
            assert_eq!(stored.config.reqs[0].field_ords, [1]);
            assert_eq!(stored.config.reqs[1].card_ord, 1);
            assert_eq!(stored.config.reqs[1].kind, 0);
            assert!(stored.config.reqs[1].field_ords.is_empty());
            assert_eq!(c.reader(meguri_storage::ReaderRole::Query).unwrap().col().unwrap().schema_modified.0,
                1_894_701_600_000);
        });
    }

    #[test]
    fn hosted_legacy_sort_and_rename_keep_authored_identity_without_option_authority() {
        hosted(|c, base, backup| {
            let id = base.id;
            let sort = decoded(&base, |v| v["sortf"] = serde_json::json!(1));
            let report = save(c, base, sort).unwrap();
            assert_eq!(report.layouts[&id.0].field_map, [0, 1]);
            assert_eq!(report.layouts[&id.0].template_map, [0]);
            assert_eq!(report.reasons, [StructureReason::SortFieldChanged].into_iter().collect());
            assert_eq!(backup.calls(), 1);
            let base = read(c, id);
            let renamed = decoded(&base, |v| v["flds"][0]["name"] = serde_json::json!("  Renamed  "));
            let report = save(c, base, renamed).unwrap();
            assert!(report.reasons.is_empty());
            assert!(report.layouts[&id.0].is_identity());
            assert_eq!(read(c, id).fields[0].name, "Renamed");
            let base = read(c, id);
            let option = decoded(&base, |v| v["flds"][0]["sticky"] = Value::Bool(true));
            assert!(save(c, base, option).unwrap().is_empty());
            assert_eq!(backup.calls(), 1, "rename and options have no G2 reason");
        });
    }

    #[test]
    fn hosted_legacy_save_refuses_unsupported_origins_and_exact_base_staleness() {
        hosted(|c, base, backup| {
            let id = base.id;
            for origin in [0, 9] {
                let bad = decoded(&base, |v| v["flds"][1]["ord"] = serde_json::json!(origin));
                assert!(matches!(save(c, base.clone(), bad), Err(EngineError::InvalidArgument { .. })));
                assert!(exact_base(&base, &read(c, id)));
            }
            assert_eq!(backup.calls(), 0);
            let next = decoded(&base, |v| v["sortf"] = serde_json::json!(1));
            let mut changed = base.clone();
            changed.usn = Usn(23); // Even a head-stamp-only race invalidates this exact base.
            c.run(OpSpec::named("legacy concurrent metadata", OpSource::UserNonUndoable)
                .stamp(StampMode::CallerSupplied), move |ctx| { ctx.scope().put_notetype(&changed)?; Ok(()) }).unwrap();
            let live = read(c, id);
            assert!(matches!(save(c, base, next), Err(EngineError::Stale { .. })));
            assert!(exact_base(&live, &read(c, id)), "stale plan must write neither SCM nor aggregate");
        });
    }

    #[test]
    fn hosted_legacy_sort_identity_distinguishes_movement_from_replacement() {
        hosted(|c, base, backup| {
            let id = base.id;
            let moved = decoded(&base, |v| v["flds"].as_array_mut().unwrap().swap(0, 1));
            let report = save(c, base, moved).unwrap();
            assert_eq!(report.layouts[&id.0].field_map, [1, 0]);
            assert_eq!(report.layouts[&id.0].template_map, [0]);
            assert_eq!(report.reasons, [StructureReason::FieldsChanged].into_iter().collect());
            assert_eq!(read(c, id).config.sort_field_idx, 1);
            let base = read(c, id);
            let replacement = decoded(&base, |v| v["flds"][1]["ord"] = Value::Null);
            let report = save(c, base, replacement).unwrap();
            assert_eq!(report.layouts[&id.0].field_map, [0, -1]);
            assert_eq!(report.layouts[&id.0].template_map, [0]);
            assert_eq!(report.reasons, [StructureReason::FieldsChanged, StructureReason::SortFieldChanged].into_iter().collect());
            assert_eq!(read(c, id).config.sort_field_idx, 1);
            assert_eq!(backup.calls(), 2);
            assert_eq!(scm(c), 1_894_701_600_000);
        });
    }

    #[test]
    fn hosted_legacy_converted_unused_id_creates_new_destination_without_source_layout_or_g2() {
        hosted(|c, source, backup| {
            let source_id = source.id;
            let id = NotetypeId(900_000_001);
            let before_scm = scm(c);
            let next = decoded(&source, |v| {
                v["id"] = serde_json::json!(id.0);
                v["name"] = serde_json::json!("Unique legacy destination");
                v["flds"].as_array_mut().unwrap().swap(0, 1);
                v["mod"] = serde_json::json!(123);
                v["usn"] = serde_json::json!(17);
            });
            let report = save(c, source.clone(), next).unwrap();
            assert!(report.is_empty());
            assert!(exact_base(&source, &read(c, source_id)));
            let stored = read(c, id);
            assert_eq!(stored.name, "Unique legacy destination");
            assert_eq!((stored.mtime, stored.usn), (EpochSecs(123), Usn(17)));
            assert_eq!(stored.fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(), ["Back", "Front"]);
            assert_eq!(backup.calls(), 0);
            assert_eq!(scm(c), before_scm);
        });
    }

    #[test]
    fn hosted_legacy_destination_origins_use_its_keys_not_source_names_or_ids() {
        hosted(|c, source, backup| {
            let mut destination = source.clone();
            destination.id = NotetypeId(900_000_002);
            destination.name = "Stored destination".into();
            destination.fields[0].ord = 7;
            destination.fields[1].ord = 42;
            destination.templates[0].ord = 9;
            insert_new_destination(c, &destination);
            let destination = read(c, destination.id);
            // Optional IDs and names are identical to the source. Only the
            // destination's physical previous keys can author its origins.
            let bad = decoded(&source, |v| v["id"] = serde_json::json!(destination.id.0));
            assert!(matches!(save(c, source.clone(), bad), Err(EngineError::InvalidArgument { .. })));
            assert!(exact_base(&destination, &read(c, destination.id)));
            let next = decoded(&source, |v| {
                v["id"] = serde_json::json!(destination.id.0);
                v["name"] = serde_json::json!("Updated destination");
                v["flds"][0]["ord"] = serde_json::json!(7);
                v["flds"][1]["ord"] = serde_json::json!(42);
                v["tmpls"][0]["ord"] = serde_json::json!(9);
                v["css"] = serde_json::json!(".card { color: navy; }");
            });
            let before_scm = scm(c);
            let report = save(c, source.clone(), next).unwrap();
            assert_eq!(report.layouts.len(), 1);
            assert!(report.layouts[&destination.id.0].is_identity());
            assert!(report.reasons.is_empty());
            assert!(!report.layouts.contains_key(&source.id.0));
            assert!(exact_base(&source, &read(c, source.id)));
            assert_eq!(read(c, destination.id).config.css, ".card { color: navy; }");
            assert_eq!(backup.calls(), 0);
            assert_eq!(scm(c), before_scm);
        });
    }

    #[test]
    fn hosted_legacy_sparse_field_7_template_9_css_save_has_logical_identity() {
        hosted(|c, mut base, backup| {
            base.id = NotetypeId(900_000_003);
            base.name = "Sparse singleton".into();
            base.fields.truncate(1);
            base.fields[0].ord = 7;
            base.templates[0].ord = 9;
            base.templates[0].config.a_format = "{{Front}}".into();
            insert_new_destination(c, &base);
            let base = read(c, base.id);
            let id = base.id;
            let before_scm = scm(c);
            let next = decoded(&base, |v| v["css"] = serde_json::json!(".card { color: green; }"));
            let report = save(c, base, next).unwrap();
            assert_eq!(report.layouts[&id.0].field_map, [0]);
            assert_eq!(report.layouts[&id.0].template_map, [0]);
            assert!(report.layouts[&id.0].is_identity());
            assert!(report.reasons.is_empty());
            let stored = read(c, id);
            assert_eq!((stored.fields[0].ord, stored.templates[0].ord, stored.config.sort_field_idx), (0, 0, 0));
            assert_eq!(stored.config.css, ".card { color: green; }");
            assert_eq!(scm(c), before_scm);
            assert_eq!(backup.calls(), 0);
        });
    }

    #[test]
    fn hosted_legacy_sparse_reorder_resolves_exact_keys_and_preserves_sort_identity() {
        hosted(|c, mut base, backup| {
            base.id = NotetypeId(900_000_004);
            base.name = "Sparse reorder".into();
            base.fields[0].ord = 7;
            base.fields[1].ord = 42;
            base.templates[0].ord = 9;
            let mut second = base.templates[0].clone();
            second.ord = 99;
            second.name = "Other view".into();
            base.templates.push(second);
            base.config.sort_field_idx = 1;
            insert_new_destination(c, &base);
            let base = read(c, base.id);
            let id = base.id;
            let before_scm = scm(c);
            for (part, key) in [("flds", 0), ("flds", 7), ("tmpls", 0), ("tmpls", 9)] {
                let bad = decoded(&base, |v| v[part][1]["ord"] = serde_json::json!(key));
                assert!(matches!(save(c, base.clone(), bad), Err(EngineError::InvalidArgument { .. })));
                assert!(exact_base(&base, &read(c, id)));
                assert_eq!(scm(c), before_scm);
            }
            assert_eq!(backup.calls(), 0);
            let next = decoded(&base, |v| {
                v["flds"].as_array_mut().unwrap().swap(0, 1);
                v["tmpls"].as_array_mut().unwrap().swap(0, 1);
            });
            let report = save(c, base, next).unwrap();
            let layout = &report.layouts[&id.0];
            assert_eq!(layout.field_map, [1, 0]);
            assert_eq!(layout.template_map, [1, 0]);
            assert_eq!((layout.before.sort_field, layout.after.sort_field), (1, 0));
            assert_eq!(report.reasons, [StructureReason::FieldsChanged, StructureReason::TemplatesChanged].into_iter().collect());
            let stored = read(c, id);
            assert_eq!(stored.fields.iter().map(|field| field.ord).collect::<Vec<_>>(), [0, 1]);
            assert_eq!(stored.templates.iter().map(|template| template.ord).collect::<Vec<_>>(), [0, 1]);
            assert_eq!(stored.config.sort_field_idx, 0);
            assert_eq!(backup.calls(), 1);
            assert_eq!(scm(c), 1_894_701_600_000);
        });
    }

    #[test]
    fn hosted_legacy_namespace_race_is_stale_without_target_or_scm_write() {
        hosted(|c, base, _| {
            let mut next = decoded(&base, |v| {
                v["name"] = serde_json::json!("Occupied after planning");
                v["sortf"] = serde_json::json!(1);
            });
            let plan = prepare(c, base.clone(), &mut next).unwrap();
            let mut competitor = base.clone();
            competitor.id = NotetypeId(900_000_005);
            competitor.name = next.name.clone();
            insert_new_destination(c, &competitor);
            let competitor = read(c, competitor.id);
            let after_competitor_scm = scm(c);
            assert!(exact_base(&base, &read(c, base.id)), "target did not race");
            assert!(matches!(execute(c, plan, next), Err(EngineError::Stale { .. })));
            assert!(exact_base(&base, &read(c, base.id)));
            assert!(exact_base(&competitor, &read(c, competitor.id)));
            assert_eq!(scm(c), after_competitor_scm);
        });
    }

    #[test]
    fn hosted_legacy_converted_id_validates_source_destination_and_new_destination_absence() {
        hosted(|c, source, _| {
            // Each competitor changes only a stamp, so namespace validation
            // cannot substitute for either exact aggregate dependency.
            for race_source in [true, false] {
                let source = read(c, source.id);
                let mut destination = source.clone();
                destination.id = NotetypeId(if race_source { 900_000_006 } else { 900_000_007 });
                destination.name = format!("Destination race {race_source}");
                insert_new_destination(c, &destination);
                let mut next = decoded(&source, |v| {
                    v["id"] = serde_json::json!(destination.id.0);
                    v["name"] = serde_json::json!(destination.name);
                    v["sortf"] = serde_json::json!(1);
                });
                let plan = prepare(c, source.clone(), &mut next).unwrap();
                let mut competitor = if race_source { source.clone() } else { read(c, destination.id) };
                competitor.usn = Usn(competitor.usn.0.checked_add(1).unwrap());
                c.run(OpSpec::named("legacy dependency stamp race", OpSource::UserNonUndoable)
                    .stamp(StampMode::CallerSupplied), move |ctx| { ctx.scope().put_notetype(&competitor)?; Ok(()) }).unwrap();
                let live_source = read(c, source.id);
                let live_destination = read(c, destination.id);
                if race_source {
                    assert!(!exact_base(&source, &live_source), "source stamp must actually race");
                } else {
                    assert!(!exact_base(&destination, &live_destination), "destination stamp must actually race");
                }
                let before_scm = scm(c);
                assert!(matches!(execute(c, plan, next), Err(EngineError::Stale { .. })));
                assert!(exact_base(&live_source, &read(c, source.id)));
                assert!(exact_base(&live_destination, &read(c, destination.id)));
                assert_eq!(scm(c), before_scm);
            }
            let source = read(c, source.id);
            let mut next = decoded(&source, |v| {
                v["id"] = serde_json::json!(900_000_008);
                v["name"] = serde_json::json!("New destination raced by another aggregate");
            });
            let plan = prepare(c, source.clone(), &mut next).unwrap();
            let mut competitor = next.clone();
            competitor.name = "Different competitor name".into();
            insert_new_destination(c, &competitor);
            let live = read(c, competitor.id);
            let before_scm = scm(c);
            assert!(matches!(execute(c, plan, next), Err(EngineError::Stale { .. })));
            assert!(exact_base(&source, &read(c, source.id)));
            assert!(exact_base(&live, &read(c, live.id)));
            assert_eq!(scm(c), before_scm);
        });
    }

    #[test]
    fn hosted_legacy_cloze_addition_has_empty_card_identity_prefix_and_explicit_fields() {
        hosted(|c, _, backup| {
            let (base, note) = cloze(c, "{{c1::One}} {{c3::Three}} {{c8::Eight}}");
            let id = base.id;
            seed_noncanonical_cards(c, note);
            let before_cards = cards(c, note);
            let before_images = card_images(c, note);
            assert_eq!(before_cards.0, [0, 2, 7]);
            let next = decoded(&base, |v| {
                let mut added = v["tmpls"][0].clone();
                added["ord"] = Value::Null;
                added["name"] = serde_json::json!("Added cloze view");
                v["tmpls"].as_array_mut().unwrap().push(added);
                v["flds"].as_array_mut().unwrap().swap(0, 1);
            });
            assert_eq!(next.templates[1].prior_ord, None);
            let report = save(c, base, next).unwrap();
            let layout = &report.layouts[&id.0];
            assert!(layout.before.cloze && layout.after.cloze);
            assert_eq!(layout.field_map, [1, 0]);
            assert_eq!(layout.template_map, Vec::<i32>::new(), "identity over every cloze number, not physical [0, -1]");
            assert_eq!(report.reasons, [StructureReason::FieldsChanged, StructureReason::TemplatesChanged].into_iter().collect());
            assert_eq!(cards(c, note), before_cards, "all card data and ordinals must survive L2");
            assert_eq!(card_images(c, note), before_images, "every stored card column must survive L2 exactly");
            assert_eq!(read(c, id).templates.len(), 2);
            assert_eq!(backup.calls(), 1);
            assert_eq!(scm(c), 1_894_701_600_000);
        });
    }

    #[test]
    fn hosted_legacy_representable_standard_to_cloze_reports_kind_without_new_reason() {
        hosted(|c, base, backup| {
            let id = base.id;
            let note = meguri_engine::notes::add(c, meguri_engine::notes::AddNote {
                notetype_id: id, fields: vec!["{{c1::One}}".into(), "Answer context".into()], tags: Vec::new(),
                deck_id: meguri_model::ids::DeckId(1), guid: None, update_add_defaults: false,
            }, OpSource::UserNonUndoable).unwrap().value.id;
            seed_noncanonical_cards(c, note);
            let before_cards = cards(c, note);
            let before_images = card_images(c, note);
            assert_eq!(before_cards.0, [0]);
            let before_scm = scm(c);
            let next = decoded(&base, |v| {
                v["type"] = serde_json::json!(1);
                v["tmpls"][0]["qfmt"] = serde_json::json!("{{cloze:Front}}");
                v["tmpls"][0]["afmt"] = serde_json::json!("{{cloze:Front}}<hr>{{Back}}");
            });
            let report = save(c, base, next).unwrap();
            let layout = &report.layouts[&id.0];
            assert!(!layout.before.cloze && layout.after.cloze);
            assert_eq!(layout.field_map, [0, 1]);
            assert_eq!(layout.template_map, [0]);
            assert!(report.reasons.is_empty());
            assert_eq!(read(c, id).config.kind, 1);
            assert_eq!(cards(c, note), before_cards);
            assert_eq!(card_images(c, note), before_images, "every stored card column must survive L2 exactly");
            assert_eq!(backup.calls(), 0);
            assert_eq!(scm(c), before_scm);
        });
    }

    #[test]
    fn hosted_legacy_cloze_to_standard_requires_actual_preserved_cards_in_domain() {
        for (text, representable) in [("{{c1::One}}", true), ("{{c1::One}} {{c3::Three}}", false)] {
            hosted(|c, unrelated_type, backup| {
                let (base, note) = cloze(c, text);
                let id = base.id;
                let (unrelated, unrelated_image) = unreadable_note_control(c, unrelated_type.id);
                seed_noncanonical_cards(c, note);
                let before_cards = cards(c, note);
                let before_images = card_images(c, note);
                assert_eq!(before_cards.0, if representable { vec![0] } else { vec![0, 2] });
                let before_scm = scm(c);
                let next = decoded(&base, |v| {
                    v["type"] = serde_json::json!(0);
                    v["tmpls"][0]["qfmt"] = serde_json::json!("{{Front}}");
                    v["tmpls"][0]["afmt"] = serde_json::json!("{{Back}}");
                });
                let result = save(c, base.clone(), next);
                if representable {
                    let report = result.unwrap();
                    let layout = &report.layouts[&id.0];
                    assert!(layout.before.cloze && !layout.after.cloze);
                    assert_eq!(layout.field_map, [0, 1]);
                    assert!(layout.template_map.is_empty());
                    assert!(report.reasons.is_empty());
                    assert_eq!(read(c, id).config.kind, 0);
                } else {
                    assert!(matches!(result, Err(EngineError::Storage(meguri_storage::StorageError::StructureNotReported { notetype_id })) if notetype_id == id));
                    assert!(exact_base(&base, &read(c, id)));
                }
                assert_eq!(cards(c, note), before_cards);
                assert_eq!(card_images(c, note), before_images, "every stored card column must survive L2 exactly");
                let rows = c.reader(meguri_storage::ReaderRole::Query).unwrap();
                assert!(rows.note(unrelated).is_err(), "unrelated note remains unreadable");
                assert_eq!(rows.row_image(Table::Notes, &RowKey::Id(unrelated.0)).unwrap(), Some(unrelated_image));
                drop(rows);
                assert_eq!(backup.calls(), 0);
                assert_eq!(scm(c), before_scm);
            });
        }
    }

    #[test]
    fn hosted_legacy_mixed_kind_physical_arity_gap_keeps_typed_missing_authority() {
        hosted(|c, base, _| {
            let id = base.id;
            let before_scm = scm(c);
            let next = decoded(&base, |v| {
                v["type"] = serde_json::json!(1);
                v["tmpls"][0]["qfmt"] = serde_json::json!("{{cloze:Front}}");
                v["tmpls"][0]["afmt"] = serde_json::json!("{{cloze:Front}}<hr>{{Back}}");
                let mut added = v["tmpls"][0].clone();
                added["ord"] = Value::Null;
                added["name"] = serde_json::json!("Second cloze view");
                v["tmpls"].as_array_mut().unwrap().push(added);
            });
            assert!(matches!(save(c, base.clone(), next), Err(EngineError::Storage(meguri_storage::StorageError::StructureNotReported { notetype_id })) if notetype_id == id));
            assert!(exact_base(&base, &read(c, id)));
            assert_eq!(scm(c), before_scm);
        });
    }
}
