#!/usr/bin/env bats
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "schema validation preserves every serial error with parallel workers" {
  run python3 -B -I "$REPO_ROOT/tools/conformance/tests/test_check_schemas.py"
  printf '%s\n' "$output"
  [ "$status" -eq 0 ]
}
