# Package test bundles on a simulator (U66). Sourced by tools/mg; a host job.
#
# `swift test` runs the packages' tests on this Mac only. unit-sim runs test
# bundles of the app's Unit plan (app/TestPlans/Unit.xctestplan: the
# MeguriKit and MeguriCore package targets) on a pool simulator in the macOS
# test VM, from a Debug build-for-testing of that plan: the card runtime
# suites (CardRenderingTests) on iOS, as U66's verification asks. Without
# the VM it is queued (warn, user step mac-vm), like every simulator job.
#
# A package test bundle runs in the hostless xctest agent, which has no
# UIApplication and so no window scene: a UIWindow needs one since iOS 26,
# and WebKit runs no animation frames and applies no appearance to a web
# view outside a window. The bundles in MG_UNIT_SIM_HOSTED are therefore
# hosted in the Debug app (built by the Unit plan too): the job rewrites
# their xctestrun entries as Xcode writes an app-hosted bundle's (host app,
# XCTest's bundle injection) before the products go to the VM.

mg_job unit-sim host mg_unit_sim \
  "Unit-plan package test bundles on a pool simulator in the macOS test VM (queued without it): [--platform iphone|ipad] [--device <leased udid>] [--only <Target[/Suite...[/test]]>]... (default CardRenderingTests/WebKitSuites/CardRuntimeSuites: the card runtime suites); Debug build-for-testing of the Unit plan, then test-without-building there"

# The card runtime suites. The bundle's U65 rendering spikes are Mac-host
# checks: their iOS legs run in the app (perf-ui), and their audio, seek and
# web content footprint checks cannot run in a simulator in the VM.
MG_UNIT_SIM_DEFAULT=CardRenderingTests/WebKitSuites/CardRuntimeSuites
MG_UNIT_SIM_HOSTED=CardRenderingTests

mg_unit_sim__usage() {
  mg_failure usage "unit-sim: $1" "tools/mg --list"
  mg_message "unit-sim: $1"
  mg_status error
}

mg_unit_sim() {
  local kind=iphone udid="" only=() args=() o prefix="$MG_RUN_DIR/unit" rc summary
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "iphone ipad" || { mg_unit_sim__usage "--platform takes iphone or ipad"; return 0; }
        kind="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_unit_sim__usage "--device needs a UDID (tools/mg sim acquire <kind>)"; return 0; }
        udid="$2" && shift 2
        ;;
      --only)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[A-Za-z_][A-Za-z0-9_]*(/[A-Za-z_][A-Za-z0-9_.()]*)*$' ||
          { mg_unit_sim__usage "--only takes a Unit-plan test target, optionally with /Suite... and /test, e.g. CardRenderingTests/WebKitSuites/CardRuntimeSuites"; return 0; }
        only[${#only[@]}]="$2" && shift 2
        ;;
      *) mg_unit_sim__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  [ "${#only[@]}" -gt 0 ] || only=("$MG_UNIT_SIM_DEFAULT")
  for o in "${only[@]}"; do args[${#args[@]}]="-only-testing:$o"; done
  mg_detail only "$(mg_json_array "${only[@]}")"
  mg_detail platform "$(jq -n --arg p "$kind" '$p')"
  export MG_APP_TEST_PLAN=Unit

  mg_sim_begin || return 0
  [ -z "$udid" ] || mg_sim_check_given "$udid" || return 0
  mg_app_build ios-sim Debug Unit Meguri ARCHS=arm64 || return 0
  mg_unit_sim__host_in_app || return 0
  mg_sim_take || return 0
  if [ -n "$udid" ]; then
    mg_sim_check_given "$udid" || return 0
  else
    mg_sim_lease "$kind" release || return 0
    udid="$MG_SIM_UDID"
    mg_sim_check_runtime "$udid" || return 0
  fi
  mg_app_xcodebuild_test "$prefix" "$kind" "id=$udid" Debug "${args[@]}"
  rc=$?
  [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "Unit plan result bundle ($kind)"

  summary="$(xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -c '{passed: .passedTests, failed: .failedTests, skipped: .skippedTests}')" || summary=""
  if [ -n "$summary" ]; then
    mg_count passed "$(jq .passed <<<"$summary")"
    mg_count failed "$(jq .failed <<<"$summary")"
    mg_count skipped "$(jq .skipped <<<"$summary")"
    mg_count total "$(jq '.passed + .failed + .skipped' <<<"$summary")"
    xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null | jq -c '
      [.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed")
        | {id: .nodeIdentifier, message: ([.children[]? | select(.nodeType == "Failure Message") | .name] | join("; ") | .[0:800])}] | .[]' |
      while IFS= read -r o; do
        mg_failure "test/$(jq -r .id <<<"$o")" "$(jq -r .message <<<"$o")"
      done
  fi
  case "$rc" in
    0)
      if [ -z "$summary" ] || [ "$(jq '.passed' <<<"$summary")" -eq 0 ]; then
        mg_failure no-tests "the run passed without running a test; check --only against app/TestPlans/Unit.xctestplan"
        mg_message "unit-sim: no test ran on $kind"
        mg_status fail
        return 0
      fi
      mg_message "unit-sim: $(jq -r '"\(.passed) passed, \(.skipped) skipped"' <<<"$summary") on $kind ($(printf '%s ' "${only[@]}" | sed 's/ $//'))"
      mg_status pass
      ;;
    1)
      # mg_app_xcodebuild_test returns 1 only with a result summary.
      mg_message "unit-sim: $(jq -r '.failed' <<<"$summary") test(s) failed on $kind; see $prefix.log"
      mg_status fail
      ;;
    *)
      mg_message "unit-sim: the tests could not run on $kind; see log.txt"
      mg_status error
      ;;
  esac
}

# mg_unit_sim__host_in_app: points the simulator Unit-plan xctestrun's
# entries for the MG_UNIT_SIM_HOSTED bundles at the Debug app as their test
# host, with XCTest's bundle injection (the simulator platform's
# libXCTestBundleInject). Idempotent. On failure records it and returns 1.
mg_unit_sim__host_in_app() {
  local run json inject="__PLATFORMS__/iPhoneSimulator.platform/Developer/usr/lib/libXCTestBundleInject.dylib"
  run="$(mg__vm_xctestrun Unit iphonesimulator Debug)"
  if [ -z "$run" ]; then
    mg_failure xctestrun "the Unit-plan build left no simulator xctestrun in $MG_APP_DD/Build/Products"
    mg_status error
    return 1
  fi
  if ! json="$(plutil -convert json -o - "$run")" || [ -z "$json" ] || ! json="$(jq -c --argjson hosted "$(mg_json_array $MG_UNIT_SIM_HOSTED)" \
    --arg app "__TESTROOT__/Debug-iphonesimulator/Meguri.app" --arg id "$MG_APP_DEV_ID" --arg inject "$inject" '
    .TestConfigurations |= map(.TestTargets |= map(
      if (.BlueprintName as $n | $hosted | index($n)) then
        .TestHostPath = $app | .TestHostBundleIdentifier = $id | .IsAppHostedTestBundle = true
        | .TestingEnvironmentVariables.XCInjectBundleInto = "unused"
        | .TestingEnvironmentVariables.DYLD_INSERT_LIBRARIES |=
          ((. // "") | split(":") | map(select(. != "" and . != $inject)) | [$inject] + . | join(":"))
      else . end))' <<<"$json")" || [ -z "$json" ] || ! printf '%s' "$json" | plutil -convert xml1 -o "$run" -; then
    mg_failure xctestrun "could not host $MG_UNIT_SIM_HOSTED in the app in $run"
    mg_status error
    return 1
  fi
  mg_detail hosted "$(mg_json_array $MG_UNIT_SIM_HOSTED)"
  return 0
}
