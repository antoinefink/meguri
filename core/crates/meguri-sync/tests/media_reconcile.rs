//! Scripted production reconciliation, on generated engine collections.
mod common;
mod media_files_support;
use meguri_engine::{CancelToken, Collection, Engine, JobKind, OpSource, OpSpec, media::MediaService};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::{RandomSource, SeededRandom},
};
use meguri_storage::media::{MediaBaselineOwner, MediaSyncState};
use meguri_sync::{
    Job, JobOptions, Method, Payload, SyncError,
    media::{self, DownloadWindow, MediaFailure, MediaPhase, MediaSnapshot, MediaSyncResult},
    transport::{BoxFuture, SyncTransport, TransportRequest, TransportResponse},
};
use meguri_testkit::sync_stub::{Reply, SyncStub};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Harness {
    engine: Engine,
    collection: Collection,
    media: MediaService,
    random: Arc<SeededRandom>,
}
fn with_collection(test: impl FnOnce(Harness)) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("collection");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("collection.anki2");
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(111));
    meguri_engine::bootstrap::create(&path, clock.as_ref(), random.as_ref()).unwrap();
    let host = meguri_testkit::bench::fixtures::open_engine_collection(&path, clock).unwrap();
    let h = Harness {
        engine: host.engine().clone(),
        collection: host.collection().unwrap(),
        media: host.media().unwrap(),
        random,
    };
    test(h);
    host.close().unwrap();
}
impl Harness {
    fn run(
        &self,
        stub: &SyncStub,
        progress: impl FnMut(&MediaSnapshot, &CancelToken) + Send + 'static,
    ) -> Result<MediaSyncResult, Box<MediaFailure>> {
        self.run_using(
            common::options(&stub.url()),
            |options, random| Job::new(options, Some(meguri_sync::HostKey::new("synthetic")), random).unwrap(),
            progress,
        )
    }
    fn run_using(
        &self,
        options: JobOptions,
        make_job: impl FnOnce(&JobOptions, &dyn RandomSource) -> Job + Send + 'static,
        mut progress: impl FnMut(&MediaSnapshot, &CancelToken) + Send + 'static,
    ) -> Result<MediaSyncResult, Box<MediaFailure>> {
        let service = self.media.clone();
        let generation = self.collection.generation().0;
        let engine = self.engine.clone();
        let random = self.random.clone();
        let (send, receive) = std::sync::mpsc::channel();
        self.engine
            .start_job(JobKind::MediaSync, move |context| {
                let options = options.cancel(context.cancel.clone());
                let mut job = make_job(&options, random.as_ref());
                let result = media::reconcile(
                    engine.threads(),
                    &mut job,
                    &service,
                    generation,
                    &MediaBaselineOwner::new("configured-owner-111"),
                    &mut |snapshot| progress(snapshot, &context.cancel),
                );
                send.send(result).unwrap();
                Ok(())
            })
            .unwrap();
        receive.recv().unwrap()
    }
}
fn envelope(data: Value) -> Reply {
    Reply::ok(&serde_json::to_vec(&json!({"data": data,"err":""})).unwrap())
}
fn begin(usn: u64) -> Reply {
    envelope(json!({"usn": usn, "sk":"synthetic"}))
}
fn names(range: std::ops::Range<usize>) -> Vec<String> {
    range.map(|i| format!("file-{i:03}.png")).collect()
}
fn changes(names: &[String], first_usn: usize) -> Reply {
    envelope(Value::Array(
        names
            .iter()
            .enumerate()
            .map(|(i, n)| json!([n, first_usn + i, sha(b"payload")]))
            .collect(),
    ))
}
fn sha(bytes: &[u8]) -> String {
    media_files_support::content(bytes)
        .sha1
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn download(names: &[String]) -> Reply {
    let entry_names: Vec<_> = (0..names.len()).map(|i| i.to_string()).collect();
    let meta: BTreeMap<_, _> = entry_names.iter().zip(names).collect();
    let meta = serde_json::to_vec(&meta).unwrap();
    let mut entries: Vec<(&str, &[u8], bool)> = entry_names
        .iter()
        .map(|n| (n.as_str(), b"payload".as_slice(), false))
        .collect();
    entries.push(("_meta", &meta, false));
    Reply::ok(&media_files_support::zip(&entries))
}
fn paths(stub: &SyncStub) -> Vec<String> {
    stub.requests().iter().map(|r| r.head.path().to_owned()).collect()
}
fn requested_names(stub: &SyncStub) -> Vec<Vec<String>> {
    stub.requests()
        .iter()
        .filter(|r| r.head.path().ends_with("downloadFiles"))
        .map(|r| {
            serde_json::from_slice::<Value>(r.body.as_ref().unwrap()).unwrap()["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_str().unwrap().into())
                .collect()
        })
        .collect()
}
fn uploaded(stub: &SyncStub) -> Vec<Vec<u8>> {
    stub.requests()
        .into_iter()
        .filter(|r| r.head.path().ends_with("uploadChanges"))
        .map(|r| {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(r.body.unwrap())).unwrap();
            let mut bytes = Vec::new();
            archive.by_name("0").unwrap().read_to_end(&mut bytes).unwrap();
            bytes
        })
        .collect()
}

#[test]
fn same_owner_unchanged_media_stage_skips_listing_and_hashing() {
    with_collection(|h| {
        let store = h.media.store();
        store
            .bind_sync_owner(&MediaBaselineOwner::new("configured-owner-111"))
            .unwrap();
        h.media.add("cached.png", b"cached").unwrap();
        store.check_changes_report().unwrap();
        let entry = store.sync_entry("cached.png").unwrap().unwrap();
        assert!(store.acknowledge(&entry.name, entry.change_counter).unwrap());
        let before = store.diagnostics();
        let marker = store.change_marker().unwrap();
        assert!(marker.is_some());
        let counter = store.change_counter().unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(0),
            _ => panic!("unchanged media must only begin"),
        });
        let result = h
            .run(&stub, |snapshot, _| assert_eq!(snapshot.totals.checked, 0))
            .unwrap();
        assert_eq!(result, MediaSyncResult::default());
        let report = store.last_check_report().unwrap().unwrap();
        assert!(report.completed);
        assert_eq!((report.listed, report.seen, report.hashed), (false, 0, 0));
        assert_eq!(store.diagnostics(), before);
        assert_eq!(store.change_marker().unwrap(), marker);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(paths(&stub), ["/msync/begin"]);
    });
}

#[test]
fn missing_media_folder_stage_fails_with_fresh_zero_counts_and_resumes() {
    with_collection(|h| {
        let store = h.media.store();
        store
            .bind_sync_owner(&MediaBaselineOwner::new("configured-owner-111"))
            .unwrap();
        fs::write(store.media_dir().join("cached.png"), b"cached").unwrap();
        let successful = store.register_full_report(|_, _| true).unwrap();
        assert!(successful.completed && successful.listed);
        assert_eq!((successful.seen, successful.hashed, successful.added), (1, 1, 1));
        let entry = store.sync_entry("cached.png").unwrap().unwrap();
        assert!(store.acknowledge(&entry.name, entry.change_counter).unwrap());
        let before = store.diagnostics();
        let marker = store.change_marker().unwrap();
        assert!(marker.is_some());
        let counter = store.change_counter().unwrap();
        let cached = store.sync_entry("cached.png").unwrap();
        let moved = store.media_dir().with_file_name("missing-media");
        fs::rename(store.media_dir(), &moved).unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(0),
            _ => panic!("restored unchanged media must only begin"),
        });
        let failure = h
            .run(&stub, |snapshot, _| {
                assert_eq!(snapshot.totals, MediaSyncResult::default())
            })
            .unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::LocalStorage);
        assert_eq!(failure.snapshot.phase, MediaPhase::Checking);
        assert_eq!(failure.snapshot.totals, MediaSyncResult::default());
        assert!(stub.requests().is_empty());
        let failed = store.last_check_report().unwrap().unwrap();
        assert_ne!(failed, successful);
        assert!(!failed.completed && !failed.listed);
        assert_eq!(
            (
                failed.seen,
                failed.hashed,
                failed.added,
                failed.changed,
                failed.removed,
                failed.renamed,
                failed.skipped,
            ),
            (0, 0, 0, 0, 0, 0, 0)
        );
        assert_eq!(store.diagnostics(), before);
        assert_eq!(store.change_marker().unwrap(), marker);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(store.sync_entry("cached.png").unwrap(), cached);
        assert!(!store.pending_any().unwrap());
        fs::rename(moved, store.media_dir()).unwrap();
        assert_eq!(h.run(&stub, |_, _| {}).unwrap(), MediaSyncResult::default());
        let restored = store.last_check_report().unwrap().unwrap();
        assert!(restored.completed);
        assert_eq!((restored.listed, restored.seen, restored.hashed), (false, 0, 0));
        assert_eq!(store.diagnostics(), before);
        assert_eq!(store.change_marker().unwrap(), marker);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(store.sync_entry("cached.png").unwrap(), cached);
        assert!(!store.pending_any().unwrap());
        assert_eq!(paths(&stub), ["/msync/begin"]);
    });
}

#[test]
fn changed_folder_media_stage_finds_and_uploads_an_external_add() {
    with_collection(|h| {
        let store = h.media.store();
        store
            .bind_sync_owner(&MediaBaselineOwner::new("configured-owner-111"))
            .unwrap();
        h.media.add("cached.png", b"cached").unwrap();
        store.check_changes_report().unwrap();
        let entry = store.sync_entry("cached.png").unwrap().unwrap();
        assert!(store.acknowledge(&entry.name, entry.change_counter).unwrap());
        let cached = store.sync_entry("cached.png").unwrap().unwrap();
        let before = store.diagnostics();
        let marker = store.change_marker().unwrap();
        let directory = fs::File::open(store.media_dir()).unwrap();
        let previous = directory.metadata().unwrap().modified().unwrap();
        fs::write(store.media_dir().join("outside.png"), b"outside").unwrap();
        directory
            .set_times(fs::FileTimes::new().set_modified(previous + Duration::from_secs(1)))
            .unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(0),
            1 => envelope(json!([1, 1])),
            2 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.checked, 2);
        assert_eq!(result.uploaded_files, 1);
        assert_eq!(result.bytes_uploaded, 7);
        assert_eq!(uploaded(&stub), [b"outside".to_vec()]);
        let report = store.last_check_report().unwrap().unwrap();
        assert!(report.completed && report.listed);
        assert_eq!((report.seen, report.hashed, report.added), (2, 1, 1));
        assert_eq!(store.diagnostics().scans, before.scans + 1);
        assert_eq!(store.diagnostics().hashes, before.hashes + 1);
        assert_ne!(store.change_marker().unwrap(), marker);
        assert_eq!(store.sync_entry("cached.png").unwrap(), Some(cached));
        let outside = store.sync_entry("outside.png").unwrap().unwrap();
        assert_eq!(outside.sha1, Some(sha(b"outside")));
        assert_eq!(outside.sync_state, MediaSyncState::Live);
        assert!(!store.pending_any().unwrap());
    });
}

#[test]
fn media_stage_reset_or_rebind_requires_one_comparison_and_reuses_file_facts() {
    for rebind in [false, true] {
        with_collection(|h| {
            let store = h.media.store();
            let owner = if rebind {
                "previous-owner"
            } else {
                "configured-owner-111"
            };
            store.bind_sync_owner(&MediaBaselineOwner::new(owner)).unwrap();
            h.media.add("cached.png", b"cached").unwrap();
            store.check_changes_report().unwrap();
            let cached = store.sync_entry("cached.png").unwrap().unwrap();
            assert!(store.acknowledge(&cached.name, cached.change_counter).unwrap());
            let before = store.diagnostics();
            let marker = store.change_marker().unwrap();
            assert!(marker.is_some());
            if !rebind {
                store.reset_sync().unwrap();
                assert!(store.change_marker().unwrap().is_none());
                assert_eq!(store.sync_entry("cached.png").unwrap().unwrap().sha1, cached.sha1);
            }
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(0),
                1 => envelope(json!([1, 1])),
                2 => envelope(json!("OK")),
                3 => begin(1),
                _ => panic!("unexpected request"),
            });
            let result = h.run(&stub, |_, _| {}).unwrap();
            assert_eq!((result.checked, result.uploaded_files), (1, 1));
            let report = store.last_check_report().unwrap().unwrap();
            assert!(report.completed && report.listed);
            assert_eq!((report.seen, report.hashed, report.added), (1, 0, 1));
            assert_eq!(store.diagnostics().scans, before.scans + 1);
            assert_eq!(store.diagnostics().hashes, before.hashes);
            assert_eq!(store.change_marker().unwrap(), marker);
            let after = store.sync_entry("cached.png").unwrap().unwrap();
            assert_eq!(
                (after.sha1, after.size, after.mtime_secs, after.mtime_nanos),
                (cached.sha1, cached.size, cached.mtime_secs, cached.mtime_nanos)
            );
            assert_eq!(after.sync_state, MediaSyncState::Live);
            assert!(!after.dirty);
            let compared = store.diagnostics();
            assert_eq!(h.run(&stub, |_, _| {}).unwrap().checked, 0);
            assert_eq!(store.diagnostics(), compared);
            let report = store.last_check_report().unwrap().unwrap();
            assert!(report.completed);
            assert_eq!((report.listed, report.seen, report.hashed), (false, 0, 0));
        });
    }
}

#[test]
fn changed_folder_media_stage_cancellation_keeps_honest_reports_and_resumes() {
    for during_hash in [false, true] {
        with_collection(|h| {
            let store = h.media.store();
            store
                .bind_sync_owner(&MediaBaselineOwner::new("configured-owner-111"))
                .unwrap();
            store.check_changes_report().unwrap();
            let marker = store.change_marker().unwrap();
            assert!(marker.is_some());
            let before = store.diagnostics();
            let directory = fs::File::open(store.media_dir()).unwrap();
            let previous = directory.metadata().unwrap().modified().unwrap();
            let bytes = vec![7; 131_072];
            fs::write(store.media_dir().join("first.png"), &bytes).unwrap();
            fs::write(store.media_dir().join("second.png"), &bytes).unwrap();
            directory
                .set_times(fs::FileTimes::new().set_modified(previous + Duration::from_secs(1)))
                .unwrap();
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(0),
                1 => envelope(json!([2, 2])),
                2 => envelope(json!("OK")),
                _ => panic!("unexpected request"),
            });
            let observed = store.clone();
            let old_marker = marker.clone();
            let mut hash_callbacks = 0;
            let failure = h
                .run(&stub, move |snapshot, cancel| {
                    if snapshot.phase != MediaPhase::Checking {
                        return;
                    }
                    // Re-enter the journal from progress: no guard may span it.
                    assert_eq!(observed.change_marker().unwrap(), old_marker);
                    if during_hash {
                        if observed.diagnostics().hashes == before.hashes + 1 {
                            hash_callbacks += 1;
                            if hash_callbacks == 2 {
                                cancel.cancel();
                            }
                        }
                    } else if snapshot.totals.checked == 1 {
                        cancel.cancel();
                    }
                })
                .unwrap_err();
            assert_eq!(failure.error.kind, meguri_sync::ErrorKind::Cancelled);
            assert_eq!(failure.snapshot.totals.checked, 1);
            assert!(stub.requests().is_empty());
            assert_eq!(store.change_marker().unwrap(), marker);
            let failed = store.last_check_report().unwrap().unwrap();
            assert!(!failed.completed && failed.listed);
            let committed = u64::from(!during_hash);
            assert_eq!((failed.seen, failed.hashed, failed.added), (1, 1, committed));
            assert_eq!(store.entries().unwrap().len() as u64, committed);
            assert_eq!(store.pending().unwrap().len() as u64, committed);
            for entry in store.entries().unwrap() {
                assert_eq!(entry.sha1, Some(sha(&bytes)));
                assert_eq!(entry.size, bytes.len() as u64);
                assert!(entry.dirty);
            }
            assert_eq!(store.diagnostics().scans, before.scans + 1);
            assert_eq!(store.diagnostics().hashes, before.hashes + 1);
            let result = h.run(&stub, |_, _| {}).unwrap();
            assert_eq!((result.checked, result.uploaded_files), (2, 2));
            let report = store.last_check_report().unwrap().unwrap();
            assert!(report.completed && report.listed);
            assert_eq!(
                (report.seen, report.hashed, report.added),
                (2, 2 - committed, 2 - committed)
            );
            assert_eq!(store.diagnostics().scans, before.scans + 2);
            assert_eq!(store.diagnostics().hashes, before.hashes + 3 - committed);
            assert_ne!(store.change_marker().unwrap(), marker);
            assert!(!store.pending_any().unwrap());
        });
    }
}

#[test]
fn lists_every_page_before_download_and_resumes_sixty_as_25_25_10() {
    with_collection(|h| {
        let listings = Arc::new(AtomicUsize::new(0));
        let downloads = Arc::new(AtomicUsize::new(0));
        let (listed, fetched) = (listings.clone(), downloads.clone());
        let stub = SyncStub::serve(move |head| match head.path() {
            "/msync/begin" => begin(60),
            "/msync/mediaChanges" => match listed.fetch_add(1, Ordering::SeqCst) {
                0 => changes(&names(0..30), 1),
                1 => changes(&names(30..60), 31),
                2 | 4 => envelope(json!([])),
                3 => changes(&names(50..60), 51),
                _ => panic!("unexpected listing"),
            },
            "/msync/downloadFiles" => {
                assert!(listed.load(Ordering::SeqCst) >= 3, "all pages precede file requests");
                match fetched.fetch_add(1, Ordering::SeqCst) {
                    0 => download(&names(0..25)),
                    1 => download(&names(25..50)),
                    2 => download(&names(50..60)),
                    _ => panic!("retransferred committed file"),
                }
            }
            "/msync/mediaSanity" => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let failure = h
            .run(&stub, |s, cancel| {
                if s.phase == MediaPhase::Downloading && s.download_received == 50 && s.files_left == 10 {
                    cancel.cancel();
                }
            })
            .unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::Cancelled);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 50);
        assert_eq!(h.media.store().download_intents().unwrap().len(), 10);
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.downloaded_files, 10);
        assert_eq!(result.awaiting, 0);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 60);
        let requested = requested_names(&stub);
        assert_eq!(requested.iter().map(Vec::len).collect::<Vec<_>>(), [25, 25, 10]);
        assert_eq!(requested[2], names(50..60));
    });
}

#[test]
fn partial_answer_is_matched_by_real_name_and_unreceived_names_are_reasked() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(3),
            1 => changes(&names(0..3), 1),
            2 => envelope(json!([])),
            3 => download(&[names(0..3)[1].clone()]), // Deliberately not a prefix.
            4 => download(&[names(0..3)[2].clone(), names(0..3)[0].clone()]),
            5 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.downloaded_files, 3);
        assert_eq!(
            requested_names(&stub),
            [names(0..3), vec![names(0..3)[0].clone(), names(0..3)[2].clone()]]
        );
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 3);
    });
}

#[test]
fn cancel_after_publication_keeps_whole_answer_uncommitted_and_resumable() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(2),
            1 => changes(&names(0..2), 1),
            2 => envelope(json!([])),
            3 => download(&names(0..2)),
            _ => panic!("cancel must stop before more requests"),
        });
        let failure = h
            .run(&stub, |s, cancel| {
                if s.phase == MediaPhase::Downloading && s.published_files == 1 {
                    cancel.cancel();
                }
            })
            .unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::Cancelled);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
        assert_eq!(h.media.store().download_intents().unwrap().len(), 2);
        assert!(h.media.store().media_dir().join(&names(0..2)[0]).is_file());
        assert!(!h.media.store().media_dir().join(&names(0..2)[1]).exists());
        // Lease and all owned staging files are released only after work drains.
        let _lease = h.media.try_job(meguri_engine::media::MediaJobKind::CheckMedia).unwrap();
        assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
    });
}

#[test]
fn zero_byte_member_rejects_the_whole_answer_before_publication() {
    with_collection(|h| {
        let reply = Reply::ok(&media_files_support::zip(&[
            ("0", b"valid", false),
            ("1", b"", false),
            ("_meta", br#"{"0":"file-000.png","1":"file-001.png"}"#, false),
        ]));
        let stub = SyncStub::serve(move |head| match head.index {
            0 => begin(2),
            1 => changes(&names(0..2), 1),
            2 => envelope(json!([])),
            3 => reply.clone(),
            _ => panic!("unexpected request"),
        });
        let error = h.run(&stub, |_, _| {}).unwrap_err().error;
        assert_eq!(error.kind, meguri_sync::ErrorKind::BadResponse);
        assert_eq!(error.method, Some(Method::DownloadFiles));
        assert_eq!(fs::read_dir(h.media.store().media_dir()).unwrap().count(), 0);
        assert_eq!(h.media.store().download_intents().unwrap().len(), 2);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
    });
}

#[test]
fn late_stamp_edit_stays_pending_and_next_batch_uploads_the_new_bytes() {
    with_collection(|h| {
        h.media.add("a", b"before").unwrap();
        let media = h.media.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let sent = requests.clone();
        let stub = SyncStub::serve(move |head| match head.path() {
            "/msync/begin" => begin(0),
            "/msync/uploadChanges" => {
                let n = sent.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    media.add("a", b"after!").unwrap();
                }
                envelope(json!([1, n + 1]))
            }
            "/msync/mediaSanity" => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert_eq!(uploaded(&stub), [b"before".to_vec(), b"after!".to_vec()]);
        assert_eq!(result.uploaded_files, 2);
        assert!(!h.media.store().pending_any().unwrap());
    });
}

#[test]
fn uploaded_hash_mismatch_forces_registration_even_with_the_same_size_and_mtime() {
    with_collection(|h| {
        h.media.add("a", b"before").unwrap();
        let file = h.media.store().media_dir().join("a");
        let old_hash = h.media.store().sync_entry("a").unwrap().unwrap().sha1;
        let requests = Arc::new(AtomicUsize::new(0));
        let uploads = requests.clone();
        let stub = SyncStub::serve(move |head| match head.path() {
            "/msync/begin" => begin(0),
            "/msync/uploadChanges" => envelope(json!([1, uploads.fetch_add(1, Ordering::SeqCst) + 1])),
            "/msync/mediaSanity" => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let mut changed = false;
        h.run(&stub, move |s, _| {
            if s.phase == MediaPhase::Uploading && !changed {
                let m = fs::metadata(&file).unwrap();
                let mut writer = fs::OpenOptions::new().write(true).open(&file).unwrap();
                writer.write_all(b"after!").unwrap();
                writer
                    .set_times(fs::FileTimes::new().set_modified(m.modified().unwrap()))
                    .unwrap();
                assert_eq!(fs::metadata(&file).unwrap().len(), m.len());
                assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), m.modified().unwrap());
                changed = true;
            }
        })
        .unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        let entry = h.media.store().sync_entry("a").unwrap().unwrap();
        assert_ne!(entry.sha1, old_hash);
        assert_eq!(entry.sha1, Some(sha(b"after!")));
        assert!(!entry.dirty);
    });
}

#[test]
fn missing_upload_becomes_a_deletion_without_resetting_another_pending_deletion() {
    with_collection(|h| {
        let owner = MediaBaselineOwner::new("configured-owner-111");
        h.media.store().bind_sync_owner(&owner).unwrap();
        h.media.add("old", b"old").unwrap();
        let old = h.media.store().pending_batch(1).unwrap().remove(0);
        h.media
            .store()
            .commit_media_upload(&h.media.store().sync_baseline().unwrap(), &[old], 1)
            .unwrap();
        h.media.store().trash(&["old".into()]).unwrap();
        h.media.add("gone", b"gone").unwrap();
        let path = h.media.store().media_dir().join("gone");
        let stub = SyncStub::serve(|head| match head.path() {
            "/msync/begin" => begin(1),
            "/msync/uploadChanges" => envelope(json!([2, 3])),
            "/msync/mediaSanity" => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let mut removed = false;
        let result = h
            .run(&stub, move |s, _| {
                if s.phase == MediaPhase::Uploading && !removed {
                    fs::remove_file(&path).unwrap();
                    removed = true;
                }
            })
            .unwrap();
        assert_eq!(result.uploaded_deletions, 2);
        assert_eq!(
            h.media.store().sync_entry("old").unwrap().unwrap().sync_state,
            MediaSyncState::Deleted
        );
        assert!(!h.media.store().pending_any().unwrap());
    });
}

#[test]
fn usn_gap_is_preserved_and_processed_count_is_clamped() {
    with_collection(|h| {
        h.media.add("a", b"a").unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(0),
            1 => envelope(json!([99, 7])),
            2 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.uploaded_files, 1);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
        assert!(!h.media.store().pending_any().unwrap());
    });
}

#[test]
fn accepted_prefix_only_is_acknowledged_and_zero_processed_is_bad_response() {
    with_collection(|h| {
        h.media.add("a", b"a").unwrap();
        h.media.add("b", b"b").unwrap();
        assert!(h.media.store().sync_baseline().unwrap().owner.is_none());
        let entries = Arc::new(Mutex::new(Vec::new()));
        let captured = entries.clone();
        let store = h.media.store().clone();
        let stub = SyncStub::serve(move |head| match head.index {
            0 => {
                assert_eq!(head.path(), "/msync/begin");
                begin(0)
            }
            1 => {
                assert_eq!(head.path(), "/msync/uploadChanges");
                // Initial owner binding resets and re-registers in filesystem
                // order. Y10.1 orders by current stamps, not add/name order.
                let mut current: Vec<_> = ["a", "b"]
                    .into_iter()
                    .map(|name| store.sync_entry(name).unwrap().unwrap())
                    .collect();
                assert!(current.iter().all(|e| e.dirty));
                current.sort_by_key(|e| e.change_counter);
                assert!(current[0].change_counter < current[1].change_counter);
                *captured.lock().unwrap() = current;
                envelope(json!([1, 1]))
            }
            2 => {
                assert_eq!(head.path(), "/msync/uploadChanges");
                // The accepted prefix must already be committed before the
                // next request receives its zero-processed answer (Y10.5).
                for (i, before) in captured.lock().unwrap().iter().enumerate() {
                    let after = store.sync_entry(&before.name).unwrap().unwrap();
                    assert_eq!(after.change_counter, before.change_counter);
                    assert_eq!(after.dirty, i != 0);
                }
                assert_eq!(store.sync_baseline().unwrap().usn, 1);
                envelope(json!([0, 1]))
            }
            _ => panic!("unexpected request"),
        });
        let failure = h.run(&stub, |_, _| {}).unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::BadResponse);
        assert_eq!(failure.error.method, Some(Method::UploadChanges));
        let requests = stub.requests();
        assert_eq!(
            paths(&stub),
            ["/msync/begin", "/msync/uploadChanges", "/msync/uploadChanges"]
        );
        let batches: Vec<Value> = requests
            .iter()
            .skip(1)
            .map(|request| {
                assert!(request.complete, "accepted HTTP response must follow a drained request");
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(request.body.as_ref().unwrap())).unwrap();
                let mut meta = Vec::new();
                archive.by_name("_meta").unwrap().read_to_end(&mut meta).unwrap();
                let meta: Value = serde_json::from_slice(&meta).unwrap();
                for change in meta.as_array().unwrap() {
                    let mut bytes = Vec::new();
                    archive
                        .by_name(change[1].as_str().unwrap())
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .unwrap();
                    assert_eq!(bytes, change[0].as_str().unwrap().as_bytes());
                }
                meta
            })
            .collect();
        let entries = entries.lock().unwrap();
        assert_eq!(entries.len(), 2);
        // Check wire order against independently read stamps, then use the
        // actual accepted wire prefix to check retained journal state.
        assert_eq!(batches[0], json!([[entries[0].name, "0"], [entries[1].name, "1"]]));
        let accepted = batches[0][0][0].as_str().unwrap();
        let rejected = batches[0][1][0].as_str().unwrap();
        assert_eq!(batches[1], json!([[rejected, "0"]]));
        assert!(!h.media.store().sync_entry(accepted).unwrap().unwrap().dirty);
        assert!(h.media.store().sync_entry(rejected).unwrap().unwrap().dirty);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 1);
        assert_eq!(failure.snapshot.totals.uploaded_files, 1);
        assert_eq!(failure.snapshot.totals.uploaded_deletions, 0);
    });
}

#[test]
fn sanity_has_one_fresh_begin_rerun_and_second_mismatch_resets_again() {
    for second_failure in [false, true] {
        with_collection(|h| {
            h.media.add("a", b"payload").unwrap();
            let stub = SyncStub::serve(move |head| match head.index {
                0 => begin(0),
                1 => envelope(json!([1, 1])),
                2 => envelope(json!("mediaSanity")),
                3 => begin(1),
                4 => envelope(json!([["a", 1, sha(b"payload")]])),
                5 => envelope(json!([])),
                6 => envelope(json!(if second_failure { "mediaSanity" } else { "OK" })),
                _ => panic!("unbounded sanity rerun"),
            });
            let result = h.run(&stub, |_, _| {});
            assert_eq!(paths(&stub).iter().filter(|p| p.ends_with("/begin")).count(), 2);
            if second_failure {
                assert_eq!(
                    result.unwrap_err().error.kind,
                    meguri_sync::ErrorKind::MediaResyncFailed
                );
                assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
                assert_eq!(
                    h.media.store().sync_entry("a").unwrap().unwrap().sync_state,
                    MediaSyncState::None
                );
            } else {
                assert!(result.unwrap().resynced);
            }
        });
    }
}

#[test]
fn pending_work_created_at_confirmation_returns_to_push() {
    with_collection(|h| {
        h.media.add("a", b"a").unwrap();
        let uploads = Arc::new(AtomicUsize::new(0));
        let count = uploads.clone();
        let stub = SyncStub::serve(move |head| match head.path() {
            "/msync/begin" => begin(0),
            "/msync/uploadChanges" => envelope(json!([1, count.fetch_add(1, Ordering::SeqCst) + 1])),
            "/msync/mediaSanity" => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let service = h.media.clone();
        let mut added = false;
        let result = h
            .run(&stub, move |s, _| {
                if s.phase == MediaPhase::Confirming && !added {
                    service.add("b", b"b").unwrap();
                    added = true;
                }
            })
            .unwrap();
        assert_eq!(result.uploaded_files, 2);
        assert_eq!(uploads.load(Ordering::SeqCst), 2);
        let requests = stub.requests();
        let count: Value = serde_json::from_slice(requests.last().unwrap().body.as_ref().unwrap()).unwrap();
        assert_eq!(count, json!({"local":2}));
    });
}

#[test]
fn owner_or_reset_epoch_change_rejects_continuation_before_download() {
    for switch_owner in [false, true] {
        with_collection(|h| {
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(1),
                1 => changes(&names(0..1), 1),
                _ => panic!("must reject changed baseline"),
            });
            let service = h.media.clone();
            let mut changed = false;
            let failure = h
                .run(&stub, move |s, _| {
                    if s.phase == MediaPhase::Listing && s.listed == 1 && !changed {
                        if switch_owner {
                            service
                                .store()
                                .bind_sync_owner(&MediaBaselineOwner::new("different-owner"))
                                .unwrap();
                        } else {
                            service.store().reset_sync().unwrap();
                        }
                        changed = true;
                    }
                })
                .unwrap_err();
            assert_eq!(failure.error.kind, meguri_sync::ErrorKind::Interrupted);
            assert!(!paths(&stub).iter().any(|p| p.ends_with("downloadFiles")));
        });
    }
}

#[test]
fn out_of_order_page_is_rejected_before_any_decision_commits() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(2),
            1 => envelope(json!([["a", 2, sha(b"a")], ["b", 1, sha(b"b")]])),
            _ => panic!("unexpected request"),
        });
        let failure = h.run(&stub, |_, _| {}).unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::BadResponse);
        assert!(h.media.store().download_intents().unwrap().is_empty());
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
    });
}

#[test]
fn later_duplicate_deletion_retires_intent_but_preserves_pending_local_file() {
    with_collection(|h| {
        h.media.add("a", b"local").unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(2),
            1 => envelope(json!([["a", 1, sha(b"server")]])),
            2 => envelope(json!([["a", 2, ""]])),
            3 => envelope(json!([])),
            4 => envelope(json!([1, 3])),
            5 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.downloaded_files, 0);
        assert_eq!(result.uploaded_files, 1);
        assert_eq!(fs::read(h.media.store().media_dir().join("a")).unwrap(), b"local");
        assert!(h.media.store().download_intents().unwrap().is_empty());
    });
}

#[test]
fn progress_window_uses_only_last_twenty_saved_batches_and_requires_three() {
    let mut window = DownloadWindow::default();
    window.saved_batch(Duration::from_secs(1000), 1);
    window.saved_batch(Duration::from_secs(1000), 1);
    assert_eq!(window.estimate(10), None);
    window.saved_batch(Duration::from_secs(1000), 1);
    assert_eq!(window.estimate(10), Some(Duration::from_secs(10000)));
    for _ in 0..20 {
        window.saved_batch(Duration::from_secs(2), 4);
    }
    assert_eq!(window.estimate(10), Some(Duration::from_secs(5)));
    window.saved_batch(Duration::from_secs(1000), 0);
    assert_eq!(window.estimate(10), Some(Duration::from_secs(5)));
}

#[test]
fn callbacks_can_commit_collection_work_and_second_media_job_is_busy() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(1),
            1 => changes(&names(0..1), 1),
            2 => envelope(json!([])),
            3 => download(&names(0..1)),
            4 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let collection = h.collection.clone();
        let media = h.media.clone();
        let operations = Arc::new(AtomicUsize::new(0));
        let committed = operations.clone();
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let seen = snapshots.clone();
        let result = h
            .run(&stub, move |s, _| {
                seen.lock().unwrap().push(s.clone());
                if s.phase == MediaPhase::Downloading {
                    assert!(matches!(
                        media.try_job(meguri_engine::media::MediaJobKind::Sync),
                        Err(meguri_engine::EngineError::Busy { .. })
                    ));
                    collection
                        .run(
                            OpSpec::named("media_review_concurrency", OpSource::UserNonUndoable),
                            |ctx| {
                                // A real writer op, independent of media's journal.
                                ctx.scope()
                                    .set_config("media_test", &meguri_storage::ConfigValue::Integer(1))?;
                                Ok(())
                            },
                        )
                        .unwrap();
                    committed.fetch_add(1, Ordering::SeqCst);
                }
            })
            .unwrap();
        assert!(operations.load(Ordering::SeqCst) >= 2);
        assert_eq!(result.downloaded_files, 1);
        let seen = snapshots.lock().unwrap();
        assert!(
            seen.iter()
                .any(|s| s.phase == MediaPhase::Listing && s.totals.awaiting == 1)
        );
        assert_eq!(seen.last().unwrap().phase, MediaPhase::Confirming);
    });
}

// FileTransport deliberately accepts a streamed upload without ever reading
// its source. Job's successful JSON body alone must never clear pending state.
struct UnreadUpload {
    begin: media_files_support::FileTransport,
    upload: media_files_support::FileTransport,
}
impl SyncTransport for UnreadUpload {
    fn post(&self, request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        if request.url.ends_with("/begin") {
            self.begin.post(request)
        } else {
            self.upload.post(request)
        }
    }
}
#[test]
fn accepted_response_without_verified_source_eof_does_not_acknowledge() {
    with_collection(|h| {
        h.media.add("a", b"payload").unwrap();
        let files = tempfile::tempdir().unwrap();
        let a = files.path().join("begin");
        let b = files.path().join("upload");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let begin = media_files_support::FileTransport::new(
            &mut media_files_support::disk_zip(&a, br#"{"data":{"usn":0}}"#),
            &a,
        );
        let upload =
            media_files_support::FileTransport::new(&mut media_files_support::disk_zip(&b, br#"{"data":[1,1]}"#), &b);
        let transport = UnreadUpload { begin, upload };
        let error = h
            .run_using(
                common::options("http://media.test/"),
                move |options, random| {
                    Job::with_transport(
                        options,
                        Some(meguri_sync::HostKey::new("synthetic")),
                        random,
                        Box::new(transport),
                    )
                    .unwrap()
                },
                |_, _| {},
            )
            .unwrap_err()
            .error;
        assert_eq!(error.kind, meguri_sync::ErrorKind::Interrupted);
        assert!(h.media.store().sync_entry("a").unwrap().unwrap().dirty);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
        assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
    });
}

#[test]
fn upload_redirect_is_refused_and_keeps_pending_state() {
    with_collection(|h| {
        h.media.add("a", b"payload").unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(0),
            1 => Reply::redirect(308, Some("http://media.test/elsewhere/")),
            _ => panic!("media upload must never replay"),
        });
        let failure = h.run(&stub, |_, _| {}).unwrap_err();
        assert!(matches!(
            failure.error.kind,
            meguri_sync::ErrorKind::RedirectRefused { code: 308 }
        ));
        assert!(h.media.store().pending_any().unwrap());
        assert_eq!(paths(&stub).len(), 2);
    });
}

#[test]
fn legal_meta_redirect_then_media_upload_accounts_for_the_final_source() {
    with_collection(|h| {
        h.media.add("a", b"payload").unwrap();
        let target = SyncStub::serve(|head| match head.index {
            0 => Reply::ok(b"{}"),
            1 => begin(0),
            2 => envelope(json!([1, 1])),
            3 => envelope(json!("OK")),
            _ => panic!("unexpected target request"),
        });
        let location = target.url();
        let origin = SyncStub::serve(move |_| Reply::redirect(308, Some(&location)));
        let threads = h.engine.threads().clone();
        let result = h
            .run_using(
                common::options(&origin.url()),
                move |options, random| {
                    let mut job = Job::new(options, Some(meguri_sync::HostKey::new("synthetic")), random).unwrap();
                    meguri_sync::transport::runtime::block_on(&threads, options.cancel_token(), async {
                        job.request(Method::Meta, Payload::Json(b"{}".to_vec()))
                            .await?
                            .bytes()
                            .await
                    })
                    .unwrap();
                    assert!(job.new_endpoint().is_some());
                    job
                },
                |_, _| {},
            )
            .unwrap();
        assert_eq!(result.uploaded_files, 1);
        assert_eq!(result.bytes_uploaded, 7);
        assert!(!h.media.store().pending_any().unwrap());
        assert_eq!(origin.requests().len(), 1);
        assert_eq!(uploaded(&target), [b"payload".to_vec()]);
    });
}

#[test]
fn upload_fill_includes_the_file_that_crosses_two_and_a_half_mib() {
    with_collection(|h| {
        for name in names(0..5) {
            h.media.add(&name, &vec![42; 1024 * 1024]).unwrap();
        }
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(0),
            1 => envelope(json!([3, 3])),
            2 => envelope(json!([2, 5])),
            3 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        let batch_sizes: Vec<_> = stub
            .requests()
            .into_iter()
            .filter(|r| r.head.path().ends_with("uploadChanges"))
            .map(|r| {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(r.body.unwrap())).unwrap();
                let mut meta = Vec::new();
                archive.by_name("_meta").unwrap().read_to_end(&mut meta).unwrap();
                serde_json::from_slice::<Value>(&meta)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len()
            })
            .collect();
        assert_eq!(batch_sizes, [3, 2]);
        assert_eq!(result.uploaded_files, 5);
        assert_eq!(result.bytes_uploaded, 5 * 1024 * 1024);
    });
}

#[test]
fn expected_generation_is_checked_before_binding_owner_or_requesting() {
    with_collection(|h| {
        let stub = SyncStub::serve(|_| panic!("stale generation cannot issue a request"));
        let options = common::options(&stub.url());
        let service = h.media.clone();
        let generation = h.collection.generation().0 + 1;
        let engine = h.engine.clone();
        let random = h.random.clone();
        let (send, receive) = std::sync::mpsc::channel();
        h.engine
            .start_job(JobKind::MediaSync, move |context| {
                let mut job = Job::new(
                    &options.cancel(context.cancel.clone()),
                    Some(meguri_sync::HostKey::new("synthetic")),
                    random.as_ref(),
                )
                .unwrap();
                let result = media::reconcile(
                    engine.threads(),
                    &mut job,
                    &service,
                    generation,
                    &MediaBaselineOwner::new("configured-owner-111"),
                    &mut |_| {},
                );
                send.send(result).unwrap();
                Ok(())
            })
            .unwrap();
        let failure = receive.recv().unwrap().unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::Interrupted);
        assert!(h.media.store().sync_baseline().unwrap().owner.is_none());
        assert!(stub.requests().is_empty());
    });
}

#[test]
fn progress_counts_real_files_bytes_and_saved_batches_before_offering_eta() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(90),
            1 => changes(&names(0..90), 1),
            2 => envelope(json!([])),
            3 => download(&names(0..25)),
            4 => download(&names(25..50)),
            5 => download(&names(50..75)),
            6 => download(&names(75..90)),
            7 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let seen = snapshots.clone();
        let result = h.run(&stub, move |s, _| seen.lock().unwrap().push(s.clone())).unwrap();
        let snapshots = snapshots.lock().unwrap();
        let download: Vec<_> = snapshots
            .iter()
            .filter(|s| s.phase == MediaPhase::Downloading)
            .collect();
        assert!(
            download
                .iter()
                .filter(|s| s.download_received < 75)
                .all(|s| s.estimate.is_none())
        );
        assert!(
            download
                .iter()
                .any(|s| s.download_received == 75 && s.files_left == 15 && s.estimate.is_some())
        );
        assert_eq!(result.checked, 90);
        assert_eq!(result.downloaded_files, 90);
        assert_eq!(result.bytes_downloaded, 90 * 7);
        assert_eq!(result.awaiting, 0);
        assert_eq!(result.uploaded_files, 0);
    });
}

struct FaultGuard(&'static str);
impl FaultGuard {
    fn io(point: &'static str, kind: std::io::ErrorKind) -> Self {
        meguri_model::faultpoint::arm(point, meguri_model::faultpoint::Fault::IoError(kind));
        Self(point)
    }
}
impl Drop for FaultGuard {
    fn drop(&mut self) {
        meguri_model::faultpoint::disarm(self.0);
    }
}
fn media_arrivals(events: &meguri_engine::Subscription) -> Vec<Vec<String>> {
    let mut names = Vec::new();
    while let Some(event) = events.try_recv() {
        if let meguri_engine::EventKind::Extension(extension) = &event.kind
            && let Some(arrived) = extension.as_any().downcast_ref::<meguri_engine::media::MediaArrived>()
        {
            names.push(arrived.names.clone());
        }
    }
    names
}

#[test]
fn pending_add_during_sanity_request_is_pushed_and_confirmed_again() {
    with_collection(|h| {
        h.media.add("a", b"a").unwrap();
        let service = h.media.clone();
        let uploads = Arc::new(AtomicUsize::new(0));
        let sent = uploads.clone();
        let sanity = Arc::new(AtomicUsize::new(0));
        let checks = sanity.clone();
        let stub = SyncStub::serve(move |head| match head.path() {
            "/msync/begin" => begin(0),
            "/msync/uploadChanges" => envelope(json!([1, sent.fetch_add(1, Ordering::SeqCst) + 1])),
            "/msync/mediaSanity" => {
                if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                    service.add("b", b"b").unwrap();
                }
                envelope(json!("OK"))
            }
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.uploaded_files, 2);
        assert_eq!(sanity.load(Ordering::SeqCst), 2);
        assert_eq!(
            paths(&stub),
            [
                "/msync/begin",
                "/msync/uploadChanges",
                "/msync/mediaSanity",
                "/msync/uploadChanges",
                "/msync/mediaSanity"
            ]
        );
        assert!(!h.media.store().pending_any().unwrap());
    });
}

#[test]
fn transformed_answer_member_is_repaired_after_later_exact_destination_replaces_it() {
    with_collection(|h| {
        let reply = Reply::ok(&media_files_support::zip(&[
            ("0", b"first", false),
            ("1", b"exact", false),
            ("_meta", br#"{"0":"BAD:Name","1":"badname"}"#, false),
        ]));
        let stub = SyncStub::serve(move |head| match head.index {
            0 => begin(2),
            1 => envelope(json!([["BAD:Name", 1, sha(b"first")], ["badname", 2, sha(b"exact")]])),
            2 => envelope(json!([])),
            3 => reply.clone(),
            4 => envelope(json!([2, 4])),
            5 => envelope(json!("OK")),
            _ => panic!("repair must use the already-staged answer"),
        });
        let events = h.engine.stream().subscribe();
        let store = h.media.store().clone();
        let result = h
            .run(&stub, move |s, _| {
                if s.phase == MediaPhase::Downloading && s.files_left != 0 {
                    assert_eq!(store.sync_baseline().unwrap().usn, 0);
                    assert_eq!(store.download_intents().unwrap().len(), 2);
                    assert!(
                        media_arrivals(&events).is_empty(),
                        "no arrival before whole-answer commit"
                    );
                }
            })
            .unwrap();
        assert_eq!(result.downloaded_files, 2);
        assert_eq!(result.bytes_downloaded, 10);
        assert_eq!(
            requested_names(&stub),
            [vec!["BAD:Name".to_owned(), "badname".to_owned()]]
        );
        assert_eq!(fs::read(h.media.store().media_dir().join("badname")).unwrap(), b"exact");
        let renamed = h
            .media
            .store()
            .entries()
            .unwrap()
            .into_iter()
            .find(|e| e.live && e.sha1.as_deref() == Some(&sha(b"first")))
            .unwrap();
        assert_ne!(renamed.name, "badname");
        assert_eq!(
            fs::read(h.media.store().media_dir().join(renamed.name)).unwrap(),
            b"first"
        );
        assert!(h.media.store().download_intents().unwrap().is_empty());
        assert!(!h.media.store().pending_any().unwrap());
    });
}

#[test]
fn late_local_edit_reobserves_and_resaves_the_complete_staged_answer() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(2),
            1 => changes(&names(0..2), 1),
            2 => envelope(json!([])),
            3 => download(&names(0..2)),
            4 => envelope(json!("OK")),
            _ => panic!("no second download"),
        });
        let store = h.media.store().clone();
        let events = h.engine.stream().subscribe();
        let mut edited = false;
        let result = h
            .run(&stub, move |s, _| {
                if s.phase == MediaPhase::Downloading && s.files_left != 0 {
                    assert!(media_arrivals(&events).is_empty());
                    assert_eq!(store.download_intents().unwrap().len(), 2);
                    assert_eq!(store.sync_baseline().unwrap().usn, 0);
                    if s.published_files == 2 && !edited {
                        store.ensure_static_media(&names(0..1)[0], b"late edit").unwrap();
                        edited = true;
                    }
                }
            })
            .unwrap();
        assert_eq!(result.downloaded_files, 2);
        assert_eq!(requested_names(&stub).len(), 1);
        for name in names(0..2) {
            assert_eq!(fs::read(h.media.store().media_dir().join(&name)).unwrap(), b"payload");
            assert!(!h.media.store().sync_entry(&name).unwrap().unwrap().dirty);
        }
        assert_eq!(h.media.store().trash_entries().unwrap().len(), 2);
    });
}

#[test]
fn final_publication_truncated_or_grown_reuses_the_answer_and_arrives_after_atomic_commit() {
    for length in [0, 104_857_601] {
        with_collection(|h| {
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(2),
                1 => changes(&names(0..2), 1),
                2 => envelope(json!([])),
                3 => download(&names(0..2)),
                4 => envelope(json!("OK")),
                _ => panic!("repair must not download again"),
            });
            let store = h.media.store().clone();
            let events = Arc::new(h.engine.stream().subscribe());
            let observed = events.clone();
            let commits = Arc::new(AtomicUsize::new(0));
            let committed = commits.clone();
            let mut edited = false;
            let result = h
                .run(&stub, move |s, _| {
                    if s.phase != MediaPhase::Downloading {
                        return;
                    }
                    if s.files_left != 0 {
                        assert!(
                            media_arrivals(&observed).is_empty(),
                            "publication is not a committed arrival"
                        );
                        assert_eq!(store.sync_baseline().unwrap().usn, 0);
                        assert_eq!(store.download_intents().unwrap().len(), 2);
                        if s.published_files == 2 && !edited {
                            // Edit only the destination, leaving the journal and
                            // the independently owned received payload intact.
                            fs::OpenOptions::new()
                                .write(true)
                                .open(store.media_dir().join(&names(0..1)[0]))
                                .unwrap()
                                .set_len(length)
                                .unwrap();
                            edited = true;
                        }
                    } else {
                        assert_eq!(store.sync_baseline().unwrap().usn, 2);
                        assert!(store.download_intents().unwrap().is_empty());
                        assert_eq!(media_arrivals(&observed), [names(0..2)]);
                        assert_eq!(s.published_files, 3, "one destination was published again");
                        committed.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .unwrap();
            assert_eq!(commits.load(Ordering::SeqCst), 1);
            assert!(media_arrivals(&events).is_empty());
            assert_eq!(requested_names(&stub), [names(0..2)]);
            assert_eq!(result.downloaded_files, 2);
            assert_eq!(result.bytes_downloaded, 14);
            assert_eq!(h.media.store().sync_baseline().unwrap().usn, 2);
            assert!(h.media.store().download_intents().unwrap().is_empty());
            for name in names(0..2) {
                assert_eq!(fs::read(h.media.store().media_dir().join(&name)).unwrap(), b"payload");
                assert!(!h.media.store().sync_entry(&name).unwrap().unwrap().dirty);
            }
        });
    }
}

type ReceiptHook = Box<dyn FnOnce() + Send>;
static RECEIPT_HOOK: Mutex<Option<ReceiptHook>> = Mutex::new(None);
fn edit_before_receipt() {
    let hook = RECEIPT_HOOK.lock().unwrap().take();
    if let Some(hook) = hook {
        hook();
    }
}

#[test]
fn stale_during_save_restarts_every_member_of_the_retained_answer() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(2),
            1 => changes(&names(0..2), 1),
            2 => envelope(json!([])),
            3 => download(&names(0..2)),
            4 => envelope(json!("OK")),
            _ => panic!("save-time repair must not download again"),
        });
        let _fault = FaultGuard("media.download.before_receipt");
        let store = h.media.store().clone();
        let events = Arc::new(h.engine.stream().subscribe());
        let observed = events.clone();
        let mut armed = false;
        let result = h
            .run(&stub, move |s, _| {
                if s.phase != MediaPhase::Downloading || s.files_left == 0 {
                    return;
                }
                assert!(media_arrivals(&observed).is_empty());
                assert_eq!(store.sync_baseline().unwrap().usn, 0);
                assert_eq!(store.download_intents().unwrap().len(), 2);
                if s.published_files == 1 && !armed {
                    let first = store.media_dir().join(&names(0..1)[0]);
                    let second = store.media_dir().join(&names(1..2)[0]);
                    *RECEIPT_HOOK.lock().unwrap() = Some(Box::new(move || {
                        // The second save has published but has no receipt yet.
                        // Also invalidate the first receipt to require a full pass.
                        fs::write(first, b"intervening edit").unwrap();
                        fs::remove_file(second).unwrap();
                    }));
                    meguri_model::faultpoint::arm(
                        "media.download.before_receipt",
                        meguri_model::faultpoint::Fault::Callback(edit_before_receipt),
                    );
                    armed = true;
                }
            })
            .unwrap();
        assert!(RECEIPT_HOOK.lock().unwrap().is_none(), "the save-time race ran");
        assert_eq!(requested_names(&stub), [names(0..2)]);
        assert_eq!(result.downloaded_files, 2);
        assert_eq!(result.bytes_downloaded, 14);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 2);
        assert!(h.media.store().download_intents().unwrap().is_empty());
        assert_eq!(media_arrivals(&events), [names(0..2)]);
        for name in names(0..2) {
            assert_eq!(fs::read(h.media.store().media_dir().join(&name)).unwrap(), b"payload");
            assert!(!h.media.store().sync_entry(&name).unwrap().unwrap().dirty);
        }
        assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
        assert!(h.media.try_job(meguri_engine::media::MediaJobKind::CheckMedia).is_ok());
    });
}

#[test]
fn repeatedly_invalidated_download_receipts_stop_without_committing_or_arriving() {
    with_collection(|h| {
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(1),
            1 => changes(&names(0..1), 1),
            2 => envelope(json!([])),
            3 => download(&names(0..1)),
            _ => panic!("bounded repair sends no request"),
        });
        let store = h.media.store().clone();
        let events = h.engine.stream().subscribe();
        let attempts = Arc::new(AtomicUsize::new(0));
        let edits = attempts.clone();
        let failure = h
            .run(&stub, move |s, _| {
                if s.phase == MediaPhase::Downloading && s.published_files != 0 {
                    assert!(media_arrivals(&events).is_empty());
                    let edit = edits.fetch_add(1, Ordering::SeqCst);
                    store
                        .ensure_static_media(&names(0..1)[0], format!("edit {edit}").as_bytes())
                        .unwrap();
                }
            })
            .unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::LocalStorage);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
        assert_eq!(h.media.store().download_intents().unwrap().len(), 1);
        assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
        assert!(h.media.try_job(meguri_engine::media::MediaJobKind::CheckMedia).is_ok());
    });
}

#[test]
fn stop_cause_survives_publication_and_drain_as_cancelled_or_interrupted() {
    for system in [false, true] {
        with_collection(|h| {
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(2),
                1 => changes(&names(0..2), 1),
                2 => envelope(json!([])),
                3 => download(&names(0..2)),
                _ => panic!("stop before further requests"),
            });
            let events = h.engine.stream().subscribe();
            let failure = h
                .run(&stub, move |s, token| {
                    if s.phase == MediaPhase::Downloading && s.published_files == 1 {
                        if system {
                            token.interrupt();
                        } else {
                            token.cancel();
                        }
                    }
                })
                .unwrap_err();
            assert_eq!(
                failure.error.kind,
                if system {
                    meguri_sync::ErrorKind::Interrupted
                } else {
                    meguri_sync::ErrorKind::Cancelled
                }
            );
            assert!(media_arrivals(&events).is_empty());
            assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
            assert_eq!(h.media.store().download_intents().unwrap().len(), 2);
            assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
            assert!(h.media.try_job(meguri_engine::media::MediaJobKind::CheckMedia).is_ok());
        });
    }
}

#[test]
fn typed_capacity_failures_during_copy_and_journal_keep_partial_state() {
    for point in [
        "media.sync.before_staging",
        "media.download.after_stage",
        "media.sync.downloads.before_commit",
    ] {
        with_collection(|h| {
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(1),
                1 => changes(&names(0..1), 1),
                2 => envelope(json!([])),
                3 => download(&names(0..1)),
                _ => panic!("capacity failure stops requests"),
            });
            let _fault = FaultGuard::io(point, std::io::ErrorKind::StorageFull);
            let failure = h.run(&stub, |_, _| {}).unwrap_err();
            assert!(matches!(failure.error.kind, meguri_sync::ErrorKind::DiskFull { .. }));
            assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
            assert_eq!(h.media.store().download_intents().unwrap().len(), 1);
            assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
        });
    }
}

fn prepare_clean(h: &Harness, names: &[&str]) {
    h.media
        .store()
        .bind_sync_owner(&MediaBaselineOwner::new("configured-owner-111"))
        .unwrap();
    for name in names {
        h.media.store().add(name, b"old").unwrap();
    }
    let captured = h.media.store().pending().unwrap();
    let baseline = h.media.store().sync_baseline().unwrap();
    h.media
        .store()
        .commit_media_upload(&baseline, &captured, captured.len() as u64)
        .unwrap();
}

#[test]
fn vanished_clean_file_does_not_count_as_a_downloaded_deletion() {
    with_collection(|h| {
        prepare_clean(&h, &["gone"]);
        let path = h.media.store().media_dir().join("gone");
        let stub = SyncStub::serve(move |head| match head.index {
            0 => begin(2),
            1 => {
                fs::remove_file(&path).unwrap();
                envelope(json!([["gone", 2, ""]]))
            }
            2 => envelope(json!([])),
            3 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let result = h.run(&stub, |_, _| {}).unwrap();
        assert_eq!(result.downloaded_deletions, 0);
        assert!(h.media.store().sync_entry("gone").unwrap().is_none());
        assert!(h.media.store().trash_entries().unwrap().is_empty());
    });
}

#[test]
fn removed_local_deletion_record_counts_once_even_without_a_file() {
    with_collection(|h| {
        prepare_clean(&h, &["gone"]);
        h.media.store().trash(&["gone".into()]).unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(2),
            1 => envelope(json!([["gone", 2, ""]])),
            2 => envelope(json!([])),
            3 => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        assert_eq!(h.run(&stub, |_, _| {}).unwrap().downloaded_deletions, 1);
        assert!(h.media.store().sync_entry("gone").unwrap().is_none());
        assert_eq!(h.media.store().trash_entries().unwrap().len(), 1);
    });
}

#[test]
fn retained_trash_move_is_counted_when_later_page_work_fails() {
    with_collection(|h| {
        prepare_clean(&h, &["a", "b"]);
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(4),
            1 => envelope(json!([["a", 3, ""], ["b", 4, ""]])),
            _ => panic!("page failure stops the stage"),
        });
        let _fault = FaultGuard::io("media.sync.page.after_trash", std::io::ErrorKind::PermissionDenied);
        let failure = h.run(&stub, |_, _| {}).unwrap_err();
        assert_eq!(failure.error.kind, meguri_sync::ErrorKind::LocalStorage);
        assert_eq!(failure.snapshot.totals.downloaded_deletions, 1);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 2);
        assert!(!h.media.store().media_dir().join("a").exists());
        assert!(h.media.store().media_dir().join("b").is_file());
        assert_eq!(h.media.store().trash_entries().unwrap().len(), 1);
    });
}

#[test]
fn rolled_back_deletion_records_are_not_counted_but_retained_file_moves_are() {
    with_collection(|h| {
        prepare_clean(&h, &["a", "b"]);
        h.media.store().trash(&["b".into()]).unwrap();
        let stub = SyncStub::serve(|head| match head.index {
            0 => begin(4),
            1 => envelope(json!([["a", 3, ""], ["b", 4, ""]])),
            _ => panic!("page failure stops the stage"),
        });
        let _fault = FaultGuard::io("media.sync.page.before_commit", std::io::ErrorKind::StorageFull);
        let failure = h.run(&stub, |_, _| {}).unwrap_err();
        assert!(matches!(failure.error.kind, meguri_sync::ErrorKind::DiskFull { .. }));
        assert_eq!(failure.snapshot.totals.downloaded_deletions, 1);
        assert_eq!(h.media.store().sync_baseline().unwrap().usn, 2);
        assert_eq!(
            h.media.store().sync_entry("b").unwrap().unwrap().sync_state,
            MediaSyncState::Deleted
        );
        assert_eq!(h.media.store().trash_entries().unwrap().len(), 2);
    });
}

#[test]
fn close_and_suspend_supply_the_system_stop_cause_to_an_active_media_stage() {
    for suspend in [false, true] {
        with_collection(|h| {
            let engine = h.engine.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let controller = std::thread::spawn(move || {
                receive.recv().unwrap();
                if suspend {
                    engine.suspend(Duration::from_secs(5)).unwrap();
                } else {
                    engine.close().unwrap();
                }
            });
            let stub = SyncStub::serve(|head| match head.index {
                0 => begin(1),
                1 => changes(&names(0..1), 1),
                2 => envelope(json!([])),
                3 => download(&names(0..1)),
                _ => panic!("lifecycle stop must prevent more requests"),
            });
            let mut stopping = false;
            let failure = h
                .run(&stub, move |s, token| {
                    if s.phase == MediaPhase::Downloading && s.published_files == 1 && !stopping {
                        stopping = true;
                        send.send(()).unwrap();
                        // close/suspend signals before it drains the job. The
                        // callback releases its wait as soon as that real token fires.
                        while !token.is_cancelled() {
                            std::thread::yield_now();
                        }
                        assert_eq!(token.stop_cause(), Some(meguri_engine::StopCause::System));
                    }
                })
                .unwrap_err();
            assert_eq!(failure.error.kind, meguri_sync::ErrorKind::Interrupted);
            controller.join().unwrap();
            if suspend {
                h.engine.resume().unwrap();
            }
        });
    }
}

static SPELLING_LISTINGS: AtomicUsize = AtomicUsize::new(0);
fn spelling_listing() {
    SPELLING_LISTINGS.fetch_add(1, Ordering::SeqCst);
}

#[test]
#[ignore = "orchestrator: focused 19k upload cancel/resume and spelling cost hook"]
fn nineteen_thousand_in_place_changes_resume_without_resending_or_relisting_each_file() {
    with_collection(|h| {
        for name in names(0..19_000) {
            fs::write(h.media.store().media_dir().join(name), b"changed").unwrap();
        }
        let _listing_guard = FaultGuard("media.sync.upload.spelling_listing");
        SPELLING_LISTINGS.store(0, Ordering::SeqCst);
        meguri_model::faultpoint::arm(
            "media.sync.upload.spelling_listing",
            meguri_model::faultpoint::Fault::Callback(spelling_listing),
        );
        let accepted = Arc::new(AtomicUsize::new(0));
        let server = accepted.clone();
        let stub = SyncStub::serve(move |head| match head.path() {
            "/msync/begin" => begin(server.load(Ordering::SeqCst) as u64),
            "/msync/uploadChanges" => envelope(json!([25, server.fetch_add(25, Ordering::SeqCst) + 25])),
            "/msync/mediaSanity" => envelope(json!("OK")),
            _ => panic!("unexpected request"),
        });
        let failure = h
            .run(&stub, |s, token| {
                if s.phase == MediaPhase::Uploading && s.totals.uploaded_files == 9_500 {
                    token.cancel();
                }
            })
            .unwrap_err();
        assert_eq!(failure.snapshot.totals.uploaded_files, 9_500);
        assert_eq!(h.media.store().pending().unwrap().len(), 9_500);
        assert_eq!(h.run(&stub, |_, _| {}).unwrap().uploaded_files, 9_500);
        assert_eq!(accepted.load(Ordering::SeqCst), 19_000);
        assert_eq!(
            SPELLING_LISTINGS.load(Ordering::SeqCst),
            2,
            "one inventory per push, reused across batches and ack checks"
        );
        let mut sent = std::collections::BTreeSet::new();
        let mut batches = 0;
        for request in stub
            .requests()
            .into_iter()
            .filter(|r| r.head.path().ends_with("uploadChanges"))
        {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(request.body.unwrap())).unwrap();
            let mut meta = Vec::new();
            archive.by_name("_meta").unwrap().read_to_end(&mut meta).unwrap();
            let meta: Value = serde_json::from_slice(&meta).unwrap();
            for record in meta.as_array().unwrap() {
                assert!(sent.insert(record[0].as_str().unwrap().to_owned()));
            }
            batches += 1;
        }
        assert_eq!(batches, 760);
        assert_eq!(sent.len(), 19_000);
        assert!(!h.media.store().pending_any().unwrap());
    });
}

#[test]
fn stop_during_upload_drains_before_classifying_and_never_acknowledges() {
    for system in [false, true] {
        with_collection(|h| {
            h.media.add("upload", &[7; 128 * 1024]).unwrap();
            let engine = h.engine.clone();
            let stub = SyncStub::serve(move |head| match head.path() {
                "/msync/begin" => begin(0),
                "/msync/uploadChanges" => {
                    let running = engine.jobs().running();
                    let media = running.iter().find(|j| j.name == JobKind::MediaSync.name()).unwrap();
                    if system {
                        media.cancel.interrupt();
                    } else {
                        media.cancel.cancel();
                    }
                    envelope(json!([1, 1]))
                }
                _ => panic!("stopped upload cannot reach sanity"),
            });
            let failure = h.run(&stub, |_, _| {}).unwrap_err();
            assert_eq!(
                failure.error.kind,
                if system {
                    meguri_sync::ErrorKind::Interrupted
                } else {
                    meguri_sync::ErrorKind::Cancelled
                }
            );
            assert_eq!(failure.snapshot.totals.uploaded_files, 0);
            assert!(h.media.store().sync_entry("upload").unwrap().unwrap().dirty);
            assert_eq!(h.media.store().sync_baseline().unwrap().usn, 0);
            assert_eq!(fs::read_dir(h.media.store().staging_dir()).unwrap().count(), 0);
            assert!(h.media.try_job(meguri_engine::media::MediaJobKind::CheckMedia).is_ok());
        });
    }
}
