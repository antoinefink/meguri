use crate::{Result, need};
use meguri_engine::{OpSource, OpSpec, decks, notes, notetypes, session::{StartResult, AnswerOutcome}};
use meguri_host::CoreHost;
use meguri_model::{clock::Clock, ids::*, proto::deck::DeckKind, time::EpochMillis};
use meguri_sched::{answer::Rating, options::{Field, Value as OptionValue}};
use meguri_storage::{Cell, ConfigValue, ReadRows, ReaderRole, RowKey, Table};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const RECIPES: &[u8] = include_bytes!("../../../recordings/scenarios/meguri-sync-server/final-fixture-recipes.json");
pub const SELECTION: &[u8] = include_bytes!("../../../recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json");

pub fn recipes() -> Value { serde_json::from_slice(RECIPES).expect("committed recipes") }
fn strings(v: &Value) -> Result<Vec<String>> {
    v.as_array().ok_or("recipe-array")?.iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or("recipe-string".into())).collect()
}
pub fn initialize(host: &CoreHost, replacement: bool) -> Result<()> {
    let r = recipes();
    let rich = &r["rich_initial"];
    let left = &r["replacement"]["left_objects"];
    let c = host.collection()?;
    let nt = notetypes::add(&c, notetypes::NewNotetype {
        name: if replacement { left["note_type"].as_str() } else { rich["note_type"].as_str() }.ok_or("recipe-type")?.into(),
        fields: strings(&rich["fields_in_order"] )?,
        templates: vec![notetypes::NewTemplate {
            name: rich["template"]["name"].as_str().ok_or("recipe-template")?.into(),
            front: rich["template"]["question"].as_str().ok_or("recipe-template")?.into(),
            back: rich["template"]["answer"].as_str().ok_or("recipe-template")?.into(),
            ..Default::default()
        }],
        css: Some(rich["template"]["css"].as_str().ok_or("recipe-css")?.into()),
        ..Default::default()
    })?.value.id;
    let name = if replacement { &left["deck"] } else { &rich["deck"]["name"] };
    let mut deck = decks::ops::add(&c, name.as_str().ok_or("recipe-deck")?.into())?.value;
    if let DeckKind::Normal(ref mut normal) = deck.kind.kind {
        normal.description = rich["deck"]["description"].as_str().ok_or("recipe-description")?.into();
    }
    deck.common.study_collapsed = rich["deck"]["collapsed"].as_bool().ok_or("recipe-collapse")?;
    let did = deck.id;
    c.run(OpSpec::named("synthetic deck settings", OpSource::UserUndoable), move |ctx| {
        decks::ops::save_in(ctx.scope(), deck)
    })?;
    let preset_name = if replacement { &left["preset"] } else { &rich["preset"]["name"] };
    let preset = meguri_engine::deck_config::ops::add(&c, preset_name.as_str().ok_or("recipe-preset")?.into())?.value;
    meguri_engine::deck_config::ops::assign(&c, did, preset, false)?;
    set_preset(host, did, &rich["preset"])?;
    let config = if replacement { &left["config"] } else { &rich["config"] };
    set_config(host, config)?;
    if replacement {
        notes::add(&c, notes::AddNote { notetype_id: nt, deck_id: did,
            fields: strings(&r["replacement"]["left_fields"] )?, tags: strings(&rich["tags"] )?,
            guid: None, update_add_defaults: true }, OpSource::UserUndoable)?;
        set_media(host, &r["replacement"]["local_media_utf8_exact"])?;
    } else {
        for note in rich["notes"].as_array().ok_or("recipe-notes")? {
            notes::add(&c, notes::AddNote { notetype_id: nt, deck_id: did,
                fields: strings(&note["fields"] )?, tags: strings(&rich["tags"] )?,
                guid: None, update_add_defaults: true }, OpSource::UserUndoable)?;
        }
        set_media(host, &r["media_utf8_exact"]["initial"])?;
    }
    Ok(())
}
fn set_config(host: &CoreHost, values: &Value) -> Result<()> {
    for (key, value) in values.as_object().ok_or("recipe-config")? {
        // Typed config operation with ordinary user stamps; no SQL/raw stamp mode.
        let key = key.clone();
        let value = ConfigValue::parse(&serde_json::to_vec(value)?).ok_or("recipe-config-value")?;
        host.collection()?.run(OpSpec::named("synthetic config", OpSource::UserUndoable), move |ctx| {
            ctx.scope().set_config(&key, &value)?;
            Ok(())
        })?;
    }
    Ok(())
}
fn set_preset(host: &CoreHost, deck: DeckId, p: &Value) -> Result<()> {
    use meguri_engine::sched::deck_options;
    let c = host.collection()?;
    let mut draft = deck_options::Draft::new(deck_options::snapshot(&c, deck)?);
    let selected = draft.selected;
    for (field, key) in [(Field::NewPerDay, "new_per_day"), (Field::ReviewsPerDay, "reviews_per_day"),
        (Field::CapAnswerTimeToSecs, "maximum_answer_seconds")] {
        draft.edit(Some(selected), field, OptionValue::Number(p[key].as_u64().ok_or("recipe-preset-number")? as f64))?;
    }
    draft.edit(Some(selected), Field::DisableAutoplay, OptionValue::Boolean(!p["autoplay"].as_bool().ok_or("recipe-autoplay")?))?;
    deck_options::save(&c, draft, &Default::default())?;
    Ok(())
}
pub fn forward(host: &CoreHost, clock: &dyn Clock) -> Result<Vec<Value>> {
    let r = recipes();
    let changes = &r["rich_forward_changes"];
    let c = host.collection()?;
    let reader = c.reader(ReaderRole::Query)?;
    let nt = notetypes::read::by_name(&*reader, "Observatory Study")?.ok_or("missing-authored-type")?;
    // Retain the original aggregate and template identity through the other edits.
    let formats = notetypes::TemplateFormatsUpdate {
        notetype: nt.id,
        base: notetypes::AggregateDigest::of(&nt),
        template: notetypes::TemplateIdentity::of(nt.templates.first().ok_or("missing-authored-template")?),
        front: changes["question"].as_str().ok_or("recipe-question")?.into(),
        back: changes["answer"].as_str().ok_or("recipe-answer")?.into(),
        css: Some(changes["css"].as_str().ok_or("recipe-css")?.into()),
    };
    let mut deck = reader.deck_rows()?.into_iter().find(|d| d.name == "Observatory\u{1f}North").ok_or("missing-authored-deck")?;
    let mut keep = None;
    let mut delete = None;
    for id in reader.ids_after(Table::Notes, None, 250)? {
        let note = reader.note(NoteId(id))?.ok_or("missing-note")?;
        if note.field_values().next() == Some("Where is the observatory?") { keep = Some(note); }
        else if note.field_values().next() == Some("Which path crosses the stream?") { delete = Some(NoteId(id)); }
    }
    drop(reader);
    deck.common.study_collapsed = changes["deck_collapsed"].as_bool().ok_or("recipe-collapse")?;
    if let DeckKind::Normal(ref mut normal) = deck.kind.kind {
        normal.description = changes["deck_description"].as_str().ok_or("recipe-description")?.into();
    }
    let did = deck.id;
    c.run(OpSpec::named("synthetic forward deck", OpSource::UserUndoable), move |ctx| decks::ops::save_in(ctx.scope(), deck))?;
    set_preset(host, did, &changes["preset"])?;
    set_config(host, &changes["config"])?;
    let keep = keep.ok_or("missing-keep-note")?;
    let mut fields: Vec<String> = keep.field_values().map(str::to_owned).collect();
    fields[1] = changes["keep_answer"].as_str().ok_or("recipe-answer")?.into();
    notes::update(&c, notes::UpdateNote { id: keep.id, fields, tags: vec!["orbit".into(), "synthetic::field".into()] }, OpSource::UserUndoable)?;
    let (session_id, head) = match host.review().start(did, None)? {
        StartResult::Started { session_id, head, .. } => (session_id, head),
        _ => return Err("review-start-refused".into()),
    };
    let item = head.items.first().ok_or("review-head-empty")?;
    need(item.note_id == keep.id, "keep-note-not-public-review-head")?;
    let result = host.review().answer(session_id, item.card_id, Rating::Good, item.state_token.clone(), EpochMillis(clock.now_ms()), 900)?;
    need(matches!(result, AnswerOutcome::Accepted { .. }), "review-not-accepted")?;
    host.review().end(session_id)?;
    notes::delete(&c, vec![delete.ok_or("missing-delete-note")?], OpSource::UserUndoable)?;
    let saved = notetypes::update_template_formats(&c, formats, Default::default(), Default::default())?.value;
    need(saved.id == nt.id, "template-formats-notetype-mismatch")?;
    Ok(Vec::new())
}
pub fn set_media(host: &CoreHost, target: &Value) -> Result<()> {
    let service = host.media()?;
    let desired = target.as_object().ok_or("recipe-media")?;
    let root = service.store().media_dir();
    let mut remove = Vec::new();
    for item in std::fs::read_dir(root)? {
        let item = item?;
        need(item.file_type()?.is_file(), "unexpected-media-entry")?;
        let name = item.file_name().into_string().map_err(|_| "media-name")?;
        if !desired.contains_key(&name) { remove.push(name); }
    }
    if !remove.is_empty() { service.store().trash(&remove)?; }
    for (name, body) in desired {
        service.write_exact(name, body.as_str().ok_or("recipe-media-body")?.as_bytes())?;
    }
    Ok(())
}
pub fn snapshot(host: &CoreHost) -> Result<Value> {
    let c = host.collection()?;
    let reader = c.reader(ReaderRole::Query)?;
    let mut tables = serde_json::Map::new();
    for table in Table::ALL {
        let mut rows = Vec::new();
        let mut after = None;
        loop {
            let page = reader.row_page(table, after.as_ref())?;
            if page.is_empty() { break; }
            after = Some(RowKey::from_stored(table, page.last().ok_or("row-page")?)?);
            for row in page { rows.push(Value::Array(row.iter().map(cell).collect())); }
        }
        tables.insert(table.name().into(), json!({"columns": table.columns(), "rows": rows}));
    }
    drop(reader);
    let media = host.media()?;
    let mut files = BTreeMap::new();
    for item in std::fs::read_dir(media.store().media_dir())? {
        let item = item?;
        need(item.file_type()?.is_file(), "unexpected-media-entry")?;
        let name = item.file_name().into_string().map_err(|_| "media-name")?;
        files.insert(name, crate::sha(&std::fs::read(item.path())?));
    }
    Ok(json!({"format":"meguri-retained-snapshot-v1", "tables":tables, "media_sha256":files}))
}
fn cell(v: &Cell) -> Value {
    match v {
        Cell::Null => json!({"null":true}),
        Cell::Integer(v) => json!({"integer":v.to_string()}),
        Cell::Real(v) => json!({"real_bits":format!("{:016x}", v.to_bits())}),
        Cell::Text(v) => json!({"text_hex":crate::hex(v)}),
        Cell::Blob(v) => json!({"blob_hex":crate::hex(v)}),
    }
}
/// Only the declared protocol stamps. Keep all original cells in the receipt.
pub fn supported(snapshot: &Value) -> Value {
    let mut result = snapshot.clone();
    for table in Table::ALL {
        let excluded: &[&str] = match table {
            Table::Col => &["mod", "scm", "usn", "ls"],
            Table::Notes | Table::Cards => &["mod", "usn"],
            Table::DeckConfig | Table::Notetypes | Table::Decks | Table::Config | Table::Templates => &["mtime_secs", "usn"],
            Table::Revlog | Table::Graves | Table::Tags => &["usn"],
            Table::Fields => &[],
        };
        for row in result["tables"][table.name()]["rows"].as_array_mut().expect("snapshot rows") {
            for (i, name) in table.columns().iter().enumerate() {
                if excluded.contains(name) { row[i] = json!({"protocol_stamp":true}); }
            }
        }
    }
    result
}
