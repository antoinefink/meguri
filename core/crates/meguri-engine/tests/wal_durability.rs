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

// Production receipts: commit order and the test-only WalSync log are not proof.
fn covered(
    collection: &meguri_engine::Collection,
    identity: meguri_engine::WalIdentity,
    seq: meguri_engine::Seq,
    timeout: Duration,
) -> meguri_engine::WalReceipt {
    match collection.wait_wal_covered(identity, seq, timeout).unwrap() {
        meguri_engine::WalCoverage::Covered(receipt) => {
            assert_eq!(receipt.identity(), identity);
            assert!(receipt.through() >= seq);
            receipt
        }
        outcome => panic!("missing production WAL coverage for {seq:?}: {outcome:?}"),
    }
}

#[test]
fn production_receipt_auto_flush_matches_cloud_snapshot_and_native_full_sync() {
    use meguri_storage::counting_vfs::{CountingVfs, clock};
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let path = path.canonicalize().unwrap();
    let probe = CountingVfs::for_path(&path);
    let h = Harness::open_path(dir, path);
    let lease = h.collection.reader(ReaderRole::Cloud).unwrap();
    let old = lease.snapshot().unwrap();
    let before = old.col().unwrap();
    let (committed_tx, committed_rx) = std::sync::mpsc::channel();
    h.collection
        .after_next_commit(move |seq| committed_tx.send((seq, clock::now())).unwrap());
    let commit = add_note_with_card(&h.collection, OpSource::UserUndoable, &["receipt", "x"]);
    let (seq, committed_at) = committed_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(seq, commit.seq);
    let identity = old.wal_identity();
    covered(&h.collection, identity, commit.seq, Duration::from_secs(1));
    let receipt_at = clock::now();
    let wal = h.path.with_extension("anki2-wal").to_string_lossy().into_owned();
    assert!(probe.records().iter().any(|r| {
        r.filename == wal
            && r.open_flags & 0x80000 != 0
            && r.flags & 0x0f == 3
            && r.result == 0
            && r.started >= committed_at
            && r.completed <= receipt_at
    }));
    assert_eq!(old.col().unwrap(), before);
    drop(old);
    let snapshot = lease.snapshot().unwrap();
    assert_eq!(snapshot.seq(), commit.seq);
    assert_eq!(snapshot.wal_identity(), identity);
    covered(&h.collection, identity, snapshot.seq(), Duration::ZERO);
}

#[test]
fn production_receipt_never_uses_commit_seq_as_durability_and_rejects_writer_waits() {
    use meguri_engine::{Seq, WalCoverage};
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    // The commit hook freezes the writer after commit, before the auto flush.
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    h.collection.after_next_commit(move |seq| {
        entered_tx.send(seq).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let pending = h
        .collection
        .submit(OpSpec::named("add_note", OpSource::UserUndoable), |ctx| {
            ctx.scope().add_note(&common::note(&["not synced", "x"]))?;
            Ok(())
        });
    let seq = entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        h.collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap(),
        WalCoverage::Timeout
    );
    release_tx.send(()).unwrap();
    pending.wait().unwrap();
    covered(&h.collection, identity, seq, Duration::from_secs(1));
    let collection = h.collection.clone();
    h.collection
        .run(OpSpec::named("no_op", OpSource::Rollover), move |_| {
            assert!(matches!(
                collection.wait_wal_covered(identity, Seq(0), Duration::ZERO),
                Err(EngineError::InvalidArgument { .. })
            ));
            Ok(())
        })
        .unwrap();
}

#[test]
fn production_receipt_initial_no_write_and_event_seqs_do_not_wait_or_sync() {
    use meguri_engine::EventKind;
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    // Settle the open's rollover and its real WAL frames first.
    covered(&h.collection, identity, h.collection.last_seq(), Duration::from_secs(1));
    h.collection.record_durability_events();
    let noop = h
        .collection
        .run(OpSpec::named("no_op", OpSource::Rollover), |_| Ok(()))
        .unwrap();
    covered(&h.collection, identity, noop.seq, Duration::ZERO);
    let event = h
        .engine
        .stream()
        .publish(h.collection.generation(), None, EventKind::Available);
    covered(&h.collection, identity, event, Duration::ZERO);
    assert!(h.collection.durability_events().syncs.is_empty());
}

fn paused_wal_baseline(h: &Harness) -> meguri_engine::WalIdentity {
    h.collection.pause_automatic_wal_flush_for_testing(true);
    h.collection
        .run(
            OpSpec::named("rollover", OpSource::Rollover).stamp(meguri_storage::StampMode::Raw),
            |_| Ok(()),
        )
        .unwrap();
    h.engine.flush_for_background().unwrap();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    covered(&h.collection, identity, h.collection.last_seq(), Duration::ZERO);
    h.collection.record_durability_events();
    identity
}

#[test]
fn production_receipt_noop_keeps_prior_real_write_uncovered_until_explicit_sync() {
    use meguri_engine::WalCoverage;
    let h = Harness::open();
    let identity = paused_wal_baseline(&h);
    let baseline = h.collection.last_seq();
    let write = add_note_with_card(&h.collection, OpSource::UserUndoable, &["unsynced before noop", "x"]);
    assert!(write.seq > baseline);
    assert!(!write.changes.is_empty());
    let lease = h.collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = lease.snapshot().unwrap();
    assert_eq!(snapshot.seq(), write.seq);
    assert!(snapshot.note(write.value.0).unwrap().is_some());
    // Fail before the no-op if an automatic flush consumed the precondition.
    assert!(h.collection.durability_events().syncs.is_empty());
    assert_eq!(
        h.collection
            .wait_wal_covered(identity, write.seq, Duration::ZERO)
            .unwrap(),
        WalCoverage::Timeout
    );
    let noop = h
        .collection
        .run(OpSpec::named("no_op", OpSource::Rollover), |_| Ok(()))
        .unwrap();
    assert!(noop.seq > write.seq);
    assert!(noop.changes.is_empty());
    assert_eq!(noop.timing.rows, 0);
    for seq in [write.seq, noop.seq] {
        assert_eq!(
            h.collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap(),
            WalCoverage::Timeout
        );
    }
    assert!(h.collection.durability_events().syncs.is_empty());
    h.engine.flush_for_background().unwrap();
    covered(&h.collection, identity, write.seq, Duration::ZERO);
    covered(&h.collection, identity, noop.seq, Duration::ZERO);
}

#[test]
fn production_receipt_noop_keeps_unsequenced_maintenance_uncovered_until_explicit_sync() {
    use meguri_engine::WalCoverage;
    let h = Harness::open();
    let identity = paused_wal_baseline(&h);
    let baseline = h.collection.last_seq();
    let clock = h.clock.clone();
    let id = h
        .collection
        .submit_maintenance(catalog::WAL_CHECKPOINT, Duration::ZERO, move |writer| {
            let mut scope = writer.begin_write(meguri_storage::StampMode::Raw, clock.as_ref(), None)?;
            let id = scope.add_note(&common::note(&["unsequenced before noop", "x"]))?;
            let committed = scope.commit()?;
            assert!(committed.changed);
            assert!(committed.post_commit_error.is_none());
            Ok(id)
        })
        .wait()
        .unwrap();
    let lease = h.collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = lease.snapshot().unwrap();
    assert_eq!(snapshot.seq(), baseline, "maintenance takes no op seq");
    assert!(snapshot.note(id).unwrap().is_some());
    assert!(h.collection.durability_events().syncs.is_empty());
    // Even the previously covered seq must wait for the unsequenced write.
    assert_eq!(
        h.collection
            .wait_wal_covered(identity, baseline, Duration::ZERO)
            .unwrap(),
        WalCoverage::Timeout
    );
    let noop = h
        .collection
        .run(OpSpec::named("no_op", OpSource::Rollover), |_| Ok(()))
        .unwrap();
    assert!(noop.seq > baseline);
    assert!(noop.changes.is_empty());
    assert_eq!(noop.timing.rows, 0);
    for seq in [baseline, noop.seq] {
        assert_eq!(
            h.collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap(),
            WalCoverage::Timeout
        );
    }
    assert!(h.collection.durability_events().syncs.is_empty());
    h.engine.flush_for_background().unwrap();
    covered(&h.collection, identity, baseline, Duration::ZERO);
    covered(&h.collection, identity, noop.seq, Duration::ZERO);
}

#[test]
fn production_receipt_failed_sync_wakes_waiters_without_coverage_then_explicit_flush_recovers() {
    use meguri_engine::WalCoverage;
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    h.collection.after_next_commit(move |seq| {
        entered_tx.send(seq).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    h.collection.fail_wal_sync_after_next_commit_for_testing();
    let pending = h
        .collection
        .submit(OpSpec::named("add_note", OpSource::UserUndoable), |ctx| {
            ctx.scope().add_note(&common::note(&["failure receipt", "x"]))?;
            Ok(())
        });
    let seq = entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    h.collection.notify_next_wal_wait_for_testing(waiting_tx);
    let collection = h.collection.clone();
    let waiter = thread::spawn(move || {
        collection
            .wait_wal_covered(identity, seq, Duration::from_secs(10))
            .unwrap()
    });
    waiting_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    release_tx.send(()).unwrap();
    pending.wait().unwrap();
    assert_eq!(waiter.join().unwrap(), WalCoverage::Unavailable);
    h.engine.flush_for_background().unwrap();
    covered(&h.collection, identity, seq, Duration::ZERO);
}

#[test]
fn production_receipt_close_suspend_resume_reopen_and_replacement_are_file_bound() {
    use meguri_engine::WalCoverage;
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    let commit = add_note_with_card(&h.collection, OpSource::UserUndoable, &["suspend receipt", "x"]);
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    covered(&h.collection, identity, commit.seq, Duration::ZERO);
    h.engine.resume().unwrap();
    let resumed = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    assert_eq!(resumed, identity);
    covered(&h.collection, resumed, commit.seq, Duration::ZERO);
    let replacement = h.dir.path().join("cloud-replacement.anki2");
    std::fs::copy(common::fixture("schema-18.anki2"), &replacement).unwrap();
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    exclusive
        .replace(move |live| {
            std::fs::rename(replacement, live)
                .map_err(meguri_storage::StorageError::from)
                .map_err(EngineError::from)
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    assert_eq!(
        h.collection
            .wait_wal_covered(identity, commit.seq, Duration::ZERO)
            .unwrap(),
        WalCoverage::Unavailable
    );
    assert!(matches!(
        h.collection.reader(ReaderRole::Cloud),
        Err(EngineError::StaleGeneration { .. })
    ));
    let current = h.engine.collection().unwrap();
    let new_identity = current
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    assert_ne!(new_identity, identity);
    assert_ne!(new_identity.native_file(), identity.native_file());
    assert_eq!(
        current.wait_wal_covered(identity, commit.seq, Duration::ZERO).unwrap(),
        WalCoverage::Unavailable
    );
    let final_commit = add_note_with_card(&current, OpSource::UserUndoable, &["close receipt", "x"]);
    h.engine.close().unwrap();
    covered(&current, new_identity, final_commit.seq, Duration::ZERO);
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    let reopened_identity = reopened
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    assert_ne!(new_identity, reopened_identity);
    assert_eq!(
        current
            .wait_wal_covered(new_identity, final_commit.seq, Duration::ZERO)
            .unwrap(),
        WalCoverage::Unavailable
    );
    covered(
        &reopened,
        reopened_identity,
        reopened.last_seq(),
        Duration::from_secs(1),
    );
}

#[test]
fn production_receipt_pending_waits_wake_on_close_and_generation_change() {
    use meguri_engine::{Seq, WalCoverage};
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    h.collection.notify_next_wal_wait_for_testing(waiting_tx);
    let collection = h.collection.clone();
    // A not-yet-taken seq cannot be covered by any flush or clean close.
    let waiter = thread::spawn(move || {
        collection
            .wait_wal_covered(identity, Seq(u64::MAX), Duration::from_secs(10))
            .unwrap()
    });
    waiting_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    h.engine.close().unwrap();
    assert_eq!(waiter.join().unwrap(), WalCoverage::Unavailable);
    let current = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_ne!(current.generation(), identity.generation());
    assert_eq!(
        current.wait_wal_covered(identity, Seq(0), Duration::ZERO).unwrap(),
        WalCoverage::Unavailable
    );
}

#[test]
fn production_receipt_timeout_is_bounded_and_does_not_block_writer() {
    use meguri_engine::{Seq, WalCoverage};
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    h.collection.notify_next_wal_wait_for_testing(waiting_tx);
    let collection = h.collection.clone();
    let waiter = thread::spawn(move || {
        collection
            .wait_wal_covered(identity, Seq(u64::MAX), Duration::from_millis(100))
            .unwrap()
    });
    waiting_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let commit = add_note_with_card(&h.collection, OpSource::UserUndoable, &["while waiting", "x"]);
    assert_eq!(waiter.join().unwrap(), WalCoverage::Timeout);
    covered(&h.collection, identity, commit.seq, Duration::from_secs(1));
}

#[test]
fn production_receipt_rejects_another_native_file_even_at_the_same_generation() {
    use meguri_engine::WalCoverage;
    let first = Harness::open();
    let second = Harness::open();
    assert_eq!(first.collection.generation(), second.collection.generation());
    let foreign = second
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    assert_eq!(
        first
            .collection
            .wait_wal_covered(foreign, first.collection.last_seq(), Duration::ZERO)
            .unwrap(),
        WalCoverage::Unavailable
    );
}

#[test]
fn production_receipt_failed_close_checkpoint_wakes_pending_waiters() {
    use meguri_engine::{Seq, WalCoverage};
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    // A connection outside engine ownership prevents its TRUNCATE checkpoint.
    let external = meguri_storage::raw::PlainDatabase::open(&h.path).unwrap();
    external.execute_batch("BEGIN DEFERRED").unwrap();
    external.rows("SELECT mod FROM col", &[]).unwrap();
    add_note_with_card(&h.collection, OpSource::UserUndoable, &["blocked checkpoint", "x"]);
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    h.collection.notify_next_wal_wait_for_testing(waiting_tx);
    let collection = h.collection.clone();
    let waiter = thread::spawn(move || {
        collection
            .wait_wal_covered(identity, Seq(u64::MAX), Duration::from_secs(10))
            .unwrap()
    });
    waiting_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(
        h.engine.close(),
        Err(EngineError::CollectionUnavailable { reason })
            if reason == "checkpoint blocked by an external reader"
    ));
    assert_eq!(waiter.join().unwrap(), WalCoverage::Unavailable);
    external.execute_batch("ROLLBACK").unwrap();
    drop(external);
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    let identity = reopened
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    covered(&reopened, identity, reopened.last_seq(), Duration::from_secs(1));
}

#[test]
fn production_receipt_refuses_sync_attribution_while_the_native_path_is_stale() {
    use meguri_engine::WalCoverage;
    let h = Harness::open();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    h.collection.after_next_commit(move |seq| {
        entered_tx.send(seq).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    });
    let pending = h
        .collection
        .submit(OpSpec::named("add_note", OpSource::UserUndoable), |ctx| {
            ctx.scope().add_note(&common::note(&["stale path", "x"]))?;
            Ok(())
        });
    let seq = entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // Only disposable fixture files: simulate a foreign pathname replacement,
    // restoring the retained file before any connection is closed.
    let retained = h.dir.path().join("retained.anki2");
    std::fs::rename(&h.path, &retained).unwrap();
    std::fs::copy(common::fixture("schema-18.anki2"), &h.path).unwrap();
    release_tx.send(()).unwrap();
    pending.wait().unwrap();
    assert!(h.engine.flush_for_background().is_err());
    assert_eq!(
        h.collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap(),
        WalCoverage::Unavailable
    );
    std::fs::rename(retained, &h.path).unwrap();
    h.engine.flush_for_background().unwrap();
    covered(&h.collection, identity, seq, Duration::ZERO);
}

#[test]
fn production_receipt_is_available_to_an_initial_open_step_before_the_first_op() {
    struct InitialProof;
    impl meguri_engine::OpenStep for InitialProof {
        fn name(&self) -> &str {
            "initial-WAL-proof"
        }
        fn run(&self, collection: &meguri_engine::Collection) -> Result<(), EngineError> {
            let lease = collection.reader(ReaderRole::Cloud)?;
            let snapshot = lease.snapshot()?;
            covered(
                collection,
                snapshot.wal_identity(),
                snapshot.seq(),
                Duration::from_secs(1),
            );
            Ok(())
        }
    }
    let h = Harness::open_with(meguri_engine::Ports::default().open_step(std::sync::Arc::new(InitialProof)));
    h.engine.close().unwrap();
}

#[test]
fn production_receipt_covers_maintenance_rows_without_an_op_seq() {
    let h = Harness::open();
    let seq = h.collection.last_seq();
    let identity = h
        .collection
        .reader(ReaderRole::Cloud)
        .unwrap()
        .snapshot()
        .unwrap()
        .wal_identity();
    covered(&h.collection, identity, seq, Duration::from_secs(1));
    let (committed_tx, committed_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let clock = h.clock.clone();
    let pending = h
        .collection
        .submit_maintenance(catalog::WAL_CHECKPOINT, Duration::ZERO, move |writer| {
            let mut scope = writer.begin_write(meguri_storage::StampMode::Raw, clock.as_ref(), None)?;
            let note = common::note(&["maintenance WAL", "x"]);
            let id = scope.add_note(&note)?;
            scope.commit()?;
            committed_tx.send(id).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        });
    let id = committed_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let lease = h.collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = lease.snapshot().unwrap();
    assert_eq!(snapshot.seq(), seq, "maintenance takes no op seq");
    assert!(snapshot.note(id).unwrap().is_some());
    assert_eq!(
        h.collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap(),
        meguri_engine::WalCoverage::Timeout
    );
    release_tx.send(()).unwrap();
    pending.wait().unwrap();
    covered(
        &h.collection,
        snapshot.wal_identity(),
        snapshot.seq(),
        Duration::from_secs(1),
    );
}
