# UI test suites (U150): any MeguriUITests class on a pool simulator or for
# the Mac, both in the macOS test VM (mac-vm.sh; queued, warn, without it),
# as a11y runs its suites. Sourced by tools/mg; a host job.

mg_job ui-test host mg_ui_test \
  "XCUITest suites of MeguriUITests: --suite <class>[/<test>]... [--platform iphone|ipad|mac] [--device <leased udid>] [--config Debug|Profile]; Debug by default, simulator and Mac runs in the macOS test VM (queued without it); functional assertions only, no performance receipt"

mg_ui_test__usage() {
  mg_failure usage "ui-test: $1" "tools/mg --list"
  mg_message "ui-test: $1"
  mg_status error
}

mg_ui_test() {
  local platform=iphone device="" config=Debug suites="" only=() build_args=() s result since udid
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "iphone ipad mac" || { mg_ui_test__usage "--platform takes iphone, ipad or mac"; return 0; }
        platform="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_ui_test__usage "--device needs a UDID (tools/mg sim acquire <kind>)"; return 0; }
        device="$2" && shift 2
        ;;
      --config)
        [ "$#" -ge 2 ] && mg_word_in "$2" "Debug Profile" || { mg_ui_test__usage "--config takes Debug or Profile"; return 0; }
        config="$2" && shift 2
        ;;
      --suite)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[A-Z][A-Za-z0-9]*(/[A-Za-z0-9_]+)?$' ||
          { mg_ui_test__usage "--suite takes a test class of MeguriUITests, or Class/testMethod, e.g. NavigationLayoutTests"; return 0; }
        suites="$suites $2" && shift 2
        ;;
      *) mg_ui_test__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  [ -n "$suites" ] || { mg_ui_test__usage "name at least one --suite"; return 0; }
  for s in $suites; do only[${#only[@]}]="-only-testing:MeguriUITests/$s"; done
  mg_detail suites "$(printf '%s\n' $suites | jq -R . | jq -sc .)"
  mg_detail platform "$(jq -n --arg p "$platform" '$p')"
  mg_detail config "$(jq -n --arg c "$config" '$c')"
  if [ "$config" = Profile ]; then
    export MG_APP_TEST_SCHEME=Meguri-Profile MG_APP_TEST_PLAN=Performance
    build_args=(Performance Meguri-Profile)
  fi
  if [ "$platform" = mac ]; then
    [ -z "$device" ] || { mg_ui_test__usage "--device is for simulators"; return 0; }
    mg_mac_ui_begin || return 0
    mg_app_build mac "$config" ${build_args[0]+"${build_args[@]}"} || return 0
    mg_mac_ui_take || return 0
    since="$(mg_app_log_now)"
    mg_app_xcodebuild_test "$MG_RUN_DIR/mac" mac "platform=macOS,arch=arm64" "$config" "${only[@]}"
    result=$?
    if [ "$result" -le 1 ] && mg_app_mac_input_refused "$since" "$(printf '%s' "${suites# }" | tr ' ' ',')"; then
      mg_app_mac_input_result "$MG_RUN_DIR/mac.xcresult" "$result" test
      result=$?
    fi
    mg_ui_test__report "$MG_RUN_DIR/mac" "$result"
    return 0
  fi
  mg_sim_begin || return 0
  if [ -n "$device" ]; then
    mg_sim_check_given "$device" || return 0
  fi
  mg_app_build ios-sim "$config" ${build_args[0]+"${build_args[@]}"} || return 0
  mg_sim_take || return 0
  if [ -n "$device" ]; then
    mg_sim_check_given "$device" || return 0
    udid="$device"
  else
    mg_sim_lease "$platform" release || return 0
    udid="$MG_SIM_UDID"
    mg_sim_check_runtime "$udid" || return 0
  fi
  mg_sim_prepare "$udid" || return 0
  mg_app_xcodebuild_test "$MG_RUN_DIR/$platform" "$platform" "id=$udid" "$config" "${only[@]}"
  result=$?
  mg_ui_test__report "$MG_RUN_DIR/$platform" "$result"
}

# mg_ui_test__report <prefix> <test result 0|1|2|3|4>: counts and failing tests
# from the result bundle, then the status. 3: dropped input (queued,
# mac-input; independent failures were already recorded). 4: the test VM
# could not be reached (queued, mac-vm).
mg_ui_test__report() {
  local prefix="$1" result="$2" summary f
  if [ "$result" -eq 4 ]; then
    mg_status pass
    mg_queued_settle
    return 0
  fi
  summary="$(xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -c '{passed: .passedTests, failed: .failedTests, skipped: .skippedTests}')" || summary=""
  if [ -n "$summary" ]; then
    mg_count passed "$(jq .passed <<<"$summary")"
    mg_count failed "$(jq .failed <<<"$summary")"
    mg_count skipped "$(jq .skipped <<<"$summary")"
    mg_count total "$(jq '.passed + .failed + .skipped' <<<"$summary")"
    [ "$result" -eq 3 ] || [ -n "${MG_MAC_INPUT_REFUSED:-}" ] || xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null | jq -c '
      [.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed")
        | {id: .nodeIdentifier, message: ([.children[]? | select(.nodeType == "Failure Message") | .name] | join("; ") | .[0:800])}] | .[]' |
      while IFS= read -r f; do mg_failure "test/$(jq -r .id <<<"$f")" "$(jq -r .message <<<"$f")"; done
  fi
  [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "result bundle"
  case "$result" in
    0)
      mg_message "ui-test: $(jq -r '"\(.passed) passed, \(.skipped) skipped"' <<<"${summary:-{\"passed\":0,\"skipped\":0\}}")"
      mg_status pass
      ;;
    1)
      mg_message "ui-test: $(jq -r '.failed' <<<"${summary:-{\"failed\":\"some\"\}}") test(s) failed"
      mg_status fail
      mg_queued_settle
      ;;
    3)
      mg_message "ui-test: queued: macOS dropped the input XCUITest synthesized in $(mg_queued_parts_text); the suites did not test the app"
      mg_status pass
      mg_queued_settle
      ;;
    *)
      mg_message "ui-test: the suites could not run; see log.txt"
      mg_status error
      ;;
  esac
}
