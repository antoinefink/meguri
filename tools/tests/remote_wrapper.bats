#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "remote VM launcher transport and hosting Mac detachment" {
  python3 -B "$REPO_ROOT/tools/tests/remote_wrapper_test.py"
}
