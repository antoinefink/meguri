# tools/mg self-tests. Sourced by tools/mg.

mg_job selftest docker:dev mg_selftest \
  "tools/mg self-tests (bats, tools/tests/*.bats) in the dev container; args go to bats, e.g. --filter doctor"
mg_job selftest-host host mg_selftest_host \
  "tools/mg host self-tests: macOS bash, real doctor, Docker placement, image rebuild (tools/tests/mg_host_selftest.sh)"
mg_job selftest-xcframework host mg_selftest_xcframework \
  "build-core-xcframework.sh self-tests in a sandbox copy: no-change skip, input hashing, compare-before-replace, device pre-action and link, symbol and bindings checks, failed replace, AppStore features, concurrent runs"

# mg_tap_report <tap file> <test-runner rc> <repro prefix>
# Records counts and one failure per `not ok` line. With a repro prefix, a
# failure's repro is `<prefix> '<regex of the test name>'`.
mg_tap_report() {
  local tap="$1" rc="$2" prefix="$3" report
  report="$(jq -R -n -c '
    reduce inputs as $l ({passed: 0, failed: 0, skipped: 0, plan: null, fails: []};
      if ($l | test("^ok ")) then
        (if ($l | test(" # [Ss][Kk][Ii][Pp]")) then .skipped += 1 else .passed += 1 end)
      elif ($l | test("^not ok ")) then
        .failed += 1 | .fails += [{name: ($l | sub("^not ok [0-9]+ "; "")), msg: ""}]
      elif ($l | test("^# ")) and (.fails | length) > 0 then
        .fails[(.fails | length) - 1].msg += (($l | .[2:]) + "\n")
      elif ($l | test("^1\\.\\.[0-9]+$")) then
        .plan = ($l | .[3:] | tonumber)
      else . end)' "$tap")"
  local passed failed skipped seen plan
  passed="$(jq .passed <<<"$report")"
  failed="$(jq .failed <<<"$report")"
  skipped="$(jq .skipped <<<"$report")"
  plan="$(jq '.plan // empty' <<<"$report")"
  seen=$((passed + failed + skipped))
  mg_count total "${plan:-$seen}"
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count skipped "$skipped"
  jq -c --arg prefix "$prefix" --arg dflt "$MG_DEFAULT_REPRO" '.fails[] |
    {id: .name, message: (.msg | rtrimstr("\n")),
     repro: (if $prefix == "" then $dflt
             else $prefix + " " + (.name | gsub("(?<c>[][(){}.*+?^$|\\\\])"; "\\\(.c)") | @sh) end)}' \
    <<<"$report" >>"$MG_STATE/failures.jsonl"
  if [ -n "$plan" ] && [ "$seen" -lt "$plan" ]; then
    mg_failure tap-incomplete "the test run stopped after $seen of $plan tests (runner exit $rc); see log.txt"
  elif [ "$rc" -ne 0 ] && [ "$failed" -eq 0 ]; then
    mg_failure runner-exit "the test runner exited with code $rc without a failing test; see log.txt"
  fi
  if [ "$failed" -eq 0 ] && [ "$rc" -eq 0 ] && { [ -z "$plan" ] || [ "$seen" -ge "$plan" ]; }; then
    mg_message "$MG_JOB: $passed passed, $skipped skipped"
    mg_status pass
  else
    mg_message "$MG_JOB: $failed failed, $passed passed, $skipped skipped"
    mg_status fail
  fi
}

mg_selftest() {
  local tap="$MG_RUN_DIR/selftest.tap" rc
  bats --formatter tap ${1+"$@"} tools/tests/*.bats | tee "$tap"
  rc=${PIPESTATUS[0]}
  mg_artifact tap "$tap"
  mg_tap_report "$tap" "$rc" "tools/mg selftest --filter"
}

mg_selftest_host() {
  local tap="$MG_RUN_DIR/selftest-host.tap" rc
  /bin/bash tools/tests/mg_host_selftest.sh "$MG_RUN_DIR/sandbox" | tee "$tap"
  rc=${PIPESTATUS[0]}
  mg_artifact tap "$tap"
  mg_tap_report "$tap" "$rc" ""
}

# The sandbox and its cargo target dir stay in build/selftest-xcframework/
# between runs, so dependencies build once per slice.
mg_selftest_xcframework() {
  local tap="$MG_RUN_DIR/selftest-xcframework.tap" rc
  /bin/bash tools/tests/xcframework_selftest.sh build/selftest-xcframework/sandbox build/selftest-xcframework/target |
    tee "$tap"
  rc=${PIPESTATUS[0]}
  mg_artifact tap "$tap"
  mg_artifact log build/selftest-xcframework/sandbox/build.log "build-core-xcframework.sh output"
  mg_tap_report "$tap" "$rc" ""
}
