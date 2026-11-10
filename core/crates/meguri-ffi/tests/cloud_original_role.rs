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

// Compare every stored cell and identity across every page, including rows
// outside the edited note. No typed model equality or lossy projection.
fn stored_rows(rows: &impl ReadRows, table: Table) -> Vec<meguri_storage::RowImage> {
    let mut result = Vec::new();
    let mut after = None;
    loop {
        let page = rows.row_page(table, after.as_ref()).unwrap();
        if page.is_empty() {
            return result;
        }
        let id = page.last().unwrap()[0].as_i64().unwrap();
        if let Some(RowKey::Id(previous)) = after {
            assert!(id > previous);
        }
        after = Some(RowKey::Id(id));
        result.extend(page);
    }
}

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
fn canonical_original_note_digest_includes_every_represented_metadata_cell() {
    for cell in [
        "mod=mod+1",
        "csum=csum+1",
        "sfld='different stored sort'",
        "tags=' different-tag '",
        "flags=flags+1",
        "data='different opaque data'",
    ] {
        let temp = tempfile::tempdir().unwrap();
        generate(temp.path());
        let before = bytes(temp.path());
        let original = test_cloud_original_role(before.0, before.1).unwrap();
        let db = PlainDatabase::open(&temp.path().join("collection.anki2")).unwrap();
        db.execute_batch(&format!("UPDATE notes SET {cell} WHERE id=(SELECT min(id) FROM notes)"))
            .unwrap();
        let after = bytes(temp.path());
        let observed = test_cloud_original_role(after.0, after.1).unwrap();
        assert_ne!(
            observed.facts().unwrap().note_digest,
            original.facts().unwrap().note_digest
        );
        assert_ne!(
            observed.facts().unwrap().baseline_digest,
            original.facts().unwrap().baseline_digest
        );
    }
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
fn fixed_edit_uses_real_owned_host_and_normal_update_with_committed_readback_and_refusals() {
    use meguri_ffi::{
        CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
        collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
        notes::{self, UpdateNoteRequest},
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicI64, Ordering},
            mpsc,
        },
        time::Duration,
    };
    struct Clock {
        next: AtomicI64,
        step: i64,
    }
    impl HostClock for Clock {
        fn now_ms(&self) -> i64 {
            self.next.fetch_add(self.step, Ordering::SeqCst)
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
    // One process Host and listener at a time; all scenarios exercise the
    // exported capability's actual registered job and ordinary engine updater.
    for (mode, swapped) in [
        ("success", false),
        ("success", true),
        ("advancing", false),
        ("second-boundary", true),
        ("unrelated", true),
        ("wrong-mtime", true),
        ("wrong-usn", true),
        ("changed", true),
        ("failed", true),
        ("cancelled", true),
        ("matching", true),
        ("stale", true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(Clock {
            next: AtomicI64::new(cloud_small::NOW_MS + 60_000),
            step: match mode {
                "advancing" => 1,
                "second-boundary" => 1001,
                _ => 0,
            },
        });
        let original = temp.path().join("original");
        let live = temp.path().join("live");
        generate(&original);
        {
            let db = PlainDatabase::open(&original.join("collection.anki2")).unwrap();
            if swapped {
                db.execute_batch("BEGIN; UPDATE fields SET name='temporary-role' WHERE name='Q'; UPDATE fields SET name='Q' WHERE name='A'; UPDATE fields SET name='A' WHERE name='temporary-role'; COMMIT").unwrap();
            }
            db.execute_batch("UPDATE notes SET tags=' retained-tag ',flags=19,data='retained opaque data' WHERE id=(SELECT min(id) FROM notes)").unwrap();
        }
        let original_bytes = bytes(&original);
        let role = test_cloud_original_role(original_bytes.0.clone(), original_bytes.1.clone()).unwrap();
        let baseline = role.facts().unwrap();
        assert_eq!(baseline.q_ordinal, u32::from(swapped));
        std::fs::create_dir(&live).unwrap();
        std::fs::write(live.join("collection.anki2"), &original_bytes.0).unwrap();
        std::fs::create_dir(temp.path().join("backups")).unwrap();
        let (tx, rx) = mpsc::channel();
        meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
        let wait = |job_id| loop {
            let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
            if let CoreEventKind::JobFinished { id, outcome } = event.kind
                && id == job_id
            {
                break outcome;
            }
        };
        let location = CollectionLocation {
            collection_directory: live.to_string_lossy().into_owned(),
            backups_directory: temp.path().join("backups").to_string_lossy().into_owned(),
            app_group_snapshot: temp.path().join("widget.snapshot").to_string_lossy().into_owned(),
            manual_transfer_directory: temp.path().join("manual").to_string_lossy().into_owned(),
        };
        let open = || collection::start_open(location.clone(), clock.clone(), Arc::new(Space), 7).unwrap();
        assert_eq!(wait(open().id), JobOutcome::Succeeded);
        let collection = meguri_ffi::test_hooks::collection_for_testing().unwrap();
        let (first, card_rows, deck_rows) = {
            let reader = collection.reader(ReaderRole::Query).unwrap();
            let first = reader.notes_after(None, 1).unwrap().remove(0);
            (
                first,
                stored_rows(&*reader, Table::Cards),
                stored_rows(&*reader, Table::Decks),
            )
        };
        let owned = role.validate_owned().unwrap();
        assert_eq!(owned.generation, collection.generation().0);
        assert_eq!(owned.note_digest, baseline.note_digest);
        assert!(!owned.q_changed);
        let mut extra = first.clone();
        extra.id = meguri_model::ids::NoteId(1);
        extra.guid = "unrelated-live-row".into();
        let lower = extra.clone();
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
        if mode == "stale" {
            meguri_ffi::lifecycle::close_collection().unwrap();
            assert_eq!(wait(open().id), JobOutcome::Succeeded);
            assert!(!role.validate_owned().unwrap().q_changed);
            assert!(
                role.observe().is_err(),
                "successor Host cannot regain original observation authority"
            );
            assert!(
                role.clone().start_fixed_edit().is_err(),
                "validation cannot rebind edit authority to a successor Host"
            );
            let reopened = meguri_ffi::test_hooks::collection_for_testing().unwrap();
            assert_eq!(
                reopened.reader(ReaderRole::Query).unwrap().note(first.id).unwrap(),
                Some(first)
            );
            meguri_ffi::lifecycle::close_collection().unwrap();
            assert_eq!(bytes(&original), original_bytes);
            continue;
        }
        let mut fields: Vec<String> = first.field_values().map(str::to_owned).collect();
        let tags: Vec<String> = meguri_engine::tags::split_text(&first.tags)
            .map(str::to_owned)
            .collect();
        if mode == "changed" || mode == "matching" {
            let ordinal = if mode == "changed" {
                1 - baseline.q_ordinal as usize
            } else {
                baseline.q_ordinal as usize
            };
            fields[ordinal] = if mode == "changed" {
                "concurrent other field".into()
            } else {
                format!("{}-sync-now-n1-Q", cloud_small::Request::new(7).marker())
            };
            notes::update_note(UpdateNoteRequest {
                id: first.id.0,
                fields: fields.clone(),
                tags: tags.clone(),
            })
            .unwrap();
        }
        let before = collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(first.id)
            .unwrap()
            .unwrap();
        let before_seq = collection.last_seq();
        let before_col = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
        // Hold the actual writer so cancellation happens before the edit's
        // admitted mutation, without sleeps or a custom alternate updater.
        let mut barrier = None;
        let mut release = None;
        if mode == "cancelled" {
            let (entered, reached) = mpsc::channel();
            let (tx, rx) = mpsc::channel();
            barrier = Some(collection.submit(
                meguri_engine::OpSpec::named("fixed edit cancellation barrier", meguri_engine::OpSource::Maintenance),
                move |_| {
                    entered.send(()).unwrap();
                    rx.recv().unwrap();
                    Ok(())
                },
            ));
            reached.recv_timeout(Duration::from_secs(20)).unwrap();
            release = Some(tx);
        }
        if mode == "failed" {
            meguri_model::faultpoint::arm(
                "storage.commit.before",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
        }
        // Pause only after the real commit, before the job receives its
        // OpResult. Capture the actual operation's stamps and queue a later
        // interactive op before the job's background WAL flush/readback.
        let mut committed_barrier = None;
        let mut continue_writer = None;
        if matches!(
            mode,
            "advancing" | "second-boundary" | "unrelated" | "wrong-mtime" | "wrong-usn"
        ) {
            let (entered, reached) = mpsc::channel();
            let (tx, rx) = mpsc::channel();
            collection.after_next_commit(move |seq| {
                entered.send(seq).unwrap();
                let _ = rx.recv();
            });
            committed_barrier = Some(reached);
            continue_writer = Some(tx);
        }
        let edit = role.clone().start_fixed_edit().unwrap();
        let mut operation_image = None;
        let mut later = None;
        if let Some(reached) = committed_barrier {
            let seq = reached.recv_timeout(Duration::from_secs(20)).unwrap();
            let reader = collection.reader(ReaderRole::Query).unwrap();
            let snapshot = reader.snapshot().unwrap();
            assert_eq!(snapshot.seq(), seq);
            operation_image = Some((
                seq,
                snapshot.note(first.id).unwrap().unwrap(),
                snapshot.col().unwrap().modified,
            ));
            drop(snapshot);
            drop(reader);
            if matches!(mode, "unrelated" | "wrong-mtime" | "wrong-usn") {
                clock.next.fetch_add(2000, Ordering::SeqCst);
                let target = if mode == "unrelated" { lower.id } else { first.id };
                let stamp = if mode == "unrelated" {
                    meguri_storage::StampMode::Local
                } else {
                    meguri_storage::StampMode::CallerSupplied
                };
                later = Some(
                    collection.submit(
                        meguri_engine::OpSpec::named("post fixed-edit mutation", meguri_engine::OpSource::LocalApi)
                            .stamp(stamp),
                        move |ctx| {
                            let mut note = ctx.scope().note(target)?.unwrap();
                            match mode {
                                "unrelated" => note.data = "later unrelated data".into(),
                                "wrong-mtime" => note.mtime.0 += 1,
                                "wrong-usn" => note.usn.0 = 77,
                                _ => unreachable!(),
                            }
                            ctx.scope().put(&note)?;
                            Ok(())
                        },
                    ),
                );
            }
            continue_writer.take().unwrap().send(()).unwrap();
        }
        let later = later.map(|pending| pending.wait().unwrap());
        if let Some(release) = release {
            edit.handle.cancel.cancel().unwrap();
            release.send(()).unwrap();
            barrier.take().unwrap().wait().unwrap();
        }
        let outcome = wait(edit.handle.id);
        if mode == "failed" {
            meguri_model::faultpoint::disarm("storage.commit.before");
        }
        let result = edit.result.result();
        if matches!(mode, "success" | "advancing" | "second-boundary" | "unrelated") {
            assert_eq!(outcome, JobOutcome::Succeeded);
            let committed = result.unwrap().unwrap();
            assert_eq!(committed.generation, collection.generation().0);
            assert!(committed.seq > before_seq.0);
            assert_eq!(committed.source, meguri_ffi::changes::OpSource::UserUndoable);
            assert_eq!(
                committed.undo_name.as_deref(),
                Some(meguri_engine::undo::names::UPDATE_NOTE)
            );
            assert_eq!(committed.updated_notes, 1);
            assert_eq!(committed.local_revision, None);
            let reader = collection.reader(ReaderRole::Query).unwrap();
            let snapshot = reader.snapshot().unwrap();
            let after = snapshot.note(first.id).unwrap().unwrap();
            let mut expected = first.clone();
            fields[baseline.q_ordinal as usize] = format!("{}-sync-now-n1-Q", cloud_small::Request::new(7).marker());
            expected.fields = fields.join("\u{1f}");
            expected.sort_field = meguri_text::checksum::sort_text_of(&fields, 0).into_owned();
            expected.checksum = meguri_text::checksum::note_checksum(&fields[0]);
            expected.mtime = operation_image.as_ref().map_or(
                meguri_model::time::EpochSecs((cloud_small::NOW_MS + 60_000) / 1000),
                |(_, note, _)| note.mtime,
            );
            expected.usn = meguri_model::usn::Usn(-1);
            assert_eq!(
                after, expected,
                "every saved field, GUID, type, tag, flag/data and derived stamp"
            );
            if let Some((seq, stamped_note, col_mod)) = &operation_image {
                assert_eq!(seq.0, committed.seq);
                assert_eq!(
                    after, *stamped_note,
                    "readback keeps the actual operation's exact metadata"
                );
                if mode == "second-boundary" {
                    assert!(
                        col_mod.0 >= stamped_note.mtime.0 * 1000 + 1001,
                        "the advancing clock crosses a second between note and commit samples"
                    );
                }
                if let Some(later) = &later {
                    assert!(later.seq.0 > committed.seq);
                    assert_ne!(later.col_mod, Some(*col_mod));
                    assert_eq!(snapshot.col().unwrap().modified, later.col_mod.unwrap());
                } else {
                    assert_eq!(snapshot.col().unwrap().modified, *col_mod);
                }
            } else {
                // Earlier local work set these exact bytes already: the fixed
                // edit must succeed even without a collection journal delta.
                assert_eq!(snapshot.col().unwrap().modified.0, cloud_small::NOW_MS + 60_000);
            }
            assert_eq!(stored_rows(&*snapshot, Table::Cards), card_rows);
            assert_eq!(stored_rows(&*snapshot, Table::Decks), deck_rows);
            // Maintenance stamped the extra row, so compare the original role
            // fields and identity rather than assuming its fixture stamps.
            let current_lower = snapshot.note(lower.id).unwrap().unwrap();
            assert_eq!(current_lower.fields, lower.fields);
            assert_eq!(current_lower.guid, lower.guid);
            let through = later.as_ref().map_or(committed.seq, |r| r.seq.0);
            assert_eq!(snapshot.seq().0, through);
            let view = row_payload::ReadView(&*snapshot);
            let layout = row_payload::notetype(
                &view,
                &view
                    .row(Table::Notetypes, &RowKey::Id(after.notetype_id.0))
                    .unwrap()
                    .unwrap(),
                1,
            )
            .unwrap();
            let raw = view.row(Table::Notes, &RowKey::Id(after.id.0)).unwrap().unwrap();
            let observed = row_payload::project(&view, c::Kind::Note, &raw, |_| Ok(layout.clone())).unwrap();
            assert_eq!(committed.note_digest, observed.payload_digest().unwrap().to_vec());
            drop(snapshot);
            drop(reader);
            let edited = role.validate_owned().unwrap();
            assert!(edited.q_changed);
            assert_eq!(edited.note_digest, committed.note_digest);
            assert_ne!(edited.note_digest, baseline.note_digest);
            assert_eq!(role.facts().unwrap(), baseline);
            assert!(role.clone().start_fixed_edit().is_err());
            assert_eq!(collection.last_seq().0, through);
            meguri_ffi::lifecycle::close_collection().unwrap();
            assert_eq!(wait(open().id), JobOutcome::Succeeded);
            let reopened = meguri_ffi::test_hooks::collection_for_testing().unwrap();
            assert_eq!(
                reopened.reader(ReaderRole::Query).unwrap().note(first.id).unwrap(),
                Some(after)
            );
            assert_eq!(role.validate_owned().unwrap().note_digest, committed.note_digest);
            assert!(
                role.clone().start_fixed_edit().is_err(),
                "old owner cannot mutate reopened Host"
            );
        } else if matches!(mode, "wrong-mtime" | "wrong-usn") {
            assert_ne!(outcome, JobOutcome::Succeeded);
            let message = format!("{:?}", result.unwrap_err());
            let category = if mode == "wrong-mtime" {
                "postcommit.metadata.note_afterimage"
            } else {
                "postcommit.metadata.usn"
            };
            assert!(message.contains(category), "{message}");
            let (seq, mut expected, _) = operation_image.unwrap();
            if mode == "wrong-mtime" {
                expected.mtime.0 += 1;
            } else {
                expected.usn.0 = 77;
            }
            assert_eq!(
                collection.reader(ReaderRole::Query).unwrap().note(first.id).unwrap(),
                Some(expected)
            );
            assert!(later.unwrap().seq > seq);
            assert!(
                role.clone().start_fixed_edit().is_err(),
                "a refused proof cannot repeat its committed edit"
            );
        } else {
            assert_ne!(outcome, JobOutcome::Succeeded);
            assert!(
                result.is_err(),
                "failed/cancelled/preexisting content never yields committed facts"
            );
            assert_eq!(
                collection.reader(ReaderRole::Query).unwrap().note(first.id).unwrap(),
                Some(before)
            );
            assert_eq!(collection.reader(ReaderRole::Query).unwrap().col().unwrap(), before_col);
            assert!(role.clone().start_fixed_edit().is_err());
        }
        meguri_ffi::lifecycle::close_collection().unwrap();
        assert!(role.validate_owned().is_err());
        assert!(
            role.observe().is_err(),
            "a closed Host supplies no current ACK/apply observation"
        );
        assert_eq!(bytes(&original), original_bytes);
    }
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

// A real process boundary for the explicitly unsupported committed-before-
// sufficient-proof window. It must not obtain an edit witness from equal text,
// the reset engine seq, undo history or a second execution.
#[test]
fn original_edit_committed_without_bound_original_witness_fails_after_process_exit_without_replay() {
    let root = tempfile::tempdir().unwrap();
    generate(&root.path().join("original"));
    std::fs::create_dir(root.path().join("live")).unwrap();
    std::fs::copy(
        root.path().join("original/collection.anki2"),
        root.path().join("live/collection.anki2"),
    )
    .unwrap();
    let run = |stage: &str| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "original_edit_reopen_process_entry", "--nocapture"])
            .env("MEGURI_ORIGINAL_EDIT_PROCESS_ROOT", root.path())
            .env("MEGURI_ORIGINAL_EDIT_PROCESS_STAGE", stage)
            .output()
            .unwrap()
    };
    let committed = run("commit");
    assert!(committed.status.success());
    assert!(root.path().join("original-observations.json").is_file());
    let before = expected(&root.path().join("live")).0;
    assert_ne!(before, expected(&root.path().join("original")).0);
    let recovered = run("refuse");
    assert!(recovered.status.success());
    assert_eq!(expected(&root.path().join("live")).0, before);
}
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
enum OriginalEditCheckpoint {
    ConfigurePlatform,
    StartOpen,
    WaitOpen,
    PrepareOriginalRole,
    PrepareRuntime,
    ValidateOwned,
    StartEdit,
    WaitEdit,
    ReadEditResult,
    SyncOriginalObservations,
    ReadyWriter,
    ReadOriginalObservations,
    HistoricalRead,
    RepeatedHistoricalRead,
    CompareRowsAndSeq,
    CloseReader,
    SyncCheckedMarker,
    ReadyReader,
}
impl OriginalEditCheckpoint {
    const ALL: [Self; 18] = [
        Self::ConfigurePlatform,
        Self::StartOpen,
        Self::WaitOpen,
        Self::PrepareOriginalRole,
        Self::PrepareRuntime,
        Self::ValidateOwned,
        Self::StartEdit,
        Self::WaitEdit,
        Self::ReadEditResult,
        Self::SyncOriginalObservations,
        Self::ReadyWriter,
        Self::ReadOriginalObservations,
        Self::HistoricalRead,
        Self::RepeatedHistoricalRead,
        Self::CompareRowsAndSeq,
        Self::CloseReader,
        Self::SyncCheckedMarker,
        Self::ReadyReader,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::ConfigurePlatform => "configurePlatform",
            Self::StartOpen => "startOpen",
            Self::WaitOpen => "waitOpen",
            Self::PrepareOriginalRole => "prepareOriginalRole",
            Self::PrepareRuntime => "prepareRuntime",
            Self::ValidateOwned => "validateOwned",
            Self::StartEdit => "startEdit",
            Self::WaitEdit => "waitEdit",
            Self::ReadEditResult => "readEditResult",
            Self::SyncOriginalObservations => "syncOriginalObservations",
            Self::ReadyWriter => "readyWriter",
            Self::ReadOriginalObservations => "readOriginalObservations",
            Self::HistoricalRead => "historicalRead",
            Self::RepeatedHistoricalRead => "repeatedHistoricalRead",
            Self::CompareRowsAndSeq => "compareRowsAndSeq",
            Self::CloseReader => "closeReader",
            Self::SyncCheckedMarker => "syncCheckedMarker",
            Self::ReadyReader => "readyReader",
        }
    }
    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|checkpoint| checkpoint.name() == name)
    }
}
static ORIGINAL_EDIT_CHECKPOINT: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(OriginalEditCheckpoint::ConfigurePlatform as u8);

fn original_edit_checkpoint(checkpoint: OriginalEditCheckpoint) {
    use std::{io::Write, sync::atomic::Ordering};
    ORIGINAL_EDIT_CHECKPOINT.store(checkpoint as u8, Ordering::SeqCst);
    let mut output = std::io::stdout().lock();
    let _ = writeln!(output, "\noriginal-edit-checkpoint {}", checkpoint.name());
    let _ = output.flush();
}

// Installed only in the selected test child. No payload or default panic hook.
fn original_edit_child_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        use std::{io::Write, sync::atomic::Ordering};
        let checkpoint = OriginalEditCheckpoint::ALL[usize::from(ORIGINAL_EDIT_CHECKPOINT.load(Ordering::SeqCst))];
        let (source, line, column) = match info.location() {
            Some(location) => (
                if location.file().ends_with("/meguri-ffi/tests/cloud_original_role.rs") {
                    "fixture"
                } else {
                    "other"
                },
                location.line(),
                location.column(),
            ),
            None => ("other", 0, 0),
        };
        let mut output = std::io::stdout().lock();
        let _ = writeln!(
            output,
            "\noriginal-edit-panic {} {source} {line} {column}",
            checkpoint.name()
        );
        let _ = output.flush();
    }));
}

enum OriginalEditDiagnostic {
    Checkpoint(OriginalEditCheckpoint),
    Panic {
        checkpoint: OriginalEditCheckpoint,
        source: &'static str,
        line: u32,
        column: u32,
    },
}
impl std::fmt::Display for OriginalEditDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checkpoint(checkpoint) => f.write_str(checkpoint.name()),
            Self::Panic {
                checkpoint,
                source,
                line,
                column,
            } => {
                write!(f, "{} {source} {line}:{column}", checkpoint.name())
            }
        }
    }
}
impl OriginalEditDiagnostic {
    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<_> = line.split(' ').collect();
        match fields.as_slice() {
            ["original-edit-checkpoint", checkpoint] => {
                Some(Self::Checkpoint(OriginalEditCheckpoint::parse(checkpoint)?))
            }
            ["original-edit-panic", checkpoint, source, line, column] => Some(Self::Panic {
                checkpoint: OriginalEditCheckpoint::parse(checkpoint)?,
                source: match *source {
                    "fixture" => "fixture",
                    "other" => "other",
                    _ => return None,
                },
                line: line.parse().ok()?,
                column: column.parse().ok()?,
            }),
            _ => None,
        }
    }
}

// Both subprocess stages stop at a content-free readiness message. The writer
// sends it only after syncing the real edit observations and witness; the reader
// sends it only after all historical read/no-mutation assertions have completed.
fn witnessed_edit_process(root: &Path, stage: &str, ack: bool) {
    use std::{
        io::{BufRead, BufReader, Read},
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    let mode = match stage {
        "bound-commit" => "boundCommit",
        "bound-read" => "boundRead",
        _ => panic!("unsupported witnessed process mode"),
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "original_edit_reopen_process_entry", "--nocapture"])
        .env("MEGURI_ORIGINAL_EDIT_PROCESS_ROOT", root)
        .env("MEGURI_ORIGINAL_EDIT_PROCESS_STAGE", stage)
        .env("MEGURI_ORIGINAL_ACK_PROCESS", if ack { "yes" } else { "no" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let output = child.stdout.take().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut diagnostic = None;
        let mut ready = false;
        for line in BufReader::new(output.take(64 * 1024)).lines() {
            let Ok(line) = line else { break };
            if line == "original-edit-process-ready" {
                ready = true;
                break;
            }
            if let Some(retained) = OriginalEditDiagnostic::parse(&line) {
                diagnostic = Some(retained);
            }
        }
        let _ = ready_tx.send(ready);
        diagnostic
    });
    let ready = ready_rx.recv_timeout(Duration::from_secs(20));
    // Kill and reap on failure as well. No assertion may abandon the live owner.
    let killed = child.kill();
    let (exit_tx, exit_rx) = mpsc::channel();
    let reaper = std::thread::spawn(move || {
        let result = child.wait();
        let _ = exit_tx.send(result);
    });
    let exited = exit_rx.recv_timeout(Duration::from_secs(20));
    assert!(exited.is_ok(), "subprocess cleanup must finish within its bound");
    reaper.join().unwrap();
    // Killing closes the pipe even if readiness timed out. Join its reader
    // through a bounded notification before joining the already-finished thread.
    if ready.is_err() {
        let _ = ready_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    }
    let diagnostic = reader
        .join()
        .unwrap()
        .map_or_else(|| "none".into(), |value| value.to_string());
    assert!(
        ready.is_ok_and(|ready| ready),
        "subprocess did not reach its proven boundary: mode={mode} last={diagnostic}"
    );
    killed.unwrap();
    let status = exited.unwrap().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(9));
}

#[test]
fn bound_original_edit_witness_survives_process_kill_without_a_second_mutation() {
    bound_original_edit_process(false);
}

#[test]
fn bound_original_ack_survives_process_kill_without_a_second_mutation_or_exchange() {
    bound_original_edit_process(true);
}

fn bound_original_edit_process(ack: bool) {
    use meguri_model::rng::SeededRandom;
    use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
    use meguri_testkit::cloud::{Config, Scope, Service, coordinator::PublicationRig, devices::IdentityReads};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Keys(AtomicUsize);
    impl KeyByteSource for Keys {
        fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
            match self.0.fetch_add(1, Ordering::SeqCst) {
                0 => Ok([31; 32]),
                1 => Ok([124; 32]),
                _ => Err(KeySourceError::Unavailable),
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    generate(&root.path().join("original"));
    let live = root.path().join("live");
    std::fs::create_dir(&live).unwrap();
    std::fs::copy(
        root.path().join("original/collection.anki2"),
        live.join("collection.anki2"),
    )
    .unwrap();
    std::fs::create_dir(live.join("collection.media")).unwrap();
    for entry in std::fs::read_dir(root.path().join("original/collection.media")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), live.join("collection.media").join(entry.file_name())).unwrap();
    }
    // Actual unbound publication supplies controls and Active binding; no edit,
    // observer tuple, receipt or witness is seeded into tracking.
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut publisher = PublicationRig::open(
        &live,
        &scope,
        IdentityReads::Available(scope.account.clone()),
        false,
        cloud_small::NOW_MS,
        809,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap();
    let publication = (|| -> meguri_testkit::cloud::devices::Result<()> {
        let preview = publisher.preview(&mut service, 10_000)?;
        publisher.confirm_publication(&preview)?;
        publisher.drive_ready(&mut service, 10_000)?;
        Ok(())
    })();
    publisher.close_clean().unwrap();
    publication.unwrap();
    assert_eq!(expected(&live).0, expected(&root.path().join("original")).0);
    witnessed_edit_process(root.path(), "bound-commit", ack);
    let committed = expected(&live).0;
    assert_ne!(committed, expected(&root.path().join("original")).0);
    let observations = std::fs::read(root.path().join("original-observations.json")).unwrap();
    let request_count = service.request_log().unwrap().len();
    witnessed_edit_process(root.path(), "bound-read", ack);
    assert_eq!(expected(&live).0, committed);
    assert_eq!(
        std::fs::read(root.path().join("original-observations.json")).unwrap(),
        observations
    );
    if ack {
        assert_eq!(service.request_log().unwrap().len(), request_count);
    }
    assert!(root.path().join("historical-read-checked").is_file());
}

#[test]
fn original_edit_reopen_process_entry() {
    use meguri_ffi::{
        CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
        collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    };
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };
    let Some(root) = std::env::var_os("MEGURI_ORIGINAL_EDIT_PROCESS_ROOT") else {
        return;
    };
    let stage = std::env::var("MEGURI_ORIGINAL_EDIT_PROCESS_STAGE").unwrap();
    let bound = stage == "bound-commit" || stage == "bound-read";
    let ack = std::env::var("MEGURI_ORIGINAL_ACK_PROCESS").is_ok_and(|value| value == "yes");
    if bound {
        original_edit_child_panic_hook();
    }
    let checkpoint = |point| {
        if bound {
            original_edit_checkpoint(point);
        }
    };
    let root = std::path::PathBuf::from(root);
    struct Clock;
    impl HostClock for Clock {
        fn now_ms(&self) -> i64 {
            cloud_small::NOW_MS + 60_000
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
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let wait = |job_id| loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id, outcome } = event.kind
            && id == job_id
        {
            break outcome;
        }
    };
    let location = CollectionLocation {
        collection_directory: root.join("live").to_string_lossy().into_owned(),
        backups_directory: root.join("backups").to_string_lossy().into_owned(),
        app_group_snapshot: root.join("widget").to_string_lossy().into_owned(),
        manual_transfer_directory: root.join("manual").to_string_lossy().into_owned(),
    };
    std::fs::create_dir_all(root.join("backups")).unwrap();
    let opened = if bound {
        use meguri_ffi::sync::{self, CloudAccountStatus, CloudEnvironment, CloudKeyByteProvider};
        struct NoNewKeys;
        impl CloudKeyByteProvider for NoNewKeys {
            fn draw_key(&self) -> Option<Vec<u8>> {
                panic!("reopening the original binding must not draw a new key")
            }
        }
        checkpoint(OriginalEditCheckpoint::ConfigurePlatform);
        sync::configure_cloud_platform(
            "synthetic-u114".into(),
            CloudEnvironment::Development,
            CloudAccountStatus::Available,
            Some(b"synthetic-account-A".to_vec()),
            Arc::new(NoNewKeys),
        )
        .unwrap();
        checkpoint(OriginalEditCheckpoint::StartOpen);
        meguri_ffi::test_hooks::test_cloud_start_native_policy_open(location, Arc::new(Clock), Arc::new(Space), 771)
            .unwrap()
    } else {
        collection::start_open(location, Arc::new(Clock), Arc::new(Space), 771).unwrap()
    };
    checkpoint(OriginalEditCheckpoint::WaitOpen);
    assert_eq!(wait(opened.id), JobOutcome::Succeeded);
    checkpoint(OriginalEditCheckpoint::PrepareOriginalRole);
    let source = bytes(&root.join("original"));
    let role = test_cloud_original_role(source.0, source.1).unwrap();
    if bound {
        checkpoint(OriginalEditCheckpoint::PrepareRuntime);
        meguri_ffi::sync::CloudWorkflow::new()
            .unwrap()
            .prepare_runtime()
            .unwrap();
    }
    if stage == "commit" || stage == "bound-commit" {
        checkpoint(OriginalEditCheckpoint::ValidateOwned);
        assert!(!role.validate_owned().unwrap().q_changed);
        checkpoint(OriginalEditCheckpoint::StartEdit);
        let edit = role.clone().start_fixed_edit().unwrap();
        checkpoint(OriginalEditCheckpoint::WaitEdit);
        assert_eq!(wait(edit.handle.id), JobOutcome::Succeeded);
        checkpoint(OriginalEditCheckpoint::ReadEditResult);
        let facts = edit.result.result().unwrap().unwrap();
        assert_eq!(facts.updated_notes, 1);
        if bound {
            assert!(facts.local_revision.is_some_and(|revision| revision > 0));
            assert_eq!(facts.original_witness.as_ref().unwrap().len(), 32);
        } else {
            assert!(facts.local_revision.is_none() && facts.original_witness.is_none());
        }
        let ack_facts = if ack {
            Some(ack_original_edit_with_service(&root, &role))
        } else {
            None
        };
        let observations = serde_json::json!({"ack": ack_facts, "committed": facts.seq, "generation": facts.generation,
            "digest": facts.note_digest, "durable_revision": facts.local_revision,
            "witness": facts.original_witness, "writer_process": std::process::id()});
        checkpoint(OriginalEditCheckpoint::SyncOriginalObservations);
        let file = std::fs::File::create(root.join("original-observations.json")).unwrap();
        serde_json::to_writer(&file, &observations).unwrap();
        file.sync_all().unwrap();
        std::fs::File::open(&root).unwrap().sync_all().unwrap();
        if bound {
            checkpoint(OriginalEditCheckpoint::ReadyWriter);
            original_edit_process_ready();
        }
        // Exit with the actual flushed owner still open, before any simulated
        // done or clean close. Nothing survives in the next process's EditState.
        std::process::exit(0);
    }
    checkpoint(OriginalEditCheckpoint::ValidateOwned);
    assert!(role.validate_owned().unwrap().q_changed);
    if stage == "bound-read" {
        checkpoint(OriginalEditCheckpoint::ReadOriginalObservations);
        let observations: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("original-observations.json")).unwrap()).unwrap();
        assert_ne!(
            observations["writer_process"].as_u64().unwrap(),
            u64::from(std::process::id())
        );
        let witness: Vec<u8> = serde_json::from_value(observations["witness"].clone()).unwrap();
        let collection = meguri_ffi::test_hooks::collection_for_testing().unwrap();
        let before_seq = collection.last_seq();
        let before = {
            let reader = collection.reader(ReaderRole::Query).unwrap();
            [Table::Col, Table::Notes, Table::Cards, Table::Decks, Table::Revlog]
                .map(|table| stored_rows(&*reader, table))
        };
        checkpoint(OriginalEditCheckpoint::HistoricalRead);
        let original = role.read_original_edit(witness.clone()).unwrap();
        assert_eq!(
            original.original_generation,
            observations["generation"].as_u64().unwrap()
        );
        assert_eq!(original.original_seq, observations["committed"].as_u64().unwrap());
        assert_eq!(
            original.local_revision,
            observations["durable_revision"].as_u64().unwrap()
        );
        assert_eq!(
            original.note_digest,
            serde_json::from_value::<Vec<u8>>(observations["digest"].clone()).unwrap()
        );
        checkpoint(OriginalEditCheckpoint::RepeatedHistoricalRead);
        assert_eq!(
            role.read_original_edit(witness.clone()).unwrap().original_seq,
            original.original_seq
        );
        for missing in [vec![], vec![0; 31], vec![0; 32]] {
            assert!(role.read_original_ack(missing).is_err());
        }
        if ack {
            let historical = role.read_original_ack(witness.clone()).unwrap();
            assert_eq!(historical.original_generation, original.original_generation);
            assert_eq!(historical.original_seq, original.original_seq);
            assert_eq!(historical.local_revision, original.local_revision);
            assert_eq!(historical.acked_revision, original.local_revision);
            assert_eq!(historical.note_digest, original.note_digest);
            assert_eq!(historical.generation, collection.generation().0);
            assert_eq!(
                historical.acked_envelope_digest,
                serde_json::from_value::<Vec<u8>>(observations["ack"]["envelope_digest"].clone()).unwrap()
            );
            let workflow = meguri_ffi::sync::CloudWorkflow::new().unwrap();
            let exchange = workflow
                .exchange_result(observations["ack"]["exchange"].as_u64().unwrap())
                .unwrap()
                .unwrap();
            assert!(matches!(
                exchange.collection,
                meguri_ffi::sync::CloudCollectionOutcome::Succeeded
            ));
            assert_eq!(
                exchange.selected_names,
                serde_json::from_value::<Vec<String>>(observations["ack"]["selected"].clone()).unwrap()
            );
            assert_eq!(
                exchange.acked_names,
                serde_json::from_value::<Vec<String>>(observations["ack"]["acked"].clone()).unwrap()
            );
            assert!(exchange.outstanding_names.is_empty());
            // Repeated reads admit no SDK, operation or exchange.
            assert_eq!(
                role.read_original_ack(witness.clone()).unwrap().acked_envelope_digest,
                historical.acked_envelope_digest
            );
        } else {
            assert!(role.read_original_ack(witness.clone()).is_err());
            // An actual new SDK ACK of this same current edit cannot repair the
            // retained unACKed original row captured before this open's scan.
            let fresh = ack_original_edit_with_service(&root, &role);
            assert!(fresh["envelope_digest"].as_array().is_some());
            assert!(role.observe().unwrap().entity.acknowledged);
            assert!(role.read_original_ack(witness.clone()).is_err());
            assert_eq!(
                role.read_original_edit(witness).unwrap().original_seq,
                original.original_seq
            );
        }
        checkpoint(OriginalEditCheckpoint::CompareRowsAndSeq);
        assert_eq!(collection.last_seq(), before_seq);
        let after = {
            let reader = collection.reader(ReaderRole::Query).unwrap();
            [Table::Col, Table::Notes, Table::Cards, Table::Decks, Table::Revlog]
                .map(|table| stored_rows(&*reader, table))
        };
        assert_eq!(after, before);
        if ack {
            let workflow = meguri_ffi::sync::CloudWorkflow::new().unwrap();
            workflow
                .account_changed(
                    None,
                    meguri_ffi::sync::CloudAccountStatus::Available,
                    Some(b"other-account".to_vec()),
                )
                .unwrap();
            let witness: Vec<u8> = serde_json::from_value(observations["witness"].clone()).unwrap();
            assert!(role.read_original_ack(witness).is_err());
        }
        checkpoint(OriginalEditCheckpoint::CloseReader);
        meguri_ffi::lifecycle::close_collection().unwrap();
        checkpoint(OriginalEditCheckpoint::SyncCheckedMarker);
        let checked = std::fs::File::create(root.join("historical-read-checked")).unwrap();
        checked.sync_all().unwrap();
        std::fs::File::open(&root).unwrap().sync_all().unwrap();
        checkpoint(OriginalEditCheckpoint::ReadyReader);
        original_edit_process_ready();
    }
    for missing in [vec![], vec![0; 31], vec![0; 32]] {
        assert!(role.read_original_edit(missing.clone()).is_err());
        assert!(role.read_original_ack(missing).is_err());
    }
    meguri_ffi::lifecycle::close_collection().unwrap();
}

// Real Core batch and Service Saved response. This is a protocol fixture, never
// signed SDK evidence. No tracking row or observer tuple is seeded.
fn ack_original_edit_with_service(
    root: &Path,
    role: &std::sync::Arc<meguri_ffi::test_hooks::CloudOriginalRole>,
) -> serde_json::Value {
    use meguri_ffi::sync::{CloudCollectionOutcome, CloudRecord, CloudWorkflow};
    use meguri_model::rng::SeededRandom;
    use meguri_testkit::cloud::{
        Caller, Config, Field, Mutation, MutationSuccess, Operation, Record, RecordInput, Reply, RequestedFields,
        Scope, Service, SystemFields,
    };
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    let mut service = Service::open(root, Config::default(), SeededRandom::new(812)).unwrap();
    let workflow = CloudWorkflow::new().unwrap();
    let registration = workflow.register_sdk(false).unwrap();
    let sdk = registration.instance_id;
    let caller = Caller {
        device: Some("original-ack-fixture".into()),
        instance: Some(sdk.clone()),
        own_operation: None,
    };
    let project = |record: Record, zone: &str| -> CloudRecord {
        let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
            panic!("Service must return its actual encrypted record");
        };
        CloudRecord {
            name: record.name,
            envelope: envelope.clone(),
            system_fields: Some(record.system_fields.bytes().to_vec()),
            record_type: Some(record.record_type),
            zone: Some(zone.into()),
            asset_path: None,
        }
    };
    let exchange = workflow.begin_exchange(sdk.clone()).unwrap();
    let fetch = workflow.begin_fetch(sdk.clone(), vec!["meguri-root".into()]).unwrap();
    let group = format!("meguri-{fetch}");
    workflow.will_fetch(sdk.clone(), group.clone(), true).unwrap();
    workflow
        .fetch_zone(sdk.clone(), "meguri-root".into(), true, false)
        .unwrap();
    let root_reply = service
        .request(
            &scope,
            caller.clone(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let Reply::Record(active) = root_reply.result.unwrap() else {
        panic!("actual Active read")
    };
    workflow
        .fetched(
            sdk.clone(),
            "meguri-root".into(),
            vec![project(active, "meguri-root")],
            vec![],
        )
        .unwrap();
    workflow
        .fetch_zone(sdk.clone(), "meguri-root".into(), false, false)
        .unwrap();
    workflow.did_fetch(sdk.clone(), group, true).unwrap();
    let fetched = workflow.finish_fetch(sdk.clone(), fetch).unwrap();
    assert!(fetched);
    workflow.exchange_fetched(sdk.clone(), fetched).unwrap();
    let mut pending = std::collections::HashMap::new();
    for update in workflow.take_pending_updates(sdk.clone()).unwrap() {
        if matches!(update.kind, meguri_ffi::sync::CloudUpdateKind::Full) {
            pending.clear();
        }
        for name in update.removes {
            pending.remove(&name);
        }
        let added = update.adds.keys().cloned().collect();
        pending.extend(update.adds);
        workflow.update_applied(sdk.clone(), update.id, added).unwrap();
    }
    let batch = workflow
        .build_batch(sdk.clone(), registration.collection_zone.clone(), pending)
        .unwrap()
        .unwrap();
    assert!(!batch.saves.is_empty() && batch.deletes.is_empty());
    let response = service
        .request(
            &scope,
            caller,
            Operation::Modify {
                zone: registration.collection_zone.clone(),
                atomic: false,
                changes: batch
                    .saves
                    .iter()
                    .map(|record| Mutation::Save {
                        record: RecordInput::encrypted(&record.name, "MGEntity", record.envelope.clone()),
                        if_unchanged: record.system_fields.clone().map(SystemFields::from_bytes),
                    })
                    .collect(),
            },
        )
        .unwrap();
    let Reply::Modified(results) = response.result.unwrap() else {
        panic!("actual Modify result")
    };
    let saved: Vec<_> = results
        .into_iter()
        .map(|result| {
            let MutationSuccess::Saved(record) = result.result.unwrap() else {
                panic!("actual Saved result")
            };
            project(record, &registration.collection_zone)
        })
        .collect();
    let names = saved.iter().map(|record| record.name.clone()).collect();
    let count = u32::try_from(saved.len()).unwrap();
    workflow
        .sent(sdk.clone(), batch.id, saved, vec![], vec![], vec![], count)
        .unwrap();
    workflow.handler_returned(sdk.clone()).unwrap();
    workflow.sdk_saved_removed(sdk.clone(), batch.id, names).unwrap();
    workflow
        .state_update(sdk.clone(), b"original-ack-protocol-fixture-state".to_vec())
        .unwrap();
    let result = workflow
        .complete_exchange(sdk, exchange, fetched, false, false)
        .unwrap();
    assert!(matches!(result.collection, CloudCollectionOutcome::Succeeded));
    assert!(!result.selected_names.is_empty());
    assert_eq!(result.selected_names, result.acked_names);
    assert!(result.outstanding_names.is_empty());
    let observed = role.observe().unwrap();
    assert!(observed.entity.acknowledged);
    serde_json::json!({ "exchange": exchange, "selected": result.selected_names, "acked": result.acked_names,
                        "envelope_digest": observed.entity.acked_envelope_digest })
}

fn original_edit_process_ready() -> ! {
    use std::io::{Read, Write};
    println!("\noriginal-edit-process-ready");
    std::io::stdout().flush().unwrap();
    // Parent retains stdin until its controlled kill. EOF or a byte is a
    // fixture failure, never permission to mutate, close or report success.
    let _ = std::io::stdin().read_exact(&mut [0]);
    panic!("parent must kill the original process after readiness");
}
