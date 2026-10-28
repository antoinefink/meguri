mod common;
use meguri_engine::{DisabledReason, EngineError, EventKind, OpSource, OpSpec, UndoScope};
use meguri_storage::{ReadRows, ReaderRole, raw::PlainDatabase};
use std::time::Duration;

fn external_edit(h: &common::Harness, nid: meguri_model::ids::NoteId) {
    let db = PlainDatabase::open(&h.path).unwrap();
    db.execute(
        "UPDATE notes SET flds = 'external' || char(31) || 'back', sfld = 'external' WHERE id = ?1",
        &[meguri_storage::raw::RawValue::Integer(nid.0)],
    )
    .unwrap();
}

#[test]
fn resume_detects_external_writer_clears_undo_and_publishes_whole_collection() {
    let h = common::Harness::open();
    let (nid, _) = common::steps::setup_note(&h.collection, "before");
    common::steps::edit(&h.collection, None, nid, "local").unwrap();
    let sub = h.engine.stream().subscribe();
    external_edit(&h, nid);
    assert!(h.engine.resume().unwrap().external_change);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().reason(),
        Some(DisabledReason::ChangedExternally)
    );
    assert_eq!(common::note_fields(&h.collection, nid), "external\u{1f}back");
    let events = common::drain(&sub, Duration::from_millis(10));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(&e.kind, EventKind::Changes(c) if c.whole_collection))
            .count(),
        1
    );
    assert!(!h.engine.resume().unwrap().external_change);
}

#[test]
fn before_write_check_refuses_stale_work_then_accepts_current_work() {
    let h = common::Harness::open();
    let (nid, _) = common::steps::setup_note(&h.collection, "before");
    external_edit(&h, nid);
    let result = h
        .collection
        .run::<(), _>(OpSpec::named("planned", OpSource::UserUndoable), |_| {
            panic!("stale closure ran")
        });
    assert!(matches!(result, Err(EngineError::Stale { .. })));
    common::steps::edit(&h.collection, None, nid, "fresh").unwrap();
    assert_eq!(common::note_fields(&h.collection, nid), "fresh\u{1f}back");
}

#[test]
fn suspended_external_edit_and_rollover_report_both_without_starting_jobs() {
    let h = common::Harness::open();
    let (nid, _) = common::steps::setup_note(&h.collection, "before");
    let next_day = h.collection.check_rollover().unwrap().next_day_at;
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    external_edit(&h, nid);
    h.clock.set_now_ms((next_day + 1) * 1000);
    let report = h.engine.resume().unwrap();
    assert!(report.external_change);
    assert!(report.rolled_over);
    assert!(report.next_day_at > next_day);
    assert!(h.engine.jobs().running().is_empty());
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(nid)
            .unwrap()
            .unwrap()
            .fields,
        "external\u{1f}back"
    );
}

#[test]
fn sidecar_detects_offline_edit_and_retains_unknown_device_state() {
    let h = common::Harness::open();
    h.engine.close().unwrap();
    let sidecar_path = h.dir.path().join("meguri-sidecar.json");
    let mut sidecar = meguri_storage::sidecar::Sidecar::load(&sidecar_path).unwrap();
    assert!(sidecar.clean_shutdown);
    sidecar.extra.insert("future".into(), serde_json::json!({"kept": 7}));
    sidecar.save(&sidecar_path).unwrap();
    let db = PlainDatabase::open(&h.path).unwrap();
    db.execute("UPDATE col SET mod = mod + 99", &[]).unwrap();
    drop(db);
    let sub = h.engine.stream().subscribe();
    let _collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    let events = common::drain(&sub, Duration::from_millis(10));
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, EventKind::Changes(c) if c.whole_collection))
    );
    h.engine.close().unwrap();
    assert_eq!(
        meguri_storage::sidecar::Sidecar::load(&sidecar_path).unwrap().extra["future"],
        serde_json::json!({"kept": 7})
    );
}

#[test]
fn failed_reopen_keeps_external_edit_detection_for_the_next_resume() {
    use meguri_model::faultpoint::{self, Fault};
    let h = common::Harness::open();
    let (nid, _) = common::steps::setup_note(&h.collection, "before");
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    // This client does not update col.mod or scm: only the saved file stamp
    // can detect its edit after SQLite connections have been closed.
    external_edit(&h, nid);
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
    assert!(h.engine.resume().is_err());
    faultpoint::disarm_all();
    assert!(h.engine.resume().unwrap().external_change);
    assert_eq!(common::note_fields(&h.collection, nid), "external\u{1f}back");
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().reason(),
        Some(DisabledReason::ChangedExternally)
    );
}

#[test]
fn external_notification_uses_live_rows_and_isolates_failure_hooks_before_publication() {
    use meguri_engine::{ChangeObserver, ObservedCommit, ObserverRows, Ports, Seq};
    use meguri_storage::{Record, RowKey, Table};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct ExternalRows {
        note: Mutex<Option<meguri_model::ids::NoteId>>,
        seen: Mutex<Vec<Seq>>,
        failed: Mutex<Vec<Seq>>,
    }
    impl ChangeObserver for ExternalRows {
        fn name(&self) -> &str {
            "external-live-rows"
        }
        fn observe(&self, _: &ObservedCommit<'_>) -> Result<(), EngineError> {
            panic!("external notify must use rows callback")
        }
        fn observe_with_rows(&self, commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Result<(), EngineError> {
            if !commit.changes.whole_collection {
                return Ok(());
            }
            let id = self.note.lock().unwrap().unwrap();
            let row = rows.row_image(Table::Notes, &RowKey::Id(id.0))?.unwrap();
            assert_eq!(
                meguri_model::note::Note::from_cells(&row)
                    .map_err(meguri_storage::StorageError::from)?
                    .fields,
                "external\u{1f}back"
            );
            assert_eq!(commit.payload.row_keys(Table::Notes).count(), 0);
            self.seen.lock().unwrap().push(commit.seq);
            Err(EngineError::InvalidArgument {
                message: "external observer error".into(),
            })
        }
        fn observe_failed(&self, seq: Seq) {
            self.failed.lock().unwrap().push(seq);
            panic!("external hook panic");
        }
    }
    #[derive(Default)]
    struct Following(Mutex<Vec<Seq>>);
    impl ChangeObserver for Following {
        fn name(&self) -> &str {
            "external-following"
        }
        fn observe(&self, c: &ObservedCommit<'_>) -> Result<(), EngineError> {
            if c.changes.whole_collection {
                self.0.lock().unwrap().push(c.seq);
            }
            Ok(())
        }
    }
    let observer = Arc::new(ExternalRows::default());
    let following = Arc::new(Following::default());
    let h = common::Harness::open_with(Ports::new().observer(observer.clone()).observer(following.clone()));
    let (nid, _) = common::steps::setup_note(&h.collection, "before");
    *observer.note.lock().unwrap() = Some(nid);
    let events = h.engine.stream().subscribe();
    external_edit(&h, nid);
    assert!(h.engine.resume().unwrap().external_change);
    let seqs = observer.seen.lock().unwrap().clone();
    assert_eq!(seqs.len(), 1);
    assert_eq!(*observer.failed.lock().unwrap(), seqs);
    assert_eq!(*following.0.lock().unwrap(), seqs);
    let published: Vec<_> = std::iter::from_fn(|| events.try_recv())
        .filter(|e| matches!(&e.kind, EventKind::Changes(c) if c.whole_collection))
        .map(|e| e.seq)
        .collect();
    assert_eq!(published, seqs);
}
