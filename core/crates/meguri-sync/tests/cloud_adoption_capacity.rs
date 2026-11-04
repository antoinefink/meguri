//! Capacity witnesses through real publication, CoreHost intake and owned files.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    adoption::{AdoptOp, AdoptStep, RecoveryKind},
    entity_codec::{self as codec, Payload},
    publication::PublicationFacts,
};
use meguri_testkit::cloud::{
    Config, Field, LogOutcome, RequestKind, Service, child_driver::IncomingObservation, coordinator::Point,
};
use std::{collections::BTreeMap, path::PathBuf};
use support::*;

const MIB: u64 = 1024 * 1024;
const FLOOR: u64 = 512 * MIB;

// preserve a real old binding/cursor, rather than merely leave an empty host.
fn published(external_payload: bool) -> Pair {
    let root = tempfile::tempdir().unwrap();
    let sender_dir = generated(root.path(), "sender");
    let receiver_dir = generated(root.path(), "receiver");
    let mut service = Service::open(
        root.path(),
        Config {
            page_size: 2,
            ..Config::default()
        },
        SeededRandom::new(811),
    )
    .unwrap();
    let mut receiver = online(&receiver_dir, true);
    receiver.driver().put_media("local.jpg", LOCAL).unwrap();
    publish(&mut receiver, &mut service);
    let mut sender = online(&sender_dir, true);
    if external_payload {
        let note = notes(&mut sender)[0].id;
        let answer = "ordinary adoption payload ".repeat(44_000);
        sender
            .driver()
            .edit_note_fields(note, &["capacity front", &answer])
            .unwrap();
    }
    sender.driver().put_media("remote.jpg", REMOTE).unwrap();
    publish(&mut sender, &mut service);
    Pair {
        service,
        sender,
        receiver,
        receiver_dir,
        root,
    }
}

struct Before {
    facts: PublicationFacts,
    rows: Vec<Payload>,
    media: BTreeMap<String, Vec<u8>>,
    observation: IncomingObservation,
}
impl Before {
    fn capture(pair: &mut Pair) -> Self {
        let facts = pair.receiver.facts().unwrap();
        let binding = facts.current.as_ref().unwrap().id;
        Self {
            facts,
            rows: pair.receiver.driver().collection_rows().unwrap(),
            media: pair.receiver.driver().media_files().unwrap(),
            observation: pair.receiver.driver().incoming_observation(binding).unwrap(),
        }
    }
    fn assert_preserved(&self, pair: &mut Pair, op: &AdoptOp) {
        let after = pair.receiver.facts().unwrap();
        assert!(after.recovery_ops.is_empty());
        assert!(after.current == self.facts.current);
        assert!(after.bindings == self.facts.bindings);
        assert!(!after.bindings.iter().any(|binding| binding.id == op.binding_after));
        assert!(after.decision == self.facts.decision);
        assert_eq!(after.native_file, self.facts.native_file);
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), self.rows);
        assert_eq!(pair.receiver.driver().media_files().unwrap(), self.media);
        let old = self.facts.current.as_ref().unwrap().id;
        let observed = pair.receiver.driver().incoming_observation(old).unwrap();
        // A fetched adoption continuation never grants an SDK collection cursor.
        assert_eq!(observed.engine_state, self.observation.engine_state);
        assert_eq!(
            observed.last_fetch_engine_state,
            self.observation.last_fetch_engine_state
        );
        assert_eq!(observed.seq, self.observation.seq);
        assert_eq!(observed.generation, self.observation.generation);
        assert_eq!(observed.col_mod, self.observation.col_mod);
        assert_eq!(observed.undo_depth, self.observation.undo_depth);
        assert_eq!(observed.change_events, self.observation.change_events);
        let completion = pair.receiver.driver().adoption_completion(op).unwrap();
        assert!(completion.slot_available);
        assert!(!completion.proof_matches);
        assert!(!completion.replacement_recorded);
        assert!(!candidate(pair, op).exists());
        assert_no_stages(pair);
    }
}

fn candidate(pair: &Pair, op: &AdoptOp) -> PathBuf {
    use std::path::{Component, Path};
    let mut components = Path::new(&op.staging_name).components();
    assert!(op.staging_name.starts_with(".meguri-stage-"));
    assert!(op.staging_name.len() > ".meguri-stage-".len());
    assert!(matches!(components.next(), Some(Component::Normal(_))));
    assert!(components.next().is_none());
    pair.receiver_dir.join(&op.staging_name)
}

fn assert_no_stages(pair: &Pair) {
    assert!(std::fs::read_dir(&pair.receiver_dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".meguri-stage-")
    }));
}

fn begin_held(pair: &mut Pair, point: Point) -> AdoptOp {
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(point).unwrap();
    let id = pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(point));
    let facts = pair.receiver.facts().unwrap();
    assert_eq!(facts.recovery_ops.len(), 1);
    let RecoveryKind::Adopt(op) = &facts.recovery_ops[0].kind else {
        panic!("expected the current real adoption row");
    };
    assert_eq!(op.id, id);
    assert_eq!(op.step, AdoptStep::Staging);
    assert!(candidate(pair, op).symlink_metadata().unwrap().file_type().is_dir());
    *op.clone()
}

fn assert_capacity(error: &(dyn std::error::Error + Send + Sync + 'static), available: u64, required: u64) {
    let Some(meguri_storage::StorageError::InsufficientSpace {
        available: actual_available,
        required: actual_required,
    }) = error.downcast_ref::<meguri_storage::StorageError>()
    else {
        panic!("expected typed insufficient capacity from actual CoreHost adoption");
    };
    assert_eq!(*actual_available, available);
    assert_eq!(*actual_required, required);
}

fn finish_success(pair: &mut Pair, before: &Before) {
    let after = pair.receiver.facts().unwrap();
    assert!(after.recovery_ops.is_empty());
    assert_ne!(
        after.current.as_ref().unwrap().id,
        before.facts.current.as_ref().unwrap().id
    );
    assert_ne!(
        after.decision.publication.identity,
        before.facts.decision.publication.identity
    );
    assert_eq!(
        pair.receiver.collection_zone().unwrap(),
        pair.sender.collection_zone().unwrap()
    );
    assert_eq!(
        pair.receiver.driver().collection_rows().unwrap(),
        pair.sender.driver().collection_rows().unwrap()
    );
    assert_eq!(pair.receiver.driver().media_files().unwrap()["local.jpg"], LOCAL);
    assert_eq!(pair.receiver.driver().media_files().unwrap()["remote.jpg"], REMOTE);
    assert_no_stages(pair);
}

fn fetch_count(pair: &Pair, after: u64) -> usize {
    let zone = pair.sender.collection_zone().unwrap();
    pair.service
        .request_log()
        .unwrap()
        .iter()
        .filter(|request| {
            request.request_id > after
                && request.kind == RequestKind::FetchZone
                && request.zone.as_deref() == Some(zone.as_str())
                && request.outcome == LogOutcome::Success
        })
        .count()
}

fn last_request(pair: &Pair) -> u64 {
    pair.service.request_log().unwrap().last().unwrap().request_id
}

// Materialize only this small synthetic fixture. The entity was encoded and
// published by Core, and only one ordinary entity has an external payload.
fn published_payload(pair: &Pair) -> (String, u64) {
    let zone = pair.sender.collection_zone().unwrap();
    let database = pair.service.inspect(&scope()).unwrap().unwrap();
    let records: Vec<_> = database.zones[&zone]
        .records
        .values()
        .filter(|record| record.record_type == "MGEntity" && matches!(record.fields.get("pa"), Some(Field::Asset(_))))
        .collect();
    assert_eq!(records.len(), 1);
    let record = records[0];
    let Field::Asset(asset) = &record.fields["pa"] else {
        unreachable!()
    };
    let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
        panic!("published envelope")
    };
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(envelope).unwrap() else {
        panic!("supported published entity");
    };
    assert!(entity.payload.is_none());
    assert_eq!(entity.fields.kind, codec::Kind::Note);
    assert_eq!(codec::sha256(asset.bytes()), entity.payload_sha256);
    assert!(asset.size() > codec::INLINE_ENVELOPE_BYTES as u64);
    assert!(asset.size() < codec::MAX_PAYLOAD_BYTES);
    (record.name.clone(), asset.size())
}

fn assert_payload_read(pair: &Pair, after: u64, name: &str) {
    let zone = pair.sender.collection_zone().unwrap();
    let reads = pair
        .service
        .request_log()
        .unwrap()
        .into_iter()
        .filter(|request| {
            request.request_id > after
                && request.kind == RequestKind::Read
                && request.zone.as_deref() == Some(zone.as_str())
                && request.outcome == LogOutcome::Success
                && request.records.iter().any(|record| record.name == name)
        })
        .count();
    assert_eq!(reads, 1, "deliver exactly the original Core-selected payload read");
}

#[test]
fn adoption_capacity_floor_refuses_below_512_mib_and_accepts_exact_floor() {
    let mut pair = published(false);
    let before = Before::capture(&mut pair);
    let op = begin_held(&mut pair, Point::AdoptionStagingRecorded);
    pair.receiver.driver().set_available_capacity(Some(FLOOR - 1)).unwrap();
    pair.receiver.release(Point::AdoptionStagingRecorded).unwrap();
    let error = pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap_err();
    assert_capacity(error.as_ref(), FLOOR - 1, FLOOR);
    before.assert_preserved(&mut pair, &op);

    pair.receiver.driver().set_available_capacity(Some(FLOOR)).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::BeforeScan).unwrap();
    let id = pair.receiver.adopt_cloud(&preview).unwrap();
    let pending = pair.receiver.facts().unwrap();
    assert_eq!(pending.recovery_ops.len(), 1);
    let RecoveryKind::Adopt(next) = &pending.recovery_ops[0].kind else {
        panic!("expected the current real adoption row");
    };
    assert_eq!(next.id, id);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeScan));
    let after = pair.receiver.facts().unwrap();
    assert!(after.recovery_ops.is_empty());
    assert_eq!(after.current.as_ref().unwrap().id, next.binding_after);
    assert_ne!(
        after.current.as_ref().unwrap().id,
        before.facts.current.as_ref().unwrap().id
    );
    assert_eq!(
        after.current.as_ref().unwrap().facts.epoch,
        pair.sender.facts().unwrap().current.unwrap().facts.epoch
    );
    assert_eq!(after.decision.publication, next.candidate);
    assert_ne!(
        after.decision.publication.identity,
        before.facts.decision.publication.identity
    );
    assert_ne!(after.native_file, before.facts.native_file);
    assert_eq!(
        pair.receiver.driver().collection_rows().unwrap(),
        pair.sender.driver().collection_rows().unwrap()
    );
    assert_eq!(pair.receiver.driver().media_files().unwrap(), before.media);
    let completion = pair.receiver.driver().adoption_completion(next).unwrap();
    assert!(completion.slot_available);
    assert!(completion.proof_matches);
    assert!(completion.replacement_recorded);
    assert!(!candidate(&pair, next).exists());
    assert_no_stages(&pair);
    pair.receiver.driver().set_available_capacity(None).unwrap();
    pair.receiver.release(Point::BeforeScan).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    finish_success(&mut pair, &before);
    pair.close();
}

#[test]
fn adoption_capacity_asset_write() {
    let mut pair = published(true);
    let (name, payload_bytes) = published_payload(&pair);
    let before = Before::capture(&mut pair);
    pair.receiver.use_metadata_first_adoption();
    let op = begin_held(&mut pair, Point::AdoptionBeforeFetchPayload);
    let directory = candidate(&pair, &op);
    assert_eq!(std::fs::read_dir(directory.join("payloads")).unwrap().count(), 0);
    let bytes = pair.receiver.driver().pad_adoption_candidate().unwrap();
    let padding = directory.join(".capacity-padding");
    assert!(padding.symlink_metadata().unwrap().file_type().is_file());
    assert!(padding.metadata().unwrap().len() > 0);
    let present = (2 * bytes + 64 * MIB).max(FLOOR);
    let required = (2 * (bytes + payload_bytes) + 64 * MIB).max(FLOOR);
    let available = present + payload_bytes;
    assert!(available > FLOOR && available > present && available < required);
    // Entire logical candidate remains below the unchanged 256 MiB staging cap.
    assert!(bytes + payload_bytes < 256 * MIB);
    let after = last_request(&pair);
    pair.receiver.driver().set_available_capacity(Some(available)).unwrap();
    pair.receiver.release(Point::AdoptionBeforeFetchPayload).unwrap();
    let error = pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap_err();
    assert_capacity(error.as_ref(), available, required);
    assert_payload_read(&pair, after, &name);
    before.assert_preserved(&mut pair, &op);
    assert!(!padding.exists());
    pair.receiver.driver().set_available_capacity(None).unwrap();
    pair.close();
}

#[test]
fn adoption_capacity_drop_between_complete_service_pages_discards_staged_rows() {
    let mut pair = published(false);
    let before = Before::capture(&mut pair);
    let after = last_request(&pair);
    let op = begin_held(&mut pair, Point::AdoptionBeforeNextFetch);
    assert_eq!(fetch_count(&pair, after), 1);
    let directory = candidate(&pair, &op);
    assert!(directory.join("collection.anki2").is_file());
    assert!(std::fs::read_dir(directory.join("incoming")).unwrap().count() > 0);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before.rows);
    pair.receiver.driver().set_available_capacity(Some(FLOOR - 1)).unwrap();
    pair.receiver.release(Point::AdoptionBeforeNextFetch).unwrap();
    let error = pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap_err();
    assert_capacity(error.as_ref(), FLOOR - 1, FLOOR);
    assert_eq!(fetch_count(&pair, after), 2);
    before.assert_preserved(&mut pair, &op);
    pair.receiver.driver().set_available_capacity(None).unwrap();
    pair.close();
}

#[test]
fn adoption_capacity_drop_after_metadata_cleans_exact_candidate_and_real_space_recovers() {
    let mut pair = published(true);
    let (name, _) = published_payload(&pair);
    let before = Before::capture(&mut pair);
    pair.receiver.use_metadata_first_adoption();
    let op = begin_held(&mut pair, Point::AdoptionBeforeFetchPayload);
    let directory = candidate(&pair, &op);
    assert!(directory.join("collection.anki2").is_file());
    assert_eq!(std::fs::read_dir(directory.join("payloads")).unwrap().count(), 0);
    let after = last_request(&pair);
    pair.receiver.driver().set_available_capacity(Some(FLOOR - 1)).unwrap();
    pair.receiver.release(Point::AdoptionBeforeFetchPayload).unwrap();
    let error = pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap_err();
    assert_capacity(error.as_ref(), FLOOR - 1, FLOOR);
    assert_payload_read(&pair, after, &name);
    before.assert_preserved(&mut pair, &op);

    // Restore the existing provider in this child, without a reopen or a new
    // executor. A fresh confirmed preview owns the later normal attempt.
    pair.receiver.driver().set_available_capacity(None).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    let next = pair.receiver.adopt_cloud(&preview).unwrap();
    assert_ne!(next, op.id);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    finish_success(&mut pair, &before);
    assert!(!directory.exists());
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
