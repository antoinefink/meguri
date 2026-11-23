//! Deck package import (`specs/package/apkg-import.md` I18.3), with
//! packages this file builds: a source collection written by the
//! production package writer, repacked with the entries a case needs.
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use meguri_engine::{CancelToken, OpSource, OpSpec, ProgressSink, UndoScope};
use meguri_model::clock::Clock;
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_package::apkg::{
    self, ApkgContext, ApkgImportError, ImportOptions, ImportResult, ImportSession, PackageClass, PackageFormat,
    UpdateCondition,
};
use meguri_package::{CollectionPackageWriteContext, Finalisation, write_collection_package_from_live};
use meguri_storage::atomic_file::Purpose;
use meguri_storage::space::{SpaceProvider, StatvfsSpace};
use meguri_storage::{ReadRows, ReaderRole, RowImage, RowKey, StorageError, Table};
use meguri_testkit::normal_support::Fixture;
use sha1::{Digest, Sha1};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub fn context(f: &Fixture) -> ApkgContext {
    ApkgContext {
        collection_directory: f.collection.path().parent().unwrap().to_owned(),
        clock: f.clock.clone(),
        random: f.random.clone(),
        space: Arc::new(StatvfsSpace),
    }
}

/// The whole of `f`'s collection as a latest-format package without media.
pub fn export(f: &Fixture, name: &str) -> PathBuf {
    let path = f.dir.path().join(name);
    let lease = f.collection.reader(ReaderRole::Maintenance).unwrap();
    write_collection_package_from_live(
        &lease,
        &path,
        &CollectionPackageWriteContext {
            collection_directory: f.collection.path().parent().unwrap(),
            purpose: Purpose::Export,
            finalisation: Finalisation::Replace,
            clock: f.clock.as_ref(),
            random: f.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    path
}

pub fn entry(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    archive.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// A latest-format media map and its payload members.
pub fn latest_media(files: &[(&str, &[u8])]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut map = Vec::new();
    let mut payloads = Vec::new();
    for (name, bytes) in files {
        let mut record = vec![10];
        varint(&mut record, name.len() as u64);
        record.extend_from_slice(name.as_bytes());
        record.push(16);
        varint(&mut record, bytes.len() as u64);
        record.extend_from_slice(&[26, 20]);
        record.extend_from_slice(&Sha1::digest(bytes));
        map.push(10);
        varint(&mut map, record.len() as u64);
        map.extend_from_slice(&record);
        payloads.push(zstd::encode_all(*bytes, 3).unwrap());
    }
    (zstd::encode_all(&map[..], 3).unwrap(), payloads)
}

pub fn write_zip(path: &Path, members: &[(String, Vec<u8>)]) {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in members {
        archive.start_file(name.as_str(), options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    std::fs::write(path, archive.finish().unwrap().into_inner()).unwrap();
}

/// `source` (a package of [`export`]) with these media files.
pub fn with_media(source: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let mut archive = ZipArchive::new(std::fs::File::open(source).unwrap()).unwrap();
    let (map, payloads) = latest_media(files);
    let mut members = vec![
        ("meta".to_owned(), entry(&mut archive, "meta")),
        ("collection.anki21b".to_owned(), entry(&mut archive, "collection.anki21b")),
    ];
    for (number, payload) in payloads.into_iter().enumerate() {
        members.push((number.to_string(), payload));
    }
    members.push(("media".to_owned(), map));
    let path = source.with_file_name(name);
    write_zip(&path, &members);
    path
}

/// `source` as a legacy 2 package: the raw collection and a JSON map.
pub fn as_legacy(source: &Path, name: &str, files: &[(&str, &[u8])], meta: Option<&[u8]>) -> PathBuf {
    let mut archive = ZipArchive::new(std::fs::File::open(source).unwrap()).unwrap();
    let collection = zstd::decode_all(&entry(&mut archive, "collection.anki21b")[..]).unwrap();
    let mut members = Vec::new();
    if let Some(meta) = meta {
        members.push(("meta".to_owned(), meta.to_vec()));
    }
    members.push(("collection.anki21".to_owned(), collection));
    let mut map = serde_json::Map::new();
    for (number, (file, bytes)) in files.iter().enumerate() {
        map.insert(number.to_string(), serde_json::Value::String((*file).to_owned()));
        members.push((number.to_string(), bytes.to_vec()));
    }
    members.push(("media".to_owned(), serde_json::to_vec(&map).unwrap()));
    let path = source.with_file_name(name);
    write_zip(&path, &members);
    path
}

pub fn inspect(f: &Fixture, package: &Path) -> Result<ImportSession, ApkgImportError> {
    let name = package.file_name().unwrap().to_str().unwrap();
    apkg::inspect(package, name, &context(f), &CancelToken::new(), &ProgressSink::new())
}

pub fn apply(f: &Fixture, session: &ImportSession, options: ImportOptions) -> Result<ImportResult, ApkgImportError> {
    apkg::apply(f.host.engine(), session, options, None, &CancelToken::new(), &ProgressSink::new())
}

pub fn import(f: &Fixture, package: &Path, options: ImportOptions) -> Result<ImportResult, ApkgImportError> {
    let session = inspect(f, package)?;
    apply(f, &session, options)
}

fn key_of(table: Table, row: &RowImage) -> RowKey {
    use meguri_storage::Cell;
    match (table, &row[..]) {
        (Table::Fields | Table::Templates | Table::Graves, [Cell::Integer(a), Cell::Integer(b), ..]) => {
            RowKey::Pair(*a, *b)
        }
        (_, [Cell::Integer(id), ..]) => RowKey::Id(*id),
        (_, [Cell::Text(text), ..]) => RowKey::Text(text.clone()),
        _ => panic!("a key this dump cannot page by"),
    }
}

/// Every row of every table, as stored.
pub fn dump(f: &Fixture) -> BTreeMap<Table, Vec<RowImage>> {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    let mut out = BTreeMap::new();
    for table in Table::ALL {
        let mut rows: Vec<RowImage> = Vec::new();
        loop {
            let after = rows.last().map(|row| key_of(table, row));
            let page = reader.row_page(table, after.as_ref()).unwrap();
            if page.is_empty() {
                break;
            }
            rows.extend(page);
        }
        out.insert(table, rows);
    }
    out
}

/// The collection directory's entries, without SQLite's own sidecars.
pub fn listing(f: &Fixture) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(f.collection.path().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with("-wal") && !name.ends_with("-shm"))
        .collect();
    names.sort();
    names
}

pub fn media_files(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    let folder = f.collection.path().parent().unwrap().join("collection.media");
    let mut out = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries {
            let entry = entry.unwrap();
            out.insert(entry.file_name().to_string_lossy().into_owned(), std::fs::read(entry.path()).unwrap());
        }
    }
    out
}

pub fn note_ids(f: &Fixture) -> Vec<NoteId> {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    reader.notes_after(None, 100_000).unwrap().iter().map(|note| note.id).collect()
}

pub fn add_note(f: &Fixture, notetype: NotetypeId, fields: &[&str]) -> NoteId {
    meguri_engine::notes::add(
        &f.collection,
        meguri_engine::notes::AddNote {
            notetype_id: notetype,
            fields: fields.iter().map(|field| (*field).to_owned()).collect(),
            tags: vec![],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value
    .id
}

/// A raw edit of one note through an op, as a test's setup.
pub fn edit_note(f: &Fixture, id: NoteId, edit: impl Fn(&mut meguri_model::note::Note) + Send + 'static) {
    f.collection
        .run(
            OpSpec::named("test note edit", OpSource::Maintenance).stamp(meguri_storage::StampMode::CallerSupplied),
            move |ctx| {
                let mut note = ctx.scope().note(id)?.unwrap();
                edit(&mut note);
                ctx.scope().put(&note)?;
                Ok(())
            },
        )
        .unwrap();
}

pub const ALL_OFF: ImportOptions = ImportOptions {
    merge_notetypes: false,
    update_notes: UpdateCondition::IfNewer,
    update_notetypes: UpdateCondition::IfNewer,
    with_scheduling: false,
    with_deck_configs: false,
};

#[test]
fn classification_goes_by_name_then_by_content() {
    let f = Fixture::new(1);
    let package = export(&f, "deck.apkg");
    for (name, class) in [
        ("deck.apkg", PackageClass::Apkg),
        ("Deck.APKG", PackageClass::Apkg),
        ("deck.zip", PackageClass::Apkg),
        ("collection.apkg", PackageClass::Colpkg),
        ("COLLECTION.APKG", PackageClass::Colpkg),
        ("backup-2026-01-01-10.00.00.apkg", PackageClass::Colpkg),
        ("x.colpkg", PackageClass::Colpkg),
        ("deck", PackageClass::Apkg),
    ] {
        assert_eq!(apkg::classify(&package, name), class, "{name}");
    }
    let text = f.dir.path().join("notes.txt");
    std::fs::write(&text, b"plain words").unwrap();
    assert_eq!(apkg::classify(&text, "notes.txt"), PackageClass::Unsupported);
    assert!(matches!(
        apkg::inspect(&text, "notes.txt", &context(&f), &CancelToken::new(), &ProgressSink::new()),
        Err(ApkgImportError::UnsupportedFile)
    ));
}

#[test]
fn new_notes_arrive_with_their_cards_and_undo_restores_every_row() {
    let source = Fixture::new(3);
    source.clock.step_ms(5000);
    let extra = add_note(&source, source.notetype, &["fresh <b>front</b>", "fresh back"]);
    let package = export(&source, "deck.apkg");
    // The target holds the first three notes already (same ids and GUIDs).
    let target = Fixture::new(3);
    target.clock.step_ms(60_000);
    let before = dump(&target);
    let files = listing(&target);
    let session = inspect(&target, &package).unwrap();
    let facts = session.facts();
    assert_eq!((facts.format, facts.notes, facts.cards), (PackageFormat::Latest, 4, 4));
    assert!(!facts.has_scheduling);
    assert_eq!(facts.notetypes, vec!["Normal fixture".to_owned()]);
    assert_eq!(dump(&target), before, "inspect writes nothing to the target");
    let result = apply(&target, &session, ALL_OFF).unwrap();
    assert_eq!(result.undo_step, Some("Import"));
    assert_eq!(result.log.found_notes, 4);
    assert_eq!(result.log.new.len(), 1);
    assert_eq!(result.log.new[0].note, extra.0);
    assert_eq!(result.log.new[0].fields, vec!["fresh front".to_owned(), "fresh back".to_owned()]);
    assert_eq!(result.log.duplicate.len(), 3);
    assert_eq!((result.counts.notes_added, result.counts.cards_added), (1, 1));
    assert!(matches!(apply(&target, &session, ALL_OFF), Err(ApkgImportError::SessionClosed)));
    drop(session);
    assert_eq!(listing(&target), files, "the staging directory is gone");
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let note = reader.note(extra).unwrap().unwrap();
    assert_eq!(note.usn.0, -1);
    let cards = reader.cards_of_note(extra).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!((cards[0].usn.0, cards[0].queue.stored(), cards[0].card_type.stored()), (-1, 0, 0));
    drop(reader);
    let after = dump(&target);
    assert_ne!(after, before);
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&target), before, "undo restores the collection exactly");
    target.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&target), after);
}

#[test]
fn update_notes_follows_the_rule_and_keeps_the_incoming_mod() {
    for (rule, target_mod, updated) in [
        (UpdateCondition::IfNewer, 100, true),
        (UpdateCondition::IfNewer, 200, false),
        (UpdateCondition::IfNewer, 300, false),
        (UpdateCondition::Always, 100, true),
        (UpdateCondition::Always, 200, false),
        (UpdateCondition::Always, 300, true),
        (UpdateCondition::Never, 100, false),
        (UpdateCondition::Never, 300, false),
    ] {
        let source = Fixture::new(1);
        let id = note_ids(&source)[0];
        edit_note(&source, id, |note| {
            note.fields = "incoming front\u{1f}incoming back".into();
            note.mtime.0 = 200;
        });
        let package = export(&source, "deck.apkg");
        let target = Fixture::new(1);
        edit_note(&target, id, move |note| note.mtime.0 = target_mod);
        let stored = target.collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap();
        let result = import(&target, &package, ImportOptions { update_notes: rule, ..ALL_OFF }).unwrap();
        let note = target.collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap();
        if updated {
            assert_eq!(result.log.updated.len(), 1, "{rule:?} {target_mod}");
            assert_eq!((note.fields.as_str(), note.mtime.0, note.usn.0), ("incoming front\u{1f}incoming back", 200, -1));
            assert_eq!(note.sort_field, "incoming front");
        } else {
            assert_eq!(result.log.duplicate.len(), 1, "{rule:?} {target_mod}");
            assert_eq!(note.fields, stored.fields);
            assert_eq!(note.mtime.0, target_mod);
        }
    }
}

#[test]
fn refusals_leave_no_staging_and_change_nothing() {
    let f = Fixture::new(2);
    let good = export(&f, "deck.apkg");
    let before = dump(&f);
    let files = listing(&f);
    let mut archive = ZipArchive::new(std::fs::File::open(&good).unwrap()).unwrap();
    let collection = entry(&mut archive, "collection.anki21b");
    let newer = f.dir.path().join("newer.apkg");
    write_zip(&newer, &[("meta".into(), vec![8, 4]), ("collection.anki21b".into(), collection.clone())]);
    assert!(matches!(inspect(&f, &newer), Err(ApkgImportError::TooNew)));
    let not_zip = f.dir.path().join("words.apkg");
    std::fs::write(&not_zip, b"not an archive at all").unwrap();
    assert!(matches!(inspect(&f, &not_zip), Err(ApkgImportError::Corrupt { .. })));
    let traversal = as_legacy(&good, "traversal.apkg", &[("../x.jpg", b"bytes")], None);
    assert!(matches!(inspect(&f, &traversal), Err(ApkgImportError::Corrupt { .. })));
    let nested = with_media(&good, "nested.apkg", &[("a/b.jpg", b"bytes")]);
    assert!(matches!(inspect(&f, &nested), Err(ApkgImportError::Corrupt { .. })));
    let no_collection = f.dir.path().join("empty.apkg");
    write_zip(&no_collection, &[("meta".into(), vec![8, 3])]);
    assert!(matches!(inspect(&f, &no_collection), Err(ApkgImportError::Corrupt { .. })));
    let garbage = f.dir.path().join("garbage.apkg");
    write_zip(&garbage, &[("meta".into(), vec![8, 3]), ("collection.anki21b".into(), b"not zstd".to_vec())]);
    assert!(matches!(inspect(&f, &garbage), Err(ApkgImportError::Corrupt { .. })));
    assert_eq!(dump(&f), before);
    assert_eq!(listing(&f), files);
    assert!(media_files(&f).is_empty());
}

fn without_map(package: &Path) -> PathBuf {
    let mut archive = ZipArchive::new(std::fs::File::open(package).unwrap()).unwrap();
    let collection = zstd::decode_all(&entry(&mut archive, "collection.anki21b")[..]).unwrap();
    let path = package.with_file_name("bare.apkg");
    write_zip(&path, &[("meta".into(), vec![8, 2]), ("collection.anki21".into(), collection)]);
    path
}

#[test]
fn a_legacy_package_imports_with_its_media_and_a_package_without_a_map_imports_none() {
    let source = Fixture::new(0);
    let note = add_note(&source, source.notetype, &["<img src=\"Sky.png\">", "[sound:rain.mp3]"]);
    let package = export(&source, "deck.apkg");
    let legacy = as_legacy(&package, "legacy.apkg", &[("Sky.png", b"sky"), ("rain.mp3", b"rain"), ("unused.gif", b"x")], None);
    let target = Fixture::new(0);
    let session = inspect(&target, &legacy).unwrap();
    assert_eq!(session.facts().format, PackageFormat::Legacy2);
    assert_eq!((session.facts().media_files, session.facts().media_bytes), (3, 8));
    let result = apply(&target, &session, ALL_OFF).unwrap();
    assert_eq!(result.counts.media_written, 2);
    let files = media_files(&target);
    assert_eq!(files.keys().cloned().collect::<Vec<_>>(), vec!["rain.mp3".to_owned(), "sky.png".to_owned()]);
    assert_eq!(files["sky.png"], b"sky");
    let stored = target.collection.reader(ReaderRole::Query).unwrap().note(note).unwrap().unwrap();
    assert_eq!(stored.fields, "<img src=\"sky.png\">\u{1f}[sound:rain.mp3]");

    let bare = without_map(&package);
    let other = Fixture::new(0);
    let result = import(&other, &bare, ALL_OFF).unwrap();
    assert_eq!(result.counts.notes_added, 1);
    assert!(media_files(&other).is_empty());
}

#[test]
fn media_rules_reuse_rename_skip_and_keep() {
    let source = Fixture::new(0);
    let note = add_note(
        &source,
        source.notetype,
        &["<img src=\"cat.jpg\"> <img src=\"same.png\"> [sound:new&amp;one.mp3]", "back"],
    );
    let package = with_media(
        &export(&source, "deck.apkg"),
        "media.apkg",
        &[
            ("cat.jpg", b"incoming cat"),
            ("same.png", b"identical"),
            ("new&one.mp3", b"sound"),
            ("unused.gif", b"never referenced"),
            ("_font.woff", b"incoming font"),
            ("_extra.css", b"static sheet"),
        ],
    );
    let target = Fixture::new(0);
    let media = meguri_engine::media::MediaService::open(&target.collection, target.clock.clone()).unwrap();
    media.add("cat.jpg", b"target cat").unwrap();
    media.add("same.png", b"identical").unwrap();
    media.write_exact("_font.woff", b"target font").unwrap();
    let result = import(&target, &package, ALL_OFF).unwrap();
    let files = media_files(&target);
    let hash: String = Sha1::digest(b"incoming cat").iter().map(|b| format!("{b:02x}")).collect();
    let renamed = format!("cat-{hash}.jpg");
    assert_eq!(files["cat.jpg"], b"target cat", "no target file is replaced");
    assert_eq!(files[&renamed], b"incoming cat");
    assert_eq!(files["_font.woff"], b"target font");
    assert_eq!(files["_extra.css"], b"static sheet", "a static entry is copied unreferenced");
    assert_eq!(files["new&one.mp3"], b"sound");
    assert!(!files.contains_key("unused.gif"));
    assert_eq!(files.len(), 6);
    let stored = target.collection.reader(ReaderRole::Query).unwrap().note(note).unwrap().unwrap();
    assert_eq!(
        stored.fields,
        format!("<img src=\"{renamed}\"> <img src=\"same.png\"> [sound:new&amp;one.mp3]\u{1f}back")
    );
    assert_eq!((result.counts.media_written, result.counts.media_reused, result.counts.media_skipped), (3, 1, 1));
    // Undo keeps the files (I14.3).
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(media_files(&target).len(), 6);
}

#[test]
fn a_payload_past_its_declared_size_or_with_another_hash_is_corrupt_and_stages_nothing() {
    // One note both sides hold, then the one that brings the payload.
    let source = Fixture::new(1);
    source.clock.step_ms(1000);
    add_note(&source, source.notetype, &["<img src=\"big.png\">", "back"]);
    let exported = export(&source, "deck.apkg");
    let mut archive = ZipArchive::new(std::fs::File::open(&exported).unwrap()).unwrap();
    // A latest map that declares 4 bytes for a payload of 4,000.
    let (map, _) = latest_media(&[("big.png", b"four")]);
    let oversized = exported.with_file_name("oversized.apkg");
    write_zip(
        &oversized,
        &[
            ("meta".into(), entry(&mut archive, "meta")),
            ("collection.anki21b".into(), entry(&mut archive, "collection.anki21b")),
            ("0".into(), zstd::encode_all(&vec![7_u8; 4000][..], 3).unwrap()),
            ("media".into(), map.clone()),
        ],
    );
    // The same size with other bytes: the SHA-1 differs.
    let other = exported.with_file_name("other.apkg");
    write_zip(
        &other,
        &[
            ("meta".into(), entry(&mut archive, "meta")),
            ("collection.anki21b".into(), entry(&mut archive, "collection.anki21b")),
            ("0".into(), zstd::encode_all(&b"five"[..], 3).unwrap()),
            ("media".into(), map),
        ],
    );
    let target = Fixture::new(1);
    let before = dump(&target);
    for package in [&oversized, &other] {
        let session = inspect(&target, package).unwrap();
        assert!(matches!(apply(&target, &session, ALL_OFF), Err(ApkgImportError::Corrupt { .. })));
        drop(session);
        assert_eq!(dump(&target), before);
        assert!(media_files(&target).is_empty());
        let staging = target.collection.path().parent().unwrap().join("media.staging");
        assert!(std::fs::read_dir(&staging).map_or(true, |mut entries| entries.next().is_none()));
    }
}

#[test]
fn cancelling_before_the_write_phase_changes_nothing_and_keeps_the_session() {
    let source = Fixture::new(1);
    source.clock.step_ms(1000);
    add_note(&source, source.notetype, &["<img src=\"a.png\">", "back"]);
    let package = with_media(&export(&source, "deck.apkg"), "media.apkg", &[("a.png", b"pixels")]);
    let target = Fixture::new(1);
    let before = dump(&target);
    let session = inspect(&target, &package).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        apkg::apply(target.host.engine(), &session, ALL_OFF, None, &cancel, &ProgressSink::new()),
        Err(ApkgImportError::Cancelled)
    ));
    assert_eq!(dump(&target), before);
    assert!(media_files(&target).is_empty());
    // The session is still open: the next apply imports.
    let result = apply(&target, &session, ALL_OFF).unwrap();
    assert_eq!(result.counts.notes_added, 1);
    assert_eq!(media_files(&target)["a.png"], b"pixels");
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert!(matches!(
        apkg::inspect(&package, "media.apkg", &context(&target), &cancelled, &ProgressSink::new()),
        Err(ApkgImportError::Cancelled)
    ));
}

struct NoSpace;
impl SpaceProvider for NoSpace {
    fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
        Ok(1024)
    }
}

#[test]
fn low_space_refuses_before_anything_is_copied() {
    let f = Fixture::new(1);
    let package = export(&f, "deck.apkg");
    let files = listing(&f);
    let mut context = context(&f);
    context.space = Arc::new(NoSpace);
    assert!(matches!(
        apkg::inspect(&package, "deck.apkg", &context, &CancelToken::new(), &ProgressSink::new()),
        Err(ApkgImportError::InsufficientSpace { available: 1024, .. })
    ));
    assert_eq!(listing(&f), files);
}

#[test]
fn the_options_are_stored_with_the_import_and_read_back() {
    let source = Fixture::new(1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(1);
    assert_eq!(apkg::read_options(&target.collection).unwrap(), ImportOptions::default());
    let chosen = ImportOptions {
        merge_notetypes: false,
        update_notes: UpdateCondition::Never,
        update_notetypes: UpdateCondition::Always,
        with_scheduling: true,
        with_deck_configs: true,
    };
    import(&target, &package, chosen).unwrap();
    assert_eq!(apkg::read_options(&target.collection).unwrap(), chosen);
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let stored = reader.config_entry("updateNotes").unwrap().unwrap();
    assert_eq!(stored.entry.value, b"\"Never\"");
    assert_eq!(stored.entry.mtime.0, target.clock.now_ms() / 1000);
    assert_eq!(reader.config_entry("withScheduling").unwrap().unwrap().entry.value, b"true");
    drop(reader);
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(apkg::read_options(&target.collection).unwrap(), ImportOptions::default());
}
