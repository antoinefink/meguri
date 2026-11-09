//! Actual Q4.2 generator/storage/codec readback, separate from Swift delivery
//! admission and protocol fakes. No expected row is used as observed output.
#![cfg(feature = "test-hooks")]
use meguri_ffi::test_hooks::test_cloud_original_role;
use meguri_storage::{Initializers, ReadRows, ReaderFactory, ReaderRole, RowKey, Table, raw::PlainDatabase};
use meguri_sync::cloud::{
    entity_codec as c,
    row_payload::{self, View},
};
use meguri_testkit::generate::cloud_small;
use sha2::{Digest, Sha256};
use std::path::Path;

fn generate(root: &Path) {
    cloud_small::generate(&cloud_small::Request::new(7), root).unwrap();
}
fn bytes(root: &Path) -> (Vec<u8>, Option<Vec<u8>>) {
    (
        std::fs::read(root.join("collection.anki2")).unwrap(),
        std::fs::read(root.join("collection.anki2-wal")).ok(),
    )
}
fn expected(root: &Path) -> (Vec<u8>, Vec<u8>, u32) {
    let reader = ReaderFactory::recovery(&root.join("collection.anki2"), &Initializers::default())
        .open(ReaderRole::Query)
        .unwrap();
    let rows = reader.begin_snapshot().unwrap();
    let notes = rows.notes_after(None, 100).unwrap();
    assert_eq!(notes.len(), 20);
    let note = notes.iter().min_by_key(|n| n.id).unwrap();
    assert_eq!(rows.notes_after(None, 1).unwrap()[0].id, note.id);
    let nt = rows.notetype(note.notetype_id).unwrap().unwrap();
    let q = nt.fields.iter().find(|f| f.name == "Q").unwrap().ord;
    let cards = rows.cards_of_note(note.id).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].note_id, note.id);
    assert_eq!(cards[0].deck_id.0, 1);
    let mut decks = rows.deck_rows().unwrap();
    decks.sort_by_key(|d| d.id);
    assert_eq!(decks.len(), 2);
    assert!(decks.iter().any(|d| d.id == cards[0].deck_id));
    let view = row_payload::ReadView(&rows);
    let layout = row_payload::notetype(
        &view,
        &view.row(Table::Notetypes, &RowKey::Id(nt.id.0)).unwrap().unwrap(),
        1,
    )
    .unwrap();
    let mut payloads = vec![];
    for (kind, id) in [(c::Kind::Note, note.id.0), (c::Kind::Notetype, nt.id.0)]
        .into_iter()
        .chain(cards.iter().map(|c| (c::Kind::Card, c.id.0)))
        .chain(decks.iter().map(|d| (c::Kind::Deck, d.id.0)))
    {
        let raw = view
            .row(row_payload::table(kind).unwrap(), &RowKey::Id(id))
            .unwrap()
            .unwrap();
        payloads.push(row_payload::project(&view, kind, &raw, |_| Ok(layout.clone())).unwrap());
    }
    let mut hash = Sha256::new();
    for p in &payloads {
        hash.update([p.kind() as u8]);
        hash.update(p.payload_digest().unwrap());
    }
    (
        payloads[0].payload_digest().unwrap().to_vec(),
        hash.finalize().to_vec(),
        q,
    )
}
#[test]
fn real_small_main_and_optional_wal_return_exact_canonical_original_role_without_writing_source() {
    for wal in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        generate(temp.path());
        let db = PlainDatabase::open(&temp.path().join("collection.anki2")).unwrap();
        if wal {
            db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE notes SET tags=' actual-wal-tag ' WHERE id=(SELECT min(id) FROM notes)").unwrap();
        }
        let before = bytes(temp.path());
        if wal {
            assert!(!before.1.as_ref().unwrap().is_empty());
        }
        let role = test_cloud_original_role(before.0.clone(), before.1.clone()).unwrap();
        let facts = role.facts().unwrap();
        let (note, baseline, q) = expected(temp.path());
        assert_eq!(facts.note_digest, note);
        assert_eq!(facts.baseline_digest, baseline);
        assert_eq!(facts.q_ordinal, q);
        assert_eq!(
            (
                facts.field_count,
                facts.template_count,
                facts.card_count,
                facts.deck_count
            ),
            (2, 1, 1, 2)
        );
        assert_eq!(facts.source_main_sha256, Sha256::digest(&before.0).to_vec());
        assert_eq!(
            facts.source_wal_sha256,
            before.1.as_ref().map(|b| Sha256::digest(b).to_vec())
        );
        assert_eq!(bytes(temp.path()), before);
        assert_eq!(format!("{role:?}"), "CloudOriginalRole");
        drop(db);
    }
}
#[test]
fn changing_live_collection_does_not_change_retained_original_witness() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    let live = temp.path().join("live");
    generate(&original);
    std::fs::create_dir(&live).unwrap();
    let original_bytes = bytes(&original);
    let role = test_cloud_original_role(original_bytes.0.clone(), original_bytes.1.clone()).unwrap();
    let baseline = role.facts().unwrap();
    std::fs::write(live.join("collection.anki2"), &original_bytes.0).unwrap();
    let db = PlainDatabase::open(&live.join("collection.anki2")).unwrap();
    db.execute_batch("UPDATE notes SET tags=' edited-live ' WHERE id=(SELECT min(id) FROM notes)")
        .unwrap();
    let current = bytes(&live);
    assert_ne!(
        test_cloud_original_role(current.0, current.1)
            .unwrap()
            .facts()
            .unwrap()
            .note_digest,
        baseline.note_digest
    );
    assert_eq!(role.facts().unwrap(), baseline);
    assert_eq!(bytes(&original), original_bytes);
}
#[test]
fn missing_invalid_ambiguous_relations_empty_and_corrupt_images_fail_closed() {
    for sql in [
        "DELETE FROM notes; DELETE FROM cards; DELETE FROM revlog",
        "UPDATE notes SET mid=-1 WHERE id=(SELECT min(id) FROM notes)",
        "UPDATE notes SET flds='one field' WHERE id=(SELECT min(id) FROM notes)",
        "UPDATE notes SET guid=(SELECT guid FROM notes ORDER BY id LIMIT 1) WHERE id=(SELECT max(id) FROM notes)",
        "DELETE FROM cards WHERE nid=(SELECT min(id) FROM notes)",
        "UPDATE cards SET did=-1 WHERE nid=(SELECT min(id) FROM notes)",
        "UPDATE cards SET ord=99 WHERE nid=(SELECT min(id) FROM notes)",
        "DELETE FROM fields WHERE name='Q'",
        "DELETE FROM templates",
        "UPDATE decks SET name='invalid-child' WHERE id<>1",
        "DELETE FROM decks WHERE id<>1",
    ] {
        let temp = tempfile::tempdir().unwrap();
        generate(temp.path());
        let db = PlainDatabase::open(&temp.path().join("collection.anki2")).unwrap();
        db.execute_batch(sql).unwrap();
        let input = bytes(temp.path());
        assert!(test_cloud_original_role(input.0, input.1).is_err());
    }
    assert!(test_cloud_original_role(vec![], None).is_err());
    assert!(test_cloud_original_role(vec![0; 1024], None).is_err());
    assert!(test_cloud_original_role(vec![0; 64 * 1024 * 1024 + 1], None).is_err());
    let temp = tempfile::tempdir().unwrap();
    generate(temp.path());
    let (main, _) = bytes(temp.path());
    assert!(test_cloud_original_role(main[..main.len() / 2].to_vec(), None).is_err());
    assert!(test_cloud_original_role(main.clone(), Some(vec![0; 64 * 1024 * 1024 + 1])).is_err());
    assert!(test_cloud_original_role(main, Some(vec![0; 80])).is_err());
}
#[test]
fn damaged_wal_cannot_be_silently_ignored() {
    let temp = tempfile::tempdir().unwrap();
    generate(temp.path());
    let db = PlainDatabase::open(&temp.path().join("collection.anki2")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE notes SET tags=' WAL observation '")
        .unwrap();
    let (main, wal) = bytes(temp.path());
    let wal = wal.unwrap();
    for at in [0, 16, 24, 32, wal.len() - 1] {
        let mut corrupt = wal.clone();
        corrupt[at] ^= 1;
        assert!(test_cloud_original_role(main.clone(), Some(corrupt)).is_err());
    }
    assert!(test_cloud_original_role(main, Some(wal[..wal.len() - 1].to_vec())).is_err());
}

#[test]
fn reused_wal_keeps_exact_obsolete_tail_and_reads_the_new_committed_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    generate(temp.path());
    let db = PlainDatabase::open(&temp.path().join("collection.anki2")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE notes SET data=hex(zeroblob(8000))")
        .unwrap();
    let older = bytes(temp.path()).1.unwrap();
    let checkpoint = db.rows("PRAGMA wal_checkpoint(RESTART)", &[]).unwrap();
    assert_eq!(checkpoint[0][0], meguri_storage::raw::RawValue::Integer(0));
    db.execute_batch("UPDATE notes SET tags=' reset-wal ' WHERE id=(SELECT min(id) FROM notes)")
        .unwrap();
    let before = bytes(temp.path());
    let wal = before.1.as_ref().unwrap();
    assert_eq!(wal.len(), older.len());
    assert_ne!(&wal[16..24], &older[16..24]);
    let page = u32::from_be_bytes(wal[8..12].try_into().unwrap()) as usize;
    let frames: Vec<_> = (32..wal.len()).step_by(page + 24).collect();
    let tail = frames
        .iter()
        .position(|at| wal[*at + 8..*at + 16] != wal[16..24])
        .unwrap();
    assert!(tail > 0 && tail < frames.len());
    assert!(frames[..tail].iter().any(|at| wal[*at + 4..*at + 8] != [0; 4]));
    for at in &frames[tail..] {
        assert_eq!(&wal[*at + 8..*at + 16], &older[16..24]);
        assert_eq!(&wal[*at..*at + page + 24], &older[*at..*at + page + 24]);
    }
    let role = test_cloud_original_role(before.0.clone(), before.1.clone()).unwrap();
    let facts = role.facts().unwrap();
    let (note, baseline, q) = expected(temp.path());
    assert_eq!(facts.note_digest, note);
    assert_eq!(facts.baseline_digest, baseline);
    assert_eq!(facts.q_ordinal, q);
    assert_eq!(facts.source_main_sha256, Sha256::digest(&before.0).to_vec());
    assert_eq!(facts.source_wal_sha256, Some(Sha256::digest(wal).to_vec()));
    let mut corrupt = wal.clone();
    corrupt[frames[tail - 1] + 24] ^= 1;
    assert!(test_cloud_original_role(before.0.clone(), Some(corrupt)).is_err());
    let mut resumed = wal.clone();
    let obsolete = frames[tail];
    resumed[obsolete + 8..obsolete + 16].copy_from_slice(&wal[16..24]);
    assert!(test_cloud_original_role(before.0.clone(), Some(resumed)).is_err());
    assert_eq!(bytes(temp.path()), before);
    drop(db);
}

#[test]
fn real_owned_host_maps_captured_identity_accepts_only_fixed_q_edit_and_keeps_original_private() {
    use meguri_ffi::{
        CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
        collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
        notes::{self, UpdateNoteRequest},
    };
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };
    struct Clock;
    impl HostClock for Clock {
        fn now_ms(&self) -> i64 {
            cloud_small::NOW_MS
        }
        fn offset_from_utc(&self, _: i64) -> i32 {
            0
        }
        fn zone_id(&self) -> String {
            "UTC".into()
        }
    }
    struct Space;
    impl DiskSpaceProvider for Space {
        fn available_bytes(&self, _: String) -> DiskCapacity {
            DiskCapacity::Available { bytes: u64::MAX }
        }
    }
    struct Listener(mpsc::Sender<CoreEvent>);
    impl CoreEventListener for Listener {
        fn on_event(&self, event: CoreEvent) {
            let _ = self.0.send(event);
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    let live = temp.path().join("live");
    generate(&original);
    let original_bytes = bytes(&original);
    let role = test_cloud_original_role(original_bytes.0.clone(), original_bytes.1.clone()).unwrap();
    let baseline = role.facts().unwrap();
    std::fs::create_dir(&live).unwrap();
    std::fs::write(live.join("collection.anki2"), &original_bytes.0).unwrap();
    std::fs::create_dir(temp.path().join("backups")).unwrap();
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let job = collection::start_open(
        CollectionLocation {
            collection_directory: live.to_string_lossy().into_owned(),
            backups_directory: temp.path().join("backups").to_string_lossy().into_owned(),
            app_group_snapshot: temp.path().join("widget.snapshot").to_string_lossy().into_owned(),
            manual_transfer_directory: temp.path().join("manual").to_string_lossy().into_owned(),
        },
        Arc::new(Clock),
        Arc::new(Space),
        7,
    )
    .unwrap();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id, outcome } = event.kind
            && id == job.id
        {
            assert_eq!(outcome, JobOutcome::Succeeded);
            break;
        }
    }
    let collection = meguri_ffi::test_hooks::collection_for_testing().unwrap();
    let first = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        reader.notes_after(None, 1).unwrap().remove(0)
    };
    let owned = role.validate_owned().unwrap();
    assert_eq!(owned.generation, collection.generation().0);
    assert_eq!(owned.note_digest, baseline.note_digest);
    assert!(!owned.q_changed);
    let mut extra = first.clone();
    extra.id = meguri_model::ids::NoteId(1);
    extra.guid = "unrelated-live-row".into();
    collection
        .run(
            meguri_engine::OpSpec::named("lower current row", meguri_engine::OpSource::Maintenance),
            move |ctx| {
                ctx.scope().put(&extra)?;
                Ok(())
            },
        )
        .unwrap();
    assert!(!role.validate_owned().unwrap().q_changed);
    let mut fields: Vec<String> = first.field_values().map(str::to_owned).collect();
    fields[baseline.q_ordinal as usize] = format!("{}-sync-now-n1-Q", cloud_small::Request::new(7).marker());
    let tags: Vec<String> = meguri_engine::tags::split_text(&first.tags)
        .map(str::to_owned)
        .collect();
    notes::update_note(UpdateNoteRequest {
        id: first.id.0,
        fields: fields.clone(),
        tags: tags.clone(),
    })
    .unwrap();
    meguri_ffi::suspend::flush_for_background().unwrap();
    let edited = role.validate_owned().unwrap();
    assert!(edited.q_changed);
    assert_ne!(edited.note_digest, baseline.note_digest);
    assert_eq!(role.facts().unwrap(), baseline);
    let other = 1 - baseline.q_ordinal as usize;
    let prior = fields[other].clone();
    fields[other].push_str(&first.guid);
    assert_ne!(fields[other], prior);
    notes::update_note(UpdateNoteRequest {
        id: first.id.0,
        fields,
        tags,
    })
    .unwrap();
    assert!(role.validate_owned().is_err());
    meguri_ffi::lifecycle::close_collection().unwrap();
    assert!(role.validate_owned().is_err());
    assert_eq!(bytes(&original), original_bytes);
}

#[test]
fn q_ordinal_is_read_from_actual_original_layout() {
    let temp = tempfile::tempdir().unwrap();
    generate(temp.path());
    let db = PlainDatabase::open(&temp.path().join("collection.anki2")).unwrap();
    db.execute_batch("BEGIN; UPDATE fields SET name='temporary-role' WHERE name='Q'; UPDATE fields SET name='Q' WHERE name='A'; UPDATE fields SET name='A' WHERE name='temporary-role'; COMMIT").unwrap();
    let input = bytes(temp.path());
    let facts = test_cloud_original_role(input.0, input.1).unwrap().facts().unwrap();
    let (note, baseline, q) = expected(temp.path());
    assert_eq!(q, 1);
    assert_eq!(facts.q_ordinal, q);
    assert_eq!(facts.note_digest, note);
    assert_eq!(facts.baseline_digest, baseline);
}
