# U32 Apple durability evidence at U37's collection-FFI boundary. Ordinary
# Rust checks stay in Docker; these exact suites measure the native WAL path.
mg_job native-durability host mg_native_durability \
  "Apple silicon native U32/U37 suspend, WAL durability and collection FFI tests; no arguments, retries or UI"

mg_native_durability() {
  if [ "$#" -ne 0 ]; then
    mg_failure usage "usage: tools/mg native-durability (no arguments)"
    mg_status error
    return 0
  fi
  if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    mg_failure native-platform "native durability evidence requires arm64 macOS"
    mg_status error
    return 0
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    mg_failure native-cargo "cargo is unavailable on the host"
    mg_status error
    return 0
  fi
  mg_detail environment "$(jq -nc --arg os "$(uname -sm)" --arg rustc "$(cd core && rustc -V 2>&1)" \
    '{os: $os, rustc: $rustc, target: "aarch64-apple-darwin", retries: 0}')"
  local package suite json log rc binary counts passed failed skipped
  local total_passed=0 total_failed=0 total_skipped=0 broken=false
  local target="$MG_ROOT/build/native-durability/target"
  # Synthetic benchmark collections only, regardless of a caller's override.
  export MEGURI_BENCH_FIXTURES="$MG_ROOT/build/fixtures"
  # Build before taking the measurement lease. Pin the target even if the
  # caller's Cargo environment normally cross-compiles. Reuse this dedicated
  # target cache across runs without replacing Xcode's shared core products.
  # Missing locked test dependencies may download here, outside measurement.
  for package in meguri-engine meguri-ffi; do
    json="$MG_RUN_DIR/$package-build.jsonl"
    log="$MG_RUN_DIR/$package-build.log"
    if [ "$package" = meguri-engine ]; then
      (cd core && cargo test --locked -p meguri-engine --test suspend --test wal_durability \
        --target aarch64-apple-darwin --target-dir "$target" --no-run \
        --message-format json-render-diagnostics) >"$json" 2>"$log"
    else
      (cd core && cargo test --locked -p meguri-ffi --features test-hooks --test collection_ffi \
        --target aarch64-apple-darwin --target-dir "$target" --no-run \
        --message-format json-render-diagnostics) >"$json" 2>"$log"
    fi
    rc=$?
    mg_artifact build-jsonl "$json" "$package native compiler artifacts"
    mg_artifact build-log "$log" "$package native build diagnostics"
    cat "$log" >&2
    if [ "$rc" -ne 0 ]; then
      mg_failure "build/$package" "native cargo test --no-run failed (exit $rc); see build artifacts"
      mg_status fail
      return 0
    fi
  done
  # Resolve every executable before running any suite. Missing/ambiguous
  # compiler artifacts must never turn an incomplete build into a pass.
  for suite in suspend wal_durability collection_ffi; do
    package=meguri-engine
    [ "$suite" != collection_ffi ] || package=meguri-ffi
    binary="$(jq -ser --arg suite "$suite" '
      [.[] | select(.reason == "compiler-artifact" and .profile.test == true
        and .target.name == $suite and (.target.kind | index("test")) != null)
        | .executable | select(type == "string" and length > 0)]
      | if length == 1 then .[0] else error("expected one native test executable") end
      ' "$MG_RUN_DIR/$package-build.jsonl" 2>/dev/null)"
    rc=$?
    if [ "$rc" -ne 0 ] || [ ! -x "$binary" ]; then
      mg_failure "executable/$suite" "native build did not produce exactly one executable for $suite"
      mg_status error
      return 0
    fi
    printf '%s\n' "$binary" >"$MG_RUN_DIR/$suite-executable.txt"
  done
  mg_host_lease --no-session || return 0
  for suite in suspend wal_durability collection_ffi; do
    binary="$(cat "$MG_RUN_DIR/$suite-executable.txt")"
    log="$MG_RUN_DIR/$suite.log"
    "$binary" --test-threads=1 --nocapture >"$log" 2>&1
    rc=$?
    mg_artifact test-log "$log" "$suite native libtest output, including timing evidence"
    cat "$log" >&2
    counts="$(sed -nE 's/^test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored;.*$/\2 \3 \4/p' "$log")"
    passed=0 failed=0 skipped=0
    if ! printf '%s\n' "$counts" | grep -Eq '^[0-9]+ [0-9]+ [0-9]+$' || \
      [ "$(printf '%s\n' "$counts" | wc -l | tr -d ' ')" -ne 1 ]; then
      mg_failure "summary/$suite" "$suite has no unique ordinary libtest result summary (exit $rc)"
      broken=true
    else
      read -r passed failed skipped <<<"$counts"
      if [ "$((passed + failed))" -eq 0 ]; then
        mg_failure "empty/$suite" "$suite executed no tests"
        broken=true
      fi
      mg_detail "$suite" "$(jq -nc --argjson passed "$passed" --argjson failed "$failed" \
        --argjson skipped "$skipped" --argjson exit_code "$rc" \
        '{passed: $passed, failed: $failed, skipped: $skipped, exit_code: $exit_code}')"
    fi
    total_passed=$((total_passed + passed))
    total_failed=$((total_failed + failed))
    total_skipped=$((total_skipped + skipped))
    if [ "$rc" -ne 0 ] || [ "$failed" -ne 0 ]; then
      mg_failure "test/$suite" "$suite failed (exit $rc, $failed failing tests); no retry"
      broken=true
    fi
  done
  mg_count passed "$total_passed"
  mg_count failed "$total_failed"
  mg_count skipped "$total_skipped"
  mg_count total "$((total_passed + total_failed + total_skipped))"
  if [ "$broken" = true ]; then mg_status fail; else mg_status pass; fi
}
