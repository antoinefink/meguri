//! Retained RunCleanup over a published Host and original Service inventory.
use super::*;
use crate::cloud_cleanup::*;
use meguri_model::clock::Clock;

fn cleanup_command(f: &PublishedFixture, sdk: &CloudEngineRegistration) -> Arc<CloudPolicyCommand> {
    drain_commands(f, sdk)
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::RunCleanup))
        .expect("original collection drain issued cleanup")
        .handle
}
fn cleanup(f: &PublishedFixture, sdk: &CloudEngineRegistration) -> Arc<CloudCleanupPass> {
    f.policy
        .clone()
        .prepare_execution(cleanup_command(f, sdk))
        .unwrap()
        .unwrap()
        .begin_cleanup_pass()
        .unwrap()
}
struct InventoryQueue(AtomicUsize);
impl CloudCleanupQueue for InventoryQueue {
    fn enqueue(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}
fn inventory_queue() -> Arc<InventoryQueue> {
    Arc::new(InventoryQueue(AtomicUsize::new(0)))
}
fn inventory(f: &PublishedFixture) -> Vec<String> {
    let response = f
        .server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller {
                own_operation: Some("cleanup-inventory".into()),
                ..Caller::default()
            },
            Operation::FetchAllZones,
        )
        .unwrap();
    let Reply::AllZones(zones) = response.result.unwrap() else {
        panic!("authoritative original inventory")
    };
    zones
}
#[test]
fn original_drain_cleanup_computes_once_before_inventory_without_recapturing_clock() {
    let (f, sdk) = published();
    let command = cleanup_command(&f, &sdk);
    let execution = f.policy.clone().prepare_execution(command.clone()).unwrap().unwrap();
    let issued_at = f.clock.now_ms();
    f.clock.step_ms(1_000);
    let pass = execution.clone().begin_cleanup_pass().unwrap();
    assert_eq!(
        f.workflow.host.cloud_policy_driver().unwrap().state().last_cleanup_ms,
        Some(issued_at)
    );
    assert!(execution.begin_cleanup_pass().is_err());
    assert!(
        f.policy
            .clone()
            .prepare_execution(command)
            .unwrap()
            .unwrap()
            .begin_cleanup_pass()
            .is_err()
    );
    let q = inventory_queue();
    assert!(pass.submit_inventory(q.clone()).unwrap());
    assert!(pass.submit_inventory(q.clone()).is_err());
    assert_eq!(q.0.load(Ordering::SeqCst), 1);
    assert!(pass.finish().is_err(), "original terminal still owed");
    assert!(pass.complete_inventory(Some(inventory(&f))).unwrap());
    assert!(pass.complete_inventory(Some(vec![])).is_err());
    pass.finish().unwrap();
}
#[test]
fn inventory_inserts_at_handling_time_without_due_marks_or_cleanup_clock_refresh() {
    let (f, sdk) = published();
    let pass = cleanup(&f, &sdk);
    let at = f.workflow.host.cloud_policy_driver().unwrap().state().last_cleanup_ms;
    let zone = c::adapter::collection_zone(c::EpochId([91; 16]));
    f.server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::CreateZone { zone: zone.clone() },
        )
        .unwrap()
        .result
        .unwrap();
    assert!(pass.submit_inventory(inventory_queue()).unwrap());
    let original = inventory(&f);
    f.clock.step_ms(2_000);
    assert!(pass.complete_inventory(Some(original)).unwrap());
    pass.finish().unwrap();
    let row = rows(&f)[&zone].clone();
    assert!(!row.deletion_due);
    assert_eq!(row.first_seen_ms, f.clock.now_ms());
    assert_eq!(
        f.workflow.host.cloud_policy_driver().unwrap().state().last_cleanup_ms,
        at
    );
}
#[test]
fn failed_inventory_commits_nothing_and_cannot_be_replaced() {
    let (f, sdk) = published();
    let pass = cleanup(&f, &sdk);
    let before = rows(&f);
    assert!(pass.submit_inventory(inventory_queue()).unwrap());
    assert!(!pass.complete_inventory(None).unwrap());
    assert!(pass.complete_inventory(Some(vec!["c-fabricated".into()])).is_err());
    pass.finish().unwrap();
    assert_eq!(rows(&f), before);
}
#[test]
fn paused_or_changed_sdk_never_admits_original_inventory() {
    for change_sdk in [false, true] {
        let (f, sdk) = published();
        let pass = cleanup(&f, &sdk);
        if change_sdk {
            f.workflow.retire_sdk(sdk.instance_id.clone(), true).unwrap();
            f.workflow.release_sdk(sdk.instance_id.clone()).unwrap();
            f.workflow.register_sdk(false).unwrap();
        } else {
            f.workflow.set_paused(true).unwrap();
        }
        let q = inventory_queue();
        assert!(pass.submit_inventory(q.clone()).is_err());
        assert_eq!(q.0.load(Ordering::SeqCst), 0);
        pass.finish().unwrap();
    }
}
#[test]
fn stale_original_inventory_terminal_is_consumed_once() {
    let (f, sdk) = published();
    let pass = cleanup(&f, &sdk);
    assert!(pass.submit_inventory(inventory_queue()).unwrap());
    let original = inventory(&f);
    f.workflow.set_paused(true).unwrap();
    assert!(pass.complete_inventory(Some(original)).is_err());
    assert!(pass.complete_inventory(None).is_err());
    pass.finish().unwrap();
}

#[test]
fn original_owner_or_sdk_changed_before_computation_is_rejected() {
    for change_sdk in [false, true] {
        let (f, sdk) = published();
        let command = cleanup_command(&f, &sdk);
        let execution = f.policy.clone().prepare_execution(command).unwrap().unwrap();
        let before = f.workflow.host.cloud_policy_driver().unwrap().state().last_cleanup_ms;
        if change_sdk {
            f.workflow.retire_sdk(sdk.instance_id.clone(), true).unwrap();
            f.workflow.release_sdk(sdk.instance_id.clone()).unwrap();
            f.workflow.register_sdk(false).unwrap();
        } else {
            f.workflow
                .account_changed(Some(sdk.instance_id.clone()), CloudAccountStatus::NoAccount, None)
                .unwrap();
        }
        assert!(execution.begin_cleanup_pass().is_err());
        assert_eq!(
            f.workflow.host.cloud_policy_driver().unwrap().state().last_cleanup_ms,
            before
        );
    }
}
#[test]
fn actual_qualified_cleanup_allows_deletion_and_old_inventory_cannot_restore_removed_zone() {
    let (f, sdk) = published();
    let zone = c::adapter::collection_zone(c::EpochId([92; 16]));
    f.server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::CreateZone { zone: zone.clone() },
        )
        .unwrap()
        .result
        .unwrap();
    let first = cleanup(&f, &sdk);
    assert!(first.submit_inventory(inventory_queue()).unwrap());
    assert!(first.complete_inventory(Some(inventory(&f))).unwrap());
    first.finish().unwrap();
    assert!(!rows(&f)[&zone].deletion_due);
    f.clock.step_ms(30 * 86_400_000 + 1);
    let commands = drain_commands(&f, &sdk);
    let issued_cleanup = commands
        .iter()
        .find(|c| matches!(c.action, CloudPolicyAction::RunCleanup))
        .unwrap();
    let deletion = commands
        .iter()
        .find(|c| matches!(c.action, CloudPolicyAction::DeleteZones))
        .unwrap();
    let pass = f
        .policy
        .clone()
        .prepare_execution(issued_cleanup.handle.clone())
        .unwrap()
        .unwrap()
        .begin_cleanup_pass()
        .unwrap();
    assert!(
        rows(&f)[&zone].deletion_due,
        "the qualified local computation actually marked the retained row"
    );
    assert!(pass.submit_inventory(inventory_queue()).unwrap());
    let held = inventory(&f);
    let delete = f
        .policy
        .clone()
        .prepare_execution(deletion.handle.clone())
        .unwrap()
        .unwrap()
        .begin_zone_deletion_pass()
        .unwrap()
        .unwrap();
    assert_eq!(checked(&f, &delete), vec![zone.clone()]);
    assert!(delete.submit_deletion(queue(true)).unwrap());
    delete
        .complete_deletion(vec![actual_delete(&f, &zone, RemovalCause::Deleted)], None)
        .unwrap();
    delete.finish().unwrap();
    assert!(!rows(&f).contains_key(&zone));
    assert!(!pass.complete_inventory(Some(held)).unwrap());
    pass.finish().unwrap();
    assert!(!rows(&f).contains_key(&zone));
    // A new actual creation, followed by the next daily cleanup's actual
    // inventory, is a new observation and may create a fresh retained row.
    f.server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::CreateZone { zone: zone.clone() },
        )
        .unwrap()
        .result
        .unwrap();
    f.clock.step_ms(86_400_000);
    let fresh = cleanup(&f, &sdk);
    assert!(fresh.submit_inventory(inventory_queue()).unwrap());
    assert!(fresh.complete_inventory(Some(inventory(&f))).unwrap());
    fresh.finish().unwrap();
    assert_eq!(rows(&f)[&zone].first_seen_ms, f.clock.now_ms());
    assert!(!rows(&f)[&zone].deletion_due);
}
