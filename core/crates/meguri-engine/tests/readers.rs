//! Readers by role (KTD3, `engine/lifecycle` L11, `engine/op-model` O6): one
//! user per reader, snapshots that know their seq, and a sync-length
//! snapshot that never delays search, stats, backups or interactive reads.

mod common;

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::{Harness, add_note_with_card};
use meguri_engine::{EngineError, monotonic};
use meguri_model::changes::OpSource;
use meguri_model::ids::CardId;
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_testkit::bench::fixtures;

#[test]
fn a_reader_serves_one_user_at_a_time_and_roles_never_wait_for_each_other() {
    let h = Harness::open();
    let sync = h.collection.reader(ReaderRole::Sync).unwrap();
    // Other roles, and each search worker and server reader, are free.
    for role in [ReaderRole::Interactive, ReaderRole::Query, ReaderRole::Maintenance] {
        drop(h.collection.reader(role).unwrap());
    }
    let (a, b) = (
        h.collection.search_reader(0).unwrap(),
        h.collection.search_reader(3).unwrap(),
    );
    drop((a, b));
    let (s0, s1) = (
        h.collection.server_reader(0).unwrap(),
        h.collection.server_reader(1).unwrap(),
    );
    drop((s0, s1));
    assert!(matches!(
        h.collection.search_reader(4),
        Err(EngineError::InvalidArgument { .. })
    ));

    // A second user of the sync reader waits for the first.
    let (tx, rx) = mpsc::channel();
    let collection = h.collection.clone();
    let waiter = thread::spawn(move || {
        let lease = collection.reader(ReaderRole::Sync).unwrap();
        tx.send(lease.role()).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "it waits");
    drop(sync);
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), ReaderRole::Sync);
    waiter.join().unwrap();
}

#[test]
fn a_snapshot_knows_the_seq_of_the_last_commit_it_sees() {
    let h = Harness::open();
    let first = add_note_with_card(&h.collection, OpSource::UserUndoable, &["a", "b"]);
    let lease = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let snapshot = lease.snapshot().unwrap();
    assert_eq!(snapshot.seq(), first.seq);
    assert_eq!(snapshot.generation(), h.collection.generation());
    let second = add_note_with_card(&h.collection, OpSource::UserUndoable, &["c", "d"]);
    assert!(snapshot.note(second.value.0).unwrap().is_none());
    assert!(snapshot.note(first.value.0).unwrap().is_some());
    assert_eq!(h.collection.last_seq(), second.seq);
}

/// The p-th percentile of `samples` (the smallest with fewer than 100-p%
/// above it).
fn percentile(mut samples: Vec<Duration>, p: usize) -> Duration {
    samples.sort();
    let rank = (samples.len() * p / 100 + 1).min(samples.len());
    samples[rank - 1]
}

#[test]
fn a_sync_snapshot_never_delays_search_stats_or_backups_and_interactive_reads_stay_fast_at_150k() {
    let generated = fixtures::profile_collection("150k").expect("the 150k collection");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(generated.dir.join("collection.anki2"), &path).unwrap();
    let h = Harness::open_path(dir, path);
    let collection = &h.collection;
    let card_ids: Vec<i64> = collection
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(Table::Cards, None, usize::MAX)
        .unwrap();
    assert!(card_ids.len() >= 140_000, "{} cards", card_ids.len());

    // A normal sync's snapshot, held on the sync reader for the whole test.
    let sync = collection.reader(ReaderRole::Sync).unwrap();
    let snapshot = sync.snapshot().unwrap();
    let notes_seen =
        |s: &meguri_engine::Snapshot<'_>| match &s.execute_raw("SELECT count(*) FROM notes", &[]).unwrap().rows[0][0] {
            SqlValue::Integer(n) => *n,
            other => panic!("{other:?}"),
        };
    let notes_at_snapshot = notes_seen(&snapshot);
    add_note_with_card(collection, OpSource::UserUndoable, &["during sync", "x"]);

    let backup = h.dir.path().join("backup.anki2");
    let (search_time, stats_time, backup_time) = thread::scope(|scope| {
        let search = scope.spawn(|| {
            let reader = collection.search_reader(0).unwrap();
            let started = monotonic::now();
            let mut scans = 0;
            let mut after = i64::MIN;
            while monotonic::now() - started < Duration::from_millis(300) {
                // Bound each chunk: a final full-table scan could overshoot
                // the 300 ms workload by seconds while VACUUM competes for
                // I/O. Keep the existing latency limits below unchanged.
                let rows = reader
                    .execute_raw(
                        "SELECT max(id), sum(flds LIKE '%zq%') FROM \
                     (SELECT id, flds FROM notes WHERE id > ?1 ORDER BY id LIMIT 1000)",
                        &[SqlValue::Integer(after)],
                    )
                    .unwrap();
                after = match rows.rows[0][0] {
                    SqlValue::Integer(id) => id,
                    _ => i64::MIN, // wrap after the collection's last page
                };
                scans += 1;
            }
            assert!(scans > 0);
            monotonic::now() - started
        });
        let maintenance = scope.spawn(|| {
            let reader = collection.reader(ReaderRole::Maintenance).unwrap();
            let started = monotonic::now();
            // A stats-cache-sized build: every review-log row and every card.
            reader
                .execute_raw("SELECT cid, count(*), sum(time) FROM revlog GROUP BY cid", &[])
                .unwrap();
            let mut after = None;
            loop {
                let page = reader.cards_after(after, 5_000).unwrap();
                let Some(last) = page.last() else { break };
                after = Some(last.id);
            }
            let stats = monotonic::now() - started;
            let started = monotonic::now();
            reader
                .execute_raw("VACUUM INTO ?1", &[SqlValue::Text(backup.display().to_string())])
                .unwrap();
            (stats, monotonic::now() - started)
        });

        // Meanwhile, the review screen and the browser read.
        let interactive = collection.reader(ReaderRole::Interactive).unwrap();
        let query = collection.reader(ReaderRole::Query).unwrap();
        let random = SeededRandom::new(11);
        let mut card_reads = Vec::new();
        let mut page_reads = Vec::new();
        while !(search.is_finished() && maintenance.is_finished()) || card_reads.len() < 1_000 {
            let id = card_ids[(random.next_u64() % card_ids.len() as u64) as usize];
            let started = monotonic::now();
            interactive.card(CardId(id)).unwrap().unwrap();
            card_reads.push(monotonic::now() - started);
            if card_reads.len() % 10 == 0 {
                let from = card_ids[(random.next_u64() % (card_ids.len() as u64 - 200)) as usize];
                let started = monotonic::now();
                let page = query.cards_after(Some(CardId(from)), 200).unwrap();
                page_reads.push(monotonic::now() - started);
                assert_eq!(page.len(), 200);
            }
        }
        let card_p95 = percentile(card_reads, 95);
        let page_p95 = percentile(page_reads, 95);
        assert!(
            card_p95 <= Duration::from_millis(1),
            "interactive card read p95 {card_p95:?}"
        );
        assert!(
            page_p95 <= Duration::from_millis(16),
            "200-row page read p95 {page_p95:?}"
        );
        let (stats, backup) = maintenance.join().unwrap();
        (search.join().unwrap(), stats, backup)
    });

    // None of them waited for the sync, whose view never moved.
    assert!(search_time < Duration::from_secs(2), "search {search_time:?}");
    assert!(stats_time < Duration::from_secs(20), "stats build {stats_time:?}");
    assert!(backup_time < Duration::from_secs(20), "backup {backup_time:?}");
    assert!(backup.metadata().unwrap().len() > 0);
    assert_eq!(
        notes_seen(&snapshot),
        notes_at_snapshot,
        "the sync's snapshot held throughout"
    );
}

#[test]
fn cloud_has_an_independent_query_only_reader_and_releases_it_for_suspend() {
    let h = Harness::open();
    let sync = h.collection.reader(ReaderRole::Sync).unwrap();
    let backup = h.collection.backup_reader().unwrap();
    let cloud = h.collection.reader(ReaderRole::Cloud).unwrap();
    assert_eq!(cloud.role(), ReaderRole::Cloud);
    assert_eq!(
        cloud.execute_raw("PRAGMA query_only", &[]).unwrap().rows[0][0],
        SqlValue::Integer(1)
    );
    assert!(
        cloud
            .execute_raw("CREATE TEMP TABLE cloud_must_not_write(x)", &[])
            .is_err()
    );
    assert!(cloud.execute_raw("UPDATE col SET mod = mod + 1", &[]).is_err());
    let snapshot = cloud.snapshot().unwrap();
    assert_eq!(snapshot.wal_identity().generation(), h.collection.generation());
    drop(snapshot);
    drop((cloud, sync, backup));
    h.engine.suspend(Duration::from_secs(2)).unwrap();
    assert!(h.collection.reader(ReaderRole::Cloud).is_err());
    h.engine.resume().unwrap();
    assert_eq!(
        h.collection.reader(ReaderRole::Cloud).unwrap().role(),
        ReaderRole::Cloud
    );
}

#[test]
fn cloud_reader_opens_lazily_once_without_warming_other_roles_and_never_leases_on_writer() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let h = Harness::open();
    h.engine.close().unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    let mut initializers = meguri_storage::Initializers::new();
    let count = opens.clone();
    initializers.register(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let collection = h
        .engine
        .open_raw(&h.path, common::open_config(&h.clock).initializers(initializers))
        .unwrap();
    let baseline = opens.load(Ordering::SeqCst);
    let sync = collection.reader(ReaderRole::Sync).unwrap();
    let backup = collection.backup_reader().unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), baseline + 2);
    let cloud = collection.reader(ReaderRole::Cloud).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), baseline + 3);
    let current = collection.clone();
    collection
        .run(meguri_engine::OpSpec::named("no_op", OpSource::Rollover), move |_| {
            assert!(matches!(
                current.reader(ReaderRole::Cloud),
                Err(EngineError::InvalidArgument { .. })
            ));
            Ok(())
        })
        .unwrap();
    drop(cloud);
    drop(collection.reader(ReaderRole::Cloud).unwrap());
    assert_eq!(opens.load(Ordering::SeqCst), baseline + 3);
    drop((sync, backup));
    h.engine.close().unwrap();
}

#[test]
fn cloud_capture_pins_its_own_role_and_rejects_sync_without_consuming_it() {
    use meguri_engine::OpSpec;
    use meguri_storage::RowKey;
    let h = Harness::open();
    let c = &h.collection;
    let first = add_note_with_card(c, OpSource::UserUndoable, &["first", "back"]);
    c.run(OpSpec::named("settle", OpSource::Rollover), |_| Ok(())).unwrap();
    let mut sync = c.reader(ReaderRole::Sync).unwrap();
    assert!(matches!(
        sync.capture_cloud(c, Vec::new()),
        Err(EngineError::InvalidArgument { .. })
    ));
    let manual = sync.snapshot().unwrap();
    let mut cloud = c.reader(ReaderRole::Cloud).unwrap();
    let before = c.last_seq();
    let (snapshot, hold) = cloud
        .capture_cloud(c, vec![(Table::Notes, RowKey::Id(first.value.0.0))])
        .unwrap();
    assert_eq!(hold.snapshot, before);
    assert_eq!(snapshot.seq(), before);
    assert_eq!(
        snapshot.wal_identity().native_file(),
        c.open_evidence().unwrap().native_file
    );
    assert_eq!(snapshot.generation(), c.generation());
    assert!(!hold.flagged.is_empty());
    let second = add_note_with_card(c, OpSource::UserUndoable, &["second", "back"]);
    assert!(snapshot.note(first.value.0).unwrap().is_some());
    assert!(snapshot.note(second.value.0).unwrap().is_none());
    assert!(manual.note(second.value.0).unwrap().is_none());
    drop((snapshot, manual));
    drop((cloud, sync));
    assert!(
        c.reader(ReaderRole::Cloud)
            .unwrap()
            .note(second.value.0)
            .unwrap()
            .is_some()
    );
}

#[test]
fn lifetime_pin_refuses_non_cloud_and_owned_sync_capture_without_consuming_the_lease() {
    let h = Harness::open();
    let lease = h.collection.reader(ReaderRole::Sync).unwrap();
    let identity = lease.native_identity();
    let (lease, error) = lease.into_lifetime_pin().err().expect("non-Cloud refused");
    assert!(matches!(error, EngineError::InvalidArgument { .. }));
    assert_eq!(lease.role(), ReaderRole::Sync);
    assert_eq!(lease.native_identity(), identity);
    assert_eq!(lease.snapshot().unwrap().generation(), h.collection.generation());
    drop(lease);

    let mut sync = h.collection.reader(ReaderRole::Sync).unwrap();
    let (snapshot, _hold) = sync.capture_sync(&h.collection).unwrap();
    let (mut sync, error) = sync.into_lifetime_pin().err().expect("captured Sync refused");
    assert!(matches!(error, EngineError::InvalidArgument { .. }));
    assert!(
        sync.capture_sync(&h.collection).is_err(),
        "the original lease remains empty"
    );
    assert!(snapshot.col().is_ok(), "the owned snapshot stays usable");
    drop(snapshot);
    drop(sync);
    assert_eq!(h.collection.reader(ReaderRole::Sync).unwrap().role(), ReaderRole::Sync);

    let mut cloud = h.collection.reader(ReaderRole::Cloud).unwrap();
    let (snapshot, _hold) = cloud.capture_cloud(&h.collection, Vec::new()).unwrap();
    assert!(snapshot.col().is_ok());
    drop(snapshot);
    let (mut cloud, error) = cloud.into_lifetime_pin().err().expect("captured Cloud refused");
    assert!(matches!(error, EngineError::InvalidArgument { .. }));
    assert!(
        cloud.capture_cloud(&h.collection, Vec::new()).is_err(),
        "the original Cloud lease remains empty after its borrowed snapshot drops"
    );
    drop(cloud);
    assert_eq!(
        h.collection.reader(ReaderRole::Cloud).unwrap().role(),
        ReaderRole::Cloud
    );
}
