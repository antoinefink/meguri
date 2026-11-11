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

@test "CloudAgent app-only Mac profile reports only fixed refusal codes" {
  run bash -c '
    mg_job() { :; }
    mg_failure() { printf "%s\n" "$2"; }
    source "$1/tools/mg.d/app.sh"
    MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE="$2"
    printf "%s\n" "App-only Mac CloudAgent profile validation refused: profile-settings" >"$2/validation.log"
    mg_app_mac_agent_profile_refusal
    printf "%s\n" "App-only Mac CloudAgent profile validation refused: generated-hidden-detail" >"$2/validation.log"
    mg_app_mac_agent_profile_refusal
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [[ "$output" == *"refused (profile-settings)"* ]]
  [[ "$output" == *"refused (guarded-input)"* ]]
  [[ "$output" != *"generated-hidden-detail"* ]]
}

@test "CloudAgent app-only Mac profile scope permits only the selected hosted Snapshot producer" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    unset MG_JOB
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri; then exit 1; fi
    for MG_JOB in snapshots build-app xcode-check other "mac-hosted-test snapshots"; do
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri; then exit 2; fi
      mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 3
    done
    MG_JOB=mac-hosted-test
    mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri || exit 4
    for args in \
      "adhoc build-for-testing mac Debug Snapshot Meguri" \
      "team build mac Debug Snapshot Meguri" \
      "team personal-preview mac Debug Snapshot Meguri" \
      "team build-for-testing ios-sim Debug Snapshot Meguri" \
      "team build-for-testing ios-device Debug Snapshot Meguri" \
      "team build-for-testing mac Profile Snapshot Meguri" \
      "team build-for-testing mac Release Snapshot Meguri" \
      "team build-for-testing mac AppStore Snapshot Meguri" \
      "team build-for-testing mac Debug UI Meguri" \
      "team build-for-testing mac Debug Unit Meguri" \
      "team build-for-testing mac Debug Performance Meguri" \
      "team build-for-testing mac Debug Snapshot MeguriWidget" \
      "team build-for-testing mac Debug Snapshot MeguriSnapshotTests" \
      "team build-for-testing mac Debug Snapshot Other"; do
      if mg_app_mac_agent_profile_scope $args; then exit 5; fi
    done
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri extra; then exit 6; fi
    if mg_app_mac_agent_profile_scope "team build-for-testing" mac Debug Snapshot Meguri; then exit 7; fi
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "CloudAgent app-only Mac profile stale binding is cleared before ordinary Snapshot build preparation" {
  run bash -c '
    mg_job() { :; }
    mg_word_in() { [ "$1" = team ]; }
    mg_lint_ui_first() { return 1; }
    source "$1/tools/mg.d/app.sh"
    MG_JOB=snapshots
    MEGURI_APP_SIGNING=team
    MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE=synthetic-unused
    MG_CLOUD_AGENT_MAC_PROFILE_BINDING=stale
    MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE=stale
    if mg_app_build mac Debug Snapshot; then exit 1; fi
    [ -z "$MG_CLOUD_AGENT_MAC_PROFILE_BINDING" ] && [ -z "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE" ]
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}
