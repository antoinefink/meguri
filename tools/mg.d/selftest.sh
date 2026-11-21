# tools/mg self-tests. Sourced by tools/mg.

mg_job selftest docker:dev mg_selftest \
  "tools/mg self-tests (bats, four files at once, sequential within each file) in the dev container; args go to bats, e.g. --filter doctor, --jobs 1 or test files by path (tools/tests/gate.bats), which replace the full list"
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
    if [ "$MG_JOB" = selftest ] && [ "$seen" -eq 0 ]; then
      mg_failure no-tests "Bats selected no tests; verification did not run"
      mg_message "$MG_JOB: no tests selected"
      mg_status fail
    else
      mg_message "$MG_JOB: $passed passed, $skipped skipped"
      mg_status pass
    fi
  else
    mg_message "$MG_JOB: $failed failed, $passed passed, $skipped skipped"
    mg_status fail
  fi
}

mg_selftest() {
  local tap="$MG_RUN_DIR/selftest.tap" rc arg
  local defaults=(--jobs 4 --no-parallelize-within-files) files=(tools/tests/*.bats)
  # Bats rejects its file-only flag with one job. An explicit job count
  # therefore replaces both defaults and retains Bats' own CLI semantics.
  for arg in ${1+"$@"}; do
    case "$arg" in --jobs|-j|--jobs=*|-j[0-9]*) defaults=(); break ;; esac
  done
  # Named test files replace the whole list: passed beside it, bats ran each
  # named file twice (two runs of one file share its fixtures and fail). The
  # value of an option that takes one (a --filter regex) is never a file.
  local value_next=false
  for arg in ${1+"$@"}; do
    if [ "$value_next" = true ]; then value_next=false; continue; fi
    case "$arg" in
      -f|--filter|--filter-status|--filter-tags|-j|--jobs|-F|--formatter|--report-formatter|-o|--output|--code-quote-style|--line-reference-format|--gather-test-outputs-in|--setup-suite-file)
        value_next=true
        ;;
      -*) ;;
      *.bats)
        if [ ! -f "$arg" ]; then
          mg_failure usage "selftest: no test file $arg; name files by their path, e.g. tools/tests/gate.bats" "tools/mg selftest tools/tests/gate.bats"
          mg_message "selftest: no test file $arg"
          mg_status error
          return 0
        fi
        files=()
        ;;
    esac
  done
  # Bats keeps every test's temporary directory until the run ends: a full
  # run holds more than 8 GB of sandboxes. /tmp is a tmpfs of half the
  # machine's memory, so where it is under 16 GB (8 GB on a hosted runner)
  # the run's scratch goes beside the Cargo target, on disk, and is removed.
  local scratch="" size
  size="$(df -Pm /tmp 2>/dev/null | awk 'NR == 2 { print $2 }')"
  if [[ "$size" =~ ^[0-9]+$ ]] && [ "$size" -lt 16384 ] && [ -n "${CARGO_TARGET_DIR:-}" ] &&
     mkdir -p "$CARGO_TARGET_DIR/selftest-scratch"; then
    scratch="$(mktemp -d "$CARGO_TARGET_DIR/selftest-scratch/run.XXXXXX")" || scratch=""
  fi
  mg_detail scratch "$(jq -nc --arg tmp "${scratch:-${TMPDIR:-/tmp}}" --arg mb "$size" \
    '{tmpdir: $tmp, tmp_tmpfs_mb: ($mb | tonumber? // null)}')"
  TMPDIR="${scratch:-${TMPDIR:-/tmp}}" \
    bats --formatter tap ${defaults[0]+"${defaults[@]}"} ${1+"$@"} ${files[0]+"${files[@]}"} | tee "$tap"
  rc=${PIPESTATUS[0]}
  [ -z "$scratch" ] || rm -rf "$scratch"
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

mg_job image-tools-selftest docker:image-tools mg_image_tools_selftest \
  "Tests of the image-tools scripts in their container: dev-icon, contact-sheet, hierarchy-check (U8), image-diff, screen-status, critic and gallery (U10), perf-compare (U12); args go to unittest, e.g. -k overlap"

# Every script directory with a tests/ folder that runs in image-tools.
MG_IMAGE_TOOLS_TESTED="tools/scripts/dev-icon tools/scripts/hierarchy-check tools/scripts/contact-sheet tools/scripts/image-diff tools/scripts/screen-status tools/scripts/critic tools/scripts/gallery tools/scripts/perf-compare tools/scripts/research"

mg_image_tools_selftest() {
  local dir rc=0 log failed=0 ran=0 n f
  for dir in $MG_IMAGE_TOOLS_TESTED; do
    [ -d "$dir/tests" ] || continue
    log="$MG_RUN_DIR/$(basename "$dir").log"
    # A filter (-k) may match nothing in one directory: unittest then exits 5.
    (cd "$dir" && python3 -m unittest discover -s tests -v ${1+"$@"}) >"$log" 2>&1 ||
      { [ "$#" -gt 0 ] && grep -q '^NO TESTS RAN' "$log"; } || rc=1
    cat "$log" >&2
    mg_artifact log "$log" "$dir tests"
    n="$(sed -n 's/^Ran \([0-9]*\) tests\{0,1\} in .*/\1/p' "$log" | tail -n 1)"
    ran=$((ran + ${n:-0}))
    f="$(grep -cE '^(FAIL|ERROR): ' "$log")"
    failed=$((failed + f))
    grep -E '^(FAIL|ERROR): ' "$log" | while IFS= read -r line; do
      mg_failure "$(basename "$dir")/${line#*: }" "$line; see $(basename "$log")" "tools/mg image-tools-selftest -k '$(printf '%s' "$line" | sed -E 's/^(FAIL|ERROR): ([^ ]+).*/\2/')'"
    done
  done
  mg_count total "$ran"
  mg_count failed "$failed"
  mg_count passed $((ran - failed))
  if [ "$rc" -eq 0 ] && [ "$ran" -gt 0 ]; then
    mg_message "image-tools-selftest: $ran passed"
    mg_status pass
  else
    [ "$ran" -gt 0 ] || mg_failure no-tests "unittest ran no tests"
    mg_message "image-tools-selftest: $failed failed of $ran"
    mg_status fail
  fi
}
