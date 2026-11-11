#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "cloud-fetch-probe-host fixed diagnostic guards privacy selection and managed cancellation" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_fetch_probe_host_test.py"
}
