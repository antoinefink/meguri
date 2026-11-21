//! Rig for the MP7 structure save job: a ready binding over generated notes,
//! and one scripted transport for every phase of a job.
use super::bootstrap as b;
use super::round as r;
use meguri_engine::{EventKind, OpSource, notetypes::structure::{self, StructureDraft, StructureSaved}};
use meguri_host::{CoreHost, HostConfig, MeguriAccessSource, MeguriBootstrapDirection, MeguriStructureOutcome,
    MeguriStructureStage, MeguriStructureStep, MeguriStructureTask, MeguriStructureWork};
use meguri_model::{clock::{StepClock, UtcOffset}, col::Col, ids::{DeckId, NotetypeId}, rng::SeededRandom,
    time::EpochMillis, usn::Usn};
use meguri_storage::{OpenOptions, ReadRows, ReaderRole, StampMode, Table, open_collection};
use meguri_sync::{Method, provider::MeguriConnection};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::{Arc, Mutex}};

/// The server's sequence number in every scripted answer before a job.
pub const USN: i32 = 9;
/// The time the rig's clock shows once the binding is ready.
pub const LATER: i64 = b::NOW + 60_000;

pub struct Rig {
    pub host: Arc<CoreHost>,
    pub config: HostConfig,
    pub connection: MeguriConnection,
    pub notetype: NotetypeId,
    pub clock: Arc<StepClock>,
}
pub fn config(root: &Path, now: i64) -> HostConfig {
    b::config(root, Arc::new(StepClock::new(now, UtcOffset::UTC, "UTC")))
}
pub fn basic(host: &CoreHost) -> NotetypeId {
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    reader.ids_after(Table::Notetypes, None, 100).unwrap().into_iter().map(NotetypeId)
        .find(|id| reader.notetype(*id).unwrap().is_some_and(|notetype| notetype.name == "Basic")).unwrap()
}
pub fn add_note(host: &CoreHost, notetype: NotetypeId, front: &str, back: &str) {
    meguri_engine::notes::add(&host.collection().unwrap(), meguri_engine::notes::AddNote {
        notetype_id: notetype, fields: vec![front.into(), back.into()], tags: vec![], deck_id: DeckId(1),
        guid: None, update_add_defaults: false }, OpSource::UserUndoable).unwrap();
}
/// A device whose Meguri Sync binding is ready, with `notes` notes of the
/// Basic note type made before setup, so that nothing is pending. The clock
/// then moves on: a later save stamps `scm` after `ls`.
pub fn rig(root: &Path, notes: usize) -> Rig {
    let clock = Arc::new(StepClock::new(b::NOW, UtcOffset::UTC, "UTC"));
    let config = b::config(&root.join("local"), clock.clone());
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(), config.random.as_ref()).unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    b::settle(&host);
    let notetype = basic(&host);
    for index in 0..notes { add_note(&host, notetype, &format!("question {index}"), &format!("answer {index}")); }
    let connection = b::select(&host);
    let (_, bytes) = b::server(&root.join("server"));
    let script = b::collection_script(bytes, true);
    let preview = b::prepare(&host, &connection, &script, MeguriBootstrapDirection::UseDeviceCollection);
    let op = b::confirm(&host, &connection, &script, preview);
    let op = b::finish_empty_media(&host, &connection, &op);
    assert!(host.complete_meguri_bootstrap(&connection, b::USER, &op.id).unwrap().is_complete());
    clock.set_now_ms(LATER);
    Rig { host: Arc::new(host), config, connection, notetype, clock }
}
pub fn col(host: &CoreHost) -> Col {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().col().unwrap()
}
pub fn field_names(host: &CoreHost, notetype: NotetypeId) -> Vec<String> {
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    reader.notetype(notetype).unwrap().unwrap().fields.iter().map(|field| field.name.clone()).collect()
}
/// A draft of the Basic note type without its second field, whose content
/// in `confirmed` notes the user accepted to lose.
pub fn draft_without_back(host: &CoreHost, notetype: NotetypeId, confirmed: u64) -> StructureDraft {
    let collection = host.collection().unwrap();
    let stored = collection.reader(ReaderRole::Query).unwrap().notetype(notetype).unwrap().unwrap();
    let mut draft = StructureDraft::of(&stored);
    draft.fields.remove(1);
    draft.skip_checks = true;
    draft.confirmed.fields.insert(1, confirmed);
    draft
}
/// A draft that only makes the second field the sort field: a schema
/// change that deletes nothing and keeps every count.
pub fn draft_sorting_by_back(host: &CoreHost, notetype: NotetypeId) -> StructureDraft {
    let collection = host.collection().unwrap();
    let stored = collection.reader(ReaderRole::Query).unwrap().notetype(notetype).unwrap().unwrap();
    let mut draft = StructureDraft::of(&stored);
    draft.sort_field = Some(1);
    draft
}
pub type Saved = Arc<Mutex<Option<StructureSaved>>>;
pub fn save_work(draft: StructureDraft) -> (MeguriStructureWork, Saved) {
    let saved = Saved::default();
    let output = saved.clone();
    (Box::new(move |collection, job| {
        let result = structure::apply(collection, draft, job.cancel.clone(), job.progress.clone())?;
        *output.lock().unwrap() = Some(result.value);
        Ok(())
    }), saved)
}
pub fn source(connection: &MeguriConnection, script: &Arc<r::Script>) -> MeguriAccessSource {
    let (connection, script) = (connection.clone(), script.clone());
    Arc::new(move || Ok(r::access(&connection, &script)))
}
pub fn meta(modified: i64, schema: i64, usn: i32, empty: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({"mod": modified, "scm": schema, "usn": usn, "ts": LATER / 1000, "msg": "",
        "cont": true, "empty": empty, "media_usn": 0})).unwrap()
}
/// A round that finds nothing to exchange.
pub fn quiet_round(host: &CoreHost) -> Vec<r::Step> {
    let col = col(host);
    vec![r::Step::new(Method::Meta, meta(col.modified.0, col.schema_modified.0, USN, false))]
}
/// The server's collection file as it is served, with the stamps that
/// serving it leaves.
pub fn served(root: &Path, modified: i64, schema: i64, usn: i32) -> Vec<u8> {
    fs::create_dir_all(root).unwrap();
    let path = root.join("collection.anki2");
    let _ = fs::remove_file(&path);
    let clock = meguri_model::clock::FixedClock::new(b::NOW, UtcOffset::UTC);
    let random = SeededRandom::new(76);
    meguri_engine::bootstrap::create(&path, &clock, &random).unwrap();
    let db = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let mut scope = db.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    let mut col = scope.col().unwrap();
    col.modified = EpochMillis(modified);
    col.schema_modified = EpochMillis(schema);
    col.usn = Usn(usn);
    scope.put_col(&col).unwrap();
    scope.commit().unwrap();
    db.close().unwrap();
    fs::read(path).unwrap()
}
/// The upload leg against a server that still holds `(modified, schema)`:
/// the read of its facts, the preparation with the retained server copy,
/// the confirmation's read, the upload and an empty media exchange.
pub fn upload_steps(root: &Path, modified: i64, schema: i64) -> Vec<r::Step> {
    upload_steps_from(root, modified, schema, USN)
}
pub fn upload_steps_from(root: &Path, modified: i64, schema: i64, usn: i32) -> Vec<r::Step> {
    let after = modified + 500;
    vec![r::Step::new(Method::Meta, meta(modified, schema, usn, true)),
        r::Step::new(Method::Meta, meta(modified, schema, usn, true)),
        r::Step::new(Method::Download, served(&root.join("served"), after, schema, usn + 1)),
        r::Step::new(Method::Meta, meta(after, schema, usn + 1, true)),
        r::Step::new(Method::Meta, meta(after, schema, usn + 1, true)),
        r::Step::new(Method::Upload, b"OK".to_vec()),
        r::begin(0)]
}
/// Advances the job, waiting for each phase's own job, until `stop`
/// answers true for a step.
pub fn drive_until(host: &CoreHost, task: &MeguriStructureTask,
    stop: impl Fn(&MeguriStructureStep) -> bool) -> MeguriStructureStep {
    loop {
        let events = host.engine().stream().subscribe();
        let step = host.advance_meguri_structure(task).unwrap();
        if stop(&step) { return step; }
        let MeguriStructureStep::Running { job, .. } = &step else { return step };
        loop {
            let event = events.recv().expect("engine stream stays open");
            if matches!(event.kind, EventKind::JobFinished { id, .. } if id == job.id) { break; }
        }
    }
}
pub fn drive(host: &CoreHost, task: &MeguriStructureTask) -> Arc<MeguriStructureOutcome> {
    match drive_until(host, task, |step| matches!(step, MeguriStructureStep::Finished(_))) {
        MeguriStructureStep::Finished(outcome) => outcome,
        MeguriStructureStep::Running { .. } => unreachable!("the drive ends at an outcome"),
    }
}
pub fn at(stage: MeguriStructureStage) -> impl Fn(&MeguriStructureStep) -> bool {
    move |step| matches!(step, MeguriStructureStep::Running { stage: current, .. } if *current == stage)
}
/// One exchange that sends a changed note type and a new note, for the wire
/// rows another device would send: the note type's model and the note's row.
pub fn exchanged_rows(rig: &Rig) -> (Value, Value) {
    let collection = rig.host.collection().unwrap();
    let stored = collection.reader(ReaderRole::Query).unwrap().notetype(rig.notetype).unwrap().unwrap();
    let mut draft = meguri_engine::notetypes::Draft::of(stored);
    draft.fields[0].config.sticky = true;
    meguri_engine::notetypes::save(&collection, draft, Default::default()).unwrap();
    add_note(&rig.host, rig.notetype, "question sent", "answer sent");
    let script = r::Script::new(r::normal(&rig.host, json!({"done": true}), r::empty_changes(), USN));
    let result = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &script))));
    assert_eq!(result.collection.as_ref().unwrap().state, meguri_host::MeguriCollectionState::Merged);
    script.assert_done();
    let changes: Value = serde_json::from_slice(&script.bodies(Method::ApplyChanges)[0]).unwrap();
    let model = changes["changes"]["models"].as_array().unwrap().iter()
        .find(|model| model["id"] == rig.notetype.0).unwrap().clone();
    let chunk: Value = serde_json::from_slice(&script.bodies(Method::ApplyChunk)[0]).unwrap();
    let note = chunk["chunk"]["notes"].as_array().unwrap().last().unwrap().clone();
    (model, note)
}
