#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
@test "CloudAgent app-only Mac profile validates installed bytes grants and current host" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_mac_profile_test.py"
}
@test "CloudAgent app-only Mac profile scope excludes ordinary physical simulator and other configs" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    unset MG_JOB
    mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 1
    for args in "adhoc build-for-testing mac Debug CloudAgent Meguri" "team build mac Debug CloudAgent Meguri" "team build-for-testing ios-sim Debug CloudAgent Meguri" "team build-for-testing ios-device Debug CloudAgent Meguri" "team build-for-testing mac Profile CloudAgent Meguri" "team build-for-testing mac Debug UI Meguri" "team build-for-testing mac Debug CloudAgent Other"; do
      if mg_app_mac_agent_profile_scope $args; then exit 2; fi
    done
    if mg_app_mac_agent_profile_scope "team build-for-testing" mac Debug CloudAgent Meguri; then exit 3; fi
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "CloudAgent app-only Mac profile UI scope permits only the exact drive tuple" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    unset MG_JOB
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI Meguri; then exit 1; fi
    for MG_JOB in "" mac-ui build-app xcode-check snapshots mac-hosted-test ui-test run screenshot record ui-tree other "drive mac-ui" " drive" "drive "; do
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI Meguri; then exit 2; fi
      mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 3
    done
    MG_JOB=drive
    mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI Meguri || exit 4
    mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 5
    for args in \
      "adhoc build-for-testing mac Debug UI Meguri" \
      "other build-for-testing mac Debug UI Meguri" \
      "team build mac Debug UI Meguri" \
      "team personal-preview mac Debug UI Meguri" \
      "team test-without-building mac Debug UI Meguri" \
      "team other mac Debug UI Meguri" \
      "team build-for-testing ios-sim Debug UI Meguri" \
      "team build-for-testing ios-device Debug UI Meguri" \
      "team build-for-testing ios Debug UI Meguri" \
      "team build-for-testing other Debug UI Meguri" \
      "team build-for-testing mac Profile UI Meguri" \
      "team build-for-testing mac Release UI Meguri" \
      "team build-for-testing mac AppStore UI Meguri" \
      "team build-for-testing mac Other UI Meguri" \
      "team build-for-testing mac Debug Snapshot Meguri" \
      "team build-for-testing mac Debug Unit Meguri" \
      "team build-for-testing mac Debug Performance Meguri" \
      "team build-for-testing mac Debug Other Meguri" \
      "team build-for-testing mac Debug UI MeguriWidget" \
      "team build-for-testing mac Debug UI MeguriSnapshotTests" \
      "team build-for-testing mac Debug UI MeguriUITests" \
      "team build-for-testing mac Debug UI Meguri-Profile" \
      "team build-for-testing mac Debug UI Other"; do
      if mg_app_mac_agent_profile_scope $args; then exit 6; fi
    done
    if mg_app_mac_agent_profile_scope; then exit 7; fi
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI; then exit 8; fi
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI Meguri extra; then exit 9; fi
    if mg_app_mac_agent_profile_scope "team build-for-testing" mac Debug UI Meguri; then exit 10; fi
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug "UI Snapshot" Meguri; then exit 11; fi
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "drive app-only Mac profile rechecks its binding after build before returning products" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    MG_ROOT="$2" MG_RUN_DIR="$2" MG_TOOLS="$1/tools" MG_JOB=drive
    MEGURI_APP_SIGNING=team MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE=generated-unused
    mg_word_in() { [ "$1" = team ] || [ "$1" = Debug ]; }
    mg_on_exit() { :; }
    mvl_q() { printf "%q" "$1"; }
    mg_lint_ui_first() { return 0; }
    mg__xcframework() { return 0; }
    mg_app_remove_embedded_widget() { return 0; }
    mg_artifact() { :; }
    mg_detail() { :; }
    mg_failure() { printf "%s\n" "$1" >>"$MG_RUN_DIR/failure"; }
    mg_status() { printf "%s\n" "$1" >>"$MG_RUN_DIR/status"; }
    xcrun() { printf "%s\n" "{\"errorCount\":0,\"warningCount\":0}"; }
    xcodebuild() {
      [ "$1" = build-for-testing ] || return 1
      case " $* " in *" MEGURI_MAC_AGENT_PROFILE_STYLE=Manual "*) ;; *) return 1 ;; esac
      case " $* " in *" PROVISIONING_PROFILE_SPECIFIER="*) return 1 ;; esac
      touch "$MG_RUN_DIR/built"
    }
    python3() {
      case " $* " in
        *" --check "*)
          [ -f "$MG_RUN_DIR/built" ] || return 1
          local actual
          actual="$(cat)"
          [ "$actual" = "$MG_CLOUD_AGENT_MAC_PROFILE_BINDING" ] || return 1
          touch "$MG_RUN_DIR/rechecked"
          [ "$refuse" = no ] ;;
        *) printf "%s\n" "{\"file\":\"generated-unused\",\"uuid\":\"generated-profile\"}" ;;
      esac
    }
    for refuse in no yes; do
      rm -f "$MG_RUN_DIR/built" "$MG_RUN_DIR/rechecked"
      if mg_app_build mac Debug UI Meguri; then
        [ "$refuse" = no ] || exit 1
      else
        [ "$refuse" = yes ] || exit 2
      fi
      [ -f "$MG_RUN_DIR/rechecked" ] || exit 3
      rm -rf "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE"
    done
    [ "$(cat "$MG_RUN_DIR/failure")" = agent-profile ] || exit 4
    [ "$(cat "$MG_RUN_DIR/status")" = fail ] || exit 5
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
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

@test "CloudAgent app-only Mac profile Snapshot scope permits only both exact producers and tuples" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    unset MG_JOB
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri; then exit 1; fi
    for MG_JOB in "" drive mac-ui build-app xcode-check ui-test run screenshot record ui-tree other \
      "mac-hosted-test snapshots" "snapshots mac-hosted-test" " mac-hosted-test" "mac-hosted-test " \
      " snapshots" "snapshots " "snapshots --platform mac" $'"'"'snapshots\t'"'"' $'"'"'snapshots\n'"'"'; do
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri; then exit 2; fi
      mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 3
    done
    for MG_JOB in mac-hosted-test snapshots; do
      mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri || exit 4
      for args in \
        "adhoc build-for-testing mac Debug Snapshot Meguri" \
        "other build-for-testing mac Debug Snapshot Meguri" \
        "team build mac Debug Snapshot Meguri" \
        "team personal-preview mac Debug Snapshot Meguri" \
        "team test-without-building mac Debug Snapshot Meguri" \
        "team other mac Debug Snapshot Meguri" \
        "team build-for-testing ios-sim Debug Snapshot Meguri" \
        "team build-for-testing ios-device Debug Snapshot Meguri" \
        "team build-for-testing other Debug Snapshot Meguri" \
        "team build-for-testing mac Profile Snapshot Meguri" \
        "team build-for-testing mac Release Snapshot Meguri" \
        "team build-for-testing mac AppStore Snapshot Meguri" \
        "team build-for-testing mac Other Snapshot Meguri" \
        "team build-for-testing mac Debug UI Meguri" \
        "team build-for-testing mac Debug Unit Meguri" \
        "team build-for-testing mac Debug Performance Meguri" \
        "team build-for-testing mac Debug Other Meguri" \
        "team build-for-testing mac Debug Snapshot MeguriWidget" \
        "team build-for-testing mac Debug Snapshot MeguriSnapshotTests" \
        "team build-for-testing mac Debug Snapshot MeguriUITests" \
        "team build-for-testing mac Debug Snapshot Meguri-Profile" \
        "team build-for-testing mac Debug Snapshot Other"; do
        if mg_app_mac_agent_profile_scope $args; then exit 5; fi
      done
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri extra; then exit 6; fi
      if mg_app_mac_agent_profile_scope "team build-for-testing" mac Debug Snapshot Meguri; then exit 7; fi
      if mg_app_mac_agent_profile_scope; then exit 8; fi
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot; then exit 9; fi
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug "Snapshot UI" Meguri; then exit 10; fi
      if mg_app_mac_agent_profile_scope "team " build-for-testing mac Debug Snapshot Meguri; then exit 11; fi
      if mg_app_mac_agent_profile_scope team "build-for-testing " mac Debug Snapshot Meguri; then exit 12; fi
      if mg_app_mac_agent_profile_scope team build-for-testing "mac " Debug Snapshot Meguri; then exit 13; fi
      if mg_app_mac_agent_profile_scope team build-for-testing mac "Debug " Snapshot Meguri; then exit 14; fi
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug "Snapshot " Meguri; then exit 15; fi
      if mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot "Meguri "; then exit 16; fi
    done
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "CloudAgent app-only Mac profile stale binding is cleared before excluded Snapshot build preparation" {
  run bash -c '
    mg_job() { :; }
    mg_word_in() { [ "$1" = team ]; }
    mg_lint_ui_first() { return 1; }
    source "$1/tools/mg.d/app.sh"
    MG_JOB=build-app
    MEGURI_APP_SIGNING=team
    MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE=synthetic-unused
    MG_CLOUD_AGENT_MAC_PROFILE_BINDING=stale
    MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE=stale
    if mg_app_build mac Debug Snapshot; then exit 1; fi
    [ -z "$MG_CLOUD_AGENT_MAC_PROFILE_BINDING" ] && [ -z "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE" ]
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}
