//! Deck packages for the MP7.1 tests: a source collection written by the
//! production package writer under a deck package's name.
use super::bootstrap as b;
use meguri_engine::{CancelToken, OpSource, OpSpec, ProgressSink,
    notetypes::{NewNotetype, NewTemplate, NotetypeKind}};
use meguri_host::CoreHost;
use meguri_model::{clock::{FixedClock, UtcOffset}, ids::{DeckId, NotetypeId}, notetype::Notetype,
    rng::SeededRandom, time::EpochSecs};
use meguri_package::{CollectionPackageWriteContext, Finalisation, apkg::{self, ImportOptions, ImportSession, UpdateCondition},
    write_collection_package_from_live};
use meguri_storage::{ReadRows, ReaderRole, StampMode, atomic_file::Purpose, space::StatvfsSpace};
use std::{path::{Path, PathBuf}, sync::Arc};

/// The id of the note type both sides hold.
pub const PAIR: i64 = 5000;
pub const OFF: ImportOptions = ImportOptions { merge_notetypes: false, update_notes: UpdateCondition::IfNewer,
    update_notetypes: UpdateCondition::IfNewer, with_scheduling: false, with_deck_configs: false };

/// The note type `PAIR`, "Pair", with two fields and one card, sorted by
/// the field at `sort`, last changed at `mtime`.
pub fn pair(mtime: i64, sort: u32) -> Notetype {
    let mut built = meguri_engine::notetypes::from_scratch(&NewNotetype { name: "Pair".into(), kind: NotetypeKind::Standard,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![NewTemplate { name: "Card 1".into(), front: "{{Front}}".into(), back: "{{FrontSide}}".into(),
            ..NewTemplate::default() }], css: None }, &SeededRandom::new(5));
    built.id = NotetypeId(PAIR);
    built.mtime = EpochSecs(mtime);
    built.config.sort_field_idx = sort;
    for (at, field) in built.fields.iter_mut().enumerate() { field.ord = at as u32; field.config.id = Some(11 + at as i64); }
    for (at, template) in built.templates.iter_mut().enumerate() { template.ord = at as u32; template.config.id = Some(21 + at as i64); }
    built
}
pub fn put_notetype(host: &CoreHost, built: Notetype) {
    host.collection().unwrap().run(OpSpec::named("test setup", OpSource::Maintenance).stamp(StampMode::CallerSupplied),
        move |ctx| { ctx.scope().put_notetype(&built)?; Ok(()) }).unwrap();
}
pub fn add(host: &CoreHost, notetype: i64, guid: &str, front: &str, back: &str) {
    meguri_engine::notes::add(&host.collection().unwrap(), meguri_engine::notes::AddNote {
        notetype_id: NotetypeId(notetype), fields: vec![front.into(), back.into()], tags: vec![], deck_id: DeckId(1),
        guid: Some(guid.into()), update_add_defaults: false }, OpSource::UserUndoable).unwrap();
}
/// The GUIDs these tests chose (`g-…`), sorted; the rig's own notes have others.
pub fn guids(host: &CoreHost) -> Vec<String> {
    let mut found: Vec<String> = host.collection().unwrap().reader(ReaderRole::Query).unwrap()
        .notes_after(None, 1000).unwrap().into_iter().map(|note| note.guid).filter(|guid| guid.starts_with("g-")).collect();
    found.sort();
    found
}
pub fn sort_field(host: &CoreHost) -> u32 {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().notetype(NotetypeId(PAIR)).unwrap().unwrap()
        .config.sort_field_idx
}
/// A deck package `name` under `root` holding the collection `build` makes.
pub fn package(root: &Path, name: &str, build: impl FnOnce(&CoreHost)) -> PathBuf {
    let (source, _) = b::seeded(&root.join(format!("source-{name}")));
    build(&source);
    let path = root.join(name);
    let (clock, random) = (FixedClock::new(b::NOW, UtcOffset::UTC), SeededRandom::new(31));
    let collection = source.collection().unwrap();
    {
        let lease = collection.reader(ReaderRole::Maintenance).unwrap();
        write_collection_package_from_live(&lease, &path, &CollectionPackageWriteContext {
            collection_directory: collection.path().parent().unwrap(), purpose: Purpose::Export,
            finalisation: Finalisation::Replace, clock: &clock, random: &random, space: &StatvfsSpace,
            is_cancelled: &|| false }).unwrap();
    }
    drop(collection);
    source.close().unwrap();
    path
}
/// A package whose import makes `PAIR` sort by its second field: a
/// structural import with an empty merge preview. It brings the note `g-one`.
pub fn sorting_package(root: &Path) -> PathBuf {
    package(root, "sorting.apkg", |source| {
        put_notetype(source, pair(200, 1));
        add(source, PAIR, "g-one", "sun", "soleil");
    })
}
/// A package that only adds the note `g-plain` of `PAIR` as the target holds it.
pub fn plain_package(root: &Path) -> PathBuf {
    package(root, "plain.apkg", |source| {
        put_notetype(source, pair(100, 0));
        add(source, PAIR, "g-plain", "star", "etoile");
    })
}
/// The target's side: `PAIR` sorted by its first field, with the note `g-two`.
pub fn target_setup(host: &CoreHost) {
    put_notetype(host, pair(100, 0));
    add(host, PAIR, "g-two", "moon", "lune");
}
pub fn inspect(host: &CoreHost, package: &Path) -> Arc<ImportSession> {
    let name = package.file_name().unwrap().to_str().unwrap();
    Arc::new(apkg::inspect(package, name, &host.deck_package_context(), &CancelToken::new(), &ProgressSink::new()).unwrap())
}
/// The `.meguri-stage-` directories of the host's collection directory.
pub fn staging(host: &CoreHost) -> usize {
    std::fs::read_dir(host.deck_package_context().collection_directory).unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_name().to_string_lossy().starts_with(".meguri-stage-")).count()
}
