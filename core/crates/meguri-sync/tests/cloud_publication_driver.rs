//! Tests FIRST for the unbound IPC/scheduling wiring. Root extracts this file
//! before applying implementation. No prebound device or fixture acknowledgments.
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    publication::{KeyByteSource, KeySourceError, PublishStep},
    workflow::TraceEvent,
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
const STEPS: usize = 10_000;
// Deliberately non-Debug. This is platform input, never collection-id entropy.
struct Keys {
    calls: AtomicUsize,
    fail: Option<usize>,
}
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail == Some(call) {
            Err(KeySourceError::Unavailable)
        } else {
            Ok(std::array::from_fn(|i| {
                (i as u8).wrapping_mul(13).wrapping_add(call as u8)
            }))
        }
    }
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.unbound-child".into(),
        environment: "development".into(),
        account: "synthetic-account".into(),
    }
}
fn generated(directory: &Path) {
    let recipe = Recipe::parse("recipe: child-publication\ndescription: Public synthetic IPC test\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Child\n    notetype: basic\n    new: 1\n    due: 1\n    tags: []\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 71, directory).unwrap();
}
fn keys(fail: Option<usize>) -> Arc<Keys> {
    Arc::new(Keys {
        calls: AtomicUsize::new(0),
        fail,
    })
}
fn open(directory: &Path, keys: Arc<Keys>) -> PublicationRig {
    let s = scope();
    PublicationRig::open(
        directory,
        &s,
        IdentityReads::Available(s.account.clone()),
        false,
        NOW,
        71,
        keys,
    )
    .unwrap()
}
#[test]
fn unbound_boot_and_discovery_never_draw_keys_register_sdk_or_prepare_a_baseline() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generated(&directory);
    let keys = keys(None);
    let mut rig = open(&directory, keys.clone());
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(71)).unwrap();
    let before = rig.driver().collection_rows().unwrap();
    let initial = rig.facts().unwrap();
    assert!(initial.bindings.is_empty() && initial.entities.is_empty() && initial.recovery_ops.is_empty());
    rig.preview(&mut service, STEPS).unwrap();
    rig.cancel_preview().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.facts().unwrap().current.is_none());
    assert_eq!(rig.driver().collection_rows().unwrap(), before);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    assert!(
        !rig.trace()
            .iter()
            .any(|e| matches!(e, TraceEvent::EngineStarted { .. }))
    );
    assert!(
        service
            .request_log()
            .unwrap()
            .iter()
            .all(|r| r.kind == RequestKind::Read)
    );
    rig.close_clean().unwrap();
}
#[test]
fn zones_force_real_sdk_scheduling_with_auto_off_and_restore_the_setting_at_sent() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generated(&directory);
    let keys = keys(None);
    let mut rig = open(&directory, keys.clone());
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(71)).unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::ScanCommitted).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::ScanCommitted));
    assert!(
        !rig.trace()
            .iter()
            .any(|e| matches!(e, TraceEvent::EngineStarted { .. }))
    );
    let facts = rig.facts().unwrap();
    assert_eq!(facts.recovery_ops[0].step, PublishStep::Zones);
    assert!(facts.entities.values().all(|e| e.acked_rev == 0 && e.mark.is_some()));
    assert!(rig.sdk_database_pending().is_empty());
    rig.hold(Point::BeforeEpochRead).unwrap();
    rig.release(Point::ScanCommitted).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeEpochRead));
    assert!(
        rig.facts()
            .unwrap()
            .entities
            .values()
            .all(|e| e.acked_rev == e.local_rev && e.mark.is_none())
    );
    let calls = rig.driver().send_changes_calls().unwrap();
    assert!(!calls.is_empty() && calls.iter().all(|c| c.automatic && c.returned));
    // The user preference remains off after zones, without a Rig toggle.
    assert!(!rig.driver().wants_send().unwrap());
    rig.release(Point::BeforeEpochRead).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
    rig.close_clean().unwrap();
}
#[test]
fn failed_second_duplex_key_draw_leaves_every_publication_fact_unwritten() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generated(&directory);
    let keys = keys(Some(1));
    let mut rig = open(&directory, keys.clone());
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(71)).unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    let before = rig.facts().unwrap();
    assert!(rig.confirm_publication(&preview).is_err());
    let failed = rig.facts().unwrap();
    assert_eq!(failed.decision, before.decision);
    assert!(failed.bindings.is_empty() && failed.recovery_ops.is_empty() && failed.media_key.is_none());
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
    assert!(
        service
            .request_log()
            .unwrap()
            .iter()
            .all(|r| r.kind == RequestKind::Read)
    );
    rig.close_clean().unwrap();
    let mut reopened = open(&directory, keys.clone());
    reopened.drive_ready(&mut service, STEPS).unwrap();
    assert!(reopened.facts().unwrap().bindings.is_empty());
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
    reopened.close_clean().unwrap();
}
#[test]
fn prepared_active_and_guard_enqueued_active_have_distinct_durable_evidence() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generated(&directory);
    let keys = keys(None);
    let mut rig = open(&directory, keys.clone());
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(71)).unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeActiveSave).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let prepared = rig.facts().unwrap();
    assert!(!prepared.recovery_ops[0].switch_sent && prepared.recovery_ops[0].active_attempt.is_none());
    let request = rig.held_request().unwrap();
    let queued = rig.facts().unwrap();
    assert!(queued.recovery_ops[0].switch_sent && queued.recovery_ops[0].active_attempt.is_some());
    let response = service.request(&scope(), request.caller, request.operation).unwrap();
    assert!(response.result.is_ok());
    rig.kill().unwrap();
    let mut reopened = open(&directory, keys.clone());
    reopened.drive_ready(&mut service, STEPS).unwrap();
    assert!(reopened.facts().unwrap().recovery_ops.is_empty());
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
    reopened.close_clean().unwrap();
}
#[test]
fn retained_backup_rows_come_from_its_package_after_the_live_note_changes() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generated(&directory);
    let mut rig = open(&directory, keys(None));
    let backup = rig.driver().backup_now().unwrap();
    let saved = rig.driver().backup_collection_rows(&backup.name).unwrap();
    let id = saved
        .iter()
        .find_map(|p| {
            if let meguri_sync::cloud::entity_codec::Payload::Note(n) = p {
                Some(n.id)
            } else {
                None
            }
        })
        .unwrap();
    rig.driver()
        .edit_note_fields(id, &["changed after backup", "back"])
        .unwrap();
    assert_eq!(rig.driver().backup_collection_rows(&backup.name).unwrap(), saved);
    assert_ne!(rig.driver().collection_rows().unwrap(), saved);
    let restored = rig.driver().restore_backup(&backup).unwrap();
    assert!(restored.safety_backup.is_some());
    assert_eq!(rig.driver().collection_rows().unwrap(), saved);
    rig.close_clean().unwrap();
}
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
