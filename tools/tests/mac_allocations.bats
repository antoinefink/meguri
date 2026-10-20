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
