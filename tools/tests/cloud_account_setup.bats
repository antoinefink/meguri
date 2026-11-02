#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "owner account setup generated refusal publication privacy and startup boundaries only" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_account_setup_test.py"
}
