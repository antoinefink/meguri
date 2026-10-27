//! U126's retention/publication integration. RF2 lifecycle tests in meguri-host
//! retain the exhaustive 61-name exhaustion, three-race bound, WAL snapshot,
//! sidecar identity/error, low-space/missing-parent and lease-before-replace cases.
use meguri_engine::{
    CancelToken, Collection, OpSource, OpSpec, backup_limits,
    ports::{BackupFailureCause, BackupPublication},
};
use meguri_model::faultpoint::{self, Fault};
use meguri_package::{
    CollectionPackageInspectContext, ForcedBackupProvider,
    backup::{BackupLimits, listing, naming, retention},
    inspect_collection_package,
};
use meguri_storage::{
    OpenOptions, ReadRows, ReaderRole, StorageError,
    space::{SpaceProvider, StatvfsSpace},
};
use meguri_testkit::normal_support::Fixture;
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::Duration,
};

fn provider(f: &Fixture, space: Arc<dyn SpaceProvider>) -> ForcedBackupProvider {
    ForcedBackupProvider::new(
        f.host.engine().clone(),
        f.collection.path().into(),
        f.dir.path().join("backups"),
        f.clock.clone(),
        f.random.clone(),
        space,
    )
}
fn zero_limits(f: &Fixture) {
    backup_limits::save(
        &f.collection,
        BackupLimits {
            daily: 0,
            weekly: 0,
            monthly: 0,
            minimum_interval_mins: 0,
        },
    )
    .unwrap();
}
fn seed_old(f: &Fixture) -> std::path::PathBuf {
    let folder = f.dir.path().join("backups");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("backup-2029-01-01-00.00.00.colpkg");
    fs::write(&path, b"earlier backup bytes").unwrap();
    path
}
fn inspect(f: &Fixture, name: &str) -> meguri_package::InspectedCollection {
    inspect_collection_package(
        &f.dir.path().join("backups").join(name),
        &CollectionPackageInspectContext {
            collection_directory: f.collection.path().parent().unwrap(),
            live_collection_size: fs::metadata(f.collection.path()).unwrap().len(),
            reserved_space_bytes: 0,
            clock: f.clock.as_ref(),
            random: f.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap()
}
fn snapshot_has_deck(f: &Fixture, name: &str, id: meguri_model::ids::DeckId) -> bool {
    let captured = inspect(f, name);
    let db = meguri_storage::open_collection(
        &captured.collection_path(),
        &OpenOptions::new(f.clock.as_ref(), f.random.as_ref()),
    )
    .unwrap();
    let reader = db.open_reader(ReaderRole::Query).unwrap();
    let present = reader.deck(id).unwrap().is_some();
    drop(reader);
    db.close().unwrap();
    present
}

#[test]
fn same_second_snapshots_have_distinct_names_and_retained_source_bytes() {
    let f = Fixture::new(0);
    zero_limits(&f);
    let deck = meguri_engine::decks::add(&f.collection, "snapshot sentinel".into())
        .unwrap()
        .value
        .id;
    let writer = provider(&f, Arc::new(StatvfsSpace));
    let first = writer.force(&f.collection, &CancelToken::default()).unwrap();
    let original = fs::read(f.dir.path().join("backups").join(&first.receipt.name)).unwrap();
    // An isolated synthetic deletion between snapshots; production deletion's
    // pre-op backup ordering is covered by the real host lifecycle suite.
    f.collection
        .run(
            OpSpec::named("synthetic deck deletion", OpSource::UserUndoable),
            move |ctx| {
                ctx.scope().remove_deck(deck)?;
                Ok(())
            },
        )
        .unwrap();
    let second = writer.force(&f.collection, &CancelToken::default()).unwrap();
    assert_eq!(first.receipt.instant, second.receipt.instant);
    assert_eq!(first.receipt.name, "backup-2030-01-15-10.00.00.colpkg");
    assert_eq!(second.receipt.name, "backup-2030-01-15-10.00.01.colpkg");
    assert!(first.deleted_names.is_empty() && second.deleted_names.is_empty());
    assert!(snapshot_has_deck(&f, &first.receipt.name, deck));
    assert!(!snapshot_has_deck(&f, &second.receipt.name, deck));
    assert_eq!(
        fs::read(f.dir.path().join("backups").join(first.receipt.name)).unwrap(),
        original
    );
    f.host.close().unwrap();
}

#[test]
fn one_lost_publication_race_retries_with_a_new_snapshot_and_then_thins() {
    struct Race {
        collection: Collection,
        deck: meguri_model::ids::DeckId,
        folder: std::path::PathBuf,
        hits: usize,
    }
    static RACE: OnceLock<Mutex<Option<Race>>> = OnceLock::new();
    fn occupy_once() {
        let mut guard = RACE.get().unwrap().lock().unwrap();
        let state = guard.as_mut().unwrap();
        state.hits += 1;
        if state.hits != 1 {
            return;
        }
        fs::write(state.folder.join("backup-2030-01-15-10.00.00.colpkg"), b"racer bytes").unwrap();
        let deck = state.deck;
        state
            .collection
            .run(
                OpSpec::named("synthetic race deletion", OpSource::UserUndoable),
                move |ctx| {
                    ctx.scope().remove_deck(deck)?;
                    Ok(())
                },
            )
            .unwrap();
    }
    let f = Fixture::new(0);
    zero_limits(&f);
    let old = seed_old(&f);
    let deck = meguri_engine::decks::add(&f.collection, "race sentinel".into())
        .unwrap()
        .value
        .id;
    *RACE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Race {
        collection: f.collection.clone(),
        deck,
        folder: f.dir.path().join("backups"),
        hits: 0,
    });
    faultpoint::arm("storage.backup.before_rename", Fault::Callback(occupy_once));
    let result = provider(&f, Arc::new(StatvfsSpace)).force(&f.collection, &CancelToken::default());
    faultpoint::disarm("storage.backup.before_rename");
    let result = result.unwrap();
    assert_eq!(RACE.get().unwrap().lock().unwrap().take().unwrap().hits, 2);
    assert_eq!(result.receipt.name, "backup-2030-01-15-10.00.01.colpkg");
    assert!(!snapshot_has_deck(&f, &result.receipt.name, deck));
    assert_eq!(
        fs::read(f.dir.path().join("backups/backup-2030-01-15-10.00.00.colpkg")).unwrap(),
        b"racer bytes"
    );
    assert_eq!(result.deleted_names, [old.file_name().unwrap().to_str().unwrap()]);
    assert!(!old.exists());
    f.host.close().unwrap();
}

#[test]
fn prepublication_failures_preserve_previous_backup_and_collection_bytes() {
    struct NoSpace;
    impl SpaceProvider for NoSpace {
        fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
            Ok(0)
        }
    }
    for mode in 0..3 {
        let f = Fixture::new(0);
        zero_limits(&f);
        let old = seed_old(&f);
        let before = fs::read(f.collection.path()).unwrap();
        let sidecar = f.collection.path().with_file_name("meguri-sidecar.json");
        let metadata = fs::read(&sidecar).unwrap();
        let cancel = CancelToken::default();
        let space: Arc<dyn SpaceProvider> = if mode == 0 {
            Arc::new(NoSpace)
        } else {
            Arc::new(StatvfsSpace)
        };
        if mode == 1 {
            faultpoint::arm(
                "storage.atomic.after_write",
                Fault::IoError(std::io::ErrorKind::StorageFull),
            );
        }
        if mode == 2 {
            cancel.cancel();
        }
        let result = provider(&f, space).force(&f.collection, &cancel);
        faultpoint::disarm("storage.atomic.after_write");
        let meguri_engine::EngineError::SafetyBackupFailed(error) = result.unwrap_err() else {
            panic!("typed failure");
        };
        assert!(matches!(
            (mode, error.cause),
            (0, BackupFailureCause::InsufficientSpace { .. })
                | (1, BackupFailureCause::DiskFull)
                | (2, BackupFailureCause::Cancelled)
        ));
        assert_eq!(fs::read(&old).unwrap(), b"earlier backup bytes");
        assert_eq!(fs::read(f.collection.path()).unwrap(), before);
        assert_eq!(fs::read(sidecar).unwrap(), metadata);
        assert_eq!(fs::read_dir(f.dir.path().join("backups")).unwrap().count(), 1);
        assert!(
            !fs::read_dir(f.collection.path().parent().unwrap()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".meguri-stage-"))
        );
        f.host.close().unwrap();
    }
}

#[test]
fn late_cancellation_and_metadata_error_both_run_retention_and_preserve_primary_failure() {
    static TOKEN: OnceLock<Mutex<Option<Arc<CancelToken>>>> = OnceLock::new();
    fn cancel() {
        TOKEN.get().unwrap().lock().unwrap().as_ref().unwrap().cancel();
    }
    for corrupt_metadata in [false, true] {
        let f = Fixture::new(0);
        zero_limits(&f);
        let old = seed_old(&f);
        let sidecar = f.collection.path().with_file_name("meguri-sidecar.json");
        let original = fs::read(&sidecar).unwrap();
        if corrupt_metadata {
            fs::write(&sidecar, b"{broken metadata").unwrap();
        }
        let token = CancelToken::new();
        *TOKEN.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(token.clone());
        faultpoint::arm("storage.atomic.after_rename", Fault::Callback(cancel));
        let result = provider(&f, Arc::new(StatvfsSpace)).force(&f.collection, &token);
        faultpoint::disarm("storage.atomic.after_rename");
        assert!(token.is_cancelled());
        assert!(!old.exists());
        let receipt = if corrupt_metadata {
            let meguri_engine::EngineError::SafetyBackupFailed(error) = result.unwrap_err() else {
                panic!("metadata error");
            };
            assert!(matches!(error.cause, BackupFailureCause::Engine(_)));
            assert_eq!(fs::read(&sidecar).unwrap(), b"{broken metadata");
            let BackupPublication::Durable(receipt) = error.publication else {
                panic!("durable backup");
            };
            receipt
        } else {
            let written = result.unwrap();
            assert_eq!(written.deleted_names, [old.file_name().unwrap().to_str().unwrap()]);
            written.receipt
        };
        assert!(f.dir.path().join("backups").join(receipt.name).is_file());
        if corrupt_metadata {
            fs::write(sidecar, original).unwrap();
        }
        TOKEN.get().unwrap().lock().unwrap().take();
        f.host.close().unwrap();
    }
}

#[test]
fn sidecar_publication_root_or_ancestor_swap_preserves_target_and_published_bytes() {
    use std::os::unix::fs::symlink;
    use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};
    const PUBLISHED: &str = "backup-2030-01-15-10.00.00.colpkg";
    fn bytes(folder: &Path) -> BTreeMap<OsString, Vec<u8>> {
        fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect()
    }
    struct Swap {
        root: PathBuf,
        binding: PathBuf,
        retained: PathBuf,
        target: PathBuf,
        link: bool,
        hits: usize,
        published: Option<Vec<u8>>,
    }
    static SWAP: OnceLock<Mutex<Option<Swap>>> = OnceLock::new();
    fn swap_at_sidecar() {
        let mut guard = SWAP.get().unwrap().lock().unwrap();
        let state = guard.as_mut().unwrap();
        state.hits += 1;
        match state.hits {
            // The first rename publishes the backup; the second publishes the
            // sidecar, after its receipt is captured and before retention.
            1 => state.published = Some(fs::read(state.root.join(PUBLISHED)).unwrap()),
            2 => {
                fs::rename(&state.binding, &state.retained).unwrap();
                if state.link {
                    symlink(&state.target, &state.binding).unwrap();
                } else {
                    fs::rename(&state.target, &state.binding).unwrap();
                }
            }
            _ => panic!("unexpected publication at retention seam"),
        }
    }
    for ancestor in [false, true] {
        for link in [false, true] {
            let f = Fixture::new(0);
            zero_limits(&f);
            let parent = f.dir.path().join("backup-parent");
            let folder = parent.join("backups");
            fs::create_dir_all(&folder).unwrap();
            let old = "backup-2029-01-01-00.00.00.colpkg";
            fs::write(folder.join(old), b"original old backup").unwrap();
            let original = bytes(&folder);
            let binding = if ancestor { parent } else { folder.clone() };
            let retained = f.dir.path().join("retained-backups");
            let target = f.dir.path().join("foreign-target");
            let target_folder = if ancestor {
                target.join("backups")
            } else {
                target.clone()
            };
            fs::create_dir_all(&target_folder).unwrap();
            for (name, content) in [
                (old, b"target canonical old backup".as_slice()),
                ("backup-2028-02-01-00.00.00.colpkg", b"target older backup"),
                (PUBLISHED, b"target same-name file"),
                ("foreign.txt", b"foreign bytes"),
                (".meguri-tmp-backup-test", b"temporary bytes"),
                ("server-copy-2029-01-01-00.00.00.colpkg", b"recovery bytes"),
            ] {
                fs::write(target_folder.join(name), content).unwrap();
            }
            let target_before = bytes(&target_folder);
            *SWAP.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Swap {
                root: folder.clone(),
                binding: binding.clone(),
                retained: retained.clone(),
                target: target.clone(),
                link,
                hits: 0,
                published: None,
            });
            let writer = ForcedBackupProvider::new(
                f.host.engine().clone(),
                f.collection.path().into(),
                folder,
                f.clock.clone(),
                f.random.clone(),
                Arc::new(StatvfsSpace),
            );
            faultpoint::arm("storage.atomic.after_rename", Fault::Callback(swap_at_sidecar));
            let result = writer.force(&f.collection, &CancelToken::default());
            faultpoint::disarm("storage.atomic.after_rename");
            let state = SWAP.get().unwrap().lock().unwrap().take().unwrap();
            let written = result.unwrap();
            assert_eq!(state.hits, 2);
            assert_eq!(written.receipt.name, PUBLISHED);
            assert!(written.deleted_names.is_empty());
            assert_eq!(written.retention_failures.len(), 1);
            assert!(written.retention_failures[0].name.is_none());
            let target_after = if link {
                target_folder
            } else if ancestor {
                binding.join("backups")
            } else {
                binding
            };
            assert_eq!(bytes(&target_after), target_before, "ancestor={ancestor}, link={link}");
            let retained_folder = if ancestor { retained.join("backups") } else { retained };
            let mut expected = original;
            let published = state.published.unwrap();
            assert_eq!(published.len() as u64, written.receipt.size_bytes);
            expected.insert(PUBLISHED.into(), published);
            assert_eq!(bytes(&retained_folder), expected, "ancestor={ancestor}, link={link}");
            assert_eq!(
                meguri_engine::device_state::DeviceState::for_collection(f.collection.path())
                    .last_backup()
                    .unwrap()
                    .unwrap()
                    .name,
                PUBLISHED
            );
            f.host.close().unwrap();
        }
    }
}

#[test]
fn interactive_answers_commit_while_publication_is_paused_and_snapshot_keeps_prior_answer() {
    struct Pause {
        reached: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    static PAUSE: OnceLock<Mutex<Option<Pause>>> = OnceLock::new();
    fn pause() {
        // Only pause the first matching AtomicFile; answers/metadata use their
        // own durability paths. Taking it also avoids pausing later inspection.
        let pending = PAUSE.get().unwrap().lock().unwrap().take();
        if let Some(pause) = pending {
            pause.reached.send(()).unwrap();
            pause.release.recv_timeout(Duration::from_secs(20)).unwrap();
        }
    }
    let f = Fixture::new(3);
    f.answer(f.cards[0], None);
    let initial_reviews = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .execute_raw("SELECT id FROM revlog", &[])
        .unwrap()
        .rows
        .len();
    let (reached, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    *PAUSE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Pause {
        reached,
        release: resume,
    });
    faultpoint::arm("storage.atomic.after_write", Fault::Callback(pause));
    let writer = provider(&f, Arc::new(StatvfsSpace));
    let collection = f.collection.clone();
    let worker = std::thread::spawn(move || writer.force(&collection, &CancelToken::default()));
    wait.recv_timeout(Duration::from_secs(10)).unwrap();
    // Answer through the real interactive prediction/commit API. A blocked
    // maintenance reader would prevent this return and fail the handshake.
    f.answer(f.cards[1], None);
    f.answer(f.cards[2], None);
    assert_eq!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .execute_raw("SELECT id FROM revlog", &[])
            .unwrap()
            .rows
            .len(),
        initial_reviews + 2
    );
    release.send(()).unwrap();
    let result = worker.join().unwrap();
    faultpoint::disarm("storage.atomic.after_write");
    let result = result.unwrap();
    assert_eq!(inspect(&f, &result.receipt.name).facts().revlog, initial_reviews as u64);
    f.host.close().unwrap();
}

#[test]
fn retention_read_failure_is_reported_without_an_error_result_or_foreign_deletion() {
    let f = tempfile::NamedTempFile::new().unwrap();
    let report = retention::thin(f.path(), 0, BackupLimits::default(), &BTreeSet::new());
    assert!(report.deleted_names.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert!(report.failures[0].name.is_none());
    assert!(listing::list(f.path()).is_err());
    // Even with all zero limits the new file is protected if the clock moves
    // forward after publication; pure tests separately cover anchor neutrality.
    let wall = naming::parse_name("backup-2030-01-15-10.00.00.colpkg").unwrap().1;
    let names = vec!["backup-2030-01-15-10.00.00.colpkg".into()];
    let protected = names.iter().cloned().collect();
    assert!(
        retention::plan(
            wall.day_number() + 20,
            &names,
            BackupLimits {
                daily: 0,
                weekly: 0,
                monthly: 0,
                minimum_interval_mins: 0
            },
            &protected
        )
        .delete
        .is_empty()
    );
}
