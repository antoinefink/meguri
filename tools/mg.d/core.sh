# Rust core jobs. Sourced by tools/mg; runs inside the `dev` container.

mg_job core-test docker:dev mg_core_test \
  "Rust tests: cargo nextest on core/ (no retries); args after -- go to nextest"

# tools/mg core-test [-- nextest args, e.g. -E 'package(meguri-text)']
mg_core_test() {
  [ "${1:-}" = "--" ] && shift
  mg_detail environment "$(
    cd core 2>/dev/null || cd .
    jq -nc \
      --arg container "${MG_CONTAINER:-}" \
      --arg os "$(uname -sm)" \
      --arg rustc "$(rustc -V 2>&1)" \
      --arg nextest "$(cargo nextest --version 2>&1 | head -n 1)" \
      '{container: $container, os: $os, rustc: $rustc, nextest: $nextest}'
  )"

  if [ ! -f core/Cargo.toml ]; then
    mg_message "core-test: no Cargo workspace in core/ yet; 0 tests"
    mg_status pass
    return 0
  fi

  local out="$MG_RUN_DIR/nextest.jsonl" rc
  # Only a bare run may match no tests (empty workspace). With a filter, an
  # empty match must fail, or a stale repro command would pass.
  if [ "$#" -eq 0 ]; then
    set -- --no-tests=pass
  fi
  (
    cd core &&
      NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --workspace \
        --retries 0 --no-fail-fast \
        --message-format libtest-json-plus --message-format-version 0.1 \
        ${1+"$@"} >"$out"
  )
  rc=$?
  mg_artifact nextest-jsonl "$out"

  # Test names look like `<crate>::<binary>$<test path>`.
  local counts
  counts="$(jq -sc '[.[] | select(.type == "test" and .event != "started")] |
    {passed: map(select(.event == "ok")) | length,
     skipped: map(select(.event == "ignored")) | length,
     failed: map(select(.event != "ok" and .event != "ignored")) | length}' "$out")"
  local passed failed skipped
  passed="$(jq -r .passed <<<"$counts")"
  failed="$(jq -r .failed <<<"$counts")"
  skipped="$(jq -r .skipped <<<"$counts")"
  mg_count total $((passed + failed + skipped))
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count skipped "$skipped"

  jq -c 'select(.type == "test" and .event != "started" and .event != "ok" and .event != "ignored")
    | (.name | split("$")) as $p
    | {id: .name,
       message: ((.stdout // .event) | .[0:2000]),
       repro: ("tools/mg core-test -- -E " + ("package(" + ($p[0] | split("::")[0]) + ") & test(=" + ($p[1] // $p[0]) + ")" | @sh))}' \
    "$out" >>"$MG_STATE/failures.jsonl"

  if [ "$rc" -eq 4 ] && [ $((passed + failed + skipped)) -eq 0 ]; then
    mg_failure no-tests-matched "no test matched the nextest arguments; check the filter (a renamed test?)"
  elif [ "$rc" -ne 0 ] && [ "$failed" -eq 0 ]; then
    mg_failure "nextest-exit-$rc" "cargo nextest exited with code $rc before reporting a test failure (build or setup error); see log.txt"
  fi
  if [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ]; then
    mg_message "core-test: $passed passed, $skipped skipped"
    mg_status pass
  else
    mg_message "core-test: $failed failed, $passed passed, $skipped skipped (nextest exit $rc)"
    mg_status fail
  fi
}
