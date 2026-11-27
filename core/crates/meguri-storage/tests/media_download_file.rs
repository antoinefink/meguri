//! File-backed download publication and the separate whole-answer journal commit.
mod common;
use meguri_model::faultpoint::{self, Fault};
use meguri_storage::media::{
    MediaBaselineOwner, MediaDownloadIntent, MediaError, MediaPageDecision, MediaStore, MediaSyncState,
};
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use sha1::{Digest, Sha1};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, symlink},
    path::Path,
    sync::{Arc, Mutex},
};

#[global_allocator]
static ALLOCATOR: PeakAlloc = PeakAlloc;

type Hook = Box<dyn FnOnce() + Send>;
static HOOK: Mutex<Option<Hook>> = Mutex::new(None);
fn run_hook() {
    let hook = HOOK.lock().unwrap().take();
    if let Some(hook) = hook {
        hook();
    }
}
struct Armed(&'static str);
impl Armed {
    fn io(point: &'static str) -> Self {
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        Self(point)
    }
    fn hook(point: &'static str, hook: impl FnOnce() + Send + 'static) -> Self {
        *HOOK.lock().unwrap() = Some(Box::new(hook));
        faultpoint::arm(point, Fault::Callback(run_hook));
        Self(point)
    }
}
impl Drop for Armed {
    fn drop(&mut self) {
        faultpoint::disarm(self.0);
        HOOK.lock().unwrap().take();
    }
}
fn open(root: &Path) -> MediaStore {
    let store = MediaStore::open(
        &root.join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    store
        .bind_sync_owner(&MediaBaselineOwner::new("m6:download-fixture"))
        .unwrap();
    store
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}
fn plan_hash(store: &MediaStore, name: &str, sha1: String, usn: u64) -> MediaDownloadIntent {
    store
        .commit_media_page(
            &store.sync_baseline().unwrap(),
            usn + 1,
            &[MediaPageDecision::Download {
                name: name.into(),
                sha1,
                usn,
                stamp: store.sync_entry(name).unwrap().map(|e| e.change_counter),
            }],
        )
        .unwrap();
    store
        .download_intents()
        .unwrap()
        .into_iter()
        .find(|i| i.name == name)
        .unwrap()
}
fn plan(store: &MediaStore, name: &str, bytes: &[u8], usn: u64) -> MediaDownloadIntent {
    plan_hash(store, name, hash(bytes), usn)
}
fn payload(root: &Path, bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new_in(root).unwrap();
    file.write_all(bytes).unwrap();
    file
}
fn hash_disk(path: &Path) -> String {
    let mut file = File::open(path).unwrap();
    let mut hash = Sha1::new();
    let mut bytes = [0; 65_536];
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            return format!("{:x}", hash.finalize());
        }
        hash.update(&bytes[..n]);
    }
}

#[test]
fn large_payload_uses_bounded_heap_and_keeps_caller_ownership_until_batch_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut source = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    let bytes = [37; 65_536];
    let mut digest = Sha1::new();
    for _ in 0..512 {
        source.write_all(&bytes).unwrap();
        digest.update(bytes.as_slice());
    }
    let sha1 = format!("{:x}", digest.finalize());
    let intent = plan_hash(&store, "Large.BIN", sha1.clone(), 3);
    let before = store.sync_baseline().unwrap();
    let held = PeakAlloc::current();
    PeakAlloc::reset_peak();
    let saved = store.save_downloaded_file(&before, &intent, source.path()).unwrap();
    assert!(PeakAlloc::peak().saturating_sub(held) < 4 * 1024 * 1024);
    assert!(saved.changed);
    assert_eq!(saved.record.stored_name.as_deref(), Some("Large.BIN"));
    let destination = store.media_dir().join("Large.BIN");
    assert_ne!(
        fs::metadata(source.path()).unwrap().ino(),
        fs::metadata(&destination).unwrap().ino()
    );
    let entry = store.sync_entry("Large.BIN").unwrap().unwrap();
    assert_eq!(entry.size, 32 * 1024 * 1024);
    assert_eq!(entry.sha1.as_deref(), Some(sha1.as_str()));
    assert!(entry.dirty && entry.pending_download);
    assert_eq!(store.sync_baseline().unwrap(), before);
    assert_eq!(store.download_intents().unwrap(), [intent]);
    // Editing/dropping the caller's temporary file cannot change installed bytes.
    source.as_file().set_len(1).unwrap();
    drop(source);
    assert_eq!(hash_disk(&destination), sha1);
    store.commit_media_downloads(&before, &[saved.record]).unwrap();
    assert_eq!(store.sync_baseline().unwrap().usn, 4);
    assert!(!store.pending_any().unwrap());
}

#[test]
fn identical_content_is_a_file_and_journal_noop_until_recording() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("Same.PNG", b"same").unwrap();
    let intent = plan(&store, "Same.PNG", b"same", 3);
    store.check_changes().unwrap();
    let before = store.sync_entry("Same.PNG").unwrap();
    let counter = store.change_counter().unwrap();
    let metadata = fs::metadata(store.media_dir().join("Same.PNG")).unwrap();
    let source = payload(dir.path(), b"same");
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    assert!(!saved.changed);
    assert_eq!(store.sync_entry("Same.PNG").unwrap(), before);
    assert_eq!(store.change_counter().unwrap(), counter);
    let after = fs::metadata(store.media_dir().join("Same.PNG")).unwrap();
    assert_eq!(
        (after.ino(), after.mtime(), after.mtime_nsec()),
        (metadata.ino(), metadata.mtime(), metadata.mtime_nsec())
    );
    assert!(store.trash_entries().unwrap().is_empty());
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert!(!store.sync_entry("Same.PNG").unwrap().unwrap().dirty);
}

#[test]
fn current_local_edit_is_replaced_exactly_and_its_actual_bytes_are_trashed() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("Exact.PNG", b"initial").unwrap();
    let intent = plan(&store, "Exact.PNG", b"server", 3);
    let source = payload(dir.path(), b"server");
    let concurrent = store.clone();
    let armed = Armed::hook("media.download.after_stage", move || {
        // This store call also proves staging does not retain the journal guard.
        concurrent.ensure_static_media("Exact.PNG", b"late local edit").unwrap();
    });
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    drop(armed);
    assert!(saved.changed);
    assert_eq!(saved.record.stored_name.as_deref(), Some("Exact.PNG"));
    assert_eq!(fs::read(store.media_dir().join("Exact.PNG")).unwrap(), b"server");
    let trash = store.trash_entries().unwrap();
    assert_eq!(trash.len(), 2);
    assert_eq!(trash[0].sha1, hash(b"late local edit"));
    assert_eq!(trash[0].size, 15);
    assert_eq!(trash[1].sha1, hash(b"initial"));
    assert!(store.sync_entry("Exact.PNG").unwrap().unwrap().dirty);
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert!(!store.pending_any().unwrap());
}

#[test]
fn appeared_same_content_after_page_is_reused_without_trashing() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let intent = plan(&store, "appeared.png", b"remote", 3);
    fs::write(store.media_dir().join("appeared.png"), b"remote").unwrap();
    let inode = fs::metadata(store.media_dir().join("appeared.png")).unwrap().ino();
    let source = payload(dir.path(), b"remote");
    let saved = store
        .save_downloaded_file(&store.sync_baseline().unwrap(), &intent, source.path())
        .unwrap();
    assert!(!saved.changed);
    assert_eq!(
        fs::metadata(store.media_dir().join("appeared.png")).unwrap().ino(),
        inode
    );
    assert!(store.trash_entries().unwrap().is_empty());
}

#[test]
fn transformed_ignored_non_nfc_and_shared_names_are_recorded_as_one_answer() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let cases = [
        ("Clip:1.PNG", Some("clip1.png")),
        ("Clip?1.PNG", Some("clip1.png")),
        ("Thumbs.db", None),
        ("???", None),
        ("e\u{301}.png", Some("é.png")),
    ];
    let intents = cases
        .iter()
        .enumerate()
        .map(|(i, (name, _))| plan(&store, name, b"shared", i as u64 + 3))
        .collect::<Vec<_>>();
    let baseline = store.sync_baseline().unwrap();
    let source = payload(dir.path(), b"shared");
    let mut records = Vec::new();
    for ((_, stored), intent) in cases.iter().zip(intents) {
        let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
        assert_eq!(saved.record.stored_name.as_deref(), *stored);
        records.push(saved.record);
    }
    store.commit_media_downloads(&baseline, &records).unwrap();
    assert!(store.download_intents().unwrap().is_empty());
    for name in ["Clip:1.PNG", "Clip?1.PNG", "Thumbs.db", "???"] {
        let entry = store.sync_entry(name).unwrap().unwrap();
        assert_eq!(entry.sync_state, MediaSyncState::Deleted);
        assert!(entry.dirty && !entry.live);
    }
    assert!(store.sync_entry("e\u{301}.png").unwrap().is_none());
    for name in ["clip1.png", "é.png"] {
        assert_eq!(fs::read(store.media_dir().join(name)).unwrap(), b"shared");
        assert!(store.sync_entry(name).unwrap().unwrap().dirty);
    }
}

#[test]
fn case_collision_preserves_existing_spelling_and_uses_hash_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("Picture.PNG", b"local").unwrap();
    let intent = plan(&store, "picture.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    let expected = format!("picture-{}.png", hash(b"remote"));
    assert_eq!(saved.record.stored_name.as_deref(), Some(expected.as_str()));
    assert_eq!(fs::read(store.media_dir().join("Picture.PNG")).unwrap(), b"local");
    assert_eq!(fs::read(store.media_dir().join(&expected)).unwrap(), b"remote");
    assert!(store.trash_entries().unwrap().is_empty());
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert_eq!(
        store.sync_entry("picture.png").unwrap().unwrap().sync_state,
        MediaSyncState::Deleted
    );
}

#[test]
fn hash_name_collision_trashes_foreign_bytes_and_no_extension_remains_recordable() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("Readme", b"local").unwrap();
    let hashed = format!("readme-{}.", hash(b"remote"));
    fs::write(store.media_dir().join(&hashed), b"foreign bytes").unwrap();
    let intent = plan(&store, "readme", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    assert_eq!(saved.record.stored_name.as_deref(), Some(hashed.as_str()));
    assert_eq!(store.trash_entries().unwrap()[0].sha1, hash(b"foreign bytes"));
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert!(store.sync_entry(&hashed).unwrap().unwrap().dirty);
    store.register_full(|_, _| true).unwrap();
    assert_eq!(
        store.sync_entry(&hashed).unwrap().unwrap().sync_state,
        MediaSyncState::Deleted
    );
}

#[test]
fn actual_received_hash_is_kept_separate_from_the_listed_intent() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("clip.png", b"received now").unwrap();
    let intent = plan(&store, "clip.png", b"listed earlier", 3);
    let source = payload(dir.path(), b"received now");
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    assert!(!saved.changed);
    assert_eq!(saved.record.intent, intent);
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert_eq!(
        store.sync_entry("clip.png").unwrap().unwrap().sha1,
        Some(hash(b"received now"))
    );
    assert!(!store.pending_any().unwrap());
}

#[test]
fn edits_between_publication_and_batch_commit_keep_every_intent_and_cursor() {
    for identical in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        let a = plan(&store, "a.png", b"remote", 3);
        let b = plan(&store, "b.png", b"remote", 4);
        let source = payload(dir.path(), b"remote");
        let baseline = store.sync_baseline().unwrap();
        let records = [a, b]
            .iter()
            .map(|intent| {
                store
                    .save_downloaded_file(&baseline, intent, source.path())
                    .unwrap()
                    .record
            })
            .collect::<Vec<_>>();
        store.trash(&["b.png".into()]).unwrap();
        store
            .ensure_static_media("b.png", if identical { b"remote" } else { b"local" })
            .unwrap();
        let entries = store.entries().unwrap();
        let intents = store.download_intents().unwrap();
        assert!(store.commit_media_downloads(&baseline, &records).is_err());
        assert_eq!(store.entries().unwrap(), entries);
        assert_eq!(store.download_intents().unwrap(), intents);
        assert_eq!(store.sync_baseline().unwrap(), baseline);
        assert!(store.sync_entry("a.png").unwrap().unwrap().dirty);
        assert!(store.sync_entry("b.png").unwrap().unwrap().dirty);
    }
}

#[test]
fn stale_owner_epoch_intent_and_suspension_refuse_before_publication() {
    for change in 0..4 {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        if change == 2 {
            plan(&store, "cursor-blocker.png", b"blocker", 3);
        }
        let intent = plan(&store, "clip.png", b"remote", if change == 2 { 5 } else { 3 });
        let source = payload(dir.path(), b"remote");
        let baseline = store.sync_baseline().unwrap();
        let concurrent = store.clone();
        let armed = Armed::hook("media.download.after_stage", move || match change {
            0 => {
                concurrent
                    .bind_sync_owner(&MediaBaselineOwner::new("m6:other"))
                    .unwrap();
            }
            1 => {
                concurrent.reset_sync().unwrap();
                // Same owner, cursor and intent; only the reset epoch differs.
                plan(&concurrent, "clip.png", b"remote", 3);
            }
            2 => {
                plan(&concurrent, "clip.png", b"new intent", 7);
            }
            _ => assert!(concurrent.try_close().unwrap()),
        });
        assert!(store.save_downloaded_file(&baseline, &intent, source.path()).is_err());
        drop(armed);
        if change == 3 {
            store.resume().unwrap();
        }
        assert!(!store.media_dir().join("clip.png").exists());
        assert!(store.trash_entries().unwrap().is_empty());
        assert!(fs::read_dir(store.media_dir()).unwrap().next().is_none());
    }
}

#[test]
fn failed_publication_rolls_back_trash_and_failed_batch_leaves_durable_pending_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("clip.png", b"prior").unwrap();
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let baseline = store.sync_baseline().unwrap();
    let before = store.sync_entry("clip.png").unwrap();
    let armed = Armed::io("media.install.before_publish");
    assert!(store.save_downloaded_file(&baseline, &intent, source.path()).is_err());
    drop(armed);
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"prior");
    assert_eq!(store.sync_entry("clip.png").unwrap(), before);
    assert!(store.trash_entries().unwrap().is_empty());
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    let armed = Armed::io("media.sync.downloads.before_commit");
    assert!(store.commit_media_downloads(&baseline, &[saved.record]).is_err());
    drop(armed);
    drop(store);
    let store = open(dir.path());
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
    assert_eq!(store.sync_baseline().unwrap(), baseline);
    assert_eq!(store.download_intents().unwrap(), [intent]);
    assert!(store.sync_entry("clip.png").unwrap().unwrap().dirty);
    assert_eq!(store.trash_entries().unwrap()[0].sha1, hash(b"prior"));
}

#[test]
fn no_clobber_refuses_a_directory_and_preserves_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let intent = plan(&store, "occupied.png", b"remote", 3);
    fs::create_dir(store.media_dir().join("occupied.png")).unwrap();
    fs::write(store.media_dir().join("occupied.png/keep"), b"keep").unwrap();
    let source = payload(dir.path(), b"remote");
    assert!(matches!(
        store.save_downloaded_file(&store.sync_baseline().unwrap(), &intent, source.path()),
        Err(MediaError::NameTaken)
    ));
    assert_eq!(fs::read(store.media_dir().join("occupied.png/keep")).unwrap(), b"keep");
    assert!(store.trash_entries().unwrap().is_empty());
}

#[test]
fn no_follow_nonregular_size_and_mutated_source_boundaries_preserve_existing_media() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("clip.png", b"prior").unwrap();
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let link = dir.path().join("payload-link");
    symlink(source.path(), &link).unwrap();
    let too_large = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    too_large.as_file().set_len(104_857_601).unwrap();
    let empty = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    let baseline = store.sync_baseline().unwrap();
    for path in [link.as_path(), dir.path(), too_large.path(), empty.path()] {
        assert!(store.save_downloaded_file(&baseline, &intent, path).is_err());
    }
    let input = source.path().to_owned();
    let armed = Armed::hook("media.download.after_hash", move || {
        fs::write(input, b"mutated source").unwrap();
    });
    assert!(store.save_downloaded_file(&baseline, &intent, source.path()).is_err());
    drop(armed);
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"prior");
    assert!(store.trash_entries().unwrap().is_empty());
    assert_eq!(store.sync_baseline().unwrap(), baseline);
}

#[cfg(target_os = "linux")]
#[test]
fn payload_on_another_volume_is_copied_without_consuming_it() {
    let dir = tempfile::tempdir().unwrap();
    // Docker provides tmpfs here. Some hosts do not: do not require a mount or
    // privileged setup for this fixture.
    let Ok(other) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    if fs::metadata(dir.path()).unwrap().dev() == fs::metadata(other.path()).unwrap().dev() {
        return;
    }
    let store = open(dir.path());
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(other.path(), b"remote");
    let saved = store
        .save_downloaded_file(&store.sync_baseline().unwrap(), &intent, source.path())
        .unwrap();
    assert!(saved.changed);
    assert_eq!(fs::read(source.path()).unwrap(), b"remote");
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
}

#[test]
fn a_foreign_file_winning_the_final_race_is_never_overwritten_and_prior_bytes_stay_in_trash() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("clip.png", b"prior").unwrap();
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let baseline = store.sync_baseline().unwrap();
    let target = store.media_dir().join("clip.png");
    let foreign = target.clone();
    let armed = Armed::hook("media.install.before_publish", move || {
        fs::write(foreign, b"foreign winner").unwrap();
    });
    assert!(store.save_downloaded_file(&baseline, &intent, source.path()).is_err());
    drop(armed);
    assert_eq!(fs::read(target).unwrap(), b"foreign winner");
    assert_eq!(fs::read(source.path()).unwrap(), b"remote");
    assert_eq!(store.trash_entries().unwrap()[0].sha1, hash(b"prior"));
    assert_eq!(store.download_intents().unwrap(), [intent]);
    assert_eq!(store.sync_baseline().unwrap(), baseline);
}

#[test]
fn overlapping_stage_recovery_does_not_consume_the_private_download_copy() {
    let dir = tempfile::tempdir().unwrap();
    let _ownership = meguri_storage::lockfile::CollectionLock::acquire(dir.path()).unwrap();
    let store = open(dir.path());
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let concurrent = store.clone();
    let root = dir.path().to_owned();
    let armed = Armed::hook("media.download.after_stage", move || {
        assert!(meguri_storage::lockfile::CollectionLock::acquire(&root).is_err());
        let copies = fs::read_dir(root.join("media.staging"))
            .unwrap()
            .filter_map(|e| {
                let e = e.unwrap();
                e.file_name()
                    .to_str()
                    .unwrap()
                    .starts_with(".meguri-stage-")
                    .then_some(e.path())
            })
            .collect::<Vec<_>>();
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read(copies[0].join("payload")).unwrap(), b"remote");
        let report = concurrent.recover(|_| Ok(false)).unwrap();
        assert_eq!((report.published, report.trashed), (0, 0));
        concurrent.register_full(|_, _| true).unwrap();
        assert!(concurrent.tracked_names().unwrap().is_empty());
        assert_eq!(fs::read(copies[0].join("payload")).unwrap(), b"remote");
    });
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    drop(armed);
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert!(fs::read_dir(store.staging_dir()).unwrap().next().is_none());
    assert!(
        !fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_str()
            .unwrap()
            .starts_with(".meguri-stage-"))
    );
    assert!(!store.pending_any().unwrap());
}

#[test]
fn coexisting_exact_and_other_case_spellings_replace_only_exact_and_preserve_alternate_journal() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    if !store.diagnostics().case_sensitive {
        return;
    }
    fs::write(store.media_dir().join("Clip.png"), b"remote").unwrap();
    fs::write(store.media_dir().join("clip.png"), b"second").unwrap();
    store.register_full(|_, _| true).unwrap();
    let alternate = store.sync_entry("Clip.png").unwrap();
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    assert!(saved.changed);
    assert_eq!(saved.record.stored_name.as_deref(), Some("clip.png"));
    assert_eq!(fs::read(store.media_dir().join("Clip.png")).unwrap(), b"remote");
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
    assert_eq!(store.sync_entry("Clip.png").unwrap(), alternate);
    let trash = store.trash_entries().unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].sha1, hash(b"second"));
    assert_eq!(store.sync_baseline().unwrap(), baseline);
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert_eq!(store.sync_entry("Clip.png").unwrap(), alternate);
    let exact = store.sync_entry("clip.png").unwrap().unwrap();
    assert!(exact.live && !exact.dirty && !exact.pending_download);
    assert_eq!(exact.sync_state, MediaSyncState::Live);
}

#[test]
fn coexisting_case_spellings_with_identical_exact_content_leave_files_and_journal_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    if !store.diagnostics().case_sensitive {
        return;
    }
    fs::write(store.media_dir().join("Clip.png"), b"alternate").unwrap();
    fs::write(store.media_dir().join("clip.png"), b"same").unwrap();
    store.register_full(|_, _| true).unwrap();
    let intent = plan(&store, "clip.png", b"same", 3);
    let source = payload(dir.path(), b"same");
    let baseline = store.sync_baseline().unwrap();
    let before = store.entries().unwrap();
    let metadata = fs::metadata(store.media_dir().join("clip.png")).unwrap();
    let counter = store.change_counter().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    assert!(!saved.changed);
    assert_eq!(saved.record.stored_name.as_deref(), Some("clip.png"));
    assert_eq!(store.entries().unwrap(), before);
    assert_eq!(store.change_counter().unwrap(), counter);
    let after = fs::metadata(store.media_dir().join("clip.png")).unwrap();
    assert_eq!(
        (after.ino(), after.mtime(), after.mtime_nsec()),
        (metadata.ino(), metadata.mtime(), metadata.mtime_nsec())
    );
    assert!(store.trash_entries().unwrap().is_empty());
    let alternate = store.sync_entry("Clip.png").unwrap();
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    assert_eq!(store.sync_entry("Clip.png").unwrap(), alternate);
    assert_eq!(fs::read(store.media_dir().join("Clip.png")).unwrap(), b"alternate");
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"same");
}

#[test]
fn absent_exact_spelling_reuses_same_content_alias_without_rewriting_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("Clip.PNG", b"same").unwrap();
    let alternate = store.sync_entry("Clip.PNG").unwrap();
    let intent = plan(&store, "clip.png", b"same", 3);
    let source = payload(dir.path(), b"same");
    let baseline = store.sync_baseline().unwrap();
    let saved = store.save_downloaded_file(&baseline, &intent, source.path()).unwrap();
    assert!(!saved.changed);
    assert_eq!(saved.record.stored_name.as_deref(), Some("Clip.PNG"));
    assert!(store.trash_entries().unwrap().is_empty());
    assert_eq!(store.sync_entry("Clip.PNG").unwrap(), alternate);
    store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
    let stored = store.sync_entry("Clip.PNG").unwrap().unwrap();
    assert_eq!(stored.sha1.as_deref(), Some(hash(b"same").as_str()));
    assert!(stored.live && stored.dirty && !stored.pending_download);
    let original = store.sync_entry("clip.png").unwrap().unwrap();
    assert_eq!(original.sync_state, MediaSyncState::Deleted);
    assert!(original.dirty && !original.live);
    let names = fs::read_dir(store.media_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, [std::ffi::OsString::from("Clip.PNG")]);
}

#[test]
fn unchanged_candidates_are_not_rehashed_after_private_staging() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        store.ensure_static_media("clip.png", b"prior").unwrap();
        let intent = plan(&store, "clip.png", b"remote", 3);
        let source = payload(dir.path(), b"remote");
        let _hash = Armed("media.download.existing_hash");
        let _stage = Armed::hook("media.download.after_stage", || {
            // The first observation already hashed the replacement. Any
            // second observation of unchanged candidates fails this save.
            faultpoint::arm("media.download.existing_hash", Fault::IoError(std::io::ErrorKind::Other));
        });
        let baseline = store.sync_baseline().unwrap();
        let saved = if legacy {
            store.save_downloaded_file(&baseline, &intent, source.path())
        } else {
            store.save_downloaded_file_with_boundary(&baseline, &intent, source.path(), &mut || Ok(()))
        }.unwrap();
        assert!(saved.changed);
        assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
        assert_eq!(store.trash_entries().unwrap()[0].sha1, hash(b"prior"));
    }
}

#[test]
fn a_new_case_candidate_after_staging_is_reobserved_and_reused() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        let intent = plan(&store, "clip:1.png", b"remote", 3);
        let source = payload(dir.path(), b"remote");
        let appeared = store.media_dir().join("CLIP1.PNG");
        let _stage = Armed::hook("media.download.after_stage", move || {
            fs::write(appeared, b"remote").unwrap();
        });
        let baseline = store.sync_baseline().unwrap();
        let saved = if legacy {
            store.save_downloaded_file(&baseline, &intent, source.path())
        } else {
            store.save_downloaded_file_with_boundary(&baseline, &intent, source.path(), &mut || Ok(()))
        }.unwrap();
        assert!(!saved.changed);
        assert_eq!(saved.record.stored_name.as_deref(), Some("CLIP1.PNG"));
        assert_eq!(fs::read(store.media_dir().join("CLIP1.PNG")).unwrap(), b"remote");
        let names = fs::read_dir(store.media_dir()).unwrap()
            .map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
        assert_eq!(names, [std::ffi::OsString::from("CLIP1.PNG")]);
        assert!(store.trash_entries().unwrap().is_empty());
    }
}

#[test]
fn a_stale_pre_stage_observation_keeps_all_three_fresh_attempts() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        store.ensure_static_media("clip.png", b"prior").unwrap();
        let intent = plan(&store, "clip.png", b"remote", 3);
        let source = payload(dir.path(), b"remote");
        let target = store.media_dir().join("clip.png");
        let _hash = Armed("media.download.existing_hash");
        let _stage = Armed::hook("media.download.after_stage", move || {
            // Reject the cached observation, then invalidate each of the first
            // two fresh observations during its descriptor-based hash read.
            fs::write(&target, b"stage").unwrap();
            *HOOK.lock().unwrap() = Some(Box::new(move || {
                fs::write(&target, b"first").unwrap();
                *HOOK.lock().unwrap() = Some(Box::new(move || {
                    fs::write(target, b"final").unwrap();
                }));
            }));
            faultpoint::arm("media.download.existing_hash", Fault::Callback(run_hook));
        });
        let baseline = store.sync_baseline().unwrap();
        let saved = if legacy {
            store.save_downloaded_file(&baseline, &intent, source.path())
        } else {
            store.save_downloaded_file_with_boundary(&baseline, &intent, source.path(), &mut || Ok(()))
        }.unwrap();
        assert!(saved.changed);
        assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
        let trash = store.trash_entries().unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].sha1, hash(b"final"));
    }
}

#[test]
fn journal_guard_is_available_during_existing_file_hashes_before_and_after_staging() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for after_stage in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        store.ensure_static_media("clip.png", b"prior").unwrap();
        let intent = plan(&store, "clip.png", b"remote", 3);
        let source = payload(dir.path(), b"remote");
        let reached = Arc::new(AtomicBool::new(false));
        let seen = reached.clone();
        let concurrent = store.clone();
        let probe = move || {
            // try_close never waits: this fails deterministically if hashing
            // holds the journal guard, without deadlocking the test process.
            assert!(concurrent.try_close().unwrap());
            concurrent.resume().unwrap();
            concurrent.sync_entry("clip.png").unwrap();
            seen.store(true, Ordering::SeqCst);
        };
        let _hash = Armed("media.download.existing_hash");
        let _stage = if after_stage {
            let target = store.media_dir().join("clip.png");
            Armed::hook("media.download.after_stage", move || {
                // Invalidate the pre-stage observation so a fresh hash is
                // required. Unchanged candidates now reuse that observation.
                fs::write(target, b"later").unwrap();
                *HOOK.lock().unwrap() = Some(Box::new(probe));
                faultpoint::arm("media.download.existing_hash", Fault::Callback(run_hook));
            })
        } else {
            *HOOK.lock().unwrap() = Some(Box::new(probe));
            faultpoint::arm("media.download.existing_hash", Fault::Callback(run_hook));
            Armed("media.download.after_stage")
        };
        let saved = store
            .save_downloaded_file(&store.sync_baseline().unwrap(), &intent, source.path())
            .unwrap();
        assert!(reached.load(Ordering::SeqCst));
        assert!(saved.changed);
        assert_eq!(store.trash_entries().unwrap()[0].sha1, hash(if after_stage { b"later" } else { b"prior" }));
    }
}

#[test]
fn an_edit_during_existing_file_hashing_is_reobserved_and_its_actual_bytes_are_trashed() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("clip.png", b"prior").unwrap();
    let intent = plan(&store, "clip.png", b"remote", 3);
    let source = payload(dir.path(), b"remote");
    let target = store.media_dir().join("clip.png");
    let armed = Armed::hook("media.download.existing_hash", move || {
        // Change the file already being read, keeping its length unchanged.
        fs::write(target, b"later").unwrap();
    });
    let saved = store
        .save_downloaded_file(&store.sync_baseline().unwrap(), &intent, source.path())
        .unwrap();
    drop(armed);
    assert!(saved.changed);
    let trash = store.trash_entries().unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].sha1, hash(b"later"));
    assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
}

#[test]
fn download_copy_crash_child() {
    let Some(root) = std::env::var_os("MEGURI_DOWNLOAD_COPY_CRASH") else {
        return;
    };
    let root = Path::new(&root);
    let _ownership = meguri_storage::lockfile::CollectionLock::acquire(root).unwrap();
    let store = open(root);
    let intent = plan(&store, "clip.png", b"remote", 3);
    let point = match std::env::var("MEGURI_DOWNLOAD_COPY_PHASE").unwrap().as_str() {
        "partial" => "storage.atomic.after_write",
        "complete" => "media.download.after_stage",
        _ => panic!("unknown crash phase"),
    };
    faultpoint::arm(point, Fault::Abort);
    let _ = store.save_downloaded_file(&store.sync_baseline().unwrap(), &intent, &root.join("caller-payload"));
    unreachable!("the download copy fault did not abort");
}

#[test]
fn killed_download_copies_are_swept_by_owned_startup_before_reopening_and_retrying() {
    for phase in ["partial", "complete"] {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("caller-payload"), b"remote").unwrap();
        fs::write(outside.path().join("keep"), b"outside").unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "download_copy_crash_child", "--nocapture"])
            .current_dir(dir.path())
            .env("MEGURI_DOWNLOAD_COPY_CRASH", dir.path())
            .env("MEGURI_DOWNLOAD_COPY_PHASE", phase)
            .status()
            .unwrap();
        assert!(!status.success());
        let _ownership = meguri_storage::lockfile::CollectionLock::acquire(dir.path()).unwrap();
        let copies = fs::read_dir(dir.path().join("media.staging"))
            .unwrap()
            .filter_map(|e| {
                let e = e.unwrap();
                e.file_name()
                    .to_str()
                    .unwrap()
                    .starts_with(".meguri-stage-")
                    .then_some(e.path())
            })
            .collect::<Vec<_>>();
        assert_eq!(copies.len(), 1);
        let bytes = fs::read_dir(&copies[0])
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(bytes.len(), 1);
        assert_eq!(fs::read(&bytes[0]).unwrap(), b"remote");
        // The reserved copy may contain a link, but cleanup must never cross it.
        symlink(outside.path(), copies[0].join("outside-link")).unwrap();
        symlink(outside.path(), dir.path().join(".meguri-stage-link")).unwrap();
        fs::create_dir(dir.path().join("media.staging/user-files")).unwrap();
        fs::write(dir.path().join("media.staging/user-files/keep"), b"user").unwrap();
        assert_eq!(meguri_storage::leftovers::cleanup(dir.path()).unwrap(), copies);
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
        assert_eq!(
            fs::read(dir.path().join("media.staging/user-files/keep")).unwrap(),
            b"user"
        );
        assert!(
            fs::symlink_metadata(dir.path().join(".meguri-stage-link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(dir.path().join("caller-payload")).unwrap(), b"remote");
        let store = open(dir.path());
        assert_eq!(store.recover(|_| Ok(false)).unwrap().trashed, 0);
        assert!(store.trash_entries().unwrap().is_empty());
        let intent = store.download_intents().unwrap().remove(0);
        let baseline = store.sync_baseline().unwrap();
        assert_eq!(baseline.usn, 2);
        let saved = store
            .save_downloaded_file(&baseline, &intent, &dir.path().join("caller-payload"))
            .unwrap();
        store.commit_media_downloads(&baseline, &[saved.record]).unwrap();
        assert_eq!(fs::read(store.media_dir().join("clip.png")).unwrap(), b"remote");
        assert!(!store.pending_any().unwrap());
    }
}

#[test]
fn startup_copy_cleanup_preserves_legacy_boundaries_and_never_follows_the_staging_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let legacy = outside.path().join(".meguri-tmp-media-crashed");
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join("payload"), b"outside").unwrap();
    symlink(outside.path(), root.path().join("media.staging")).unwrap();
    assert!(meguri_storage::leftovers::cleanup(root.path()).unwrap().is_empty());
    assert_eq!(fs::read(legacy.join("payload")).unwrap(), b"outside");
    fs::remove_file(root.path().join("media.staging")).unwrap();
    fs::create_dir(root.path().join("media.staging")).unwrap();
    let orphan = root.path().join("media.staging/.meguri-tmp-media-crashed");
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("payload"), b"orphan").unwrap();
    symlink(
        outside.path(),
        root.path().join("media.staging/.meguri-tmp-media-linked"),
    )
    .unwrap();
    assert_eq!(meguri_storage::leftovers::cleanup(root.path()).unwrap(), [orphan]);
    assert_eq!(fs::read(legacy.join("payload")).unwrap(), b"outside");
    assert!(
        fs::symlink_metadata(root.path().join("media.staging/.meguri-tmp-media-linked"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
