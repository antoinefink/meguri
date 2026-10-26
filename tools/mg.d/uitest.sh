# UI test suites (U150): any MeguriUITests class on a pool simulator or for
# the Mac, both in the macOS test VM (mac-vm.sh; queued, warn, without it),
# as a11y runs its suites. An explicit physical iOS target runs through
# host Xcode only, with no screen or simulator fallback.

mg_job ui-test host mg_ui_test \
  "XCUITest suites of MeguriUITests: --suite <class>[/<test>]... [--platform iphone|ipad|mac] [--device <leased udid> | --physical-device <explicit identifier>] [--config Debug|Profile]; --list-physical-devices inventories connected iOS devices; functional assertions only, no performance receipt"

mg_ui_test__usage() {
  mg_failure usage "ui-test: $1" "tools/mg --list"
  mg_message "ui-test: $1"
  mg_status error
}

# xcdevice's JSON array is kept in memory; only these public destination
# fields enter the receipt. Names, errors and non-iOS identifiers stay out.
mg_ui_test__physical_inventory() {
  local raw
  raw="$(xcrun xcdevice list --timeout 20 2>/dev/null)" &&
    MG_UI_PHYSICAL_DEVICES="$(jq -cse '
      if length != 1 or (.[0] | type) != "array" then error("expected device array") else .[0] end
      | if all(.[]; type == "object" and (.identifier | type) == "string"
          and (.platform | type) == "string" and (.simulator | type) == "boolean")
        then . else error("invalid device entry") end
      | map(select(.platform == "com.apple.platform.iphoneos" and .simulator == false)
          | {identifier, modelCode, operatingSystemVersion, available})
      | if all(.[]; (.modelCode | type) == "string" and (.operatingSystemVersion | type) == "string"
          and (.available | type) == "boolean")
        then . else error("invalid physical device entry") end' <<<"$raw" 2>/dev/null)" || {
      mg_failure physical-device-inventory "xcdevice did not return a readable device JSON array"
      mg_status error
      return 1
    }
  MG_UI_PHYSICAL_REJECTED="$(jq -r --arg id "${1:-}" 'any(.[];
    .identifier == $id and (.simulator != false or .platform != "com.apple.platform.iphoneos"))' <<<"$raw")"
  mg_detail devices "$MG_UI_PHYSICAL_DEVICES"
}

mg_ui_test__physical_check() {
  local rec
  mg_ui_test__physical_inventory "$1" || return 1
  if [ "$MG_UI_PHYSICAL_REJECTED" = true ]; then
    mg_ui_test__usage "--physical-device must identify a real iOS device, never a simulator or Mac"
    return 1
  fi
  rec="$(jq -c --arg id "$1" 'first(.[] | select(.identifier == $id)) // null' <<<"$MG_UI_PHYSICAL_DEVICES")"
  if [ "$rec" != null ] && ! jq -e --arg axis "$2" '
      .modelCode | if $axis == "iphone" then test("^iPhone")
      elif $axis == "ipad" then test("^iPad") else false end' <<<"$rec" >/dev/null; then
    mg_ui_test__usage "the physical device model must match --platform iphone or ipad"
    return 1
  fi
  if [ "$rec" = null ] || [ "$(jq -r .available <<<"$rec")" != true ]; then
    mg_user_step device-developer-mode missing true "Connect, unlock and pair the explicitly selected iOS device" \
      "Connect and unlock the phone, pair it in Xcode, and enable Developer Mode on it; then rerun the same explicit --physical-device command."
    mg_queued_part device-developer-mode ui-test "the selected physical iOS device is missing or unavailable"
    mg_message "ui-test: queued: connect, unlock and Xcode-pair the selected phone with Developer Mode enabled"
    mg_status warn
    mg_queued_settle
    return 1
  fi
  if ! jq -e 'try (.operatingSystemVersion | capture("^(?<major>[0-9]+)(\\.| |$)").major | tonumber >= 27)
      catch false' <<<"$rec" >/dev/null 2>&1; then
    mg_ui_test__usage "the physical device must run iOS 27 or later"
    return 1
  fi
  MG_UI_PHYSICAL_DEVICE="$1"
}

# This one case (or its containing class) needs the generated media fixture
# and a native destination capture. No other selector acquires this evidence.
mg_ui_test__media_selected() {
  local selector
  for selector in $1; do
    case "$selector" in
      CollectionEntryJourneyTests|CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace) return 0 ;;
    esac
  done
  return 1
}

# Keep VM reachability queued; a refused capture or missing package is an error.
mg_ui_test__media_command() {
  local rc
  MG_UI_MEDIA_COMMAND_OUTPUT="$("$@" 2>&1)"
  rc=$?
  [ "$rc" -ne 0 ] || return 0
  if mvl_unreachable "$rc" "$MG_UI_MEDIA_COMMAND_OUTPUT"; then
    mg__mac_vm_unreachable "the VM could not run native media destination capture" native-media-export
    return 2
  fi
  return 1
}

mg_ui_test__media_prepare() {
  local runner="$1" rc
  if [ "$runner" != vm ]; then
    mg_failure native-media-export "native media destination capture requires the leased test VM"
    mg_status error
    return 1
  fi
  # Source guard and stable per-run output; never honor an ambient handoff.
  case "$MG_RUN_DIR" in "$MG_ROOT/build/mg/ui-test/"*) ;; *) return 1 ;; esac
  MG_UI_MEDIA_NAME="$(python3 "$MG_ROOT/tools/scripts/native-media-export-capture.py" identity "$MG_RUN_DIR")" || {
    mg_failure native-media-source "requires the closed, generated typical-1 fixture and an own-run destination"
    mg_status error
    return 1
  }
  [[ "$MG_UI_MEDIA_NAME" =~ ^Meguri-Media-[a-f0-9]{32}\.colpkg$ ]] || return 1
  mg_vm_fixture "$MG_ROOT/build/fixtures/recipes/typical-1"
  rc=$?
  [ "$rc" -eq 0 ] || { [ "$rc" -ne 2 ] || mg_queued_settle; return "$rc"; }
  if [ "$MG_UI_MEDIA_PLATFORM" != mac ]; then
    mg_mac_vm_copy_in "$MG_ROOT/tools/scripts/native-media-export-capture.py" "$MG_RUN_DIR/native-media-export" || return $?
    export TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT="$MG_RUN_DIR/native-media-export"
  fi
  export TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME="$MG_UI_MEDIA_NAME"
  MG_UI_MEDIA_READY=1
}

# Called only after a successful run and checked summary. The exact media case
# must also be present and Passed; Saved/hash alone is no content/client proof.
mg_ui_test__media_capture() {
  local prefix="$1" platform="$2" device="$3" tree container diagnostic
  MG_UI_MEDIA_PHASE=tree
  tree="$(xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null)" || return 1
  jq -e '[.. | objects | select(.nodeType? == "Test Case")
      | select(.nodeIdentifier? | strings | test("^(MeguriUITests/)?CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace(\\(\\))?$"))] as $cases
      | ($cases | length) == 1 and $cases[0].result == "Passed"' <<<"$tree" >/dev/null 2>&1 || return 1
  MG_UI_MEDIA_PHASE=copy
  if [ "$platform" != mac ]; then
    mg_ui_test__media_command mg_simctl get_app_container "$device" com.antoinefink.meguri.dev data || return $?
    container="$MG_UI_MEDIA_COMMAND_OUTPUT"
    # Validate before passing the response to any file-reading VM command.
    [[ "$device" =~ ^[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] || return 1
    [[ "$container" =~ ^/Users/[A-Za-z0-9._-]+/Library/Developer/CoreSimulator/Devices/[A-Fa-f0-9-]+/data/Containers/Data/Application/[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] || return 1
    case "$container" in */Devices/"$device"/data/Containers/Data/Application/*) ;; *) return 1 ;; esac
    mg_ui_test__media_command mg_mac_vm_ssh "python3 $(mvl_q "$MG_ROOT/tools/scripts/native-media-export-capture.py" simulator "$MG_RUN_DIR" "$container" "$device" "$MG_UI_MEDIA_NAME")" || return $?
    mg_sim_fetch "$MG_RUN_DIR/native-media-export/$MG_UI_MEDIA_NAME" || return $?
  else
    MG_UI_MEDIA_PHASE=export
    diagnostic="$(python3 "$MG_ROOT/tools/scripts/native-media-export-capture.py" mac \
      "$MG_RUN_DIR" "$MG_UI_MEDIA_NAME" "$prefix.xcresult" 2>&1)" || {
      case "$diagnostic" in
        FIXED-PHASE:export) MG_UI_MEDIA_PHASE=export ;;
        FIXED-PHASE:manifest) MG_UI_MEDIA_PHASE=manifest ;;
        FIXED-PHASE:copy) MG_UI_MEDIA_PHASE=copy ;;
      esac
      return 1
    }
  fi
  MG_UI_MEDIA_PHASE=receipt
  python3 "$MG_ROOT/tools/scripts/native-media-export-capture.py" receipt "$MG_RUN_DIR" "$MG_UI_MEDIA_NAME" >/dev/null 2>&1 || return 1
  mg_artifact native-media-export "$MG_RUN_DIR/native-media-export/$MG_UI_MEDIA_NAME" "file received by native destination; content/client proof pending"
  mg_artifact native-media-source "$MG_RUN_DIR/expected-source.json" "closed original typical-1 file hashes and manifest identity"
  mg_artifact native-media-receipt "$MG_RUN_DIR/native-media-export.json" "destination SHA256 and size; E7.4 acceptance pending"
  mg_detail native_media_export "$(cat "$MG_RUN_DIR/native-media-export.json")"
}

mg_ui_test() {
  local platform=iphone device="" config=Debug suites="" only=() build_args=() s result since udid
  local physical="" inventory="" run_args="" MG_UI_PHYSICAL_DEVICE=""
  local media="" MG_UI_MEDIA_READY="" MG_UI_MEDIA_PLATFORM="" MG_UI_MEDIA_DEVICE="" MG_UI_MEDIA_NAME="" MG_UI_MEDIA_PHASE=""
  local TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME
  unset TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME
  while [ "$#" -gt 0 ]; do
    [ "$1" = --list-physical-devices ] || run_args=1
    case "$1" in
      --list-physical-devices)
        [ -z "$inventory" ] || { mg_ui_test__usage "duplicate --list-physical-devices"; return 0; }
        inventory=1 && shift
        ;;
      --physical-device)
        [ "$#" -ge 2 ] && [ -z "$physical" ] && [[ "$2" =~ ^([A-Fa-f0-9]{8}-[A-Fa-f0-9]{16}|[A-Fa-f0-9]{8}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{12})$ ]] ||
          { mg_ui_test__usage "--physical-device needs one explicit Xcode device identifier"; return 0; }
        physical="$2" && shift 2
        ;;
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
  if [ -n "$inventory" ]; then
    [ -z "$run_args" ] || { mg_ui_test__usage "--list-physical-devices cannot be combined with run arguments"; return 0; }
    mg_ui_test__physical_inventory || return 0
    mg_message "ui-test: physical iOS device inventory"
    mg_status pass
    return 0
  fi
  if [ -n "$physical" ]; then
    [ -z "$device" ] && [ "$platform" != mac ] || { mg_ui_test__usage "--physical-device conflicts with --device and --platform mac"; return 0; }
  fi
  [ -n "$suites" ] || { mg_ui_test__usage "name at least one --suite"; return 0; }
  if mg_ui_test__media_selected "$suites"; then
    media=1
    [ -z "$physical" ] || { mg_ui_test__usage "native media export capture is unsupported on physical devices; use the leased VM"; return 0; }
  fi
  MG_UI_MEDIA_PLATFORM="$platform"
  for s in $suites; do only[${#only[@]}]="-only-testing:MeguriUITests/$s"; done
  mg_detail suites "$(printf '%s\n' $suites | jq -R . | jq -sc .)"
  mg_detail platform "$(jq -n --arg p "$platform" '$p')"
  mg_detail config "$(jq -n --arg c "$config" '$c')"
  if [ "$config" = Profile ]; then
    export MG_APP_TEST_SCHEME=Meguri-Profile MG_APP_TEST_PLAN=Performance
    build_args=(Performance Meguri-Profile)
  fi
  if [ -n "$physical" ]; then
    mg_detail destination_platform '"ios-device"'
    mg_ui_test__physical_check "$physical" "$platform" || return 0
    mg_host_lease --no-session || return 0
    mg_ui_test__physical_check "$physical" "$platform" || return 0
    if [ "$config" = Debug ]; then
      export MG_APP_TEST_SCHEME=Meguri MG_APP_TEST_PLAN=UI
    fi
    mg_app_build ios-device "$config" ${build_args[0]+"${build_args[@]}"} || return 0
    mg_ui_test__physical_check "$physical" "$platform" || return 0
    mg_app_xcodebuild_test "$MG_RUN_DIR/ios-device" ios-device "platform=iOS,id=$physical" "$config" "${only[@]}"
    result=$?
    mg_ui_test__report "$MG_RUN_DIR/ios-device" "$result" physical "$suites"
    return 0
  fi
  if [ "$platform" = mac ]; then
    [ -z "$device" ] || { mg_ui_test__usage "--device is for simulators"; return 0; }
    mg_mac_ui_begin || return 0
    mg_app_build mac "$config" ${build_args[0]+"${build_args[@]}"} || return 0
    mg_mac_ui_take || return 0
    for s in $suites; do
      case "$s" in
        DeckManagementUITests|DeckManagementUITests/*)
          mg_vm_fixture "$MG_ROOT/build/fixtures/recipes/local-review-1"
          result=$?
          if [ "$result" -ne 0 ]; then
            if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
            return 0
          fi
          break
          ;;
      esac
    done
    if [ -n "$media" ]; then
      mg_ui_test__media_prepare "$MG_MAC_RUNNER"
      result=$?
      if [ "$result" -ne 0 ]; then
        if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
        return 0
      fi
    fi
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
  MG_UI_MEDIA_DEVICE="$udid"
  if [ -n "$media" ]; then
    mg_ui_test__media_prepare "$MG_SIM_RUNNER"
    result=$?
    if [ "$result" -ne 0 ]; then
      if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
      return 0
    fi
  fi
  mg_sim_prepare "$udid" || return 0
  mg_app_xcodebuild_test "$MG_RUN_DIR/$platform" "$platform" "id=$udid" "$config" "${only[@]}"
  result=$?
  mg_ui_test__report "$MG_RUN_DIR/$platform" "$result"
}

# mg_ui_test__report <prefix> <test result 0|1|2|3|4> [physical <selectors>]: counts and failing tests
# from the result bundle, then the status. 3: dropped input (queued,
# mac-input; independent failures were already recorded). 4: the test VM
# could not be reached (queued, mac-vm).
mg_ui_test__report() {
  local prefix="$1" result="$2" summary f cases capture_result
  if [ "$result" -eq 4 ] && [ "${3:-}" != physical ]; then
    mg_status pass
    mg_queued_settle
    return 0
  fi
  summary="$(
    set -o pipefail
    xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -c '{passed: .passedTests, failed: .failedTests, skipped: .skippedTests, outcome: .result}')" || summary=""
  if [ "${3:-}" = physical ]; then
    case "$result" in 0|1|2) ;; *) result=2 ;; esac
    if ! jq -se 'length == 1 and (.[0] | type == "object" and all(.passed, .failed, .skipped;
        type == "number" and . >= 0 and . == floor))' <<<"$summary" >/dev/null 2>&1; then
      mg_failure physical-device-results "physical UI tests require a readable summary with integer test counts"
      mg_status error
      [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "result bundle"
      return 0
    fi
    if ! jq -e '.outcome == "Passed" and .passed > 0 and .failed == 0 and .skipped == 0' <<<"$summary" >/dev/null; then
      mg_failure physical-device-results "physical UI tests require passed tests, zero failures and zero skipped tests"
      result=1
    fi
    cases="$(set -o pipefail
      xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null |
      jq -cse '
        if length != 1 or (.[0] | type) != "object" or (.[0].testNodes | type) != "array"
        then error("invalid test tree") else .[0] end
        | if all(.testNodes[]; type == "object") and all(.testNodes[] | .. | objects;
            all(.children?, .subTestNodes?; . == null or (type == "array" and all(.[]; type == "object"))))
          then . else error("invalid test nodes") end
        | [.testNodes[] | .. | objects | select(.nodeType? == "Test Case")
          | select(([.children[]?, .subTestNodes[]? | .. | objects | select(.nodeType? == "Test Case")] | length) == 0)
          | if (.nodeIdentifier | type) != "string" or (.result | type) != "string"
            then error("invalid test case") else . end
          | if (.nodeIdentifier | test("^(MeguriUITests/)?[A-Z][A-Za-z0-9]*/[A-Za-z0-9_]+(\\(\\))?$"))
            then . else error("invalid method identifier") end
          | {id: (.nodeIdentifier | capture("^(MeguriUITests/)?(?<id>[A-Z][A-Za-z0-9]*/[A-Za-z0-9_]+)(\\(\\))?$").id), result}]' 2>/dev/null)" || cases=""
    if [ -z "$cases" ]; then
      mg_failure physical-device-results "physical UI tests require a readable test tree"
      result=2
    elif ! jq -e --arg selectors "${4:-}" --argjson count "$(jq .passed <<<"$summary")" '
        . as $cases | ($selectors | split(" ") | map(select(length > 0))) as $requested
        | length > 0 and length == $count and all(.[]; .result == "Passed") and ($requested | length > 0)
          and all($requested[]; . as $s | any($cases[];
            if ($s | contains("/")) then .id == $s else (.id | split("/")[0]) == $s end))' <<<"$cases" >/dev/null; then
      mg_failure physical-device-results "every physical selector must have a passed test method"
      [ "$result" -eq 2 ] || result=1
    fi
  elif [ "$result" -eq 0 ]; then
    if ! jq -se 'length == 1 and (.[0] | type == "object" and all(.passed, .failed, .skipped;
        type == "number" and . >= 0 and . == floor))' <<<"$summary" >/dev/null 2>&1; then
      mg_failure ui-test-results "UI tests require a readable summary with integer test counts"
      summary=""
      result=2
    elif ! jq -e '.outcome == "Passed" and .passed > 0 and .failed == 0' <<<"$summary" >/dev/null; then
      mg_failure ui-test-results "UI tests require a Passed outcome, at least one passed test and zero failed tests"
      result=1
    fi
  fi
  if [ "$result" -eq 0 ] && [ -n "${MG_UI_MEDIA_READY:-}" ]; then
    if ! jq -e '.skipped == 0' <<<"$summary" >/dev/null; then
      mg_failure native-media-export "native media destination capture requires a non-skipped run"
      result=1
    else
      mg_ui_test__media_capture "$prefix" "$MG_UI_MEDIA_PLATFORM" "$MG_UI_MEDIA_DEVICE"
      capture_result=$?
      if [ "$capture_result" -eq 2 ]; then
        mg_status pass
        mg_queued_settle
        return 0
      elif [ "$capture_result" -ne 0 ]; then
        mg_detail native_media_capture_failure "$(jq -nc --arg phase "$MG_UI_MEDIA_PHASE" '{phase: $phase}')"
        case "$MG_UI_MEDIA_PHASE" in
          tree) mg_failure native-media-export "FIXED-PHASE:tree: exact media method missing, duplicate or not Passed; no export evidence registered" ;;
          export) mg_failure native-media-export "FIXED-PHASE:export: own-run selective attachment export refused; no export evidence registered" ;;
          manifest) mg_failure native-media-export "FIXED-PHASE:manifest: expected one safe matching nonfailure package attachment; no export evidence registered" ;;
          copy) mg_failure native-media-export "FIXED-PHASE:copy: retained native package copy refused; no export evidence registered" ;;
          receipt) mg_failure native-media-export "FIXED-PHASE:receipt: fixture or destination receipt validation failed; no export evidence registered" ;;
        esac
        result=2
      fi
    fi
  fi
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
