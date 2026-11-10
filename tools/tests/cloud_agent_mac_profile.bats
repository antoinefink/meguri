#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
@test "CloudAgent app-only Mac profile validates installed bytes grants and current host" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_mac_profile_test.py"
}
@test "CloudAgent app-only Mac profile scope excludes ordinary physical simulator and other configs" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 1
    for args in "adhoc build-for-testing mac Debug CloudAgent Meguri" "team build mac Debug CloudAgent Meguri" "team build-for-testing ios-sim Debug CloudAgent Meguri" "team build-for-testing ios-device Debug CloudAgent Meguri" "team build-for-testing mac Profile CloudAgent Meguri" "team build-for-testing mac Debug UI Meguri" "team build-for-testing mac Debug CloudAgent Other"; do
      if mg_app_mac_agent_profile_scope $args; then exit 2; fi
    done
    if mg_app_mac_agent_profile_scope "team build-for-testing" mac Debug CloudAgent Meguri; then exit 3; fi
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}
