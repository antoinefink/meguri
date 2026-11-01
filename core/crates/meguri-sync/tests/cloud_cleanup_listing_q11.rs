//! Q11 / P24 confirmed: A21-false inventory must not undo a deletion result.
//! Tests first; uses the existing names-only runtime callback, without a fix.
use meguri_engine::{device_state::DeviceState, ports::OpenStep};
use meguri_host::CoreHost;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{
    BindingState, BuildScope, ContainerId, Environment, OpaqueBytes,
    control_codec::Active,
    publication::RecordRead,
    runtime::{
        CloudInputs, FileIdentityProvider, IdentityProvider, IdentityStatus, KeyByteSource, KeySourceError, Runtime,
    },
};
use meguri_testkit::{
    cloud::{
        coordinator::{PublicationRig, publication_control_record},
        devices::{IdentityReads, NativeFiles, options},
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
    time::Duration,
};

const NOW: i64 = 1_735_689_600_000;
const DAY: i64 = 86_400_000;
const STEPS: usize = 10_000;

struct Keys(AtomicUsize);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let draw = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(23).wrapping_add(draw as u8)
        }))
    }
}
struct Account;
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        IdentityStatus::Available(OpaqueBytes(scope().account.into_bytes()))
    }
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.cleanup-listing-q11".into(),
        environment: "development".into(),
        account: "synthetic-listing-account".into(),
    }
}
fn generated(directory: &Path, seed: u64) {
    let recipe = Recipe::parse("recipe: cleanup-listing\ndescription: Synthetic Q11 inventory\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Child\n    notetype: basic\n    new: 1\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
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
fn listing(service: &mut Service<SeededRandom>) -> Response {
    service
        .request(
            &scope(),
            Caller {
                own_operation: Some("synthetic-a21-listing".into()),
                ..Caller::default()
            },
            Operation::FetchDatabase { token: None },
        )
        .unwrap()
}
fn listed_zones(response: &Response) -> Vec<String> {
    let Ok(Reply::DatabaseChanges(page)) = &response.result else {
        panic!("actual database inventory required")
    };
    assert!(!page.more, "the small synthetic inventory must fit one page");
    page.changes
        .iter()
        .filter(|change| change.removed.is_none())
        .map(|change| change.zone.clone())
        .collect()
}

/// conformance/cloudkit-acceptance#Q11 row 456; sync/policy#P10.3;
/// sync/cloudkit-recovery#V26.1 A21 false. No timestamp or guard API is invented.
#[test]
fn p24_q11_a21_false_listing_captured_before_delete_zones_handled_after_result_leaves_no_row() {
    let run = tempfile::tempdir().unwrap();
    let old_dir = run.path().join("old-publisher");
    let current_dir = run.path().join("current-publisher");
    generated(&old_dir, 81);
    generated(&current_dir, 82);
    let old_keys = Arc::new(Keys(AtomicUsize::new(0)));
    let current_keys = Arc::new(Keys(AtomicUsize::new(0)));
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(83)).unwrap();

    // Both epochs are actually published, including their entities and Active.
    // No prebound fixture, seeded ACK or hand-written retained-zone row.
    let mut old = open(&old_dir, NOW, 81, old_keys);
    let preview = old.preview(&mut service, STEPS).unwrap();
    old.confirm_publication(&preview).unwrap();
    old.drive_ready(&mut service, STEPS).unwrap();
    assert!(old.facts().unwrap().recovery_ops.is_empty());
    let old_zone = old.collection_zone().unwrap();
    let old_epoch = old.facts().unwrap().current.unwrap().facts.epoch.0;
    old.close_clean().unwrap();

    let mut current = open(&current_dir, NOW, 82, current_keys.clone());
    let preview = current.preview(&mut service, STEPS).unwrap();
    current.confirm_publication(&preview).unwrap();
    current.drive_ready(&mut service, STEPS).unwrap();
    let published = current.facts().unwrap();
    assert!(published.recovery_ops.is_empty());
    assert_eq!(published.zones[&old_zone].first_seen_ms, NOW);
    let current_binding = published.current.unwrap();
    let current_zone = current.collection_zone().unwrap();
    assert_ne!(old_zone, current_zone);
    current.close_clean().unwrap();

    // The rig has no public direct inventory-delivery method. Open the actual
    // files through CoreHost with their saved publication binding, then bind the
    // production Runtime through its public lifecycle step. This is the same durable store,
    // not a model of the zone table or a replacement Service.
    let now = NOW + 30 * DAY;
    let clock = Arc::new(FixedClock::new(now, UtcOffset::UTC));
    let path = current_dir.join("collection.anki2");
    let decision = DeviceState::for_collection(&path)
        .publication_decision()
        .unwrap()
        .expect("the actual publication must have a durable decision");
    assert!(decision == published.decision);
    assert_eq!(NativeFiles.identity(&path).unwrap(), published.native_file);
    let mut config = options(&current_dir, now, 82);
    config.clock = clock.clone();
    config.publication_binding = decision.publication.binding.clone();
    let host = CoreHost::open(config).unwrap();
    assert_eq!(NativeFiles.identity(&path).unwrap(), published.native_file);
    let collection = host.collection().unwrap();
    let runtime = Runtime::new(
        current_dir.clone(),
        CloudInputs {
            scope: BuildScope {
                container: ContainerId::new(scope().container).unwrap(),
                environment: Environment::Development,
            },
            identity: Arc::new(Account),
            files: Arc::new(NativeFiles),
        },
    );
    runtime.attach_adapter_clock(clock);
    runtime.attach_sources(Arc::new(SeededRandom::new(84)), Default::default());
    runtime.attach_key_bytes(current_keys.clone());
    OpenStep::run(&runtime, &collection).unwrap();
    runtime.refresh_identity().unwrap();
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    runtime
        .reconcile_binding(&collection, &media, Duration::from_secs(30))
        .unwrap();
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let initial = listing(&mut service);
    runtime
        .publication_zones_seen(&instance, &listed_zones(&initial))
        .unwrap();
    runtime.sdk_fetch_completed_without_root(&instance).unwrap();
    let read = service
        .request(
            &scope(),
            Caller {
                instance: Some(instance.clone()),
                ..Caller::default()
            },
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let Ok(Reply::Record(record)) = &read.result else {
        panic!("actual Active read required")
    };
    let control = publication_control_record(record).unwrap();
    let active = Active::decode(control.encrypted.as_deref().unwrap()).unwrap();
    assert_eq!(active.epoch, current_binding.facts.epoch.0);
    assert_eq!(active.previous, Some(old_epoch));
    assert!(
        runtime
            .accept_root_read(
                &instance,
                read.request_id,
                control.encrypted.as_deref().unwrap(),
                &control.system_fields.0,
                false,
            )
            .unwrap()
    );
    runtime.publication_collection_drained(&instance).unwrap();
    assert!(runtime.publication_control_for_testing().unwrap().zones[&old_zone].deletion_due);

    // A21 false: an own all-zone inventory is captured now and kept verbatim.
    // No new fetch substitutes for this response after DeleteZones completes.
    let held_listing = listing(&mut service);
    let held_names = listed_zones(&held_listing);
    assert!(held_names.contains(&old_zone) && held_names.contains(&current_zone));
    let guard = runtime.begin_zone_deletion_read(&instance, &collection).unwrap();
    let fresh = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let Ok(Reply::Record(record)) = &fresh.result else {
        panic!("fresh deletion authorization requires actual Active")
    };
    let authorized = runtime
        .zone_deletion_check(
            &guard,
            RecordRead::Found(publication_control_record(record).unwrap()),
            &collection,
        )
        .unwrap();
    assert_eq!(authorized, vec![old_zone.clone()]);
    let mut queue = std::collections::VecDeque::new();
    runtime
        .submit_zone_deletion(&guard, &collection, || {
            queue.push_back(Operation::DeleteZone {
                zone: old_zone.clone(),
                cause: RemovalCause::Deleted,
            });
        })
        .unwrap();
    let deleted = service
        .request(
            &scope(),
            Caller {
                own_operation: Some("publication-cleanup".into()),
                ..Caller::default()
            },
            queue.pop_front().unwrap(),
        )
        .unwrap();
    assert!(matches!(&deleted.result, Ok(Reply::Done)));
    assert!(held_listing.request_id < deleted.request_id);
    runtime
        .zone_deletion_result(&guard, &old_zone, true, &collection)
        .unwrap();
    assert!(
        !runtime
            .publication_control_for_testing()
            .unwrap()
            .zones
            .contains_key(&old_zone)
    );

    // Existing API, same live binding/instance, actual earlier response.
    // The names-only callback currently resurrects the old epoch here.
    runtime.publication_zones_seen(&instance, &held_names).unwrap();
    let after = runtime.publication_facts(&collection).unwrap();
    let server = service.inspect(&scope()).unwrap().unwrap();
    assert!(server.zones[&old_zone].removed.is_some());
    assert!(server.zones[&current_zone].removed.is_none());
    assert_eq!(server.zones["meguri-root"].records["active"], *record);
    drop(runtime);
    drop(media);
    drop(collection);
    host.close().unwrap();
    drop(host);

    // Observe the committed table again before any new fetch or cleanup runs.
    let mut reopened = open(&current_dir, now, 82, current_keys.clone());
    let durable = reopened.facts().unwrap();
    reopened.close_clean().unwrap();
    for facts in [&after, &durable] {
        let binding = facts.current.as_ref().unwrap();
        assert_eq!(binding.id, current_binding.id);
        assert_eq!(binding.facts.epoch, current_binding.facts.epoch);
        assert_eq!(binding.facts.state, BindingState::Active);
        assert!(!facts.zones.contains_key(&current_zone));
        assert!(
            !facts.zones.contains_key(&old_zone),
            "Q11 A21 false: a pre-DeleteZones listing handled after its successful result must leave no retained zone row"
        );
    }
    assert_eq!(current_keys.0.load(Ordering::SeqCst), 2);
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
