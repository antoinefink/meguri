# UI test suites (U150): any MeguriUITests class on a pool simulator or for
# the Mac, both in the macOS test VM (mac-vm.sh; queued, warn, without it),
# as a11y runs its suites. Sourced by tools/mg; a host job.

mg_job ui-test host mg_ui_test \
  "XCUITest suites of MeguriUITests: --suite <class>[/<test>]... [--platform iphone|ipad|mac] [--device <leased udid>] [--config Debug|Profile]; optional --allocations only for the Profile Mac thousand-swap method; functional assertions only, no performance receipt"

mg_ui_test__usage() {
  mg_failure usage "ui-test: $1" "tools/mg --list"
  mg_message "ui-test: $1"
  mg_status error
}

mg_ui_test() {
  local platform=iphone device="" config=Debug suites="" only=() build_args=() s result since udid allocations=0
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
      --allocations) allocations=1; shift ;;
      *) mg_ui_test__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  [ -n "$suites" ] || { mg_ui_test__usage "name at least one --suite"; return 0; }
  if [ "$allocations" -eq 1 ]; then
    [ "$platform" = mac ] && [ "$config" = Profile ] && [ -z "$device" ] &&
      [ "$suites" = ' CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory' ] ||
      { mg_ui_test__usage "--allocations requires Profile, mac and exactly --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory"; return 0; }
    mg_detail diagnostic_mode '"allocations"'
    mg_detail ordinary_performance_receipt false
    mg_detail allocations_coverage '"uncertified/partial"'
  fi
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
    if [ "$allocations" -eq 1 ]; then
      # A diagnostic must never take the explicit ordinary UI host fallback.
      MEGURI_MAC_UI_ALLOW_HOST=0 mg_mac_ui_begin || return 0
      [ "$MG_MAC_RUNNER" = vm ] || { mg_ui_test__usage "--allocations requires the VM runner"; return 0; }
    else
      mg_mac_ui_begin || return 0
    fi
    mg_app_build mac "$config" ${build_args[0]+"${build_args[@]}"} || return 0
    mg_mac_ui_take || return 0
    if [ "$allocations" -eq 1 ]; then
      mg_ui_test__allocations_start || return 0
    fi
    since="$(mg_app_log_now)"
    mg_app_xcodebuild_test "$MG_RUN_DIR/mac" mac "platform=macOS,arch=arm64" "$config" "${only[@]}"
    result=$?
    if [ "$result" -le 1 ] && mg_app_mac_input_refused "$since" "$(printf '%s' "${suites# }" | tr ' ' ',')"; then
      mg_app_mac_input_result "$MG_RUN_DIR/mac.xcresult" "$result" test
      result=$?
    fi
    mg_ui_test__report "$MG_RUN_DIR/mac" "$result"
    [ "$allocations" -eq 0 ] || mg_ui_test__allocations_finish
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

# This cleanup is registered AFTER the VM lease, so stop/fetch runs while
# still holding it, on normal exit and TERM/HUP alike. Never kill local SSH
# as a substitute for stopping the VM's observer/profiler.
mg_ui_test__allocations_start() {
  [ "$MG_MAC_RUNNER" = vm ] || return 1
  local rc=0
  MG_UI_ALLOCATIONS_DIR="$MG_RUN_DIR/allocations"
  MG_UI_ALLOCATIONS_PENDING=1
  MG_UI_ALLOCATIONS_REMOTE=""
  mg_on_exit mg_ui_test__allocations_finish
  if ! mkdir -p "$MG_UI_ALLOCATIONS_DIR" || ! cp "$MG_ROOT/tools/scripts/mac-allocations.py" "$MG_UI_ALLOCATIONS_DIR/mac-allocations.py"; then
    mg_ui_test__allocations_error "could not prepare owned helper"
    MG_UI_ALLOCATIONS_PENDING=""
    return 1
  fi
  mg_mac_vm_copy_in "$MG_UI_ALLOCATIONS_DIR" || rc=$?
  if [ "$rc" -ne 0 ]; then
    if [ "$rc" -eq 2 ]; then
      mg_detail allocations_incomplete true
      mg_detail allocations_remote_evidence "$(jq -n --arg p "$MG_UI_ALLOCATIONS_DIR" '$p')"
      mg_queued_settle
    else
      mg_ui_test__allocations_error "helper copy into VM failed"
    fi
    MG_UI_ALLOCATIONS_PENDING=""
    return 1
  fi
  MG_UI_ALLOCATIONS_REMOTE=1
  if ! mg_mac_vm_ssh "$(mg_mac_vm_q python3 "$MG_UI_ALLOCATIONS_DIR/mac-allocations.py" --start "$MG_UI_ALLOCATIONS_DIR" "$MG_ROOT/$MG_APP_DD/Build/Products/Profile/Meguri.app/Contents/MacOS/Meguri" "$MG_RUN_ID")" >>"$MG_RUN_DIR/allocations-control.log" 2>&1; then
    mg_ui_test__allocations_error "observer start unknown or failed; see allocations-control.log"
    mg_ui_test__allocations_finish
    return 1
  fi
}

mg_ui_test__allocations_error() {
  mg_detail allocations_diagnostic_failed true
  mg_failure allocations-diagnostic "$1"
  # An XCTest failure and its counts remain the primary functional result.
  case "$(cat "$MG_STATE/status" 2>/dev/null)" in
    fail) ;;
    *) mg_status error; mg_message "ui-test: allocation diagnostic failed: $1" ;;
  esac
}

mg_ui_test__allocations_finish() {
  [ -n "${MG_UI_ALLOCATIONS_PENDING:-}" ] || return 0
  MG_UI_ALLOCATIONS_PENDING=""
  local rc=0 copy_rc=0 copied=0 out trace_file
  if [ -n "$MG_UI_ALLOCATIONS_REMOTE" ]; then
    out="$MG_UI_ALLOCATIONS_DIR/metadata.json"
    # Always ask the remote helper to stop, including after a lost start SSH.
    mg_mac_vm_ssh "$(mg_mac_vm_q python3 "$MG_UI_ALLOCATIONS_DIR/mac-allocations.py" --stop "$MG_UI_ALLOCATIONS_DIR")" >>"$MG_RUN_DIR/allocations-control.log" 2>&1 || rc=$?
    [ "$rc" -eq 0 ] || mg_ui_test__allocations_error "remote cleanup/trace finalization failed or unknown; see allocations-control.log"
    # Copy even on diagnostic failure, before releasing the lease. Fetching
    # the whole owned directory preserves metadata and partial trace/logs.
    mg_mac_vm_copy_out "$MG_UI_ALLOCATIONS_DIR" || copy_rc=$?
    if [ "$copy_rc" -eq 0 ]; then
      copied=1
      trace_file="$(find "$MG_UI_ALLOCATIONS_DIR/allocations.trace" -type f -size +0c -print -quit 2>/dev/null)"
      if [ -z "$trace_file" ] || [ ! -f "$out" ] || ! jq -e '.status == "pass" and .cleanup_confirmed == true and .start_seen == true and (.tracing_started_notification_at | type == "string") and .finalized == true and .trace_exit == 0 and .coverage == "uncertified/partial" and .ordinary_performance_receipt == false' "$out" >/dev/null; then
        mg_ui_test__allocations_error "requested trace evidence is incomplete; inspect allocations/metadata.json"
      fi
    elif [ "$copy_rc" -eq 2 ]; then
      mg_detail allocations_incomplete true
    else
      mg_ui_test__allocations_error "trace/metadata/log copy from VM failed"
    fi
    # A queued/failed tar can still have delivered useful partial evidence.
    if [ -d "$MG_UI_ALLOCATIONS_DIR" ]; then
      if [ -f "$out" ] && jq -e 'type == "object"' "$out" >/dev/null 2>&1; then
        mg_detail allocations "$(cat "$out")"
      fi
      mg_artifact allocations "$MG_UI_ALLOCATIONS_DIR" "diagnostic Allocations evidence (may be incomplete); coverage uncertified/partial"
    fi
    # Unknown cleanup or failed copy retains remote evidence for inspection
    # and the next lease's narrow orphan recognizer. Remove only our files.
    if [ "$rc" -le 1 ] && [ "$copied" -eq 1 ] && jq -e '.cleanup_confirmed == true' "$MG_UI_ALLOCATIONS_DIR/metadata.json" >/dev/null 2>&1; then
      mg_mac_vm_ssh "rm -rf $(mg_mac_vm_q "$MG_UI_ALLOCATIONS_DIR")" >>"$MG_RUN_DIR/allocations-control.log" 2>&1 ||
        mg_ui_test__allocations_error "owned VM files could not be removed"
    else
      mg_detail allocations_remote_evidence "$(jq -n --arg p "$MG_UI_ALLOCATIONS_DIR" '$p')"
    fi
  fi
  [ ! -f "$MG_RUN_DIR/allocations-control.log" ] || mg_artifact log "$MG_RUN_DIR/allocations-control.log" "Allocations VM control log"
  mg_queued_settle
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
