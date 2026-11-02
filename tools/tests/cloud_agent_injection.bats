#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent injection generated preparation and native embed contract" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_injection_test.py"
}
