#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "persistent VNC preparation remote transport drain and restoration" {
  python3 -B "$REPO_ROOT/tools/tests/vnc_session_test.py"
}
