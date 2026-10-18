mod common;

use common::{Harness, note};
use meguri_engine::media::{MediaJobKind, MediaService};
use meguri_engine::{EngineError, EventKind, OpSource, OpSpec};
use std::{fs, sync::Arc};

#[derive(Default)]
struct FileObserver {
    root: std::sync::Mutex<std::path::PathBuf>,
    seen: std::sync::Mutex<Vec<bool>>,
}
impl meguri_engine::ChangeObserver for FileObserver {
    fn name(&self) -> &str {
        "media-file-observer"
    }
    fn observe(&self, commit: &meguri_engine::ObservedCommit<'_>) -> Result<(), EngineError> {
        for name in &commit.changes.media_added {
            self.seen
                .lock()
                .unwrap()
                .push(fs::read(self.root.lock().unwrap().join(name)).is_ok());
        }
        Ok(())
    }
}

#[test]
fn media_op_files_are_available_inside_the_commit_observer() {
    let observer = Arc::new(FileObserver::default());
    let h = Harness::open_with(meguri_engine::Ports::default().observer(observer.clone()));
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    *observer.root.lock().unwrap() = media.store().media_dir().to_path_buf();
    let mut stage = media.begin_staging().unwrap();
    stage.add("visible.png", b"observer").unwrap();
    h.collection
        .run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
            let id = ctx.scope().add_note(&note(&["<img src='visible.png'>"]))?;
            ctx.commit_media(stage, &[id])
        })
        .unwrap();
    assert_eq!(*observer.seen.lock().unwrap(), [true]);
}

#[test]
fn media_recovery_uses_final_stored_note_rows_after_all_op_steps() {
    use meguri_storage::ReadRows;
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("final.png", b"final").unwrap();
    meguri_model::faultpoint::arm(
        "media.stage.before_publish",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = h
        .collection
        .run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
            let id = ctx.scope().add_note(&note(&["initial"]))?;
            ctx.commit_media(stage, &[id])?;
            let mut row = ctx.scope().note(id)?.unwrap();
            row.fields = "final <img src='final.png'>".into();
            ctx.scope().put(&row)?;
            Ok(id)
        });
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(result, Err(EngineError::MediaCommitPending { .. })));
    assert_eq!(media.recover().unwrap().published, 1);
    assert_eq!(fs::read(media.store().media_dir().join("final.png")).unwrap(), b"final");
}

#[test]
fn media_op_publication_continues_after_an_earlier_stage_fails() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut first = media.begin_staging().unwrap();
    first.add("first.png", b"first").unwrap();
    let mut second = media.begin_staging().unwrap();
    second.add("second.png", b"second").unwrap();
    fs::write(media.store().media_dir().join("first.png"), b"outside").unwrap();
    let result = h
        .collection
        .run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
            let id = ctx
                .scope()
                .add_note(&note(&["<img src='first.png'><img src='second.png'>"]))?;
            ctx.commit_media(first, &[id])?;
            ctx.commit_media(second, &[id])
        });
    assert!(matches!(result, Err(EngineError::MediaCommitPending { .. })));
    assert_eq!(
        fs::read(media.store().media_dir().join("first.png")).unwrap(),
        b"outside"
    );
    assert_eq!(
        fs::read(media.store().media_dir().join("second.png")).unwrap(),
        b"second"
    );
    assert_eq!(fs::read_dir(media.store().staging_dir()).unwrap().count(), 1);
}

#[test]
fn media_op_publishes_before_the_changes_event_and_rolls_back_failed_steps() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let events = h.engine.stream().subscribe();
    let mut stage = media.begin_staging().unwrap();
    let name = stage.add("Photo.PNG", b"committed").unwrap();
    assert!(!media.store().media_dir().join(&name).exists());
    let name_for_op = name.clone();
    let store = media.store().clone();
    let result = h
        .collection
        .run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
            let step: Result<(), EngineError> = ctx.step(|ctx| {
                let mut discarded = store.begin_staging().unwrap();
                discarded.add("discarded.png", b"rollback").unwrap();
                let id = ctx.scope().add_note(&note(&["discarded"]))?;
                ctx.commit_media(discarded, &[id])?;
                Err(EngineError::Cancelled)
            });
            assert!(matches!(step, Err(EngineError::Cancelled)));
            let id = ctx.scope().add_note(&note(&[&format!("<img src='{name_for_op}'>")]))?;
            ctx.commit_media(stage, &[id])?;
            Ok(id)
        })
        .unwrap();
    assert_eq!(fs::read(media.store().media_dir().join(&name)).unwrap(), b"committed");
    assert!(!media.store().media_dir().join("discarded.png").exists());
    assert_eq!(fs::read_dir(media.store().staging_dir()).unwrap().count(), 0);
    let changes = events.try_recv().unwrap();
    assert_eq!(changes.seq, result.seq);
    assert!(matches!(&changes.kind, EventKind::Changes(c) if c.media_added == [name]));
}

#[test]
fn media_op_publication_failure_reports_a_committed_state_and_recovers_from_rows() {
    let h = Harness::open();
    let initial_notes = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("recover.png", b"durable").unwrap();
    meguri_model::faultpoint::arm(
        "media.stage.before_publish",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = h
        .collection
        .run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
            let id = ctx.scope().add_note(&note(&["<img src='recover.png'>"]))?;
            ctx.commit_media(stage, &[id])?;
            Ok(id)
        });
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(result, Err(EngineError::MediaCommitPending { .. })));
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes"),
        initial_notes + 1
    );
    assert!(!media.store().media_dir().join("recover.png").exists());
    assert_eq!(media.recover().unwrap().published, 1);
    assert_eq!(
        fs::read(media.store().media_dir().join("recover.png")).unwrap(),
        b"durable"
    );
    assert_eq!(media.recover().unwrap().published, 0);
}

#[test]
fn media_job_lease_excludes_other_jobs_but_allows_adds() {
    let h = Harness::open();
    let media = Arc::new(MediaService::open(&h.collection, h.clock.clone()).unwrap());
    let lease = media.try_job(MediaJobKind::Sync).unwrap();
    assert!(matches!(
        media.try_job(MediaJobKind::CheckMedia),
        Err(EngineError::Busy { .. })
    ));
    assert_eq!(media.add("available.png", b"add").unwrap(), "available.png");
    drop(lease);
    assert!(media.try_job(MediaJobKind::CheckMedia).is_ok());
}

#[test]
fn media_services_for_the_same_collection_share_job_admission() {
    let h = Harness::open();
    let first = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let second = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let _lease = first.try_job(MediaJobKind::Sync).unwrap();
    assert!(matches!(
        second.try_job(MediaJobKind::CheckMedia),
        Err(EngineError::Busy { .. })
    ));
}

#[test]
fn media_staging_requires_a_changed_note_witness() {
    let h = Harness::open();
    let id = common::add_notes(&h.collection, 1, "<img src='witness.png'>")[0];
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("witness.png", b"not committed by a row write").unwrap();
    let result = h
        .collection
        .run(OpSpec::named("media_noop", OpSource::UserNonUndoable), move |ctx| {
            ctx.commit_media(stage, &[id])
        });
    assert!(matches!(result, Err(EngineError::InvalidArgument { .. })));
    assert!(!media.store().media_dir().join("witness.png").exists());
    assert_eq!(fs::read_dir(media.store().staging_dir()).unwrap().count(), 0);
}

#[test]
fn media_collection_keeps_the_same_journal_while_a_stage_outlives_a_service_handle() {
    let h = Harness::open();
    let first = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = first.begin_staging().unwrap();
    stage.add("held.png", b"held").unwrap();
    let before = first.store().diagnostics();
    drop(first);
    let second = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    assert_eq!(second.store().diagnostics(), before);
    second.store().check_changes().unwrap();
    let after = second.store().diagnostics();
    assert_eq!(after.scans, before.scans);
    drop(stage);
}

#[test]
fn media_crash_worker() {
    let Some(path) = std::env::var_os("MEGURI_MEDIA_CRASH_PATH") else {
        return;
    };
    let point = std::env::var("MEGURI_MEDIA_CRASH_POINT").unwrap();
    let path = std::path::PathBuf::from(path);
    let clock = Arc::new(meguri_model::clock::StepClock::new(
        common::NOW_MS,
        meguri_model::clock::UtcOffset::UTC,
        "UTC",
    ));
    let engine = meguri_engine::Engine::new(meguri_engine::EngineConfig::default()).unwrap();
    let collection = engine.open_raw(&path, common::open_config(&clock)).unwrap();
    let media = MediaService::open(&collection, clock).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("crash.png", b"survive process loss").unwrap();
    meguri_model::faultpoint::arm(&point, meguri_model::faultpoint::Fault::Abort);
    let _ = collection.run(OpSpec::named("media_crash", OpSource::UserNonUndoable), move |ctx| {
        let id = ctx.scope().add_note(&note(&["<img src='crash.png'>"]))?;
        ctx.commit_media(stage, &[id])
    });
    panic!("crash boundary was not reached");
}

#[test]
fn media_real_process_crashes_preserve_bytes_on_both_sides_of_the_collection_commit() {
    for (point, committed) in [("storage.commit.before", false), ("storage.commit.after", true)] {
        let (dir, path) = common::copy_fixture("schema-18.anki2");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "media_crash_worker", "--nocapture"])
            .env("MEGURI_MEDIA_CRASH_PATH", &path)
            .env("MEGURI_MEDIA_CRASH_POINT", point)
            .output()
            .unwrap();
        assert!(!output.status.success(), "child must crash at {point}");
        assert_eq!(fs::read_dir(dir.path().join("media.staging")).unwrap().count(), 1);
        let h = Harness::open_path(dir, path);
        let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
        let report = media.recover().unwrap();
        if committed {
            assert_eq!(report.published, 1, "{point}");
            assert_eq!(
                fs::read(media.store().media_dir().join("crash.png")).unwrap(),
                b"survive process loss"
            );
        } else {
            assert_eq!(report.trashed, 1, "{point}");
            assert!(!media.store().media_dir().join("crash.png").exists());
            assert_eq!(media.store().trash_summary().unwrap().entries, 1);
            media.store().restore().unwrap();
            assert_eq!(
                fs::read(media.store().media_dir().join("crash.png")).unwrap(),
                b"survive process loss"
            );
        }
    }
}

#[test]
fn media_post_commit_storage_error_never_rolls_back_committed_attachments() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("after.png", b"committed bytes").unwrap();
    meguri_model::faultpoint::arm(
        "storage.commit.after",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = h
        .collection
        .run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
            let id = ctx.scope().add_note(&note(&["<img src='after.png'>"]))?;
            ctx.commit_media(stage, &[id])
        });
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(result, Err(EngineError::CommitCompleted { .. })));
    media.recover().unwrap();
    assert_eq!(
        fs::read(media.store().media_dir().join("after.png")).unwrap(),
        b"committed bytes"
    );
}
