# Performance instrumentation and benchmarks (U12: R1861-R1864, R1866).
# Sourced by tools/mg.
#
# Every job here writes perf results (tools/perf/perf-result.schema.json),
# which U241's perf-gate reads; the jobs themselves fail only when a suite
# fails to run, a result is invalid, or a bench's own check fails.
#
#   perf-ui       XCTest performance tests in the Profile configuration on a
#                 pool simulator (the Mac runs through the Mac UI gate), then
#                 tools/scripts/perf-compare turns the xcresult's metrics into
#                 a perf result.
#   bench         criterion suites in the dev container: every bench target
#                 outside benches/icount/; each writes its own perf result
#                 (meguri-testkit bench::report).
#   bench-icount  gungraun suites under valgrind in the dev container: every
#                 bench target under benches/icount/; perf-compare turns
#                 gungraun's JSON into a perf result.
#   metrics       an xctrace recording around a drive script (R1961).

mg_job perf-ui host mg_perf_ui \
  "UI performance tests (R1863, R1866): [--platform iphone|ipad|mac] [--device <leased udid>] [--config Profile] [--suite <class>]... (default SmokePerfTests); Profile build of the Performance plan, intervals, hitches, launch and memory into perf-result.json (tools/perf/perf-result.schema.json); any other configuration is refused"
mg_job bench docker:dev mg_bench \
  "Criterion wall-clock suites (R1864): [--sizes 10k,150k] [--bench <target>]... [-- criterion args]; makes the collections first (meguri-cli generate, as tools/mg fixtures), then every bench target outside benches/icount/: per-operation p50/p95/p99 and peak memory into perf results (tools/perf/perf-result.schema.json)"
mg_job bench-icount docker:dev mg_bench_icount \
  "gungraun instruction-count suites under valgrind (R1864): [--bench <target>]...; every bench target under benches/icount/, per-bench instruction counts into perf results (tools/perf/perf-result.schema.json)"
mg_job metrics host mg_metrics \
  "An Instruments recording around a drive script (R1961): <script.json> [--platform iphone|iphone-small|ipad|ipad-small] [--device <leased udid>] [--config Debug|Profile] [--template <name>] (default 'Time Profiler'); xctrace records the simulator while tools/mg drive runs, the .trace is an artifact"

MG_PERF_SCHEMA=tools/perf/perf-result.schema.json
MG_PERF_COMPARE=tools/scripts/perf-compare/perf_compare.py
# The collections the criterion suites open by default (MEGURI_BENCH_SIZES),
# and those the icount suites open. The jobs make them first, out of process:
# a bench samples its own process's memory, and generating a collection
# under valgrind would take minutes.
MG_BENCH_SIZES="10k,150k"
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

# mg__perf_validate <result.json>...: each against the perf result schema;
# records a failure per invalid file. Returns 1 when one is invalid.
mg__perf_validate() {
  local f out rc=0
  for f in "$@"; do
    if ! out="$(python3 "$MG_PERF_COMPARE" validate "$f" 2>&1)"; then
      mg_failure "invalid-result/$(basename "$f")" "$f is not a perf result: $(printf '%s' "$out" | head -n 3 | tr '\n' ' ')"
      rc=1
    fi
  done
  return "$rc"
}

# tools/mg bench [--sizes 10k,150k] [--bench <target>]... [-- criterion args]
mg_bench() {
  local sizes="$MG_BENCH_SIZES" only=() extra=() line pkg target results rc failed=0 ran=0 f
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --sizes)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--sizes takes a comma-separated list such as 10k,150k"; return 0; }
        sizes="$(printf '%s' "$2" | tr 'A-Z' 'a-z')" && shift 2
        ;;
      --bench)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_usage "--bench takes a bench target name"; return 0; }
        only[${#only[@]}]="$2" && shift 2
        ;;
      --) shift && extra=("$@") && break ;;
      *) mg__perf_usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  # shellcheck disable=SC2046 # one size per word
  if ! mg__perf_fixtures $(printf '%s' "$sizes" | tr ',' ' '); then
    mg_failure fixtures "making the bench collections ($sizes) failed; see log.txt" "tools/mg fixtures $(printf '%s' "$sizes" | tr ',' ' ')"
    mg_status fail
    return 0
  fi
  results="$MG_RUN_DIR/results"
  mkdir -p "$results"
  local targets
  targets="$(mg__bench_targets criterion)" || { mg_failure cargo-metadata "cargo metadata failed; see log.txt"; mg_status error; return 0; }
  while IFS=' ' read -r pkg target; do
    [ -n "$target" ] || continue
    if [ "${#only[@]}" -gt 0 ] && ! mg_word_in "$target" "${only[*]}"; then continue; fi
    ran=$((ran + 1))
    echo "bench: $pkg $target" >&2
    (
      cd core &&
        MEGURI_BENCH_SIZES="$sizes" MEGURI_BENCH_RESULTS="$results" MEGURI_BENCH_FIXTURES="$MG_ROOT/build/fixtures" \
          cargo bench -p "$pkg" --bench "$target" -- ${extra[0]+"${extra[@]}"}
    )
    rc=$?
    if [ "$rc" -ne 0 ]; then
      mg_failure "bench/$target" "cargo bench --bench $target exited $rc (a failed build, a crash or the bench's own check); see log.txt" \
        "tools/mg bench --bench $target${sizes:+ --sizes $sizes}"
      failed=$((failed + 1))
    fi
  done <<<"$targets"
  if [ "$ran" -eq 0 ]; then
    mg__perf_usage "no criterion bench target matched${only[0]+ ${only[*]}}"
    return 0
  fi
  local files=()
  for f in "$results"/*.json; do [ -f "$f" ] && files[${#files[@]}]="$f"; done
  if [ "${#files[@]}" -eq 0 ]; then
    mg_failure no-results "no bench wrote a perf result into $results"
    failed=$((failed + 1))
  elif ! mg__perf_validate "${files[@]}"; then
    failed=$((failed + 1))
  fi
  for f in ${files[0]+"${files[@]}"}; do mg_artifact perf-result "$f" "$(basename "$f" .json)"; done
  mg_detail results "$(cat /dev/null ${files[0]+"${files[@]}"} | jq -sc '[.[] | {suite, runner: .runner.id, results: [.results[] | {name, metric, percentile, value, unit, fixture}]}]' 2>/dev/null || echo null)"
  mg_count total "$ran"
  mg_count failed "$failed"
  mg_count passed $((ran - failed < 0 ? 0 : ran - failed))
  if [ "$failed" -eq 0 ]; then
    mg_message "bench: $ran suite(s), ${#files[@]} perf result(s) in ${results#"$MG_ROOT"/}"
    mg_status pass
  else
    mg_message "bench: $failed problem(s) in $ran suite(s); see failures"
    mg_status fail
  fi
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

# mg__perf_ui_runner <mac|udid> <out.json>: the perf result's runner, the
# baseline key U241 uses (model and OS build; CPU and OS on the Mac).
mg__perf_ui_runner() {
  local kind name model runtime build version cpu os
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
  jq -n --arg m "$model" --arg v "iOS $version" --arg b "$build" --arg u "$1" \
    '{kind: "simulator", id: "\($m)/\($b)", model: $m, os: $v, os_build: $b, arch: "arm64"}' >"$2"
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
    [ -z "$udid" ] || mg_sim_check_given "$udid" || return 0
    mg_app_build ios-sim Profile Performance Meguri-Profile || return 0
    mg_host_lease || return 0
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
  # A Mac run whose synthesized input macOS dropped tested nothing it tapped.
  if [ "$platform" = mac ] && [ "$rc" -ne 2 ] && mg_app_mac_input_refused "$since"; then
    rc=2
  fi
  [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "Performance plan result bundle"
  if [ "$rc" -eq 2 ]; then
    if [ -n "${MG_MAC_INPUT_REFUSED:-}" ]; then
      mg_message "perf-ui: blocked: macOS dropped the input XCUITest synthesized (user step mac-input); the taps did not reach the app"
    else
      mg_message "perf-ui: the performance tests could not run; see log.txt"
    fi
    mg_status error
    return 0
  fi

  # The result bundle's metrics, then one perf result per suite.
  local metrics="$MG_RUN_DIR/metrics.json" runner="$MG_RUN_DIR/runner.json" results="$MG_RUN_DIR/results" out bad=0
  mkdir -p "$results"
  if ! xcrun xcresulttool get test-results metrics --path "$prefix.xcresult" --compact >"$metrics" 2>>"$MG_RUN_DIR/log.txt"; then
    mg_failure metrics "xcresulttool could not read the metrics of $prefix.xcresult"
    mg_message "perf-ui: no metrics in the result bundle$([ "$rc" -eq 0 ] || echo "; the performance tests failed too, see $prefix.log")"
    mg_status error
    return 0
  fi
  mg_artifact metrics "$metrics" "xcresulttool test-results metrics"
  mg__perf_ui_runner "$([ "$platform" = mac ] && echo mac || echo "$udid")" "$runner"
  local unmeasured=()
  for s in "${suites[@]}"; do
    out="$results/${s%%/*}.json"
    # A suite that measures nothing (the harness's own checks) has no result.
    if ! jq -e --arg s "${s%%/*}" 'any(.[]; (.testIdentifier | split("/")[0]) == $s and any(.testRuns[].metrics[]?; true))' \
      "$metrics" >/dev/null 2>&1; then
      unmeasured[${#unmeasured[@]}]="$s"
      continue
    fi
    if mg_image_tools python3 "$MG_PERF_COMPARE" xcresult "$(mg_container_path "$metrics")" --suite "${s%%/*}" \
      --configuration Profile --runner "$(mg_container_path "$runner")" --out "$(mg_container_path "$out")" >&2 &&
      mg_image_tools python3 "$MG_PERF_COMPARE" validate "$(mg_container_path "$out")" >&2; then
      mg_artifact perf-result "$out" "${s%%/*}"
    else
      mg_failure "perf-result/${s%%/*}" "perf-compare could not write a valid perf result for $s; see log.txt"
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

# mg__metrics_stop <pid>: SIGINT (xctrace saves the trace on it), then
# SIGKILL after 120 s; returns once it has exited.
mg__metrics_stop() {
  local i
  kill -INT "$1" 2>/dev/null
  for i in $(seq 1 1200); do
    kill -0 "$1" 2>/dev/null || break
    sleep 0.1
  done
  kill -9 "$1" 2>/dev/null
  wait "$1" 2>/dev/null
}

# tools/mg metrics <script.json> [--platform ...] [--device <udid>]
#   [--config Debug|Profile] [--template <name>]
mg_metrics() {
  local template="Time Profiler" args=() udid script
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
  [ -z "$udid" ] || mg_sim_check_given "$udid" || return 0
  mg_app_build ios-sim "$MG_D_CONFIG" || return 0
  mg_host_lease || return 0
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
  # headless; started with SIGINT at its default (a background command
  # ignores it), it saves the trace when stopped with SIGINT.
  local trace="$MG_RUN_DIR/metrics.trace" log="$MG_RUN_DIR/xctrace.log" note pid waiter i started=0
  note="com.antoinefink.meguri.mg.metrics.$MG_RUN_ID"
  notifyutil -1 "$note" >/dev/null 2>&1 &
  waiter=$!
  mg_detail xctrace "$(jq -nc --arg t "$template" --arg d "$udid" '{template: $t, device: $d, instruments: ["os_signpost"]}')"
  perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' \
    xcrun xctrace record --template "$template" --instrument os_signpost --device "$udid" --all-processes \
    --output "$trace" --no-prompt --notify-tracing-started "$note" </dev/null >"$log" 2>&1 &
  pid=$!
  mg_on_exit "kill -9 $pid $waiter 2>/dev/null"
  mg_artifact log "$log" "xctrace record"
  for i in $(seq 1 600); do
    if ! kill -0 "$waiter" 2>/dev/null; then
      started=1
      break
    fi
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.1
  done
  if [ "$started" = 0 ]; then
    kill -9 "$waiter" 2>/dev/null
    mg__metrics_stop "$pid"
    mg_failure xctrace "xctrace did not start recording within 60 s; see $log" "tools/mg metrics $script"
    mg_message "metrics: xctrace did not start recording; see $log"
    mg_status error
    return 0
  fi

  mg_drive_run "$script" ios "$udid" "$MG_D_CONFIG" no "$MG_RUN_DIR/drive"
  local rc=$?
  mg__metrics_stop "$pid"
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
