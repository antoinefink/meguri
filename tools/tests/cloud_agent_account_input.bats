#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent approved source and exact private projection generated boundaries only" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_account_input_test.py"
}
