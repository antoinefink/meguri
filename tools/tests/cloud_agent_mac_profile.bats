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

@test "CloudAgent app-only Mac profile UI scope without selector permits only the exact drive tuple" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    unset MG_JOB
    unset MG_UI_MAC_PROFILE_SELECTOR
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

@test "app-only Mac profile ui-test scope requires the exact selector job and complete tuple" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    selector=MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch
    MG_JOB=ui-test
    unset MG_UI_MAC_PROFILE_SELECTOR
    if mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI Meguri; then exit 1; fi
    MG_UI_MAC_PROFILE_SELECTOR="$selector"
    mg_app_mac_agent_profile_scope team build-for-testing mac Debug UI Meguri || exit 2
    tuple=(team build-for-testing mac Debug UI Meguri)
    printf -v tab "\t"
    printf -v newline "\n"
    for i in 0 1 2 3 4 5; do
      case "$i" in
        0) wrong=(adhoc other) ;;
        1) wrong=(build personal-preview test-without-building other) ;;
        2) wrong=(ios-sim ios-device ios other) ;;
        3) wrong=(Profile Release AppStore Other) ;;
        4) wrong=(Snapshot Unit Performance Other "UI Snapshot") ;;
        5) wrong=(MeguriWidget MeguriSnapshotTests MeguriUITests Meguri-Profile Other) ;;
      esac
      original="${tuple[$i]}"
      for bad in "${wrong[@]}" "" " $original" "$original " "$original$tab" "$original$newline"; do
        tuple[$i]="$bad"
        if mg_app_mac_agent_profile_scope "${tuple[@]}"; then exit 3; fi
      done
      tuple[$i]="$original"
    done
    for count in 0 1 2 3 4 5; do
      if mg_app_mac_agent_profile_scope "${tuple[@]:0:$count}"; then exit 4; fi
    done
    if mg_app_mac_agent_profile_scope "${tuple[@]}" extra; then exit 5; fi
    for MG_JOB in "" drive-ui ui-tests UI-test " ui-test" "ui-test " "ui-test$tab" "ui-test$newline" "ui-test drive"; do
      if mg_app_mac_agent_profile_scope "${tuple[@]}"; then exit 6; fi
    done
    MG_JOB=ui-test
    for MG_UI_MAC_PROFILE_SELECTOR in "" MacHomeHierarchyTests MacHomeHierarchyTests/testOther \
      "${selector}Extra" "$selector()" "MeguriUITests/$selector" " $selector" "$selector " \
      "$selector$tab" "$selector$newline" "$selector $selector" "$selector MacHomeHierarchyTests/testOther"; do
      if mg_app_mac_agent_profile_scope "${tuple[@]}"; then exit 7; fi
      MG_JOB=drive mg_app_mac_agent_profile_scope "${tuple[@]}" || exit 8
      mg_app_mac_agent_profile_scope team build-for-testing mac Debug CloudAgent Meguri || exit 9
      for job in mac-hosted-test snapshots; do
        MG_JOB="$job" mg_app_mac_agent_profile_scope team build-for-testing mac Debug Snapshot Meguri || exit 10
      done
    done
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "real ui-test parser selects only one Mac Debug provider method and resets ambient selectors" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    source "$1/tools/mg.d/uitest.sh"
    MG_JOB=ui-test
    selector=MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch
    mg_word_in() { local word; for word in $2; do [ "$1" != "$word" ] || return 0; done; return 1; }
    mg_detail() { :; }
    mg_message() { :; }
    mg_status() { :; }
    jq() { :; }
    mg_failure() { usage=1; [ -z "$MG_UI_MAC_PROFILE_SELECTOR" ] || mismatch=1; }
    mg_mac_ui_begin() { return 0; }
    mg_ui_suite_fixtures() { :; }
    mg_ui_suite_fixtures_stage() { :; }
    mg_sim_begin() { return 0; }
    mg_sim_check_given() { return 0; }
    mg_host_lease() { return 0; }
    mg_ui_test__physical_check() { return 0; }
    mg_ui_test__physical_inventory() { inventory_seen=1; [ -z "$MG_UI_MAC_PROFILE_SELECTOR" ] || mismatch=1; return 1; }
    mg_app_build() {
      built=1
      [ "$MG_UI_MAC_PROFILE_SELECTOR" = "$expected_selector" ] || mismatch=1
      if mg_app_mac_agent_profile_scope team build-for-testing "$1" "$2" "${3:-UI}" "${4:-Meguri}"; then
        admitted=1
      fi
      return 1
    }
    expect_request() {
      local expected_selector="$1" outcome="$2"
      shift 2
      built=0 usage=0 inventory_seen=0 admitted=0 mismatch=0
      mg_ui_test "$@" || return 1
      [ "$mismatch" = 0 ] || return 1
      case "$outcome" in
        build) [ "$built/$usage/$inventory_seen" = 1/0/0 ] || return 1 ;;
        usage) [ "$built/$usage/$inventory_seen" = 0/1/0 ] || return 1 ;;
        inventory) [ "$built/$usage/$inventory_seen" = 0/0/1 ] || return 1 ;;
      esac
      if [ -n "$expected_selector" ]; then [ "$admitted" = 1 ]; else [ "$admitted" = 0 ]; fi
    }
    # Assignment and export must both be shadowed, including early returns.
    for ambient in assigned exported; do
      MG_UI_MAC_PROFILE_SELECTOR="$selector"
      [ "$ambient" != exported ] || export MG_UI_MAC_PROFILE_SELECTOR
      expect_request "" build --platform mac --suite MacHomeHierarchyTests || exit 1
      expect_request "" usage --platform mac --suite "$selector" --device synthetic || exit 2
      expect_request "" inventory --list-physical-devices || exit 3
      [ "$MG_UI_MAC_PROFILE_SELECTOR" = "$selector" ] || exit 4
      unset MG_UI_MAC_PROFILE_SELECTOR
    done
    expect_request "$selector" build --platform mac --suite "$selector" || exit 5
    [ "${MG_UI_MAC_PROFILE_SELECTOR+x}" != x ] || exit 6
    expect_request "" build --platform mac --suite MacHomeHierarchyTests || exit 7
    expect_request "$selector" build --config Debug --suite "$selector" --platform mac || exit 8
    expect_request "" build --platform mac --suite "$selector" --suite "$selector" || exit 9
    expect_request "" build --platform mac --suite "$selector" --suite MacHomeHierarchyTests/testOther || exit 10
    expect_request "" build --platform mac --suite MacHomeHierarchyTests/testOther --suite "$selector" || exit 11
    expect_request "" build --platform mac --suite MacHomeHierarchyTests/testOther || exit 12
    expect_request "" build --platform mac --config Profile --suite "$selector" || exit 13
    for platform in iphone ipad; do
      expect_request "" build --platform "$platform" --suite "$selector" || exit 14
      expect_request "" build --platform "$platform" --device synthetic --suite "$selector" || exit 15
    done
    physical=00000000-0000000000000000
    expect_request "" build --physical-device "$physical" --suite "$selector" || exit 16
    expect_request "" usage --platform mac --physical-device "$physical" --suite "$selector" || exit 17
    expect_request "" usage --physical-device "$physical" --device synthetic --suite "$selector" || exit 18
    expect_request "" usage --list-physical-devices --platform mac --suite "$selector" || exit 19
    expect_request "" usage --platform mac --config Release --suite "$selector" || exit 20
    expect_request "" usage --platform mac --suite "$selector()" || exit 21
    expect_request "" usage --platform mac || exit 22
    [ "${MG_UI_MAC_PROFILE_SELECTOR+x}" != x ] || exit 23
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
}

@test "selected real ui-test prepares app-only settings and rechecks before VM delivery with private diagnostics" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/app.sh"
    source "$1/tools/mg.d/uitest.sh"
    MG_ROOT="$2" MG_RUN_DIR="$2" MG_TOOLS="$1/tools" TMPDIR="$2" MG_JOB=ui-test
    unset MG_UI_MAC_PROFILE_SELECTOR
    MEGURI_APP_SIGNING=team MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE=generated-unused
    mg_word_in() { local word; for word in $2; do [ "$1" != "$word" ] || return 0; done; return 1; }
    mg_on_exit() { cleanup="$1"; }
    mvl_q() { printf "%q" "$1"; }
    mg_lint_ui_first() { return 0; }
    mg__xcframework() { return 0; }
    mg_app_remove_embedded_widget() { return 0; }
    mg_mac_ui_begin() { return 0; }
    mg_ui_suite_fixtures() { :; }
    mg_ui_suite_fixtures_stage() { :; }
    mg_mac_ui_take() {
      [ -f "$MG_RUN_DIR/rechecked" ] || return 1
      printf "%s\n" delivery >>"$MG_RUN_DIR/order"
      touch "$MG_RUN_DIR/delivered"
      return 1
    }
    mg_artifact() { printf "%s\n" "$*" >>"$MG_RUN_DIR/public"; }
    mg_detail() { printf "%s\n" "$*" >>"$MG_RUN_DIR/public"; }
    mg_message() { printf "%s\n" "$*" >>"$MG_RUN_DIR/public"; }
    mg_failure() { printf "%s\n" "$1" "$2" >>"$MG_RUN_DIR/failure"; }
    mg_status() { printf "%s\n" "$1" >>"$MG_RUN_DIR/status"; }
    xcrun() { printf "%s\n" "{\"errorCount\":0,\"warningCount\":0,\"warnings\":[{\"message\":\"generated-private-diagnostic\"}]}"; }
    xcodebuild() {
      local arg style=0 specifier=0
      [ "$1" = build-for-testing ] && [ -f "$MG_RUN_DIR/prepared" ] || return 1
      [ ! -f "$MG_RUN_DIR/rechecked" ] || return 1
      for arg in "$@"; do
        case "$arg" in
          MEGURI_MAC_AGENT_PROFILE_STYLE=Manual) style=1 ;;
          MEGURI_MAC_AGENT_PROFILE_SPECIFIER=generated-profile) specifier=1 ;;
          PROVISIONING_PROFILE_SPECIFIER=*|CODE_SIGN_STYLE=*) return 1 ;;
        esac
      done
      [ "$style/$specifier" = 1/1 ] || return 1
      printf "%s\n" build >>"$MG_RUN_DIR/order"
      touch "$MG_RUN_DIR/built"
      printf "%s\n" generated-private-diagnostic
      [ "$mode" != build-failure ]
    }
    python3() {
      case " $* " in
        *" --check "*)
          [ -f "$MG_RUN_DIR/built" ] || return 1
          local actual
          actual="$(cat)"
          [ "$actual" = "$MG_CLOUD_AGENT_MAC_PROFILE_BINDING" ] || return 1
          printf "%s\n" check >>"$MG_RUN_DIR/order"
          touch "$MG_RUN_DIR/rechecked"
          [ "$mode" = pass ] ;;
        *)
          [ -z "$MG_CLOUD_AGENT_MAC_PROFILE_BINDING" ] || return 1
          printf "%s\n" prepare >>"$MG_RUN_DIR/order"
          touch "$MG_RUN_DIR/prepared"
          printf "%s\n" generated-private-diagnostic >&2
          printf "%s\n" "{\"file\":\"generated-unused\",\"uuid\":\"generated-profile\"}" ;;
      esac
    }
    for mode in pass changed refused build-failure; do
      rm -f "$MG_RUN_DIR/prepared" "$MG_RUN_DIR/built" "$MG_RUN_DIR/rechecked" "$MG_RUN_DIR/delivered" \
        "$MG_RUN_DIR/failure" "$MG_RUN_DIR/status" "$MG_RUN_DIR/order" "$MG_RUN_DIR/public"
      MG_CLOUD_AGENT_MAC_PROFILE_BINDING=stale MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE=stale
      cleanup=""
      mg_ui_test --platform mac --config Debug --suite MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch || exit 1
      [ -f "$MG_RUN_DIR/built" ] || exit 2
      [ "$cleanup" = "rm -rf $(mvl_q "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE")" ] || exit 3
      grep -q generated-private-diagnostic "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE/validation.log" || exit 4
      grep -q generated-private-diagnostic "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE/native-build.log" || exit 5
      if grep -Eq "generated-private-diagnostic|native-build.log|validation.log|^log " "$MG_RUN_DIR/public"; then exit 6; fi
      case "$mode" in
        pass)
          [ -f "$MG_RUN_DIR/delivered" ] && [ ! -f "$MG_RUN_DIR/failure" ] && [ ! -f "$MG_RUN_DIR/status" ] || exit 7
          [ "$(tr "\n" / <"$MG_RUN_DIR/order")" = prepare/build/check/delivery/ ] || exit 8 ;;
        changed|refused)
          [ ! -f "$MG_RUN_DIR/delivered" ] || exit 9
          [ "$(tr "\n" / <"$MG_RUN_DIR/order")" = prepare/build/check/ ] || exit 10
          [ "$(head -n 1 "$MG_RUN_DIR/failure")" = agent-profile ] || exit 11
          [ "$(tail -n 1 "$MG_RUN_DIR/failure")" = "App-only Mac profile changed before drive VM delivery" ] || exit 12
          [ "$(cat "$MG_RUN_DIR/status")" = fail ] || exit 13 ;;
        build-failure)
          [ ! -f "$MG_RUN_DIR/delivered" ] && [ ! -f "$MG_RUN_DIR/rechecked" ] || exit 14
          [ "$(tr "\n" / <"$MG_RUN_DIR/order")" = prepare/build/ ] || exit 15
          [ "$(head -n 1 "$MG_RUN_DIR/failure")" = build ] || exit 16
          [ "$(tail -n 1 "$MG_RUN_DIR/failure")" = "physical development build failed; diagnostics remain in the private build log" ] || exit 17
          [ "$(cat "$MG_RUN_DIR/status")" = fail ] || exit 18 ;;
      esac
      if grep -q generated-private-diagnostic "$MG_RUN_DIR/failure" 2>/dev/null; then exit 19; fi
      [ "${MG_UI_MAC_PROFILE_SELECTOR+x}" != x ] || exit 20
      rm -rf "$MG_CLOUD_AGENT_MAC_PROFILE_PRIVATE"
    done
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [[ "$output" != *"generated-private-diagnostic"* ]]
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
