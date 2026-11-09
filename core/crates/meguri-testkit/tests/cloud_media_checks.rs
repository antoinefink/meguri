//! Real generated collections and production media writes; observations only.
use meguri_host::CoreHost;
use meguri_sync::cloud::{
    self as production,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus},
};
use meguri_testkit::{
    cloud::{
        devices::options,
        media_observation::{
            FileObservation, MAX_FILE_BYTES, MediaObserver, Missing, NameObservation, Observation, READ_BUFFER_BYTES,
            Refusal, ScopeObservation,
        },
    },
    generate::cloud_small::{self, Request},
};
use sha1::{Digest as _, Sha1};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, os::unix::fs::symlink, path::PathBuf, sync::Arc};

const SEED: u64 = 0x4d45_4449;
fn fixture() -> (tempfile::TempDir, PathBuf, CoreHost) {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("device");
    cloud_small::generate(&Request::new(SEED), &directory).unwrap();
    let host = CoreHost::open(options(&directory, cloud_small::NOW_MS, SEED)).unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    (temporary, directory, host)
}
fn hash(bytes: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn name<'a>(observation: &'a Observation, stored: &str) -> &'a NameObservation {
    observation
        .names
        .iter()
        .find(|row| row.name_sha256 == hash(stored))
        .expect("observed name hash")
}

#[test]
fn generated_files_and_production_write_have_actual_hashes_and_bounded_reads() {
    let (_temporary, directory, host) = fixture();
    let media = host.media().unwrap();
    let content: Vec<u8> = (0..3 * READ_BUFFER_BYTES + 19).map(|n| (n % 251) as u8).collect();
    let stored = media.write_exact("observation.bin", &content).unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    let observation = observer.observe().unwrap();
    assert_eq!(observation.names.len(), 6);
    assert_eq!(observation.scope, ScopeObservation::NoRuntime);
    assert!(observation.entities.is_empty());
    assert!(observation.missing.contains(&Missing::OriginalRootPinnedRead));
    assert!(observation.missing.contains(&Missing::InstallAndTrashHistory));
    for entry in fs::read_dir(directory.join("collection.media")).unwrap() {
        let entry = entry.unwrap();
        let filename = entry.file_name().into_string().unwrap();
        // Independent expectations from these small synthetic files only.
        let bytes = fs::read(entry.path()).unwrap();
        let row = name(&observation, &filename);
        let FileObservation::Present(actual) = &row.file else {
            panic!("actual regular file observation");
        };
        assert_eq!(actual.sha256, hash(&bytes));
        let expected_sha1: [u8; 20] = Sha1::digest(&bytes).into();
        assert_eq!(actual.sha1, expected_sha1);
        assert_eq!(actual.size, bytes.len() as u64);
        assert_eq!(row.stored_name_sha256, Some(hash(filename)));
        assert!(actual.largest_read <= READ_BUFFER_BYTES);
        assert!(row.journal.as_ref().unwrap().tracked);
    }
    let FileObservation::Present(actual) = &name(&observation, &stored).file else {
        panic!("production write observation");
    };
    assert_eq!(actual.size, content.len() as u64);
    assert!(actual.read_calls >= 5); // four data reads and EOF
    assert_eq!(actual.largest_read, READ_BUFFER_BYTES);
    // Public diagnostics and artifact serialization contain no raw names/text.
    let debug = format!("{observer:?} {observation:?}");
    let json = serde_json::to_string(&observation).unwrap();
    let marker = Request::new(SEED).marker();
    for output in [&debug, &json] {
        assert!(!output.contains(&stored));
        assert!(!output.contains(&marker));
        assert!(!output.contains("collection.media"));
    }
    drop(observer);
    host.close().unwrap();
}

#[test]
fn changed_actual_bytes_are_detected_even_when_journal_size_and_mtime_stay_old() {
    let (_temporary, _directory, host) = fixture();
    let media = host.media().unwrap();
    let stored = media.write_exact("changed.bin", b"AAAA").unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    let before = observer.observe().unwrap();
    let path = media.store().media_dir().join(&stored);
    let mut file = fs::OpenOptions::new().write(true).open(path).unwrap();
    let modified = file.metadata().unwrap().modified().unwrap();
    file.write_all(b"BBBB").unwrap();
    file.set_modified(modified).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let after = observer.observe().unwrap();
    assert_eq!(name(&before, &stored).journal, name(&after, &stored).journal);
    let FileObservation::Present(old) = &name(&before, &stored).file else {
        panic!("original bytes observed");
    };
    let FileObservation::Present(changed) = &name(&after, &stored).file else {
        panic!("changed bytes observed");
    };
    assert_eq!(old.size, changed.size);
    assert_ne!(old.sha256, changed.sha256);
    assert_eq!(changed.sha256, hash(b"BBBB"));
    assert_ne!(name(&after, &stored).journal.as_ref().unwrap().sha1, Some(changed.sha1));
    drop(observer);
    host.close().unwrap();
}

#[test]
fn filesystem_absence_does_not_supply_a_runtime_missing_list() {
    let (_temporary, _directory, host) = fixture();
    let media = host.media().unwrap();
    let stored = media.write_exact("absent.bin", b"synthetic").unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    fs::remove_file(media.store().media_dir().join(&stored)).unwrap();
    let observation = observer.observe().unwrap();
    assert_eq!(name(&observation, &stored).file, FileObservation::Absent);
    assert!(name(&observation, &stored).journal.as_ref().unwrap().live);
    assert!(observation.missing.contains(&Missing::RuntimeMissingNameList));
    assert!(observation.missing.contains(&Missing::RuntimeMediaNameDisposition));
    assert_eq!(observation.names.len(), 6);
    drop(observer);
    host.close().unwrap();
}

#[test]
fn symlink_terminal_is_refused_without_hashing_its_target() {
    let (temporary, _directory, host) = fixture();
    let media = host.media().unwrap();
    let stored = media.write_exact("symlink.bin", b"synthetic").unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    let target = temporary.path().join("outside");
    fs::write(&target, b"outside synthetic bytes").unwrap();
    let path = media.store().media_dir().join(&stored);
    fs::remove_file(&path).unwrap();
    symlink(&target, &path).unwrap();
    let observation = observer.observe().unwrap();
    assert_eq!(
        name(&observation, &stored).file,
        FileObservation::Refused(Refusal::UnsafeFile)
    );
    assert_eq!(name(&observation, &stored).stored_name_sha256, None);
    fs::remove_file(path).unwrap();
    drop(observer);
    host.close().unwrap();
}

#[test]
fn retained_observer_refuses_symlink_and_replaced_media_roots() {
    let (_temporary, directory, host) = fixture();
    let media = host.media().unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    let root = media.store().media_dir();
    let parked = directory.join("parked-media");
    fs::rename(root, &parked).unwrap();
    symlink(&parked, root).unwrap();
    assert_eq!(observer.observe().unwrap_err(), Refusal::UnsafeRoot);
    assert_eq!(MediaObserver::new(&host).unwrap_err(), Refusal::UnsafeRoot);
    fs::remove_file(root).unwrap();
    fs::create_dir(root).unwrap();
    assert_eq!(observer.observe().unwrap_err(), Refusal::RootChanged);
    fs::remove_dir(root).unwrap();
    fs::rename(parked, root).unwrap();
    assert_eq!(observer.observe().unwrap().names.len(), 5);
    drop(observer);
    host.close().unwrap();
}

#[test]
fn journal_iteration_exceeds_inspect_cap_and_file_limit_refuses_unbounded_input() {
    let (temporary, _directory, host) = fixture();
    let media = host.media().unwrap();
    for index in 0..2050 {
        media
            .write_exact(&format!("scope-{index:04}.bin"), &[index as u8])
            .unwrap();
    }
    fs::write(temporary.path().join("unowned.bin"), b"synthetic").unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    let observation = observer.observe().unwrap();
    assert_eq!(observation.names.len(), 2055);
    assert!(
        observation
            .names
            .iter()
            .all(|row| matches!(&row.file, FileObservation::Present(_)))
    );
    assert!(
        observation
            .names
            .iter()
            .all(|row| row.name_sha256 != hash("unowned.bin"))
    );
    let stored = media.write_exact("too-large.bin", b"synthetic").unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(media.store().media_dir().join(&stored))
        .unwrap();
    file.set_len(MAX_FILE_BYTES + 1).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let after = observer.observe().unwrap();
    assert_eq!(after.names.len(), 2056);
    assert_eq!(
        name(&after, &stored).file,
        FileObservation::Refused(Refusal::FileTooLarge)
    );
    drop(observer);
    host.close().unwrap();
}

#[test]
fn closed_host_cannot_supply_retained_media_evidence() {
    let (_temporary, _directory, host) = fixture();
    let observer = MediaObserver::new(&host).unwrap();
    host.close().unwrap();
    assert_eq!(observer.observe().unwrap_err(), Refusal::OwnerUnavailable);
}

struct SyntheticIdentity;
impl IdentityProvider for SyntheticIdentity {
    fn read_identity(&self, _: &production::BuildScope) -> IdentityStatus {
        IdentityStatus::Available(production::OpaqueBytes(b"synthetic-account".to_vec()))
    }
}

#[test]
fn unbound_runtime_is_distinct_from_absent_runtime_and_empty_missing_list() {
    let (_temporary, directory, first) = fixture();
    first.close().unwrap();
    let mut config = options(&directory, cloud_small::NOW_MS, SEED);
    config.cloud = Some(CloudInputs {
        scope: production::BuildScope {
            container: production::ContainerId::new("synthetic-container").unwrap(),
            environment: production::Environment::Development,
        },
        identity: Arc::new(SyntheticIdentity),
        files: Arc::new(meguri_testkit::cloud::devices::NativeFiles),
    });
    let host = CoreHost::open(config).unwrap();
    let observer = MediaObserver::new(&host).unwrap();
    let observation = observer.observe().unwrap();
    assert_eq!(observation.scope, ScopeObservation::Unbound);
    assert!(observation.entities.is_empty());
    assert!(observation.missing.contains(&Missing::RuntimeMissingNameList));
    let output = serde_json::to_string(&observation).unwrap();
    assert!(!output.contains("synthetic-account"));
    assert!(!output.contains("synthetic-container"));
    drop(observer);
    host.close().unwrap();
}
