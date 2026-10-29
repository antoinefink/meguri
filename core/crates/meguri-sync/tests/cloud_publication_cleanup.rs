//! Tests FIRST: fresh cleanup authorization after an actual deletion reply wait.
//! Extract with the queue/reply hold seam before applying the authorization fix.
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    control_codec::Active,
    runtime::{KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{
        coordinator::{Point, PublicationRig},
        devices::IdentityReads,
        *,
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const NOW: i64 = 1_735_689_600_000;
const DAY: i64 = 86_400_000;
const STEPS: usize = 10_000;

// Platform key input, separate from collection-id randomness. Never Debug.
struct Keys(AtomicUsize);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let draw = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(19).wrapping_add(draw as u8)
        }))
    }
}
fn keys() -> Arc<Keys> {
    Arc::new(Keys(AtomicUsize::new(0)))
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.cleanup".into(),
        environment: "development".into(),
        account: "synthetic-account".into(),
    }
}
fn generated(directory: &Path, seed: u64) {
    let recipe = Recipe::parse("recipe: cleanup\ndescription: Public synthetic cleanup\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Child\n    notetype: basic\n    new: 1\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, seed, directory).unwrap();
}
fn open(directory: &Path, now: i64, seed: u64, keys: Arc<Keys>) -> PublicationRig {
    let scope = scope();
    PublicationRig::open(
        directory,
        &scope,
        IdentityReads::Available(scope.account.clone()),
        false,
        now,
        seed,
        keys,
    )
    .unwrap()
}
fn active(service: &Service<SeededRandom>) -> Active {
    let state = service.inspect(&scope()).unwrap().unwrap();
    let Field::EncryptedBytes(e) = &state.zones["meguri-root"].records["active"].fields["e"] else {
        panic!("actual encrypted Active required")
    };
    Active::decode(e).unwrap()
}
fn cursor(service: &Service<SeededRandom>) -> u64 {
    service.request_log().unwrap().last().map_or(0, |r| r.request_id)
}
fn prepare_pending(rig: &mut PublicationRig, service: &mut Service<SeededRandom>) {
    let preview = rig.preview(service, STEPS).unwrap();
    rig.hold(Point::BeforeEpochRead).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeEpochRead));
    assert!(!rig.facts().unwrap().recovery_ops.is_empty());
}

#[test]
fn cleanup_reauthorizes_the_second_zone_after_the_first_actual_deletion_reply_wait() {
    let run = tempfile::tempdir().unwrap();
    let owner_dir = run.path().join("owner-a");
    let b_dir = run.path().join("publisher-b");
    let c_dir = run.path().join("publisher-c");
    generated(&owner_dir, 71);
    generated(&b_dir, 72);
    generated(&c_dir, 73);
    let owner_keys = keys();
    let b_keys = keys();
    let c_keys = keys();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(74)).unwrap();

    let mut owner = open(&owner_dir, NOW, 71, owner_keys.clone());
    let preview = owner.preview(&mut service, STEPS).unwrap();
    owner.confirm_publication(&preview).unwrap();
    owner.drive_ready(&mut service, STEPS).unwrap();
    assert!(owner.facts().unwrap().recovery_ops.is_empty());
    let owner_active = active(&service);
    owner.close_clean().unwrap();

    // Both real publishers create their own zones and send actual entities,
    // but neither reaches its Active switch. No fixture binding or ACKs.
    let mut b = open(&b_dir, NOW, 72, b_keys.clone());
    let mut c = open(&c_dir, NOW, 73, c_keys.clone());
    prepare_pending(&mut b, &mut service);
    prepare_pending(&mut c, &mut service);
    let b_zone = b.collection_zone().unwrap();
    let c_zone = c.collection_zone().unwrap();
    assert_ne!(b_zone, c_zone);
    assert!(active(&service) == owner_active);
    let observed_after = cursor(&service);
    let mut owner = open(&owner_dir, NOW, 71, owner_keys.clone());
    owner.drive_ready(&mut service, STEPS).unwrap();
    assert!(
        service
            .request_log()
            .unwrap()
            .iter()
            .any(|r| r.request_id > observed_after
                && r.kind == RequestKind::FetchDatabase
                && r.outcome == LogOutcome::Success)
    );
    let seen = owner.facts().unwrap();
    assert_eq!(seen.zones.len(), 2);
    for zone in [&b_zone, &c_zone] {
        assert_eq!(seen.zones[zone].first_seen_ms, NOW);
        assert!(!seen.zones[zone].deletion_due);
    }
    owner.kill().unwrap(); // No owner activity during the retention period.

    let mut owner = open(&owner_dir, NOW + 30 * DAY, 71, owner_keys.clone());
    owner.drive_ready(&mut service, STEPS).unwrap();
    owner.hold(Point::ZoneDeletionReply).unwrap();
    let cleanup_after = cursor(&service);
    owner.sync_now_and_drain(&mut service, STEPS).unwrap();
    assert!(owner.is_held(Point::ZoneDeletionReply));
    let request = owner.held_request().unwrap();
    let again = owner.held_request().unwrap();
    assert!(request.caller == again.caller && request.operation == again.operation);
    let Operation::DeleteZone {
        zone: first,
        cause: RemovalCause::Deleted,
    } = &request.operation
    else {
        panic!("original queued deletion required")
    };
    let first = first.clone();
    let second = if first == b_zone {
        c_zone.clone()
    } else {
        assert_eq!(first, c_zone);
        b_zone.clone()
    };
    assert!(request.caller.instance.is_none());
    assert_eq!(request.caller.own_operation.as_deref(), Some("publication-cleanup"));
    let deletions: Vec<_> = service
        .request_log()
        .unwrap()
        .into_iter()
        .filter(|r| r.request_id > cleanup_after && r.kind == RequestKind::DeleteZone)
        .collect();
    assert_eq!(deletions.len(), 1);
    let first_log = &deletions[0];
    assert_eq!(first_log.zone.as_deref(), Some(first.as_str()));
    assert!(first_log.caller == request.caller);
    assert!(first_log.outcome == LogOutcome::Success);
    let first_result = service
        .outcomes()
        .unwrap()
        .into_iter()
        .find(|r| r.request_id == first_log.request_id)
        .unwrap();
    assert!(matches!(&first_result.result, Ok(ReplyReference::Done)));
    let state = service.inspect(&scope()).unwrap().unwrap();
    assert!(state.zones[&first].removed.is_some());
    assert!(state.zones[&second].removed.is_none());
    let waiting = owner.facts().unwrap();
    // Service succeeded, but the child has not consumed the actual reply yet.
    assert!(waiting.zones[&first].deletion_due && waiting.zones[&second].deletion_due);

    // Select the OTHER actual publisher, independently of random zone order.
    let publisher = if second == b_zone { &mut b } else { &mut c };
    let promoted_epoch = publisher.facts().unwrap().current.unwrap().facts.epoch.0;
    publisher.advance_clock_ms(30 * DAY).unwrap();
    publisher.release(Point::BeforeEpochRead).unwrap();
    publisher.drive_ready(&mut service, STEPS).unwrap();
    assert!(publisher.facts().unwrap().recovery_ops.is_empty());
    let promoted = active(&service);
    assert_eq!(promoted.epoch, promoted_epoch);
    assert_ne!(promoted.epoch, owner_active.epoch);
    assert_eq!(promoted.activated_at_ms, NOW + 30 * DAY);
    let resumed_after = cursor(&service);
    owner.release(Point::ZoneDeletionReply).unwrap();
    owner.drive_ready(&mut service, STEPS).unwrap();

    let logs = service.request_log().unwrap();
    assert!(
        !logs
            .iter()
            .any(|r| r.kind == RequestKind::DeleteZone && r.zone.as_deref() == Some(second.as_str()))
    );
    assert_eq!(
        logs.iter()
            .filter(|r| r.request_id > cleanup_after && r.kind == RequestKind::DeleteZone)
            .count(),
        1
    );
    assert!(logs.iter().any(|r| r.request_id > resumed_after
        && r.kind == RequestKind::Read
        && r.zone.as_deref() == Some("meguri-root")
        && r.outcome == LogOutcome::Success));
    let state = service.inspect(&scope()).unwrap().unwrap();
    assert!(state.zones[&first].removed.is_some());
    assert!(state.zones[&second].removed.is_none());
    assert!(active(&service) == promoted);
    let done = owner.facts().unwrap();
    assert!(!done.zones.contains_key(&first));
    assert_eq!(done.zones[&second].first_seen_ms, NOW);
    assert!(done.zones[&second].deletion_due);
    assert!(service.outcomes().unwrap().iter().any(|r| r == &first_result));
    for keys in [&owner_keys, &b_keys, &c_keys] {
        assert_eq!(keys.0.load(Ordering::SeqCst), 2);
    }
    owner.close_clean().unwrap();
    let mut reopened = open(&owner_dir, NOW + 30 * DAY, 71, owner_keys.clone());
    let durable = reopened.facts().unwrap();
    assert!(!durable.zones.contains_key(&first));
    assert_eq!(durable.zones[&second].first_seen_ms, NOW);
    assert!(durable.zones[&second].deletion_due);
    assert_eq!(owner_keys.0.load(Ordering::SeqCst), 2);
    reopened.close_clean().unwrap();
    b.close_clean().unwrap();
    c.close_clean().unwrap();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
