#!/usr/bin/env bats
# Self-tests for the performance jobs (U12): perf-ui's refusals and wiring
# (Profile only, the Meguri-Profile scheme's Performance plan, one perf
# result per suite from the result bundle's metrics), bench and
# bench-icount's arguments, metrics' refusals. Apple tools and builds are
# faked; perf_compare.py runs here (its own tests are in
# image-tools-selftest), on the metrics of a real SmokePerfTests run.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core" "$t/app/Packages/MeguriCore/Sources/Instrumentation"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/core/rust-toolchain.toml" "$t/core/"
  cp "$REPO_ROOT/app/Packages/MeguriCore/Sources/Instrumentation/IntervalNames.swift" "$t/app/Packages/MeguriCore/Sources/Instrumentation/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  export HOME="$BATS_TEST_TMPDIR/home"
  export MEGURI_HOST_LEASE_DIR="$BATS_TEST_TMPDIR/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1
  export TMPDIR="$BATS_TEST_TMPDIR/tmp"
  mkdir -p "$HOME" "$TMPDIR"
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  stub ioreg 'echo "+-o Root  <class IORegistryEntry>"'
  stub system_profiler 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\"}]}]}"'
  stub caffeinate 'exec sleep 600'
  stub xcodebuild 'echo "xcodebuild $*" >>"$STUB_LOG"; exit 70'
  stub xcrun 'echo "xcrun $*" >>"$STUB_LOG"
case "$*" in
  "xcresulttool get test-results metrics "*) cat "$FX_METRICS" ;;
  *) exit 70 ;;
esac'
  export FX_METRICS="$REPO_ROOT/tools/scripts/perf-compare/tests/data/xcresult-smoke-metrics.json"
  export PATH="$BIN:$PATH"
  add_fakes
}

teardown() {
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures, details}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 30
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG" || { echo "unexpected call: $(grep -E -- "$1" "$STUB_LOG")"; return 1; }; }

# Builds pass, the pool lends FAKE-<kind>, the test run writes a result
# bundle and exits FX_TEST_RC, the image-tools container's commands run here.
add_fakes() {
  cat >"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
fx_host() { case "$1" in /work/*) printf '%s\n' "$MG_ROOT/${1#/work/}" ;; *) printf '%s\n' "$1" ;; esac; }
mg_lint_ui_first() { return 0; }
mg_app_build() { echo "fake build $*" >>"$STUB_LOG"; return 0; }
mg_sim_lease() { echo "fake lease $1" >>"$STUB_LOG"; MG_SIM_UDID="FAKE-$1"; return 0; }
mg_sim_check_runtime() { return 0; }
mg_sim_check_given() { echo "fake check $1" >>"$STUB_LOG"; return 0; }
mg_app_xcodebuild_test() {
  echo "fake test scheme=${MG_APP_TEST_SCHEME:-} plan=${MG_APP_TEST_PLAN:-} $*" >>"$STUB_LOG"
  mkdir -p "$1.xcresult"
  echo "xcodebuild log" >"$1.log"
  return "${FX_TEST_RC:-0}"
}
mg__perf_ui_runner() {
  jq -n '{kind: "simulator", id: "iPhone 17 Pro/24A434", model: "iPhone 17 Pro", os: "iOS 27.0", os_build: "24A434", arch: "arm64"}' >"$2"
}
mg__bench_mac_fixtures() { echo "fake fixtures $*" >>"$STUB_LOG"; return "${FX_FIXTURES_RC:-0}"; }
mg_image_tools() {
  echo "fake image-tools $*" >>"$STUB_LOG"
  local args=() a
  for a in "$@"; do args[${#args[@]}]="$(fx_host "$a")"; done
  (cd "$MG_ROOT" && "${args[@]}")
}
EOF
}

# --- perf-ui ---------------------------------------------------------------

@test "perf-ui refuses any configuration but Profile before building" {
  mg perf-ui --config Debug
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = config ]
  [ "$(field .message)" = "perf-ui: refused: performance tests require Profile, not Debug" ]
  not_called 'fake build'
  not_called 'fake test'
}

@test "perf-ui rejects a malformed suite and an unknown platform" {
  mg perf-ui --suite 'Smoke Perf'
  [ "$(field .status)" = error ]
  mg perf-ui --platform watch
  [ "$(field .status)" = error ]
  not_called 'fake build'
}

@test "perf-ui runs the Performance plan in Profile and writes a valid perf result per suite" {
  mg perf-ui
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  called '^fake build ios-sim Profile Performance Meguri-Profile$'
  called '^fake lease iphone$'
  called '^fake test scheme=Meguri-Profile plan=Performance .* ios platform=iOS Simulator,id=FAKE-iphone Profile -only-testing:MeguriUITests/PerfTestCaseTests -only-testing:MeguriUITests/SmokePerfTests$'
  # The harness's own checks measure nothing: no perf result for them.
  [ "$(field -c .details.unmeasured)" = '["PerfTestCaseTests"]' ]
  [ "$(field '[.artifacts[] | select(.kind == "perf-result")] | length')" = 1 ]
  local out
  out="$(field '.artifacts[] | select(.kind == "perf-result") | .path')"
  [ "$out" = "$SB/build/mg/perf-ui/$(field .run_id)/results/SmokePerfTests.json" ]
  python3 "$SB/tools/scripts/perf-compare/perf_compare.py" validate "$out"
  [ "$(jq -r .configuration "$out")" = Profile ]
  [ "$(jq -r .runner.id "$out")" = "iPhone 17 Pro/24A434" ]
  # The probe interval, its percentiles and measured values.
  [ "$(jq -c '[.results[] | select(.name == "probe.syntheticDelay" and .metric == "duration") | .percentile]' "$out")" = '["p50","p95","p99","max"]' ]
  jq -e '[.results[] | select(.name == "probe.syntheticDelay" and .metric == "duration") | .value] | all(. >= 30 and . < 100)' "$out"
}

@test "perf-ui fails when a performance test fails, and names the device it was given" {
  export FX_TEST_RC=1
  mg perf-ui --device ABCD --suite SmokePerfTests/testLaunch
  [ "$(field .status)" = fail ]
  called '^fake check ABCD$'
  called 'id=ABCD Profile -only-testing:MeguriUITests/SmokePerfTests/testLaunch$'
  not_called 'fake lease'
}

# --- bench, bench-icount, metrics ------------------------------------------

@test "bench, bench-mac and bench-icount reject unknown arguments before building" {
  mg bench --nonsense
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = usage ]
  mg bench --sizes
  [ "$(field '.failures[0].id')" = usage ]
  mg bench-mac --nonsense
  [ "$(field .status)" = error ]
  [ "$(field .message)" = "bench-mac: unknown argument '--nonsense'" ]
  mg bench-icount --sizes 10k
  [ "$(field '.failures[0].id')" = usage ]
  [ "$(field .message)" = "bench-icount: unknown argument '--sizes'" ]
  not_called 'cargo bench'
}

# cargo: metadata lists one criterion and one icount suite; a bench run
# writes FX_BENCH_RESULT as its perf result, a --no-run build exits
# FX_BUILD_RC.
bench_fakes() {
  stub cargo 'echo "cargo $* sizes=${MEGURI_BENCH_SIZES:-} fixtures=${MEGURI_BENCH_FIXTURES:-}" >>"$STUB_LOG"
case "$1" in
  metadata) cat "$FX_CARGO_METADATA" ;;
  bench)
    case " $* " in *" --no-run "*) exit "${FX_BUILD_RC:-0}" ;; esac
    target="" prev=""
    for a in "$@"; do [ "$prev" != --bench ] || target="$a"; prev="$a"; done
    cp "$FX_BENCH_RESULT" "$MEGURI_BENCH_RESULTS/$target.json"
    ;;
  *) exit 64 ;;
esac'
  export FX_CARGO_METADATA="$BATS_TEST_TMPDIR/metadata.json" FX_BENCH_RESULT="$BATS_TEST_TMPDIR/open.json"
  jq -n '{packages: [{name: "meguri-testkit", targets: [
      {name: "meguri_testkit", kind: ["lib"], src_path: "/x/core/crates/meguri-testkit/src/lib.rs"},
      {name: "open", kind: ["bench"], src_path: "/x/core/crates/meguri-testkit/benches/open.rs"},
      {name: "icount_open", kind: ["bench"], src_path: "/x/core/crates/meguri-testkit/benches/icount/open.rs"}]}]}' \
    >"$FX_CARGO_METADATA"
  jq -n '{schema_version: 1, suite: "open", kind: "wall-clock", configuration: "bench",
      runner: {kind: "mac", id: "Apple M3 Max/macOS 27.0", cpu: "Apple M3 Max", os: "macOS 27.0", os_build: "24A434",
        arch: "aarch64", memory_measure: "phys_footprint"},
      results: [{name: "collection.open", metric: "duration", unit: "ms", percentile: "p95", value: 0.4, samples: 400,
        fixture: "500k"}]}' >"$FX_BENCH_RESULT"
}

@test "bench-mac runs the criterion suites natively at 10k, 100k, 150k and 500k, after building them" {
  bench_fakes
  mg bench-mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .placement)" = host ]
  [ "$(field -c .details.sizes)" = '["10k","100k","150k","500k"]' ]
  called '^fake fixtures 10k 100k 150k 500k$'
  called "^cargo bench -p meguri-testkit --bench open -- sizes=10k,100k,150k,500k fixtures=$SB/build/fixtures$"
  not_called 'icount_open'
  # Collections, then the build, then the measured run.
  local made built measured
  made="$(grep -n '^fake fixtures' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  built="$(grep -n -- '--bench open --no-run' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  measured="$(grep -n -- '--bench open -- ' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  [ "$made" -lt "$built" ] && [ "$built" -lt "$measured" ]
  # Measured holding the host lease; validated through image-tools.
  [ "$(field '.details.host_lease.job')" = bench-mac ]
  called '^fake image-tools python3 tools/scripts/perf-compare/perf_compare.py validate /work/build/mg/bench-mac/.*/results/open.json$'
  [ "$(field '[.artifacts[] | select(.kind == "perf-result")] | length')" = 1 ]
  [ "$(field -r '.details.results[0].memory_measure')" = phys_footprint ]
}

@test "bench-mac runs on a locked Mac, and takes its sizes and suites from the arguments" {
  bench_fakes
  stub ioreg 'echo "    | \"CGSSessionScreenIsLocked\"=Yes"'
  mg bench-mac --sizes 10K,500k --bench open -- --sample-size 10
  [ "$(field .status)" = pass ]
  [ "$(field .details.blocked)" = null ]
  called '^fake fixtures 10k 500k$'
  called '^cargo bench -p meguri-testkit --bench open -- --sample-size 10 sizes=10k,500k '
  mg bench-mac --bench nosuch
  [ "$(field .status)" = error ]
  [ "$(field .message)" = "bench-mac: no criterion bench target matched nosuch" ]
}

@test "bench-mac stops before measuring when the collections or the build fail" {
  bench_fakes
  export FX_FIXTURES_RC=1
  mg bench-mac
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = fixtures ]
  not_called 'cargo bench'
  export FX_FIXTURES_RC=0 FX_BUILD_RC=101
  : >"$STUB_LOG"
  mg bench-mac
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = build/open ]
  [ "$(field .details.host_lease)" = null ]
  not_called 'cargo bench -p meguri-testkit --bench open -- '
}

@test "metrics refuses the Mac, video and a missing script" {
  mg metrics tools/drive-scripts/examples/perf-probe.json --platform mac
  [ "$(field .status)" = error ]
  [[ "$(field .message)" == *"metrics records simulators"* ]]
  mg metrics tools/drive-scripts/examples/perf-probe.json --record
  [ "$(field .status)" = error ]
  mg metrics
  [ "$(field .status)" = error ]
  not_called 'fake build'
}
