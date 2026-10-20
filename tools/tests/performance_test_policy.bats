#!/usr/bin/env bats
# Generated-policy fakes only: no Apple tools, VM or app execution.
bats_require_minimum_version 1.5.0

@test "performance-test-policy: explicit false flags, supported plist layouts and bounded private metadata" {
  python3 "$BATS_TEST_DIRNAME/performance_test_policy_test.py" Policy.test_valid Policy.test_bounded_metadata
}

@test "performance-test-policy: missing, true, invalid flags and either checker injection fail" {
  python3 "$BATS_TEST_DIRNAME/performance_test_policy_test.py" Policy.test_flags Policy.test_injection
}

@test "performance-test-policy: malformed, missing and empty UI target policy fails" {
  python3 "$BATS_TEST_DIRNAME/performance_test_policy_test.py" Policy.test_unavailable_and_malformed Policy.test_no_ui_targets
}

@test "performance-test-policy: build guard selects actual scheme/sdk/config and stops before consumers" {
  python3 "$BATS_TEST_DIRNAME/performance_test_policy_test.py" BuildGuard
}
