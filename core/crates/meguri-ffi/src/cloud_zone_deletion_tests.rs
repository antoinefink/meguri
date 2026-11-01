//! P4 DeleteZones over an actual published Host, durable rows and original Service replies.
//! Main source base: 36ba535cefb570a7350d7c18527c5d2b55d32de3.
//! Integration batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
use super::*;
use crate::{cloud_connection::CloudControlRead, cloud_policy::*, cloud_zone_deletion::*};
use meguri_testkit::cloud::RemovalCause;

fn command(f: &PublishedFixture, sdk: &CloudEngineRegistration) -> Arc<CloudPolicyCommand> {
    drain_commands(f, sdk)
        .into_iter()
        .find(|c| matches!(c.action, CloudPolicyAction::DeleteZones))
        .expect("real drain-issued DeleteZones")
        .handle
}
fn drain_commands(f: &PublishedFixture, sdk: &CloudEngineRegistration) -> Vec<CloudPolicyIssuedCommand> {
    let fetch = f
        .workflow
        .begin_fetch(sdk.instance_id.clone(), vec![sdk.collection_zone.clone()])
        .unwrap();
    f.workflow
        .will_fetch(sdk.instance_id.clone(), "deletion-pass".into(), true)
        .unwrap();
    f.workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), true, false)
        .unwrap();
    f.workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), false, false)
        .unwrap();
    f.workflow
        .did_fetch(sdk.instance_id.clone(), "deletion-pass".into(), true)
        .unwrap();
    assert!(f.workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
    policy_commands(
        f.policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
    )
}
fn pass(f: &PublishedFixture, sdk: &CloudEngineRegistration) -> Arc<CloudZoneDeletionPass> {
    f.policy
        .clone()
        .prepare_execution(command(f, sdk))
        .unwrap()
        .unwrap()
        .begin_zone_deletion_pass()
        .unwrap()
        .unwrap()
}
fn due(f: &PublishedFixture, sdk: &CloudEngineRegistration) -> Vec<String> {
    let mut names: Vec<_> = [61, 62, 63]
        .map(|b| c::adapter::collection_zone(c::EpochId([b; 16])))
        .into();
    names.sort();
    for zone in &names {
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
    }
    f.workflow
        .core
        .publication_zones_seen(&sdk.instance_id, &names)
        .unwrap();
    f.clock.step_ms(30 * 86_400_000);
    f.workflow
        .core
        .publication_collection_drained(&sdk.instance_id)
        .unwrap();
    let facts = f
        .workflow
        .core
        .publication_facts(&f.workflow.host.collection().unwrap())
        .unwrap();
    assert!(names.iter().all(|z| facts.zones[z].deletion_due));
    names
}
fn read(f: &PublishedFixture) -> CloudControlRead {
    let reply = f
        .server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap()
        .result
        .unwrap();
    let Reply::Record(record) = reply else {
        panic!("actual original Active")
    };
    let value = publication_control_record(&record).unwrap();
    CloudControlRead::Found {
        record: crate::cloud_connection::CloudControlRecord {
            encrypted: value.encrypted,
            system_fields: value.system_fields.0,
        },
    }
}
struct Queue(bool, AtomicUsize);
impl CloudZoneDeletionQueue for Queue {
    fn enqueue(&self) -> bool {
        self.1.fetch_add(1, Ordering::SeqCst);
        #[cfg(debug_assertions)]
        assert!(
            std::panic::catch_unwind(crate::core_version).is_err(),
            "foreign enqueue must use callback::invoke"
        );
        self.0
    }
}
fn queue(accept: bool) -> Arc<Queue> {
    Arc::new(Queue(accept, AtomicUsize::new(0)))
}
fn checked(f: &PublishedFixture, p: &CloudZoneDeletionPass) -> Vec<String> {
    assert!(p.submit_read(queue(true)).unwrap());
    assert!(p.finish().is_err(), "an original queued read must join");
    p.complete_read(read(f), None, None).unwrap()
}
fn actual_delete(f: &PublishedFixture, zone: &str, cause: RemovalCause) -> CloudZoneDeletionResult {
    let reply = f
        .server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::DeleteZone {
                zone: zone.into(),
                cause,
            },
        )
        .unwrap();
    let failure = match reply.result {
        Ok(Reply::Done) => None,
        Err(e) => Some(CloudZoneDeletionFailure {
            code: Some(match e.code {
                ErrorCode::ZoneNotFound => 26,
                ErrorCode::UserDeletedZone => 28,
                _ => panic!("unexpected Service cause"),
            }),
            retry_after_ms: None,
            encrypted_data_reset: e.encrypted_data_reset,
        }),
        _ => panic!("original deletion result"),
    };
    CloudZoneDeletionResult {
        zone: zone.into(),
        failure,
    }
}
fn rows(f: &PublishedFixture) -> std::collections::BTreeMap<String, meguri_sync::cloud::publication::RetainedZone> {
    f.workflow
        .core
        .publication_facts(&f.workflow.host.collection().unwrap())
        .unwrap()
        .zones
}
#[test]
fn issued_deletion_with_no_due_rows_prepares_no_network_pass() {
    let (f, sdk) = published();
    assert!(rows(&f).is_empty());
    let count = f.server.lock().unwrap().request_log().unwrap().len();
    let execution = f.policy.clone().prepare_execution(command(&f, &sdk)).unwrap().unwrap();
    assert!(execution.clone().begin_zone_deletion_pass().unwrap().is_none());
    assert!(execution.begin_zone_deletion_pass().is_err());
    assert_eq!(f.server.lock().unwrap().request_log().unwrap().len(), count);
}
#[test]
fn later_drains_retry_real_due_rows_after_failure_until_deleted_without_repeating_daily_cleanup() {
    let (f, sdk) = published();
    let names = due(&f, &sdk);
    // Consume the real daily cleanup on this drain; the next two drains share
    // its clock instant and must rely on actual retained rows instead.
    let first = drain_commands(&f, &sdk);
    assert!(first.iter().any(|c| matches!(c.action, CloudPolicyAction::RunCleanup)));
    assert!(first.iter().any(|c| matches!(c.action, CloudPolicyAction::DeleteZones)));
    let driver = f.workflow.host.cloud_policy_driver().unwrap();
    let cleanup = driver.state().last_cleanup_ms;
    assert!(cleanup.is_some());
    assert_eq!(driver.state().facts.summary.zone_deletions_due, 3);

    let p = pass(&f, &sdk);
    assert_eq!(checked(&f, &p), names);
    assert!(p.submit_deletion(queue(true)).unwrap());
    let mut results: Vec<_> = names[..2]
        .iter()
        .map(|zone| actual_delete(&f, zone, RemovalCause::Deleted))
        .collect();
    // An actual user-purged zone returns UserDeletedZone on the original
    // deletion request. Unlike ZoneNotFound, that failure retains its due row.
    actual_delete(&f, &names[2], RemovalCause::Purged);
    let failed = actual_delete(&f, &names[2], RemovalCause::Deleted);
    assert_eq!(failed.failure.as_ref().unwrap().code, Some(28));
    results.push(failed);
    p.complete_deletion(results, None).unwrap();
    p.finish().unwrap();
    let retained = rows(&f);
    assert_eq!(retained.len(), 1);
    assert!(retained[&names[2]].deletion_due);
    assert_eq!(driver.state().last_cleanup_ms, cleanup);

    let p = pass(&f, &sdk);
    assert_eq!(driver.state().facts.summary.zone_deletions_due, 1);
    assert_eq!(checked(&f, &p), vec![names[2].clone()]);
    f.server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::CreateZone { zone: names[2].clone() },
        )
        .unwrap()
        .result
        .unwrap();
    assert!(p.submit_deletion(queue(true)).unwrap());
    let deleted = actual_delete(&f, &names[2], RemovalCause::Deleted);
    assert!(deleted.failure.is_none());
    p.complete_deletion(vec![deleted], None).unwrap();
    p.finish().unwrap();
    assert!(rows(&f).is_empty());
    assert_eq!(driver.state().last_cleanup_ms, cleanup);

    let final_drain = drain_commands(&f, &sdk);
    assert!(
        !final_drain
            .iter()
            .any(|c| matches!(c.action, CloudPolicyAction::DeleteZones | CloudPolicyAction::RunCleanup))
    );
    assert_eq!(driver.state().facts.summary.zone_deletions_due, 0);
    assert_eq!(driver.state().last_cleanup_ms, cleanup);
}
#[test]
fn original_batch_removes_only_actual_deleted_or_not_found_rows_and_retains_partial_evidence() {
    let (f, sdk) = published();
    let names = due(&f, &sdk);
    let original_active = match read(&f) {
        CloudControlRead::Found { record } => record.encrypted,
        _ => panic!(),
    };
    let p = pass(&f, &sdk);
    assert_eq!(checked(&f, &p), names);
    assert!(p.submit_read(queue(true)).is_err());
    assert!(p.submit_deletion(queue(true)).unwrap());
    assert!(p.finish().is_err());
    let first = actual_delete(&f, &names[0], RemovalCause::Deleted);
    actual_delete(&f, &names[1], RemovalCause::Deleted);
    let missing = actual_delete(&f, &names[1], RemovalCause::Deleted);
    assert_eq!(missing.failure.as_ref().unwrap().code, Some(26));
    let failed = CloudZoneDeletionResult {
        zone: names[2].clone(),
        failure: Some(CloudZoneDeletionFailure {
            code: Some(6),
            retry_after_ms: Some(120_000),
            encrypted_data_reset: false,
        }),
    };
    let result = vec![first, missing, failed];
    let terminal = Some(CloudZoneDeletionFailure {
        code: Some(2),
        retry_after_ms: Some(120_000),
        encrypted_data_reset: false,
    });
    let mut foreign = result.clone();
    foreign[0].zone = sdk.collection_zone.clone();
    assert!(p.complete_deletion(foreign, terminal.clone()).is_err());
    assert!(
        p.complete_deletion(vec![result[0].clone(); 3], terminal.clone())
            .is_err()
    );
    let before = f.workflow.host.cloud_policy_driver().unwrap().state();
    p.complete_deletion(result.clone(), terminal.clone()).unwrap();
    assert!(p.complete_deletion(result, terminal).is_err());
    p.finish().unwrap();
    let after = f.workflow.host.cloud_policy_driver().unwrap().state();
    assert_eq!(before.cycles, after.cycles);
    let remaining = rows(&f);
    assert_eq!(remaining.len(), 1);
    assert!(remaining[&names[2]].deletion_due);
    let actual = f.server.lock().unwrap().inspect(&f.scope).unwrap().unwrap();
    assert!(actual.zones[&sdk.collection_zone].removed.is_none());
    let read_now = match read(&f) {
        CloudControlRead::Found { record } => record.encrypted,
        _ => panic!(),
    };
    assert_eq!(read_now, original_active);
    let diagnostic = f.workflow.diagnostics().unwrap().zone_deletions;
    assert!(
        matches!(diagnostic.last(), Some(CloudZoneDeletionDiagnostic::Deleted { results, terminal: Some(t) }) if results[2].failure.as_ref().unwrap().code == Some(6) && t.code == Some(2))
    );
}
#[test]
fn original_active_switch_or_read_error_allows_no_deletion() {
    for failed in [false, true] {
        let (f, sdk) = published();
        let names = due(&f, &sdk);
        let p = pass(&f, &sdk);
        assert!(p.submit_read(queue(true)).unwrap());
        let read = if failed {
            CloudControlRead::Failed
        } else {
            let mut server = f.server.lock().unwrap();
            let inventory = server.inspect(&f.scope).unwrap().unwrap();
            let old = &inventory.zones["meguri-root"].records["active"];
            let Field::EncryptedBytes(bytes) = &old.fields["e"] else {
                panic!()
            };
            let mut active = control_codec::Active::decode(bytes).unwrap();
            active.epoch = [61; 16];
            let reply = server
                .request(
                    &f.scope,
                    Caller::default(),
                    Operation::Modify {
                        zone: "meguri-root".into(),
                        atomic: true,
                        changes: vec![Mutation::Save {
                            record: RecordInput::encrypted("active", "MGActive", active.encode().unwrap()),
                            if_unchanged: Some(old.system_fields.clone()),
                        }],
                    },
                )
                .unwrap()
                .result
                .unwrap();
            assert!(matches!(reply, Reply::Modified(_)));
            drop(server);
            read(&f)
        };
        assert!(p.complete_read(read, None, None).unwrap().is_empty());
        assert!(p.submit_deletion(queue(true)).is_err());
        p.finish().unwrap();
        assert!(names.iter().all(|z| rows(&f)[z].deletion_due));
    }
}
#[test]
fn pause_between_preparation_and_admission_never_calls_deletion_queue() {
    let (f, sdk) = published();
    let names = due(&f, &sdk);
    let p = pass(&f, &sdk);
    checked(&f, &p);
    f.policy
        .clone()
        .input(CloudPolicyEvent::Settings {
            settings: CloudPolicyDeviceSettings {
                enabled: true,
                paused: true,
                automatic: false,
                media: false,
            },
        })
        .unwrap();
    let queue = queue(true);
    assert!(p.submit_deletion(queue.clone()).is_err());
    assert_eq!(queue.1.load(Ordering::SeqCst), 0);
    p.finish().unwrap();
    assert!(names.iter().all(|z| rows(&f)[z].deletion_due));
}
#[test]
fn denied_queue_and_stale_original_read_keep_due_rows() {
    for pause in [false, true] {
        let (f, sdk) = published();
        let names = due(&f, &sdk);
        let p = pass(&f, &sdk);
        if pause {
            assert!(p.submit_read(queue(true)).unwrap());
            f.workflow.set_paused(true).unwrap();
            assert!(p.complete_read(read(&f), None, None).is_err());
        } else {
            checked(&f, &p);
            assert!(!p.submit_deletion(queue(false)).unwrap());
        }
        p.finish().unwrap();
        assert!(names.iter().all(|z| rows(&f)[z].deletion_due));
    }
}
#[test]
fn actual_encrypted_reset_reply_removes_due_zone_and_isolates_original_binding() {
    let (f, sdk) = published();
    let names = due(&f, &sdk);
    let p = pass(&f, &sdk);
    checked(&f, &p);
    assert!(p.submit_deletion(queue(true)).unwrap());
    actual_delete(&f, &names[0], RemovalCause::EncryptedDataReset);
    let reset = actual_delete(&f, &names[0], RemovalCause::Deleted);
    assert!(reset.failure.as_ref().unwrap().encrypted_data_reset);
    let mut results = vec![reset];
    for zone in &names[1..] {
        results.push(CloudZoneDeletionResult {
            zone: zone.clone(),
            failure: Some(CloudZoneDeletionFailure {
                code: Some(20),
                retry_after_ms: None,
                encrypted_data_reset: false,
            }),
        });
    }
    p.complete_deletion(results, None).unwrap();
    p.finish().unwrap();
    let status = f.workflow.core.status().unwrap();
    assert_eq!(
        status.binding.unwrap().facts.state,
        c::BindingState::Isolated(c::IsolationReason::EncryptedDataReset)
    );
    assert!(!rows(&f).contains_key(&names[0]));
    assert!(names[1..].iter().all(|z| rows(&f)[z].deletion_due));
}

#[test]
fn final_whole_candidate_revalidation_refuses_changed_list_and_another_active_pass() {
    let (f, sdk) = published();
    let names = due(&f, &sdk);
    let command = command(&f, &sdk);
    let first = f
        .policy
        .clone()
        .prepare_execution(command.clone())
        .unwrap()
        .unwrap()
        .begin_zone_deletion_pass()
        .unwrap()
        .unwrap();
    let second = f.policy.clone().prepare_execution(command).unwrap().unwrap();
    assert!(second.begin_zone_deletion_pass().unwrap().is_none());
    checked(&f, &first);
    // Another real deletion arrives between check and final native admission.
    let guard = f
        .workflow
        .core
        .begin_zone_deletion_read(&sdk.instance_id, &f.workflow.host.collection().unwrap())
        .unwrap();
    actual_delete(&f, &names[0], RemovalCause::Deleted);
    f.workflow
        .core
        .zone_deletion_result(&guard, &names[0], true, &f.workflow.host.collection().unwrap())
        .unwrap();
    let queue = queue(true);
    assert!(first.submit_deletion(queue.clone()).is_err());
    assert_eq!(queue.1.load(Ordering::SeqCst), 0);
    first.finish().unwrap();
    assert!(!rows(&f).contains_key(&names[0]));
    assert!(names[1..].iter().all(|z| rows(&f)[z].deletion_due));
}
#[test]
fn account_change_before_read_or_deletion_admission_never_calls_original_queue() {
    for after_read in [false, true] {
        let (f, sdk) = published();
        let names = due(&f, &sdk);
        let p = pass(&f, &sdk);
        if after_read {
            checked(&f, &p);
        }
        f.workflow
            .account_changed(Some(sdk.instance_id.clone()), CloudAccountStatus::NoAccount, None)
            .unwrap();
        let queue = queue(true);
        let admitted = if after_read {
            p.submit_deletion(queue.clone())
        } else {
            p.submit_read(queue.clone())
        };
        assert!(admitted.is_err());
        assert_eq!(queue.1.load(Ordering::SeqCst), 0);
        p.finish().unwrap();
        assert!(names.iter().all(|z| rows(&f)[z].deletion_due));
    }
}

#[test]
fn pause_while_original_deletion_reply_is_held_consumes_once_without_mutating_current_tracking() {
    let (f, sdk) = published();
    let names = due(&f, &sdk);
    let p = pass(&f, &sdk);
    checked(&f, &p);
    assert!(p.submit_deletion(queue(true)).unwrap());
    // The actual operation has run; its original reply is held, not replaced.
    let original: Vec<_> = names
        .iter()
        .map(|z| actual_delete(&f, z, RemovalCause::Deleted))
        .collect();
    f.workflow.set_paused(true).unwrap();
    assert!(p.complete_deletion(original.clone(), None).is_err());
    assert!(p.complete_deletion(original, None).is_err());
    p.finish().unwrap();
    assert!(names.iter().all(|z| rows(&f)[z].deletion_due));
    assert!(matches!(
        f.workflow.diagnostics().unwrap().zone_deletions.last(),
        Some(CloudZoneDeletionDiagnostic::Deleted { .. })
    ));
}
