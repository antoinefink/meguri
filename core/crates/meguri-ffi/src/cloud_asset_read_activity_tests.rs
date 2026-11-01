//! Source base dc7be3f7 plus prepared WIP. P3/P15 original-pass fact ports;
//! these tests supply raw SDK facts and assert real Driver state, never ACKs.
use super::*;
use meguri_sync::cloud::policy as p;

fn read_operation() -> p::Operation {
    p::Operation {
        zone: p::Zone::Collection,
        kind: p::OperationKind::Read,
    }
}
fn facts(started: bool, codes: &[i64], cancelled: bool) -> CloudAssetReadActivityFacts {
    CloudAssetReadActivityFacts {
        started,
        failures: codes
            .iter()
            .map(|code| CloudAssetReadFailureFacts {
                code: Some(*code),
                retry_after_ms: Some(120_000),
            })
            .collect(),
        failed: !codes.is_empty(),
        app_cancelled: cancelled,
    }
}

#[test]
fn actual_original_pass_reports_all_raw_errors_before_one_failed_cycle_even_after_finish() {
    let (fixture, sdk) = published();
    let pass = fixture.workflow.begin_first_asset_read_pass(sdk.instance_id).unwrap();
    pass.operation_activity(facts(true, &[], false)).unwrap();
    let active = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert!(active.cycles[&read_operation()].active);
    assert_eq!(active.cycles[&read_operation()].failures, 0);
    pass.finish().unwrap();
    let outputs = pass.operation_activity(facts(false, &[3, 6, 14], false)).unwrap();
    // Three error inputs plus ONE ended input, using the same P15 classifier as
    // issued execution.operation_activity. No per-record failing-cycle events.
    assert_eq!(outputs.len(), 4);
    let after = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    let cycle = &after.cycles[&read_operation()];
    assert!(!cycle.active);
    assert_eq!(cycle.failures, 1);
    assert_eq!(cycle.error.as_ref().unwrap().code, 14);
    assert_eq!(after.last_error.as_ref().unwrap().class, p::OperationError::Other);
    assert_eq!(after.last_error.as_ref().unwrap().retry_after_ms, Some(120_000));
    fixture.workflow.host.close().unwrap();
}

#[test]
fn actual_original_pass_cancel_preserves_prior_failure_count_and_soft_error_facts() {
    let (fixture, sdk) = published();
    let pass = fixture
        .workflow
        .begin_first_asset_read_pass(sdk.instance_id.clone())
        .unwrap();
    pass.operation_activity(facts(true, &[], false)).unwrap();
    pass.operation_activity(facts(false, &[14], false)).unwrap();
    pass.finish().unwrap();
    for (codes, cancelled) in [(vec![3, 6], true), (vec![20], false)] {
        let next = fixture
            .workflow
            .begin_first_asset_read_pass(sdk.instance_id.clone())
            .unwrap();
        next.operation_activity(facts(true, &[], false)).unwrap();
        next.finish().unwrap();
        let before = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        next.operation_activity(facts(false, &codes, cancelled)).unwrap();
        let after = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert!(!after.cycles[&read_operation()].active);
        assert_eq!(
            after.cycles[&read_operation()].failures,
            before.cycles[&read_operation()].failures
        );
        assert_eq!(after.cycles[&read_operation()].failures, 1);
        assert_eq!(after.last_error.as_ref().unwrap().code, 6);
        assert_eq!(after.last_error.as_ref().unwrap().class, p::OperationError::Throttled);
    }
    fixture.workflow.host.close().unwrap();
}

#[test]
fn finished_original_pass_cannot_report_into_replacement_sdk_or_closed_host() {
    let (fixture, sdk) = published();
    let pass = fixture
        .workflow
        .begin_first_asset_read_pass(sdk.instance_id.clone())
        .unwrap();
    pass.finish().unwrap();
    fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
    fixture.workflow.release_sdk(sdk.instance_id).unwrap();
    let successor = fixture.workflow.register_sdk(false).unwrap();
    let before = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert!(pass.operation_activity(facts(false, &[3], false)).is_err());
    let after = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert_eq!(before.cycles, after.cycles);
    assert_eq!(before.last_error, after.last_error);
    fixture.workflow.validate_sdk(successor.instance_id).unwrap();
    fixture.workflow.host.close().unwrap();
    assert!(pass.operation_activity(facts(false, &[6], false)).is_err());
}

#[test]
fn actual_retry_pass_uses_same_original_round_classifier_after_finish() {
    let (fixture, sdk) = published();
    waiting_note(&fixture, &sdk);
    let pass = retry_pass(&fixture, &sdk);
    pass.operation_activity(facts(true, &[], false)).unwrap();
    pass.finish().unwrap();
    pass.operation_activity(facts(false, &[4, 7], false)).unwrap();
    let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    assert_eq!(state.cycles[&read_operation()].failures, 1);
    assert!(!state.cycles[&read_operation()].active);
    assert_eq!(state.last_error.as_ref().unwrap().class, p::OperationError::Throttled);
    fixture.workflow.host.close().unwrap();
}
