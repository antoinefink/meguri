# Performance instrumentation and benchmarks (U12: R1861-R1864, R1866).
# Sourced by tools/mg.
#
# Every job here writes perf results (tools/perf/perf-result.schema.json),
# which U241's perf-gate reads; the jobs themselves fail only when a suite
# fails to run, a result is invalid, or a bench's own check fails.
#
#   perf-ui       XCTest performance tests in the Profile configuration on a
#                 pool simulator in the test VM (mac-vm.sh; the Mac runs
#                 through the Mac UI gate), then tools/scripts/perf-compare
#                 turns the xcresult's metrics into a perf result.
#   bench         criterion suites in the dev container: every bench target
#                 outside benches/icount/; each writes its own perf result
#                 (meguri-testkit bench::report).
#   bench-mac     the same suites natively on this Mac (R1864's Apple
#                 silicon numbers, phys_footprint memory) at 10k, 100k, 150k
#                 and 500k cards, built first and run holding the host lease.
#   bench-icount  gungraun suites under valgrind in the dev container: every
#                 bench target under benches/icount/; perf-compare turns
#                 gungraun's JSON into a perf result.
#   metrics       an xctrace recording around a drive script (R1961), made
#                 where the simulator is (the test VM) and copied back.

mg_job perf-ui host mg_perf_ui \
  "UI performance tests (R1863, R1866): [--platform iphone|ipad|mac] [--device <leased udid>] [--config Profile] [--suite <class>]... (default SmokePerfTests); Profile build of the Performance plan, intervals, hitches, launch and memory into perf-result.json (tools/perf/perf-result.schema.json); any other configuration is refused"
mg_job bench docker:dev mg_bench \
  "Criterion wall-clock suites (R1864): [--sizes 10k,150k] [--bench <target>]... [-- criterion args]; makes the collections first (meguri-cli generate, as tools/mg fixtures), then every bench target outside benches/icount/: per-operation p50/p95/p99 and peak memory into perf results (tools/perf/perf-result.schema.json)"
mg_job bench-mac host mg_bench_mac \
  "Criterion wall-clock suites natively on this Mac (R1864): [--sizes 10k,100k,150k,500k] [--bench <target>]... [-- criterion args]; makes the collections first (tools/mg fixtures), builds every bench target outside benches/icount/ with cargo on the host, then runs them holding the host lease (a locked screen does not stop it): per-operation p50/p95/p99 and peak phys_footprint into perf results (tools/perf/perf-result.schema.json)"
mg_job bench-icount docker:dev mg_bench_icount \
  "gungraun instruction-count suites under valgrind (R1864): [--bench <target>]...; every bench target under benches/icount/, per-bench instruction counts into perf results (tools/perf/perf-result.schema.json)"
mg_job metrics host mg_metrics \
  "An Instruments recording around a drive script (R1961): <script.json> [--platform iphone|iphone-small|ipad|ipad-small] [--device <leased udid>] [--config Debug|Profile] [--template <name>] (default Logging: os_signpost intervals; Time Profiler's sampling does not start in the test VM); xctrace records the simulator while tools/mg drive runs, the .trace is an artifact"

MG_PERF_SCHEMA=tools/perf/perf-result.schema.json
MG_PERF_COMPARE=tools/scripts/perf-compare/perf_compare.py
# The collections the criterion suites open by default (MEGURI_BENCH_SIZES),
# and those the icount suites open. The jobs make them first, out of process:
# a bench samples its own process's memory, and generating a collection
# under valgrind would take minutes.
MG_BENCH_SIZES="10k,150k"
MG_BENCH_MAC_SIZES="10k,100k,150k,500k"
MG_BENCH_ICOUNT_FIXTURES="10k"

mg__perf_usage() {
  mg_failure usage "$MG_JOB: $1" "tools/mg --list"
  mg_message "$MG_JOB: $1"
  mg_status error
}

# mg__bench_targets <criterion|icount>: "<package> <target>" per line, the
# workspace's bench targets of that kind (icount: under benches/icount/).
mg__bench_targets() {
  (cd core && cargo metadata --no-deps --format-version 1) | jq -r --arg kind "$1" '
    .packages[] | .name as $p | .targets[]
    | select(any(.kind[]; . == "bench"))
    | select((.src_path | test("/benches/icount/")) == ($kind == "icount"))
    | "\($p) \(.name)"'
}

# mg__perf_validate <result.json>...: each against the perf result schema
# (python3-jsonschema: in the dev container for docker jobs, through the
# image-tools container for host jobs); records a failure per invalid file.
# Returns 1 when one is invalid.
mg__perf_validate() {
  local f out ok rc=0
  for f in "$@"; do
    if [ "$MG_PLACEMENT" = host ]; then
      out="$(mg_image_tools python3 "$MG_PERF_COMPARE" validate "$(mg_container_path "$f")" 2>&1)" && ok=1 || ok=0
      out="$(printf '%s\n' "$out" | grep -Ev '^ *(Container|Image|Network|Volume) ')"
    else
      out="$(python3 "$MG_PERF_COMPARE" validate "$f" 2>&1)" && ok=1 || ok=0
    fi
    if [ "$ok" = 0 ]; then
      mg_failure "invalid-result/$(basename "$f")" "$f is not a perf result: $(printf '%s' "$out" | head -n 3 | tr '\n' ' ')"
      rc=1
    fi
  done
  return "$rc"
}

# mg__bench_args <default sizes> <job args...>: [--sizes <list>]
# [--bench <target>]... [-- criterion args] into MG_B_SIZES, MG_B_ONLY and
# MG_B_EXTRA. Returns 1 after recording a usage error.
mg__bench_args() {
  MG_B_SIZES="$1"
  MG_B_ONLY=()
  MG_B_EXTRA=()
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --sizes)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--sizes takes a comma-separated list such as $MG_B_SIZES"; return 1; }
        MG_B_SIZES="$(printf '%s' "$2" | tr 'A-Z' 'a-z')" && shift 2
        ;;
      --bench)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--bench takes a bench target name"; return 1; }
        MG_B_ONLY[${#MG_B_ONLY[@]}]="$2" && shift 2
        ;;
      --) shift && MG_B_EXTRA=("$@") && break ;;
      *) mg__perf_usage "unknown argument '$1'"; return 1 ;;
    esac
  done
}

# mg__bench_selected: the criterion targets (mg__bench_targets lines) that
# --bench names, all of them without it; records an error and returns 1
# when cargo metadata fails or nothing matches.
mg__bench_selected() {
  local targets pkg target
  targets="$(mg__bench_targets criterion)" || {
    mg_failure cargo-metadata "cargo metadata failed; see log.txt"
    mg_status error
    return 1
  }
  MG_B_TARGETS=""
  while IFS=' ' read -r pkg target; do
    [ -n "$target" ] || continue
    if [ "${#MG_B_ONLY[@]}" -gt 0 ] && ! mg_word_in "$target" "${MG_B_ONLY[*]}"; then continue; fi
    MG_B_TARGETS="$MG_B_TARGETS$pkg $target"$'\n'
  done <<<"$targets"
  [ -n "$MG_B_TARGETS" ] || { mg__perf_usage "no criterion bench target matched${MG_B_ONLY[0]+ ${MG_B_ONLY[*]}}"; return 1; }
}

# mg__bench_run <results dir>: runs each of MG_B_TARGETS on MG_B_SIZES, each
# writing its perf result into the directory; then validates the results
# and records the counts, artifacts and status.
mg__bench_run() {
  local results="$1" pkg target rc failed=0 ran=0 f files=()
  mkdir -p "$results"
  while IFS=' ' read -r pkg target; do
    [ -n "$target" ] || continue
    ran=$((ran + 1))
    echo "$MG_JOB: $pkg $target" >&2
    (
      cd core &&
        MEGURI_BENCH_SIZES="$MG_B_SIZES" MEGURI_BENCH_RESULTS="$results" MEGURI_BENCH_FIXTURES="$MG_ROOT/build/fixtures" \
          cargo bench -p "$pkg" --bench "$target" -- ${MG_B_EXTRA[0]+"${MG_B_EXTRA[@]}"}
    )
    rc=$?
    if [ "$rc" -ne 0 ]; then
      mg_failure "bench/$target" "cargo bench --bench $target exited $rc (a failed build, a crash or the bench's own check); see log.txt" \
        "tools/mg $MG_JOB --bench $target --sizes $MG_B_SIZES"
      failed=$((failed + 1))
    fi
  done <<<"$MG_B_TARGETS"
  for f in "$results"/*.json; do [ -f "$f" ] && files[${#files[@]}]="$f"; done
  if [ "${#files[@]}" -eq 0 ]; then
    mg_failure no-results "no bench wrote a perf result into $results"
    failed=$((failed + 1))
  elif ! mg__perf_validate "${files[@]}"; then
    failed=$((failed + 1))
  fi
  for f in ${files[0]+"${files[@]}"}; do mg_artifact perf-result "$f" "$(basename "$f" .json)"; done
  mg_detail sizes "$(mg_json_array $(printf '%s' "$MG_B_SIZES" | tr ',' ' '))"
  mg_detail results "$(cat /dev/null ${files[0]+"${files[@]}"} | jq -sc '[.[] | {suite, runner: .runner.id, memory_measure: .runner.memory_measure, results: [.results[] | {name, metric, percentile, value, unit, fixture}]}]' 2>/dev/null || echo null)"
  mg_count total "$ran"
  mg_count failed "$failed"
  mg_count passed $((ran - failed < 0 ? 0 : ran - failed))
  if [ "$failed" -eq 0 ]; then
    mg_message "$MG_JOB: $ran suite(s) on $MG_B_SIZES, ${#files[@]} perf result(s) in ${results#"$MG_ROOT"/}"
    mg_status pass
  else
    mg_message "$MG_JOB: $failed problem(s) in $ran suite(s); see failures"
    mg_status fail
  fi
}

# tools/mg bench [--sizes 10k,150k] [--bench <target>]... [-- criterion args]
mg_bench() {
  mg__bench_args "$MG_BENCH_SIZES" "$@" || return 0
  # shellcheck disable=SC2046 # one size per word
  if ! mg__perf_fixtures $(printf '%s' "$MG_B_SIZES" | tr ',' ' '); then
    mg_failure fixtures "making the bench collections ($MG_B_SIZES) failed; see log.txt" "tools/mg fixtures $(printf '%s' "$MG_B_SIZES" | tr ',' ' ')"
    mg_status fail
    return 0
  fi
  mg__bench_selected || return 0
  mg__bench_run "$MG_RUN_DIR/results"
}

# mg__bench_mac_fixtures <size>...: the bench collections, made (or reused)
# by tools/mg fixtures in the dev container. The bench opens them from
# build/fixtures through bench::fixtures, which reuses a directory whose
# manifest matches. Returns 1 when the job did not pass.
mg__bench_mac_fixtures() {
  local result
  result="$("$MG_TOOLS/mg" fixtures "$@" </dev/null)"
  [ -n "$result" ] && [ -f "$result" ] || return 1
  mg_artifact fixtures "$result" "tools/mg fixtures result"
  mg_word_in "$(jq -r .status "$result" 2>/dev/null)" "pass warn"
}

# tools/mg bench-mac [--sizes 10k,100k,150k,500k] [--bench <target>]...
#   [-- criterion args]
# The same suites as bench, natively on this Mac (R1864: Apple silicon,
# phys_footprint). It builds them first, then runs them holding the host
# lease, so no simulator, app or other bench job shares the machine; a
# locked screen does not stop it.
mg_bench_mac() {
  local pkg target
  mg__bench_args "$MG_BENCH_MAC_SIZES" "$@" || return 0
  mg__bench_selected || return 0
  # shellcheck disable=SC2046 # one size per word
  if ! mg__bench_mac_fixtures $(printf '%s' "$MG_B_SIZES" | tr ',' ' '); then
    mg_failure fixtures "tools/mg fixtures could not make the bench collections ($MG_B_SIZES); see its result" \
      "tools/mg fixtures $(printf '%s' "$MG_B_SIZES" | tr ',' ' ')"
    mg_status fail
    return 0
  fi
  while IFS=' ' read -r pkg target; do
    [ -n "$target" ] || continue
    if ! (cd core && cargo bench -p "$pkg" --bench "$target" --no-run); then
      mg_failure "build/$target" "cargo bench --no-run --bench $target failed on the host; see log.txt" "tools/mg bench-mac --bench $target"
      mg_status fail
      return 0
    fi
  done <<<"$MG_B_TARGETS"
  mg_host_lease --no-session || return 0
  mg__bench_run "$MG_RUN_DIR/results"
}

# mg__perf_fixtures <size>...: the shared bench collections
# (bench::fixtures layout), made or reused by meguri-cli generate --cache,
# as tools/mg fixtures does. Returns 1 when one could not be made.
mg__perf_fixtures() {
  local bin shape size dir
  (cd core && cargo build --profile fixtures -p meguri-cli) || return 1
  bin="$(cd core && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)/fixtures/meguri-cli"
  shape="$(sha256sum <core/crates/meguri-testkit/profiles/user-shape.json | cut -c1-16)"
  for size in "$@"; do
    dir="build/fixtures/$shape-1-$size"
    echo "$MG_JOB: collection $size -> $dir" >&2
    "$bin" generate "$size" --seed 1 --out "$dir" --cache >/dev/null || return 1
  done
}

# tools/mg bench-icount [--bench <target>]...
mg_bench_icount() {
  local only=() line pkg target results raw rc failed=0 ran=0 f
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --bench)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--bench takes a bench target name"; return 0; }
        only[${#only[@]}]="$2" && shift 2
        ;;
      --) shift ;;
      *) mg__perf_usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  if ! command -v gungraun-runner >/dev/null 2>&1 || ! command -v valgrind >/dev/null 2>&1; then
    mg_failure no-gungraun "gungraun-runner or valgrind is missing from the dev image (docker/dev.Dockerfile)" "tools/mg dev-image"
    mg_status error
    return 0
  fi
  if ! mg__perf_fixtures $MG_BENCH_ICOUNT_FIXTURES; then
    mg_failure fixtures "making the bench collections ($MG_BENCH_ICOUNT_FIXTURES) failed; see log.txt" "tools/mg fixtures $MG_BENCH_ICOUNT_FIXTURES"
    mg_status fail
    return 0
  fi
  results="$MG_RUN_DIR/results"
  raw="$MG_RUN_DIR/gungraun"
  mkdir -p "$results" "$raw"
  local targets
  targets="$(mg__bench_targets icount)" || { mg_failure cargo-metadata "cargo metadata failed; see log.txt"; mg_status error; return 0; }
  while IFS=' ' read -r pkg target; do
    [ -n "$target" ] || continue
    if [ "${#only[@]}" -gt 0 ] && ! mg_word_in "$target" "${only[*]}"; then continue; fi
    ran=$((ran + 1))
    echo "bench-icount: $pkg $target" >&2
    # gungraun prints its JSON summaries on stdout, one line per benchmark,
    # everything else on stderr. ASLR stays on: gungraun turns it off with
    # setarch, whose personality call Docker's seccomp profile refuses; the
    # variance it leaves is far below the gate's 5%.
    (
      cd core &&
        GUNGRAUN_HOME="$raw/home" GUNGRAUN_ALLOW_ASLR=yes MEGURI_BENCH_FIXTURES="$MG_ROOT/build/fixtures" \
          cargo bench -p "$pkg" --bench "$target" -- --output-format=json
    ) >"$raw/$target.json"
    rc=$?
    mg_artifact log "$raw/$target.json" "gungraun summaries of $target"
    if [ "$rc" -ne 0 ]; then
      mg_failure "bench-icount/$target" "cargo bench --bench $target exited $rc; see log.txt" "tools/mg bench-icount --bench $target"
      failed=$((failed + 1))
      continue
    fi
    if ! python3 "$MG_PERF_COMPARE" gungraun "$raw/$target.json" --suite "$target" --out "$results/$target.json" >&2; then
      mg_failure "bench-icount/$target/convert" "perf-compare could not read gungraun's summaries of $target; see log.txt" "tools/mg bench-icount --bench $target"
      failed=$((failed + 1))
    fi
  done <<<"$targets"
  if [ "$ran" -eq 0 ]; then
    mg__perf_usage "no icount bench target matched${only[0]+ ${only[*]}}"
    return 0
  fi
  local files=()
  for f in "$results"/*.json; do [ -f "$f" ] && files[${#files[@]}]="$f"; done
  if [ "${#files[@]}" -gt 0 ] && ! mg__perf_validate "${files[@]}"; then
    failed=$((failed + 1))
  fi
  for f in ${files[0]+"${files[@]}"}; do mg_artifact perf-result "$f" "$(basename "$f" .json)"; done
  mg_detail results "$(cat /dev/null ${files[0]+"${files[@]}"} | jq -sc '[.[] | {suite, runner: .runner.id, results: [.results[] | {name, metric, value, unit, fixture}]}]' 2>/dev/null || echo null)"
  mg_count total "$ran"
  mg_count failed "$failed"
  mg_count passed $((ran - failed < 0 ? 0 : ran - failed))
  if [ "$failed" -eq 0 ]; then
    mg_message "bench-icount: $ran suite(s), ${#files[@]} perf result(s) in ${results#"$MG_ROOT"/}"
    mg_status pass
  else
    mg_message "bench-icount: $failed problem(s) in $ran suite(s); see failures"
    mg_status fail
  fi
}

# The perf-ui defaults: the harness's own checks and the smoke set (U19
# picks what runs per change).
MG_PERF_UI_SUITES="PerfTestCaseTests SmokePerfTests"
# The harness's own checks: they measure nothing and have no perf result.
# Every other suite is a performance suite and must yield one.
MG_PERF_UI_CHECK_SUITES="PerfTestCaseTests CardSwapTimingGateTests"

# mg__perf_ui_runner <mac|udid> <out.json>: the perf result's runner, the
# baseline key U241 uses (model and OS build; CPU and OS on the Mac). A run
# in the test VM has keys of its own (vm/<name>...): its timings are not this
# Mac's.
mg__perf_ui_runner() {
  local kind model build version cpu os vm=""
  if [ "$1" = mac ]; then
    if [ "${MG_MAC_RUNNER:-}" = vm ]; then
      jq -n --arg vm "$(jq -r .name <<<"$MG_MAC_VM")" '{kind: "mac", id: ("vm/" + $vm), model: ("test VM " + $vm)}' >"$2"
    else
      cpu="$(sysctl -n machdep.cpu.brand_string 2>/dev/null)"
      os="$(sw_vers -productVersion 2>/dev/null)"
      jq -n --arg cpu "$cpu" --arg os "macOS $os" --arg build "$(sw_vers -buildVersion 2>/dev/null)" \
        '{kind: "mac", id: "\($cpu)/\($os)", cpu: $cpu, os: $os, os_build: $build, arch: "arm64"}' >"$2"
    fi
    return 0
  fi
  kind="$("$MG_TOOLS/scripts/sim-pool.sh" status 2>/dev/null | jq -r --arg u "$1" 'first(.devices[] | select(.udid == $u) | .kind) // ""')"
  model="$(jq -r --arg k "$kind" 'first(.devices[] | select(.kind == $k) | .device_type_name) // "unknown"' tools/sim-devices.json)"
  version="$(jq -r .runtime.version tools/sim-devices.json)"
  build="$(jq -r .runtime.build tools/sim-devices.json)"
  [ "${MG_SIM_RUNNER:-}" != vm ] || vm="vm/$(jq -r .name <<<"$MG_MAC_VM")/"
  jq -n --arg m "$model" --arg v "iOS $version" --arg b "$build" --arg vm "$vm" \
    '{kind: "simulator", id: "\($vm)\($m)/\($b)", model: $m, os: $v, os_build: $b, arch: "arm64"}' >"$2"
}

# tools/mg perf-ui [--platform iphone|iphone-small|ipad|ipad-small|mac]
#   [--device <leased udid>] [--config Profile] [--suite <class>]...
mg_perf_ui() {
  local kind=iphone udid="" config=Profile suites=() s
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "iphone iphone-small ipad ipad-small mac" ||
          { mg__perf_usage "--platform takes iphone, iphone-small, ipad, ipad-small or mac"; return 0; }
        kind="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--device takes a leased simulator's udid"; return 0; }
        udid="$2" && shift 2
        ;;
      --config)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--config takes Profile"; return 0; }
        config="$2" && shift 2
        ;;
      --suite)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[A-Za-z_][A-Za-z0-9_]*(/[A-Za-z_][A-Za-z0-9_]*)?$' ||
          { mg__perf_usage "--suite takes a test class (or Class/testMethod)"; return 0; }
        suites[${#suites[@]}]="$2" && shift 2
        ;;
      *) mg__perf_usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  # R1866: the numbers come from Release optimization (Profile is Release
  # plus the test hooks, app/AGENTS.md), never from a Debug build.
  if [ "$config" != Profile ]; then
    mg_failure config "performance tests require Profile (R1866), not $config" "tools/mg perf-ui"
    mg_message "perf-ui: refused: performance tests require Profile, not $config"
    mg_status error
    return 0
  fi
  [ "${#suites[@]}" -gt 0 ] || suites=($MG_PERF_UI_SUITES)
  [ "$kind" != mac ] || [ -z "$udid" ] || { mg__perf_usage "--device is for simulators"; return 0; }
  mg_detail suites "$(mg_json_array "${suites[@]}")"
  local only=() dest platform prefix="$MG_RUN_DIR/perf" rc
  for s in "${suites[@]}"; do only[${#only[@]}]="-only-testing:MeguriUITests/$s"; done
  export MG_APP_TEST_SCHEME=Meguri-Profile MG_APP_TEST_PLAN=Performance

  if [ "$kind" = mac ]; then
    mg_mac_ui_begin || return 0
    mg_app_build mac Profile Performance Meguri-Profile || return 0
    mg_mac_ui_take || return 0
    platform=mac
    dest="platform=macOS,arch=arm64"
  else
    # The pool's simulators are in the test VM (mac-vm.sh): the plan runs
    # there, and its result bundle comes back (mg_app_xcodebuild_test).
    mg_sim_begin || return 0
    [ -z "$udid" ] || mg_sim_check_given "$udid" || return 0
    mg_app_build ios-sim Profile Performance Meguri-Profile || return 0
    mg_sim_take || return 0
    if [ -n "$udid" ]; then
      mg_sim_check_given "$udid" || return 0
    else
      mg_sim_lease "$kind" release || return 0
      udid="$MG_SIM_UDID"
      mg_sim_check_runtime "$udid" || return 0
    fi
    platform=ios
    dest="platform=iOS Simulator,id=$udid"
  fi
  local since
  since="$(mg_app_log_now)"
  mg_app_xcodebuild_test "$prefix" "$platform" "$dest" Profile "${only[@]}"
  rc=$?
  # A Mac run whose synthesized input macOS dropped tested nothing it tapped:
  # it is queued (mac-input), unless a test failed where the drop does not
  # explain it (mg_app_mac_input_result records those tests). Its numbers
  # measured taps that did not happen: no perf result either way.
  local refused=""
  if [ "$platform" = mac ] && [ "$rc" -ne 2 ] && mg_app_mac_input_refused "$since" "$(printf '%s,' "${suites[@]}" | sed 's/,$//')"; then
    refused=1
    mg_app_mac_input_result "$prefix.xcresult" "$rc" test
    rc=$?
  fi
  [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "Performance plan result bundle"
  if [ "$rc" -eq 3 ]; then
    mg_message "perf-ui: queued: macOS dropped the input XCUITest synthesized (user step mac-input); the taps did not reach the app"
    mg_status pass
    mg_queued_settle
    return 0
  fi
  if [ -n "$refused" ]; then
    mg_message "perf-ui: performance tests failed before or without the input macOS dropped (user step mac-input); see the failures and $prefix.log"
    mg_status fail
    mg_queued_settle
    return 0
  fi
  if [ "$rc" -eq 2 ]; then
    mg_message "perf-ui: the performance tests could not run; see log.txt"
    mg_status error
    return 0
  fi

  # The result bundle's metrics, the tests that ran and the activities of
  # the performance suites' tests (their declared intervals), then one perf
  # result per performance suite.
  local metrics="$MG_RUN_DIR/metrics.json" tests="$MG_RUN_DIR/tests.json" runner="$MG_RUN_DIR/runner.json"
  local results="$MG_RUN_DIR/results" acts out err why bad=0 id n f
  mkdir -p "$results"
  if ! xcrun xcresulttool get test-results metrics --path "$prefix.xcresult" --compact >"$metrics" 2>>"$MG_RUN_DIR/log.txt" ||
    ! xcrun xcresulttool get test-results tests --path "$prefix.xcresult" --compact >"$tests" 2>>"$MG_RUN_DIR/log.txt"; then
    mg_failure metrics "xcresulttool could not read the metrics or the tests of $prefix.xcresult"
    mg_message "perf-ui: no metrics in the result bundle$([ "$rc" -eq 0 ] || echo "; the performance tests failed too, see $prefix.log")"
    mg_status error
    return 0
  fi
  mg_artifact metrics "$metrics" "xcresulttool test-results metrics"
  mg_artifact tests "$tests" "xcresulttool test-results tests"
  mg__perf_ui_runner "$([ "$platform" = mac ] && echo mac || echo "$udid")" "$runner"
  local unmeasured=() converted=""
  for s in "${suites[@]}"; do
    # The harness's own checks measure nothing: no result.
    if mg_word_in "${s%%/*}" "$MG_PERF_UI_CHECK_SUITES"; then
      unmeasured[${#unmeasured[@]}]="$s"
      continue
    fi
    # One result per class, whichever of its tests were named.
    ! mg_word_in "${s%%/*}" "$converted" || continue
    converted="$converted ${s%%/*}"
    out="$results/${s%%/*}.json"
    err="$MG_RUN_DIR/perf-compare-${s%%/*}.err"
    acts="$MG_RUN_DIR/activities/${s%%/*}"
    mkdir -p "$acts"
    : >"$err"
    n=0
    while IFS= read -r id; do
      [ -n "$id" ] || continue
      n=$((n + 1))
      xcrun xcresulttool get test-results activities --test-id "$id" --path "$prefix.xcresult" --compact \
        >"$acts/$n.json" 2>>"$MG_RUN_DIR/log.txt" || {
        echo "xcresulttool could not read the activities of $id" >>"$err"
        rm -f "$acts/$n.json"
      }
    done < <(jq -r --arg s "${s%%/*}" '.. | objects | select(.nodeType? == "Test Case")
      | .nodeIdentifier // empty | select(split("/")[0] == $s)' "$tests" 2>/dev/null)
    for f in "$acts"/*.json; do [ ! -f "$f" ] || cat "$f"; done | jq -s . >"$acts.json"
    if [ ! -s "$err" ] &&
      mg_image_tools python3 "$MG_PERF_COMPARE" xcresult "$(mg_container_path "$metrics")" --suite "${s%%/*}" \
        --configuration Profile --runner "$(mg_container_path "$runner")" --activities "$(mg_container_path "$acts.json")" \
        --out "$(mg_container_path "$out")" 2>>"$err" >&2 &&
      mg_image_tools python3 "$MG_PERF_COMPARE" validate "$(mg_container_path "$out")" >>"$err" 2>&1; then
      mg_artifact perf-result "$out" "${s%%/*}"
    else
      cat "$err" >&2
      why="$(grep -E '^(perf_compare.py|xcresulttool|/)' "$err" | tail -n 1 | sed 's/^perf_compare.py xcresult: //')"
      mg_failure "perf-result/${s%%/*}" "no valid perf result for $s: ${why:-see log.txt}"
      bad=$((bad + 1))
    fi
  done
  mg_detail unmeasured "$(mg_json_array ${unmeasured[0]+"${unmeasured[@]}"})"
  mg_detail results "$(cat /dev/null "$results"/*.json 2>/dev/null | jq -sc '[.[] | {suite, runner: .runner.id, results: [.results[] | {name, metric, percentile, value, unit, samples}]}]' 2>/dev/null || echo null)"
  if [ "$rc" -ne 0 ]; then
    mg_message "perf-ui: performance tests failed; see $prefix.log"
    mg_status fail
  elif [ "$bad" -gt 0 ]; then
    mg_message "perf-ui: $bad suite(s) without a valid perf result"
    mg_status fail
  else
    mg_message "perf-ui: ${#suites[@]} suite(s) passed on $([ "$platform" = mac ] && echo the Mac || echo "$udid"); perf results in $results"
    mg_status pass
  fi
}

# mg__metrics_record <template> <udid> <trace> <log> <note>: starts xctrace
# recording every process of the simulator where it is (MG_SIM_RUNNER: this
# Mac, or the test VM) into <trace>, after a waiter for its tracing-started
# notification <note>, appending its output to <log>. Sets MG_METRICS_PID
# (xctrace, or the SSH session that runs it) and MG_METRICS_WAITER
# (notifyutil -1, or its SSH session), which exits 0 once tracing has
# started. xctrace saves the trace when stopped with SIGINT, so it starts
# with SIGINT at its default (a background command ignores it; a command
# over SSH has it).
mg__metrics_record() {
  local args=(record --template "$1" --instrument os_signpost --device "$2" --all-processes
    --output "$3" --no-prompt --notify-tracing-started "$5")
  if [ "$MG_SIM_RUNNER" = vm ]; then
    # Darwin notifications stay on their machine: the waiter runs in the VM
    # too. xctrace replaces the shell that wrote its pid: SIGINT reaches it
    # there (mg__metrics_stop).
    mg_mac_vm_ssh "exec notifyutil -1 $(mvl_q "$5")" >/dev/null 2>&1 &
    MG_METRICS_WAITER=$!
    mg_mac_vm_ssh "mkdir -p $(mvl_q "$(dirname "$3")") && rm -rf $(mvl_q "$3") && echo \$\$ >$(mvl_q "$3.pid") &&
      exec xcrun xctrace $(mvl_q "${args[@]}")" >>"$4" 2>&1 &
    MG_METRICS_PID=$!
    return 0
  fi
  notifyutil -1 "$5" >/dev/null 2>&1 &
  MG_METRICS_WAITER=$!
  perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' xcrun xctrace "${args[@]}" </dev/null >>"$4" 2>&1 &
  MG_METRICS_PID=$!
}

# mg__metrics_start <template> <udid> <trace> <log> <note>: one start of
# xctrace and its waiter (mg__metrics_record), then a wait of up to
# MEGURI_METRICS_START_SECONDS (60) for tracing to start. Returns 0 once it
# has; otherwise 1, with MG_METRICS_WHY saying how it failed and both
# processes left to mg__metrics_end_start.
mg__metrics_start() {
  local i limit="${MEGURI_METRICS_START_SECONDS:-60}"
  mg__metrics_record "$@"
  for i in $(seq 1 $((limit * 10))); do
    if ! kill -0 "$MG_METRICS_WAITER" 2>/dev/null; then
      # notifyutil got the notification; an SSH session that failed did not.
      wait "$MG_METRICS_WAITER" && return 0
      break
    fi
    kill -0 "$MG_METRICS_PID" 2>/dev/null || break
    sleep 0.1
  done
  MG_METRICS_WHY="within $limit s"
  kill -0 "$MG_METRICS_PID" 2>/dev/null || MG_METRICS_WHY="before it ended"
  return 1
}

# mg__metrics_end_start <trace> <note>: after a start that failed, ends
# xctrace and its waiter with SIGKILL (an xctrace stuck before tracing
# ignores SIGINT), in the test VM there with the trace it began, and waits up
# to 10 s until neither runs. Returns 1 when that is not seen.
mg__metrics_end_start() {
  local i
  mg__metrics_kill_remote "$1" "$2" --drop
  kill -9 "$MG_METRICS_PID" "$MG_METRICS_WAITER" 2>/dev/null
  wait "$MG_METRICS_PID" "$MG_METRICS_WAITER" 2>/dev/null
  rm -rf "$1"
  for i in $(seq 1 20); do
    mg__metrics_gone "$2" && return 0
    sleep 0.5
  done
  return 1
}

# mg__metrics_gone <note>: whether neither an xctrace started for <note> nor
# its waiter runs where they ran (this Mac, or the test VM). pgrep exits 1
# when it finds none; an SSH session that fails proves nothing.
mg__metrics_gone() {
  # The brackets keep the shell that runs pgrep from matching itself.
  local pattern="notifyutil[ ]-1 $1\$|xctrace[ ]record .*--notify-tracing-started $1\$" rc
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_mac_vm_ssh "pgrep -f $(mvl_q "$pattern") >/dev/null"
  else
    pgrep -f "$pattern" >/dev/null
  fi
  rc=$?
  [ "$rc" -eq 1 ]
}

# mg__metrics_signal_remote <INT|KILL> <trace>: in the test VM, sends the
# signal to the xctrace that <trace>.pid names, while that pid still runs
# xctrace (a pid can be reused once it has exited).
mg__metrics_signal_remote() {
  local cmd="p=$(mvl_q "$2.pid"); [ -f \"\$p\" ] && x=\$(cat \"\$p\") && "
  cmd="$cmd ps -o command= -p \"\$x\" 2>/dev/null | grep -q 'xctrace record' && kill -$1 \"\$x\""
  mg_mac_vm_ssh "$cmd" >/dev/null 2>&1
}

# mg__metrics_kill_remote <trace> <note> [--drop]: in the test VM, ends an
# xctrace left recording there and its waiter (SIGKILL), and drops the pid
# file: killing the local SSH clients leaves both running there. With --drop
# it also removes a trace left there (one that never came back: a failed
# start, a cancelled run). Nothing to do on this Mac.
mg__metrics_kill_remote() {
  local drop=""
  [ "$MG_SIM_RUNNER" = vm ] && [ -n "$MG_MAC_VM" ] || return 0
  mg__metrics_signal_remote KILL "$1"
  [ "${3:-}" != --drop ] || drop=" $(mvl_q "$1")"
  # The brackets keep the shell that runs pkill from matching itself.
  mg_mac_vm_ssh "rm -rf $(mvl_q "$1.pid")$drop; pkill -9 -f $(mvl_q "notifyutil[ ]-1 $2")" >/dev/null 2>&1
  return 0
}

# mg__metrics_stop <pid> <trace> <note>: SIGINT to xctrace (it saves the
# trace on it), then SIGKILL after 120 s; returns once it has exited. In the
# test VM the signals go to xctrace there, and its waiter ends there too.
mg__metrics_stop() {
  local i
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg__metrics_signal_remote INT "$2"
  else
    kill -INT "$1" 2>/dev/null
  fi
  for i in $(seq 1 1200); do
    kill -0 "$1" 2>/dev/null || break
    sleep 0.1
  done
  mg__metrics_kill_remote "$2" "$3"
  kill -9 "$1" 2>/dev/null
  wait "$1" 2>/dev/null
}

# tools/mg metrics <script.json> [--platform ...] [--device <udid>]
#   [--config Debug|Profile] [--template <name>]
mg_metrics() {
  # Logging records what the drive measures, the os_signpost intervals, and
  # starts in the test VM; Time Profiler's CPU sampling never starts there
  # ("Device disconnected while trying to start tap").
  local template=Logging args=() udid script
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --template)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--template takes an Instruments template name (xcrun xctrace list templates)"; return 0; }
        template="$2" && shift 2
        ;;
      *) args[${#args[@]}]="$1" && shift ;;
    esac
  done
  mg_drive__args ${args[0]+"${args[@]}"} || return 0
  [ "${#MG_D_POSITIONAL[@]}" -eq 1 ] || { mg__perf_usage "usage: tools/mg metrics <script.json> [--platform ...] [--device <udid>] [--config Debug|Profile] [--template <name>]"; return 0; }
  [ -z "$MG_D_ROUTE" ] && [ "${#MG_D_REST[@]}" -eq 0 ] && [ "$MG_D_RECORD" = no ] ||
    { mg__perf_usage "the script sets the launch options, and metrics records Instruments, not video"; return 0; }
  if [ "$MG_D_KIND" = mac ]; then
    mg__perf_usage "metrics records simulators: Mac UI drives run in the test VM, which this Mac's Instruments does not record"
    return 0
  fi
  script="${MG_D_POSITIONAL[0]}"
  local problem
  if ! problem="$(mg_drive__check_script "$script")"; then
    mg__perf_usage "$problem"
    return 0
  fi
  udid="$MG_D_UDID"
  # The pool's simulators are in the test VM (mac-vm.sh): xctrace records
  # there, the drive runs there, and the trace comes back.
  mg_sim_begin || return 0
  [ -z "$udid" ] || mg_sim_check_given "$udid" || return 0
  mg_app_build ios-sim "$MG_D_CONFIG" || return 0
  mg_sim_take || return 0
  if [ -n "$udid" ]; then
    mg_sim_check_given "$udid" || return 0
  else
    mg_sim_lease "$MG_D_KIND" release || return 0
    udid="$MG_SIM_UDID"
    mg_sim_check_runtime "$udid" || return 0
  fi
  mg_sim_prepare "$udid" || return 0

  # xctrace records every process of the simulator, with os_signpost added
  # to the template so the registered intervals show (R1861). It runs
  # headless and saves the trace when stopped with SIGINT.
  local trace="$MG_RUN_DIR/metrics.trace" log="$MG_RUN_DIR/xctrace.log" note detail
  note="com.antoinefink.meguri.mg.metrics.$MG_RUN_ID"
  detail="$(jq -nc --arg t "$template" --arg d "$udid" '{template: $t, device: $d, instruments: ["os_signpost"]}')"
  mg_detail xctrace "$(jq -c '. + {starts: 1}' <<<"$detail")"
  MG_METRICS_PID="" MG_METRICS_WAITER=""
  # Registered after the lease: it runs first, while the job still holds it.
  # It ends the processes of the last start.
  mg_on_exit "kill -9 \$MG_METRICS_PID \$MG_METRICS_WAITER 2>/dev/null; mg__metrics_kill_remote '$trace' '$note' --drop"
  mg_artifact log "$log" "xctrace record"
  if ! mg__metrics_start "$template" "$udid" "$trace" "$log" "$note"; then
    # In the test VM a start stalls when Instruments' DTServiceHub crashes
    # starting ktrace (an assertion in authorized_ktrace_start_fd, a
    # DTServiceHub-*.ips report there): xctrace logs "Device disconnected
    # while trying to start tap" and waits for good. One more start, once
    # this one's xctrace and waiter are gone, so that two never record the
    # simulator at once.
    echo "metrics: xctrace did not start recording $MG_METRICS_WHY; ending it and its waiter, then starting it once more" |
      tee -a "$log" >&2
    if ! mg__metrics_end_start "$trace" "$note"; then
      mg_failure xctrace "xctrace did not start recording $MG_METRICS_WHY, and it or its waiter was not seen to end, so it was not started again; see $log" "tools/mg metrics $script"
      mg_message "metrics: xctrace did not start recording; see $log"
      mg_status error
      return 0
    fi
    mg_detail xctrace "$(jq -c '. + {starts: 2}' <<<"$detail")"
    if ! mg__metrics_start "$template" "$udid" "$trace" "$log" "$note"; then
      # Nothing to save: the exit cleanup kills xctrace and its waiter (an
      # xctrace stuck before tracing ignores SIGINT).
      mg_failure xctrace "xctrace did not start recording $MG_METRICS_WHY, twice; see $log" "tools/mg metrics $script"
      mg_message "metrics: xctrace did not start recording, twice; see $log"
      mg_status error
      return 0
    fi
  fi

  mg_drive_run "$script" ios "$udid" "$MG_D_CONFIG" no "$MG_RUN_DIR/drive"
  local rc=$?
  mg__metrics_stop "$MG_METRICS_PID" "$trace" "$note"
  # From the VM the trace comes back here, where it is read and kept; the VM
  # keeps no copy.
  if [ "$MG_SIM_RUNNER" = vm ] && ! mg_sim_fetch "$trace"; then
    mg_message "metrics: the trace did not come back from the test VM; see the failures and $log"
    mg_status error
    return 0
  fi
  if [ ! -d "$trace" ]; then
    mg_failure xctrace "xctrace saved no trace; see $log" "tools/mg metrics $script"
    mg_message "metrics: no trace was saved; see $log"
    mg_status error
    return 0
  fi
  mg_artifact trace "$trace" "Instruments trace ($template + os_signpost)"

  # The intervals the trace holds in Meguri's subsystem, checked against the
  # registry: the same names as in the tests (R1861).
  local table="$MG_RUN_DIR/os-signpost.xml" summary="$MG_RUN_DIR/signposts.json" unregistered
  if xcrun xctrace export --input "$trace" --xpath '/trace-toc/run[@number="1"]/data/table[@schema="os-signpost"]' \
    >"$table" 2>>"$log" &&
    mg_image_tools python3 "$MG_PERF_COMPARE" signposts "$(mg_container_path "$table")" \
      --out "$(mg_container_path "$summary")" >&2; then
    mg_artifact signposts "$summary" "intervals in the trace"
    mg_detail intervals "$(jq -c . "$summary")"
    unregistered="$(jq -r '.unregistered | join(", ")' "$summary")"
    [ -z "$unregistered" ] || mg_failure unregistered-intervals "the trace has intervals that are not in IntervalNames: $unregistered" "tools/mg metrics $script"
  else
    mg_failure signposts "could not read the trace's os-signpost table; see $log" "tools/mg metrics $script"
    rc=2
  fi
  case "$rc" in
    0)
      if [ -n "${unregistered:-}" ]; then
        mg_message "metrics: the trace has unregistered intervals: $unregistered"
        mg_status fail
      else
        mg_message "metrics: $script recorded in $trace ($(jq -r '[.intervals[] | "\(.name) x\(.count)"] | join(", ")' "$summary"))"
        mg_status pass
      fi
      ;;
    1)
      mg_message "metrics: the drive failed; the trace is in $trace"
      mg_status fail
      ;;
    *)
      mg_message "metrics: the drive or the trace's export could not run; see log.txt"
      mg_status error
      ;;
  esac
}
