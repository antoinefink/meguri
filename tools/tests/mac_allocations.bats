#!/usr/bin/env bats
# Process discovery/lifetime fakes: no VM, attach, builds or real sleeps.
bats_require_minimum_version 1.5.0

@test "allocations: exact numeric VM target and real start notification" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_exact_numeric_vm_attach
}

@test "allocations: reject wrong/multiple targets, wrong uid, nonnumeric pid and changed start" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_wrong_routes Allocations.test_multiple Allocations.test_uid_and_numeric_identity Allocations.test_recheck_start_identity Allocations.test_real_argv_boundaries Allocations.test_refuse_restart_or_preexisting_target
}

@test "allocations: missing, wrong-binary and late pid observation is bounded" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_bounded_missing_wrong_and_late_binary
}

@test "allocations: normal stop finalizes, remote stop is ordered/idempotent, reused pid is unsignalled" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_normal_stop_finalizes Allocations.test_remote_stop_order_idempotence_and_reuse Allocations.test_pid_reuse_unsignalled
}

@test "allocations: missing notification/trace, attach failure and forced termination are diagnostic errors" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_notification_trace_attach_and_finalization_failures
}

@test "allocations: orphan recovery targets only our run paths and notification prefix" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_orphan_scope_and_identity
}

@test "allocations: failed launch verification reaps direct children with bounded escalation" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_failed_remember_reaps_owned_children Allocations.test_failed_observer_remember_reaps_child Allocations.test_transient_xcrun_exec_is_verified Allocations.test_failed_direct_cleanup_retains_unknown_state
}

@test "allocations: transient procargs refusal verifies before success; persistent refusal fails bounded with last reason" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_transient_procargs_refusal_verifies_before_capture_passes Allocations.test_persistent_procargs_refusal_retains_last_reason_and_reaps_child Allocations.test_child_exit_during_unknown_identity_fails_promptly Allocations.test_procargs_refusal_reports_numeric_errno
}

@test "allocations: observer verifies current interpreter behind launcher and refuses unknown parent" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_observer_launcher_uses_current_interpreter_identity Allocations.test_unknown_parent_identity_refuses_observer_spawn
}

@test "allocations: interrupted launch intent blocks recovery without verified ownership" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_interrupted_launch_stages_block_until_certified
}

@test "allocations: defunct and terminated status differ from live procargs access failures" {
  python3 "$BATS_TEST_DIRNAME/mac_allocations_fakehelper.py" Allocations.test_defunct_and_live_procargs_failure Allocations.test_orphan_defunct_and_access_failure
}
