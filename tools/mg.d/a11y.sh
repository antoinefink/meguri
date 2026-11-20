# Accessibility checks (U10, R1977, R1978, R1980; U253 adds the screen
# sweep and VoiceOver scripts). Sourced by tools/mg; a host job.
#
# Runs the accessibility XCUITest suites (MeguriUITests/Accessibility) on a
# pool simulator or for the Mac, both in the macOS test VM (mac-vm.sh;
# queued, warn, without it). Suites attach
# `a11y-screen.<screen id>` JSON reports (AuditHelpers.screenReport), one
# per screen with a result per state; the job lists them as per-screen
# evidence of its platform (details.platform) in details.screens and
# screens.json, which `tools/mg critic --a11y <run>` reads, with the app
# sources it audited (details.input, mg_app_input_digest). Findings on the
# Mac window's own elements are listed as excluded, not counted.

mg_job a11y host mg_a11y \
  "Accessibility suites (R1977, R1978, R1980): [--platform iphone|ipad|mac] [--device <leased udid>] [--suite <test class>]... (default AccessibilityHelperTests): system audit at default and largest text size, light and dark, suppressions from Accessibility/suppressions.json, touch targets, VoiceOver order; simulator and Mac runs go to the macOS test VM (queued without it); per-screen results in screens.json"

MG_A11Y_SUITES="AccessibilityHelperTests"

mg_a11y__usage() {
  mg_failure usage "a11y: $1" "tools/mg --list"
  mg_message "a11y: $1"
  mg_status error
}

mg_a11y__disable_voiceover() {
  # Exit actions are LIFO: this runs before the VM's orphan cleanup. A live
  # XCTest can re-enable VoiceOver, so confirm it stopped under our lease
  # before disabling it. Leave the active marker for orphan cleanup on error.
  if [ "$MG_SIM_RUNNER" = vm ] && [ -n "$MG_MAC_VM_TESTING" ]; then
    if ! mg__mac_vm_stop_tests; then
      mg_failure voiceover-cleanup "could not confirm the remote test stopped before disabling simulator VoiceOver on $1 ($MG_MAC_VM_STOP_REASON); see log.txt" "tools/mg sim reset '$1'"
      mg_message "a11y: could not confirm the remote test stopped before VoiceOver cleanup on $1; see log.txt"
      mg_status error
      return 1
    fi
    MG_MAC_VM_TESTING=""
  fi
  if ! mg_devicectl device settings voiceover --device "$1" --disable >&2; then
    mg_failure voiceover-cleanup "could not disable simulator VoiceOver on $1; see log.txt" "tools/mg sim reset '$1'"
    mg_message "a11y: could not disable simulator VoiceOver on $1; see log.txt"
    mg_status error
    return 1
  fi
}

mg_a11y() {
  local platform=iphone device="" suites="" only=() s result since udid
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "iphone ipad mac" || { mg_a11y__usage "--platform takes iphone, ipad or mac"; return 0; }
        platform="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_a11y__usage "--device needs a UDID (tools/mg sim acquire <kind>)"; return 0; }
        device="$2" && shift 2
        ;;
      --suite)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[A-Z][A-Za-z0-9]*(/[A-Za-z0-9_]+)?$' ||
          { mg_a11y__usage "--suite takes a test class of MeguriUITests/Accessibility, e.g. AccessibilityHelperTests"; return 0; }
        suites="$suites $2" && shift 2
        ;;
      *) mg_a11y__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  [ -n "$suites" ] || suites="$MG_A11Y_SUITES"
  for s in $suites; do only[${#only[@]}]="-only-testing:MeguriUITests/$s"; done
  mg_detail suites "$(printf '%s\n' $suites | jq -R . | jq -sc .)"
  mg_detail platform "$(jq -n --arg p "$platform" '$p')"
  mg_detail input "$(jq -n --arg d "$(mg_app_input_digest)" 'if $d == "" then null else $d end')"
  MG_A11Y_PLATFORM="$platform"
  if [ "$platform" = mac ]; then
    [ -z "$device" ] || { mg_a11y__usage "--device is for simulators"; return 0; }
    mg_ui_suite_fixtures $suites || { mg_status error; return 0; }
    mg_mac_ui_begin || return 0
    mg_app_build mac Debug || return 0
    mg_mac_ui_take || return 0
    mg_ui_suite_fixtures_stage "$MG_MAC_RUNNER"
    result=$?
    if [ "$result" -ne 0 ]; then
      if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
      return 0
    fi
    since="$(mg_app_log_now)"
    mg_app_xcodebuild_test "$MG_RUN_DIR/mac" mac "platform=macOS,arch=arm64" Debug "${only[@]}"
    result=$?
    # Suites whose input macOS dropped are queued (mac-input), unless they
    # could not run at all or a test failed where the drop does not explain
    # it (mg_app_mac_input_result records those tests).
    if [ "$result" -le 1 ] && mg_app_mac_input_refused "$since" "$(printf '%s' "${suites# }" | tr ' ' ',')"; then
      mg_app_mac_input_result "$MG_RUN_DIR/mac.xcresult" "$result" test
      result=$?
    fi
    mg_a11y__report "$MG_RUN_DIR/mac" "$result"
    return 0
  fi
  mg_ui_suite_fixtures $suites || { mg_status error; return 0; }
  mg_sim_begin || return 0
  if [ -n "$device" ]; then
    mg_sim_check_given "$device" || return 0
  fi
  mg_app_build ios-sim Debug || return 0
  mg_sim_take || return 0
  mg_ui_suite_fixtures_stage "$MG_SIM_RUNNER"
  result=$?
  if [ "$result" -ne 0 ]; then
    if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
    return 0
  fi
  if [ -n "$device" ]; then
    mg_sim_check_given "$device" || return 0
    udid="$device"
  else
    mg_sim_lease "$platform" release || return 0
    udid="$MG_SIM_UDID"
    mg_sim_check_runtime "$udid" || return 0
  fi
  mg_on_exit "mg_a11y__disable_voiceover '$udid'"
  mg_sim_prepare "$udid" || return 0
  mg_app_xcodebuild_test "$MG_RUN_DIR/$platform" "$platform" "id=$udid" Debug "${only[@]}"
  result=$?
  mg_a11y__report "$MG_RUN_DIR/$platform" "$result"
}

# mg_a11y__report <prefix> <test result 0|1|2|3|4>: failing tests, per-screen
# reports from the attachments, the status. 3: macOS dropped the input (the
# run is queued; its failures are not the app's). Under dropped input the
# failing tests the drop does not explain are already recorded. 4: the test
# VM could not be reached (queued, mac-vm; nothing to report).
mg_a11y__report() {
  local prefix="$1" result="$2" summary attachments="$1-attachments" f
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
  rm -rf "$attachments"
  echo "[]" >"$MG_RUN_DIR/screens.json"
  if [ -d "$prefix.xcresult" ] && mg_drive__export_attachments "$prefix.xcresult" "$attachments" >/dev/null 2>&1; then
    jq -r --arg dir "$attachments" '.[].attachments[]? | select(.suggestedHumanReadableName | startswith("a11y-screen.")) | "\($dir)/\(.exportedFileName)"' \
      "$attachments/manifest.json" 2>/dev/null | while IFS= read -r f; do
      [ -f "$f" ] && jq -c . "$f"
    done | jq -sc --arg p "$MG_A11Y_PLATFORM" '[.[] | {screen, platform: $p, status,
      unsuppressed: ([.findings[] | select((.suppressed | not) and .excluded == null)] | length),
      excluded: ([.findings[] | select(.excluded != null)] | length),
      touchTargets: (.touchTargets | length), states: ((.states // {}) | map_values(.status))}]' \
      >"$MG_RUN_DIR/screens.json"
  fi
  mg_artifact screens "$MG_RUN_DIR/screens.json" "per-screen accessibility results"
  mg_detail screens "$(jq -c 'map({(.screen): .status}) | add // {}' "$MG_RUN_DIR/screens.json")"
  case "$result" in
    0)
      mg_message "a11y: $(jq -r '"\(.passed) passed, \(.skipped) skipped"' <<<"${summary:-{\"passed\":0,\"skipped\":0\}}")"
      mg_status pass
      ;;
    1)
      if [ -n "${MG_MAC_INPUT_REFUSED:-}" ]; then
        mg_message "a11y: $(jq -r '.failed' <<<"$summary") test(s) failed, not all of them where the input macOS dropped explains it (user step mac-input); see the failures"
      else
        mg_message "a11y: $(jq -r '.failed' <<<"$summary") test(s) failed"
      fi
      mg_status fail
      mg_queued_settle
      ;;
    3)
      mg_message "a11y: queued: macOS dropped the input XCUITest synthesized in $(mg_queued_parts_text); the suites did not test the app"
      mg_status pass
      mg_queued_settle
      ;;
    *)
      mg_message "a11y: the suites could not run; see log.txt"
      mg_status error
      ;;
  esac
}
