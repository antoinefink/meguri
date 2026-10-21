mod common;

use std::thread;
use std::time::Duration;

use meguri_engine::{EngineError, LongOp, OpSource, OpSpec, executor::catalog, monotonic};
use meguri_storage::{ReadRows, ReaderRole};

use common::{Harness, add_note_with_card};

#[test]
fn twenty_commits_are_full_synced_within_one_second_with_a_snapshot_open() {
    let h = Harness::open();
    h.collection.record_durability_events();
    let reader = h.collection.reader(ReaderRole::Sync).unwrap();
    let snapshot = reader.snapshot().unwrap();
    let original = snapshot.col().unwrap();
    let mut commits = Vec::new();
    for i in 0..20 {
        let result = add_note_with_card(&h.collection, OpSource::UserUndoable, &[&format!("commit {i}"), "x"]);
        commits.push((result.seq, monotonic::now()));
        // Both busy-queue and idle intervals, spanning multiple flushes.
        thread::sleep(Duration::from_millis(60));
    }
    let deadline = commits.last().unwrap().1 + Duration::from_secs(1);
    let events = loop {
        let events = h.collection.durability_events();
        if events
            .syncs
            .iter()
            .any(|s| s.succeeded && s.through >= commits.last().unwrap().0)
        {
            break events;
        }
        assert!(
            monotonic::now() < deadline,
            "idle writer never synced the last commit: {events:?}"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(snapshot.col().unwrap(), original, "the snapshot stayed pinned");
    for (seq, returned) in commits {
        let sync = events.syncs.iter().find(|s| s.succeeded && s.through >= seq).unwrap();
        assert!(
            sync.completed.saturating_sub(returned) <= Duration::from_secs(1),
            "{seq:?}: {sync:?}"
        );
    }
    let syncs: Vec<_> = events.syncs.iter().filter(|s| s.succeeded).collect();
    assert!(syncs.len() < 20, "nearby commits must share a sync");
    assert_eq!(events.writes.len(), 20);
    for sync in syncs {
        for phase in &events.writes {
            assert!(
                sync.completed <= phase.started || sync.started >= phase.completed,
                "{sync:?} overlaps {phase:?}"
            );
        }
    }
}

#[test]
fn a_failed_sync_retries_the_same_commits_and_clean_idle_does_not_sync() {
    let h = Harness::open();
    let subscription = h.engine.stream().subscribe();
    h.collection.record_durability_events();
    h.collection
        .submit_maintenance(catalog::WAL_CHECKPOINT, Duration::ZERO, |writer| {
            writer.fail_next_wal_sync_for_test();
            Ok(())
        })
        .wait()
        .unwrap();
    let commit = add_note_with_card(&h.collection, OpSource::UserUndoable, &["retry", "x"]);
    let returned = monotonic::now();
    let events = loop {
        let events = h.collection.durability_events();
        if events.syncs.iter().any(|s| s.succeeded && s.through >= commit.seq) {
            break events;
        }
        assert!(monotonic::now() - returned < Duration::from_secs(1), "{events:?}");
        thread::sleep(Duration::from_millis(5));
    };
    let failed = events.syncs.iter().find(|s| !s.succeeded).unwrap();
    let retry = events
        .syncs
        .iter()
        .find(|s| s.succeeded && s.through >= commit.seq)
        .unwrap();
    assert_eq!(failed.through, commit.seq);
    assert_eq!(retry.through, failed.through);
    assert!(retry.started >= failed.completed);

    use meguri_engine::{EventKind, suspend::DurabilityStatus};
    let statuses: Vec<_> = common::drain(&subscription, Duration::from_millis(10))
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::Extension(payload) => payload.as_any().downcast_ref::<DurabilityStatus>().cloned(),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            DurabilityStatus::Failed {
                through: commit.seq,
                error: meguri_storage::StorageError::Io {
                    detail: "injected WAL sync failure".into()
                },
            },
            DurabilityStatus::Recovered { through: commit.seq },
        ]
    );

    // Once covered, clean idle must not start periodic work.
    thread::sleep(Duration::from_millis(1100));
    assert_eq!(h.collection.durability_events().syncs.len(), events.syncs.len());
}

#[test]
fn queued_ops_do_not_starve_a_flush_and_a_long_op_flushes_earlier_commits_first() {
    let h = Harness::open();
    h.collection.record_durability_events();
    let reader = h.collection.reader(ReaderRole::Sync).unwrap();
    let _snapshot = reader.snapshot().unwrap();
    let mut pending = Vec::new();
    for i in 0..30 {
        pending.push(
            h.collection
                .submit(OpSpec::named("add_note", OpSource::UserUndoable), move |ctx| {
                    thread::sleep(Duration::from_millis(40));
                    ctx.scope().add_note(&common::note(&[&format!("queued {i}"), "x"]))?;
                    Ok(())
                }),
        );
    }
    let mut commits = Vec::new();
    for pending in pending {
        commits.push((pending.wait().unwrap().seq, monotonic::now()));
    }
    let events = h.collection.durability_events();
    // A sync left due by the open itself may run before the first commit.
    let first = events
        .syncs
        .iter()
        .find(|s| s.succeeded && s.through >= commits[0].0)
        .expect("flush during traffic");
    assert!(first.through < commits.last().unwrap().0);
    assert!(first.completed.saturating_sub(commits[0].1) < Duration::from_secs(1));

    let collection = h.collection.clone();
    let last = commits.last().unwrap().0;
    h.collection
        .run(
            OpSpec::new(catalog::CHECK_DATABASE, OpSource::Maintenance).declare_long_write(Duration::from_millis(1200)),
            move |_| {
                assert!(
                    collection
                        .durability_events()
                        .syncs
                        .iter()
                        .any(|s| s.succeeded && s.through >= last)
                );
                thread::sleep(Duration::from_millis(1100));
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn a_replace_that_fails_after_swapping_the_file_still_flushes_the_reopened_writers_upgrade() {
    let h = Harness::open();
    h.collection.record_durability_events();
    let commit = add_note_with_card(&h.collection, OpSource::UserUndoable, &["old writer", "x"]);
    let deadline = monotonic::now() + Duration::from_secs(1);
    while !h
        .collection
        .durability_events()
        .syncs
        .iter()
        .any(|s| s.succeeded && s.through >= commit.seq)
    {
        assert!(monotonic::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let before = h.collection.durability_events().syncs.len();
    let generation = h.collection.generation();
    let replacement = h.dir.path().join("replacement.anki2");
    std::fs::copy(common::fixture("schema-17.anki2"), &replacement).unwrap();
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let result = exclusive.replace(move |live| {
        std::fs::rename(&replacement, live).unwrap();
        Err(EngineError::InvalidArgument {
            message: "failed after replacing the file".to_owned(),
        })
    });
    assert!(result.is_err());
    let returned = monotonic::now();
    drop(exclusive);
    drop(slot);
    // The file changed, so the swap counts as a replacement despite its error
    // (`engine/lifecycle` L8).
    assert_eq!(h.collection.current_generation().0, generation.0 + 1);
    let current = h.engine.collection().unwrap();
    let reader = current.reader(ReaderRole::Sync).unwrap();
    let snapshot = reader.snapshot().unwrap();
    assert_eq!(snapshot.col().unwrap().version, 18);
    loop {
        let events = h.collection.durability_events();
        if events.syncs[before..].iter().any(|s| s.succeeded) {
            break;
        }
        assert!(
            monotonic::now() - returned < Duration::from_secs(1),
            "reopened writer was never synced"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn vfs_shim_counts_full_wal_syncs_covering_twenty_answers() {
    use meguri_storage::counting_vfs::{CountingVfs, clock};
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let path = path.canonicalize().unwrap();
    let probe = CountingVfs::for_path(&path);
    let h = Harness::open_path(dir, path);
    let (_, cid) = common::steps::setup_note(&h.collection, "WAL sync answers");
    let lease = h.collection.reader(ReaderRole::Sync).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let original = snapshot.col().unwrap();
    let mut commits = Vec::new();
    for _ in 0..20 {
        common::steps::answer(&h.collection, None, cid).unwrap();
        commits.push(clock::now());
        thread::sleep(Duration::from_millis(60));
    }
    let wal = h.path.with_extension("anki2-wal").to_string_lossy().into_owned();
    let deadline = *commits.last().unwrap() + Duration::from_secs(1);
    let records = loop {
        let records = probe.records();
        if records
            .iter()
            .any(|r| r.filename == wal && r.flags & 0x0f == 3 && r.result == 0 && r.started >= *commits.last().unwrap())
        {
            break records;
        }
        assert!(
            clock::now() < deadline,
            "no full WAL sync after last answer: {records:?}"
        );
        thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(snapshot.col().unwrap(), original);
    let mut costs = Vec::new();
    for committed_at in commits {
        // SQLITE_OPEN_WAL=0x80000, SQLITE_SYNC_FULL=3, SQLITE_OK=0.
        // Match the actual filename and completion clock, never an executor
        // seq, sync event, or the file pointer chosen by sync_wal.
        let covering = records
            .iter()
            .find(|r| {
                r.filename == wal
                    && r.open_flags & 0x80000 != 0
                    && r.flags & 0x0f == 3
                    && r.result == 0
                    && r.started >= committed_at
            })
            .expect("full WAL sync");
        assert!(
            covering.completed - committed_at <= Duration::from_secs(1),
            "{covering:?}"
        );
        costs.push(covering.completed - covering.started);
    }
    costs.sort();
    println!(
        "U32 full WAL sync: p50={:?}, p95={:?}, VFS calls={}",
        costs[10],
        costs[19],
        records.len()
    );
}
