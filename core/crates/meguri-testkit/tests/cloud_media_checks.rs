//! Real generated collections and production media writes; observations only.
use meguri_host::CoreHost;
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    self as production,
    entity_codec::{Kind, Payload},
    runtime::{CloudInputs, IdentityProvider, IdentityStatus, KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{
        Caller, Config as ServiceConfig, Field, Operation, Reply, RequestedFields, Scope, Service,
        coordinator::PublicationRig,
        devices::{IdentityReads, NativeFiles, options},
        media_observation::{
            FileObservation, MAX_FILE_BYTES, MediaObserver, Missing, NameObservation, Observation, READ_BUFFER_BYTES,
            Refusal, ScopeObservation,
        },
    },
    generate::{
        cloud_small::{self, Request},
        recipe::{Recipe, generate_recipe},
    },
};
use sha1::{Digest as _, Sha1};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

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

struct PublicationKeys(AtomicUsize);
impl KeyByteSource for PublicationKeys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let call = self.0.fetch_add(1, Ordering::SeqCst);
        if call >= 2 {
            return Err(KeySourceError::Unavailable);
        }
        Ok(std::array::from_fn(|index| {
            (index as u8).wrapping_mul(17).wrapping_add(31 + 93 * call as u8)
        }))
    }
}

#[test]
fn published_and_adopted_bound_runtime_pages_preserve_actual_media_facts() {
    const STEPS: usize = 10_000;
    // The production Runtime query currently returns 250 entities per page.
    const FILES: usize = 251;
    let root = tempfile::tempdir().unwrap();
    let sender_dir = root.path().join("sender");
    let receiver_dir = root.path().join("receiver");
    let recipe = Recipe::parse("recipe: bound-media-observation\ndescription: Public synthetic media witness\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Media\n    notetype: basic\n    new: 1\n    fields: [\"generated front {n}\", \"generated back {n}\"]\n").unwrap();
    generate_recipe(&recipe, SEED + 1, &sender_dir).unwrap();
    generate_recipe(&recipe, SEED + 2, &receiver_dir).unwrap();
    let scope = Scope {
        container: "synthetic-container".into(),
        environment: "development".into(),
        account: "synthetic-account".into(),
    };
    let mut service = Service::open(
        root.path(),
        ServiceConfig {
            page_size: 32,
            ..ServiceConfig::default()
        },
        SeededRandom::new(SEED + 3),
    )
    .unwrap();
    let open = |directory: &Path, seed| {
        PublicationRig::open(
            directory,
            &scope,
            IdentityReads::Available(scope.account.clone()),
            false,
            cloud_small::NOW_MS,
            seed,
            Arc::new(PublicationKeys(AtomicUsize::new(0))),
        )
        .unwrap()
    };
    let mut sender = open(&sender_dir, SEED + 4);
    assert!(sender.facts().unwrap().current.is_none());
    let preview = sender.preview(&mut service, STEPS).unwrap();
    sender.confirm_publication(&preview).unwrap();
    sender.drive_ready(&mut service, STEPS).unwrap();
    assert!(sender.facts().unwrap().recovery_ops.is_empty());
    let mut expected = BTreeMap::new();
    for index in 0..FILES {
        let desired = format!("bound-{index:04}.bin");
        // Four distinct tiny blobs, with many real names referring to each.
        let bytes = vec![(index % 4) as u8; 4];
        let stored = sender.driver().put_media(&desired, &bytes).unwrap();
        assert_eq!(stored, desired);
        assert!(expected.insert(stored, bytes).is_none());
    }
    sender.sync_now_and_drain(&mut service, STEPS).unwrap();
    assert!(sender.take_fetch_errors().is_empty());
    assert!(sender.sdk_pending().is_empty());
    let published = sender.facts().unwrap();
    assert!(published.root_acked && published.recovery_ops.is_empty());
    let published_binding = published.current.unwrap();

    let mut receiver = open(&receiver_dir, SEED + 5);
    assert!(receiver.facts().unwrap().current.is_none());
    let preview = receiver.preview(&mut service, STEPS).unwrap();
    receiver.adopt_cloud(&preview).unwrap();
    receiver.drive_ready(&mut service, STEPS).unwrap();
    assert!(receiver.take_fetch_errors().is_empty());
    assert!(receiver.sdk_pending().is_empty());
    let adopted = receiver.facts().unwrap();
    assert!(adopted.recovery_ops.is_empty());
    let adopted_binding = adopted.current.unwrap();
    assert!(adopted_binding.id != published_binding.id);
    assert!(adopted_binding.facts.epoch == published_binding.facts.epoch);
    receiver.close_clean().unwrap();
    sender.close_clean().unwrap();

    let expected_names: BTreeSet<_> = expected.keys().map(hash).collect();
    let expected_hash_counts = expected.values().fold(BTreeMap::new(), |mut counts, bytes| {
        *counts.entry(hash(bytes)).or_insert(0_usize) += 1;
        counts
    });
    assert_eq!(expected_hash_counts.len(), 4);
    let mut previous_files = None;
    for (directory, seed, binding) in [
        (&sender_dir, SEED + 4, published_binding),
        (&receiver_dir, SEED + 5, adopted_binding),
    ] {
        // Reopen only after the actual child has closed its writer and Runtime.
        let mut config = options(directory, cloud_small::NOW_MS, seed);
        config.cloud = Some(CloudInputs {
            scope: production::BuildScope {
                container: production::ContainerId::new(scope.container.clone()).unwrap(),
                environment: production::Environment::Development,
            },
            identity: Arc::new(SyntheticIdentity),
            files: Arc::new(NativeFiles),
        });
        let host = CoreHost::open(config).unwrap();
        let runtime = host.cloud_runtime().unwrap();
        runtime.refresh_identity().unwrap();
        let collection = host.collection().unwrap();
        let media = host.media().unwrap();
        runtime
            .reconcile_binding(&collection, &media, Duration::from_secs(30))
            .unwrap();
        let before = runtime.status().unwrap();
        assert!(before.binding.as_ref() == Some(&binding));
        assert_eq!(before.generation, Some(collection.generation()));
        assert!(before.stored_usable && before.identity_verified && before.scan_drained);
        assert!(!before.scan_required && !before.scan_running && !before.tracking_stopped);
        assert_eq!(binding.facts.state, production::BindingState::Active);
        let journal_before = media.store().entries().unwrap();
        let tracked_before = media.tracked_names().unwrap();

        // Query the actual Runtime, including its real continuation and EOF.
        let mut entities = runtime.entity_facts_page(Kind::MediaName, None).unwrap();
        assert_eq!(entities.len(), 250);
        let next = runtime
            .entity_facts_page(Kind::MediaName, Some(&entities.last().unwrap().key))
            .unwrap();
        assert_eq!(next.len(), 1);
        assert!(entities.last().unwrap().key < next[0].key);
        entities.extend(next);
        assert!(
            runtime
                .entity_facts_page(Kind::MediaName, Some(&entities.last().unwrap().key))
                .unwrap()
                .is_empty()
        );
        for entity in &entities {
            assert_eq!(entity.local_rev, entity.acked_rev);
            assert!(entity.server_erev > 0 && entity.mark.is_none() && entity.in_flight.is_empty());
            assert!(entity.acked_payload.as_ref() == Some(&entity.payload));
            // Compare opaque ACK evidence with an ordinary actual Service read.
            let response = service
                .request(
                    &scope,
                    Caller::default(),
                    Operation::Read {
                        zone: production::adapter::collection_zone(binding.facts.epoch),
                        name: entity.name.clone(),
                        fields: RequestedFields::All,
                    },
                )
                .unwrap();
            assert!(response.request_id > 0);
            let Ok(Reply::Record(record)) = response.result else {
                panic!("actual published media entity receipt required");
            };
            assert_eq!(record.record_type, "MGEntity");
            assert_eq!(record.name, entity.name);
            assert!(record.fields.get("e") == Some(&Field::EncryptedBytes(entity.acked_envelope.0.clone())));
            assert_eq!(record.system_fields.bytes(), entity.acked_system_fields);
        }

        let observer = MediaObserver::new(&host).unwrap();
        let observation = observer.observe().unwrap();
        assert_eq!(observation.generation, collection.generation().0);
        assert_eq!(
            observation.scope,
            ScopeObservation::Bound {
                scope_sha256: hash(serde_json::to_vec(&binding.scope).unwrap()),
                account_sha256: hash(&binding.facts.account.0),
                binding_sha256: hash(binding.id.0),
                epoch_sha256: hash(binding.facts.epoch.0),
                state: production::BindingState::Active,
                stored_usable: true,
                identity_verified: true,
                scan_required: false,
                scan_running: false,
                scan_drained: true,
                tracking_stopped: false,
            }
        );
        assert_eq!(observation.names.len(), FILES);
        assert_eq!(observation.entities.len(), FILES);
        assert_eq!(
            observation.names.iter().map(|row| row.name_sha256).collect::<BTreeSet<_>>(),
            expected_names
        );
        assert_eq!(
            observation.entities.iter().map(|row| row.name_sha256).collect::<BTreeSet<_>>(),
            expected_names
        );
        let mut actual_hash_counts = BTreeMap::new();
        for (row, entity) in observation.entities.iter().zip(&entities) {
            let Payload::MediaName(payload) = &entity.payload else {
                panic!("actual media payload required");
            };
            assert_eq!(row.name_sha256, hash(&payload.name));
            assert_eq!(row.case_key_sha256, hash(&entity.key));
            assert_eq!(
                (row.sha256, row.sha1, row.size),
                (payload.sha256, payload.sha1, payload.size)
            );
            assert_eq!((row.local_rev, row.acked_rev), (entity.local_rev, entity.acked_rev));
            assert!(entity.tombstone_at.is_none());
            assert!(!row.tombstone && !row.has_dirty_mark && row.in_flight == 0);
            let file = &observation.names.iter().find(|name| name.name_sha256 == row.name_sha256).unwrap().file;
            let FileObservation::Present(actual) = file else {
                panic!("actual file for runtime entity required");
            };
            assert_eq!((row.sha256, row.sha1, row.size), (actual.sha256, actual.sha1, actual.size));
            *actual_hash_counts.entry(row.sha256).or_insert(0_usize) += 1;
        }
        assert_eq!(actual_hash_counts, expected_hash_counts);
        for (stored, bytes) in &expected {
            let row = name(&observation, stored);
            let FileObservation::Present(actual) = &row.file else {
                panic!("actual regular media file required");
            };
            let expected_sha1: [u8; 20] = Sha1::digest(bytes).into();
            assert_eq!(actual.sha256, hash(bytes));
            assert_eq!(actual.sha1, expected_sha1);
            assert_eq!(actual.size, bytes.len() as u64);
            assert_eq!(actual.read_calls, 2);
            assert_eq!(actual.largest_read, bytes.len());
            assert!(actual.largest_read <= READ_BUFFER_BYTES);
            assert_eq!(row.stored_name_sha256, Some(hash(stored)));
            assert_eq!(row.case_key_sha256, hash(stored.to_lowercase()));
            let journal = row.journal.as_ref().unwrap();
            assert!(journal.tracked && journal.live);
            assert_eq!((journal.sha1, journal.size), (Some(expected_sha1), actual.size));
        }
        let files: BTreeMap<_, _> = observation
            .names
            .iter()
            .map(|row| (row.name_sha256, row.file.clone()))
            .collect();
        if let Some(previous) = &previous_files {
            assert_eq!(&files, previous);
        }
        previous_files = Some(files);
        assert_eq!(
            observation.missing,
            [
                Missing::RuntimeMissingNameList,
                Missing::RuntimeMediaNameDisposition,
                Missing::InstallAndTrashHistory,
                Missing::OriginalRootPinnedRead,
            ]
        );
        assert_eq!(observer.observe().unwrap(), observation);
        let after = runtime.status().unwrap();
        assert!(after.binding == before.binding);
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.change_counter, before.change_counter);
        assert_eq!(media.store().entries().unwrap(), journal_before);
        assert_eq!(media.tracked_names().unwrap(), tracked_before);
        drop(observer);
        host.close().unwrap();
    }
}

// Subprocess IPC entry required by PublicationRig; no product assertion.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
