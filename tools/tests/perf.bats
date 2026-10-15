#!/usr/bin/env bats
# Self-tests for the performance jobs (U12): perf-ui's refusals and wiring
# (Profile only, the Meguri-Profile scheme's Performance plan, one perf
# result per performance suite from the result bundle's metrics, failing a
# suite with no metrics or a declared interval with no measurements),
# bench, bench-mac and bench-icount's arguments and wiring, metrics'
# refusals, and where perf-ui and metrics reach their simulators: the test
# VM's pool (U8c), or queued without it. Apple tools, builds and cargo are
# faked; perf_compare.py runs here (its own tests are in
# image-tools-selftest), on a real SmokePerfTests run on the iPhone 17 Pro
# simulator (metrics, tests and activities). The result-conversion tests use
# this Mac's pool (MEGURI_SIM_ALLOW_HOST=1) with its calls faked; use_vm
# drops those fakes for the fake VM of tests/sim_vm.bats.

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
  # No test VM (use_vm adds one); simulators are this Mac's faked pool.
  export MEGURI_MAC_VM_CONFIG="$BATS_TEST_TMPDIR/config/mac-vm.json" MEGURI_SIM_ALLOW_HOST=1
  mkdir -p "$HOME" "$TMPDIR"
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  stub ioreg 'echo "+-o Root  <class IORegistryEntry>"'
  stub system_profiler 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\"}]}]}"'
  stub caffeinate 'exec sleep 600'
  # This Mac's Apple tools: a simulator, test or trace call here is logged
  # as host-<tool> and fails; xcresulttool and xctrace export read fixtures.
  stub xcodebuild 'echo "host-xcodebuild $*" >>"$STUB_LOG"; exit 70'
  stub notifyutil 'echo "host-notifyutil $*" >>"$STUB_LOG"; exit 97'
  stub xcrun 'echo "host-xcrun $*" >>"$STUB_LOG"
case "$*" in
  "xcresulttool get test-results metrics "*) cat "$FX_METRICS" ;;
  "xcresulttool get test-results tests "*) cat "$FX_TESTS" ;;
  "xcresulttool get test-results activities --test-id "*) jq -c --arg id "$6" ".[] | select(.testIdentifier == \$id)" "$FX_ACTIVITIES" ;;
  "xctrace export --input "*) [ -d "$4" ] && cat "$FX_SIGNPOSTS" ;;
  *) exit 70 ;;
esac'
  local run="$REPO_ROOT/tools/scripts/perf-compare/tests/data/smoke-iphone"
  export FX_METRICS="$run/metrics.json" FX_TESTS="$run/tests.json" FX_ACTIVITIES="$run/activities.json"
  export FX_SIGNPOSTS="$REPO_ROOT/tools/scripts/perf-compare/tests/data/os-signpost.xml"
  export PATH="$BIN:$PATH"
  add_fakes
}

teardown() {
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  # What the fake VM left running (its tools run here, from vm-bin).
  pkill -9 -f "$BATS_TEST_TMPDIR/vm-bin/" 2>/dev/null || true
  [ ! -f "$BATS_TEST_TMPDIR/bg.pids" ] || xargs kill <"$BATS_TEST_TMPDIR/bg.pids" 2>/dev/null || true
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

# Builds pass and write a minimal Products directory (the plan's simulator
# xctestrun and the app), a drive logs whether xctrace is recording and
# exits FX_DRIVE_RC (FX_DRIVE_UNTIL=<file> makes it touch <file>.started and
# wait for <file>), the image-tools container's commands run here. This
# Mac's pool (zz-pool-fakes.sh, which use_vm removes) lends FAKE-<kind>, and
# its test run writes a result bundle and exits FX_TEST_RC.
add_fakes() {
  cat >"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
fx_host() { case "$1" in /work/*) printf '%s\n' "$MG_ROOT/${1#/work/}" ;; *) printf '%s\n' "$1" ;; esac; }
mg_lint_ui_first() { return 0; }
mg_app_build() {
  echo "fake build $*" >>"$STUB_LOG"
  local p="$MG_ROOT/build/app/DerivedData/Build/Products" config="$2" plan="${3:-UI}" scheme="${4:-Meguri}"
  mkdir -p "$p/$config-iphonesimulator/Meguri.app" "$p/$config-iphonesimulator/MeguriUITests-Runner.app"
  printf 'built from %s\n' "$MG_ROOT" >"$p/$config-iphonesimulator/Meguri.app/Info.plist"
  printf '<plist><string>__TESTROOT__/%s-iphonesimulator/MeguriUITests-Runner.app</string></plist>\n' "$config" \
    >"$p/${scheme}_${plan}_iphonesimulator27.0-arm64.xctestrun"
  return 0
}
mg_drive_run() {
  echo "fake drive $1 $2 $3 $4 $5 runner=${MG_SIM_RUNNER:-}" >>"$STUB_LOG"
  if [ -n "${FX_DRIVE_UNTIL:-}" ]; then
    : >"$FX_DRIVE_UNTIL.started"
    while [ ! -e "$FX_DRIVE_UNTIL" ]; do sleep 0.1; done
  fi
  return "${FX_DRIVE_RC:-0}"
}
mg__bench_mac_fixtures() { echo "fake fixtures $*" >>"$STUB_LOG"; return "${FX_FIXTURES_RC:-0}"; }
mg_image_tools() {
  echo "fake image-tools $*" >>"$STUB_LOG"
  local args=() a
  for a in "$@"; do args[${#args[@]}]="$(fx_host "$a")"; done
  (cd "$MG_ROOT" && "${args[@]}")
}
EOF
  cat >"$SB/tools/mg.d/zz-pool-fakes.sh" <<'EOF'
mg_sim_lease() { echo "fake lease $1" >>"$STUB_LOG"; MG_SIM_UDID="FAKE-$1"; return 0; }
mg_sim_check_runtime() { return 0; }
mg_sim_check_given() { echo "fake check $1" >>"$STUB_LOG"; return 0; }
mg_sim_prepare() { return 0; }
mg_app_xcodebuild_test() {
  echo "fake test scheme=${MG_APP_TEST_SCHEME:-} plan=${MG_APP_TEST_PLAN:-} $*" >>"$STUB_LOG"
  mkdir -p "$1.xcresult"
  echo "xcodebuild log" >"$1.log"
  return "${FX_TEST_RC:-0}"
}
mg__perf_ui_runner() {
  jq -n '{kind: "simulator", id: "iPhone 17 Pro/24A434", model: "iPhone 17 Pro", os: "iOS 27.0", os_build: "24A434", arch: "arm64"}' >"$2"
}
EOF
}

# --- the test VM ------------------------------------------------------------
#
# use_vm: the fake test VM of tests/sim_vm.bats (mac_vm_fake_tart.sh,
# mac_vm_fake_ssh.sh): it runs remote commands here, in its own copy of the
# file system under $FAKE_VM_DIR/meguri-mac/fs, and its simulators are the
# fake simctl's (sim_pool_fake_xcrun.sh). The jobs' own pool, lease, runtime
# and test-run code runs; this Mac's pool fakes are dropped.
use_vm() {
  rm -f "$SB/tools/mg.d/zz-pool-fakes.sh"
  unset MEGURI_SIM_ALLOW_HOST
  export MEGURI_MAC_VM_POLL_SECONDS=1 MEGURI_MAC_VM_BOOT_SECONDS=6 MEGURI_MAC_VM_STOP_SECONDS=2 MEGURI_MAC_VM_LEASE_WAIT_SECONDS=60
  export FAKE_VM_DIR="$BATS_TEST_TMPDIR/vms" FAKE_VM_MIRROR="$BATS_TEST_TMPDIR" FAKE_VM_BIN="$BATS_TEST_TMPDIR/vm-bin"
  export FAKE_SIM_DB="$FAKE_VM_DIR/meguri-mac/sims.json" FAKE_SIMCTL="$REPO_ROOT/tools/tests/sim_pool_fake_xcrun.sh"
  export MEGURI_SIM_POOL_DIR="$BATS_TEST_TMPDIR/pool" MEGURI_SIM_DEVICES_DIR="$BATS_TEST_TMPDIR/Devices"
  export MEGURI_SIM_POOL_SETTLE_SECONDS=0 MEGURI_SIM_POOL_POLL_SECONDS=1 MEGURI_SIM_POOL_WAIT_SECONDS=2
  local drive="$BATS_TEST_TMPDIR/T9/meguri-vm/bin"
  mkdir -p "$FAKE_VM_DIR/meguri-mac/fs" "$FAKE_VM_BIN" "$drive" "$(dirname "$MEGURI_MAC_VM_CONFIG")"
  echo running >"$FAKE_VM_DIR/meguri-mac/state"
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_tart.sh" "$drive/tart"
  printf '#!/bin/bash\nFAKE_VM_NAME=meguri-mac exec %q "$@"\n' "$REPO_ROOT/tools/tests/mac_vm_fake_ssh.sh" >"$drive/vm-ssh"
  chmod +x "$drive/vm-ssh"
  jq -n --arg t "$drive/tart" --arg s "$drive/vm-ssh" '{tart: $t, vms: [{name: "meguri-mac", ssh: $s}]}' >"$MEGURI_MAC_VM_CONFIG"
  vm_stubs
}

vm_stub() { printf '#!/bin/bash\n%s\n' "$2" >"$FAKE_VM_BIN/$1" && chmod +x "$FAKE_VM_BIN/$1"; }

# The VM's tools. pgrep and pkill see only the fake VM's processes (those
# running from vm-bin). simctl is the fake simctl. xcodebuild checks the
# test products at the -xctestrun path (they must have been copied there)
# and the destination simulator (booted there), logs its arguments and
# writes a result bundle. xctrace record makes the trace bundle, checks its
# device, posts its tracing-started notification (FAKE_XCTRACE_FAILS=1: exits
# 1 first; FAKE_XCTRACE_STALLS=<n>: its first n starts never post it and
# ignore SIGINT, like a real one stuck before tracing), and on SIGINT saves
# the trace; notifyutil -1 waits for that notification. Both refuse to start
# (exit 1, logged as overlap) while another of them for the same notification
# still runs. Paths are logged as the VM sees them (without the fake file
# system's prefix).
vm_stubs() {
  vm_stub sudo 'exec "$@"'
  vm_stub stat "if [ \"\$*\" = '-f %Su /dev/console' ]; then echo meguri; exit 0; fi
exec $(command -v stat) \"\$@\""
  cat >"$FAKE_VM_BIN/pgrep" <<'EOF'
#!/bin/bash
rc=1
for pid in $(command -p pgrep -f -- "${!#}"); do
  case "$(ps -o command= -p "$pid" 2>/dev/null)" in *"$FAKE_VM_BIN/"*) echo "$pid" && rc=0 ;; esac
done
exit "$rc"
EOF
  cat >"$FAKE_VM_BIN/pkill" <<'EOF'
#!/bin/bash
echo "vm-pkill $*" >>"$STUB_LOG"
sig=TERM
[ "$1" != -9 ] || sig=KILL
pids="$(pgrep -f "${!#}")" || exit 1
kill -s "$sig" $pids 2>/dev/null
exit 0
EOF
  cat >"$FAKE_VM_BIN/notifyutil" <<'EOF'
#!/bin/bash
echo "vm-notifyutil $*" >>"$STUB_LOG"
[ "$1" = -1 ] && [ -n "${2:-}" ] || exit 64
pgrep -f "notifyutil[ ]-1 $2\$" >"$FAKE_VM/notifyutil.$$" || true
if grep -vqx "$$" "$FAKE_VM/notifyutil.$$"; then echo "vm-notifyutil overlap $2" >>"$STUB_LOG"; exit 1; fi
while [ ! -e "$FAKE_VM/notify/$2" ]; do sleep 0.1; done
exit 0
EOF
  cat >"$FAKE_VM_BIN/xcrun" <<'EOF'
#!/bin/bash
unmap() { local s="$*"; printf '%s' "${s//"$FAKE_VM_FS"/}"; }
case "$1" in
  simctl) exec "$FAKE_SIMCTL" "$@" ;;
  xctrace) ;;
  *) echo "vm-xcrun $(unmap "$*")" >>"$STUB_LOG" && exit 97 ;;
esac
echo "vm-xctrace $(unmap "${*:2}")" >>"$STUB_LOG"
[ "$2" = record ] || exit 97
while [ $# -gt 0 ]; do
  case "$1" in
    --device) udid="$2" && shift 2 ;;
    --output) out="$2" && shift 2 ;;
    --notify-tracing-started) note="$2" && shift 2 ;;
    *) shift ;;
  esac
done
pgrep -f "xctrace[ ]record .*--notify-tracing-started $note\$" >"$FAKE_VM/xctrace.$$" || true
if grep -vqx "$$" "$FAKE_VM/xctrace.$$"; then echo "vm-xctrace overlap" >>"$STUB_LOG"; exit 1; fi
# Like the real one, it makes the trace bundle before tracing starts.
mkdir -p "$out/Trace1.run"
[ "${FAKE_XCTRACE_FAILS:-}" != 1 ] || { echo "xctrace: error: the recording could not start"; exit 1; }
[ "$(xcrun simctl __state "$udid")" = Booted ] || { echo "xctrace: error: no booted device $udid"; exit 1; }
n=$(($(cat "$FAKE_VM/xctrace.starts" 2>/dev/null || echo 0) + 1))
echo "$n" >"$FAKE_VM/xctrace.starts"
if [ "$n" -le "${FAKE_XCTRACE_STALLS:-0}" ]; then
  trap '' INT
  echo "vm-xctrace stalled" >>"$STUB_LOG"
  while :; do sleep 0.1; done
fi
trap 'mkdir -p "$out" && echo trace >"$out/data" && echo "vm-xctrace saved $(unmap "$out")" >>"$STUB_LOG"; exit 0' INT
mkdir -p "$FAKE_VM/notify" && : >"$FAKE_VM/notify/$note"
echo "vm-xctrace recording" >>"$STUB_LOG"
while :; do sleep 0.1; done
EOF
  cat >"$FAKE_VM_BIN/xcodebuild" <<'EOF'
#!/bin/bash
unmap() { local s="$*"; printf '%s' "${s//"$FAKE_VM_FS"/}"; }
echo "vm-xcodebuild $(unmap "$*")" >>"$STUB_LOG"
while [ $# -gt 0 ]; do
  case "$1" in
    -xctestrun) run="$2" && shift 2 ;;
    -resultBundlePath) bundle="$2" && shift 2 ;;
    -destination) dest="$2" && shift 2 ;;
    *) shift ;;
  esac
done
root="$(sed -n 's|.*__TESTROOT__/\([^/]*\)/.*|\1|p' "$run" 2>/dev/null)"
app="$(dirname "$run")/$root/Meguri.app/Info.plist"
[ -n "$root" ] && [ -f "$app" ] || { echo "xcodebuild: error: no test products at $(unmap "$run")"; exit 70; }
echo "vm-products $(unmap "$(dirname "$run")/$root") $(cat "$app")" >>"$STUB_LOG"
[ "$(xcrun simctl __state "${dest##*id=}")" = Booted ] || { echo "xcodebuild: error: Unable to find a destination matching $dest"; exit 70; }
mkdir -p "$bundle"
echo '{"result": "Passed"}' >"$bundle/summary.json"
echo "** TEST SUCCEEDED **"
EOF
  chmod +x "$FAKE_VM_BIN"/*
}

vm_fs() { printf '%s/meguri-mac/fs%s\n' "$FAKE_VM_DIR" "$1"; }
vm_lease() { printf '%s/leases/%s/lease.json\n' "$(vm_fs "$MEGURI_SIM_POOL_DIR")" "$1"; }
lease_in_sandbox() { (cd "$SB" && "$SB/tools/scripts/sim-pool.sh" acquire "$1" --holder test 2>/dev/null); }
bg_track() { echo "$1" >>"$BATS_TEST_TMPDIR/bg.pids"; }
wait_for() {
  for _ in $(seq 1 150); do [ -e "$1" ] && return 0; sleep 0.1; done
  echo "$1 did not appear"
  return 1
}
# line <ERE>: the number of the first line of the stub log that matches.
line() { grep -nE -- "$1" "$STUB_LOG" | head -n 1 | cut -d: -f1; }

# No simulator, test or trace call and no lease on this Mac, and nothing
# left running in the VM.
assert_host_untouched() {
  not_called '^host-xcrun (simctl|devicectl|xctrace record)'
  not_called '^host-(xcodebuild|notifyutil)'
  not_called '^fake (lease|check|test) '
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ ! -d "$MEGURI_SIM_POOL_DIR" ]
  ! FAKE_VM_BIN="$FAKE_VM_BIN" "$FAKE_VM_BIN/pgrep" -f 'xctrace[ ]record|notifyutil[ ]-1' >/dev/null
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

@test "perf-ui fails a performance suite that yields no metrics" {
  export FX_METRICS="$BATS_TEST_TMPDIR/no-metrics.json"
  echo '[]' >"$FX_METRICS"
  mg perf-ui
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .message)" = "perf-ui: 1 suite(s) without a valid perf result" ]
  [ "$(field '.failures[0].id')" = perf-result/SmokePerfTests ]
  [[ "$(field '.failures[0].message')" == *"suite SmokePerfTests has no metrics"* ]]
  # Only the harness's own checks are exempt.
  [ "$(field -c .details.unmeasured)" = '["PerfTestCaseTests"]' ]
  [ "$(field '[.artifacts[] | select(.kind == "perf-result")] | length')" = 0 ]
}

@test "perf-ui fails a test whose declared interval has no measurements" {
  export FX_METRICS="$BATS_TEST_TMPDIR/no-signposts.json"
  jq '[.[] | .testRuns |= map(.metrics |= map(select(.identifier | contains("OSSignpost") | not)))]' \
    "$REPO_ROOT/tools/scripts/perf-compare/tests/data/smoke-iphone/metrics.json" >"$FX_METRICS"
  mg perf-ui --suite SmokePerfTests
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = perf-result/SmokePerfTests ]
  [[ "$(field '.failures[0].message')" == *"testProbeIntervalMeasuresItsSyntheticDelay() declares probe.syntheticDelay but has no measurements of it"* ]]
  called 'xcresulttool get test-results activities --test-id SmokePerfTests/testProbeIntervalMeasuresItsSyntheticDelay\(\)'
  called 'xcresulttool get test-results activities --test-id SmokePerfTests/testLaunch\(\)'
}

@test "perf-ui writes no result for the harness's own checks alone" {
  mg perf-ui --suite PerfTestCaseTests
  [ "$(field .status)" = pass ]
  [ "$(field -c .details.unmeasured)" = '["PerfTestCaseTests"]' ]
  [ "$(field '[.artifacts[] | select(.kind == "perf-result")] | length')" = 0 ]
  not_called 'activities --test-id PerfTestCaseTests'
}

@test "perf-ui fails when a performance test fails, and names the device it was given" {
  export FX_TEST_RC=1
  mg perf-ui --device ABCD --suite SmokePerfTests/testLaunch
  [ "$(field .status)" = fail ]
  called '^fake check ABCD$'
  called 'id=ABCD Profile -only-testing:MeguriUITests/SmokePerfTests/testLaunch$'
  not_called 'fake lease'
}

@test "perf-ui and metrics on a simulator are queued without the test VM: nothing is built and no simulator here is used" {
  unset MEGURI_SIM_ALLOW_HOST
  mg perf-ui
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.user_steps[] | select(.id == "mac-vm") | "\(.state) \(.required)"')" = "missing true" ]
  [[ "$(field .message)" == *"queued"*"simulators run only in the test VM"* ]]
  mg perf-ui --device ABCD
  [ "$(field .status)" = warn ]
  mg metrics "$SB/tools/drive-scripts/examples/perf-probe.json"
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  not_called '^fake (build|lease|check|test|drive)'
  not_called '^host-'
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "perf-ui on a simulator runs in the test VM: a device of its pool, the Performance plan there, the result bundle back" {
  use_vm
  mg perf-ui
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  [ "$(field .details.mac_vm.name)" = meguri-mac ]
  [ "$(field .details.host_lease)" = null ]
  [ "$(field .details.device.name)" = Meguri-Pool-iphone-1 ]
  local u dir out
  u="$(field .details.device.udid)"
  dir="$(dirname "$result")"
  # Built here, then the products went to the VM and the plan ran there, on
  # a pool device whose runtime was checked there.
  called '^fake build ios-sim Profile Performance Meguri-Profile$'
  called "^vm-products $SB/build/app/DerivedData/Build/Products/Profile-iphonesimulator built from $SB\$"
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri-Profile_Performance_iphonesimulator27.0-arm64.xctestrun -destination platform=iOS Simulator,id=$u -resultBundlePath $dir/perf.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/PerfTestCaseTests -only-testing:MeguriUITests/SmokePerfTests\$"
  called '^xcrun simctl list runtimes --json$'
  [ "$(field .details.runtime.actual.build)" = 24A434 ]
  # The result bundle came back (the VM keeps none) and was read here.
  [ -f "$dir/perf.xcresult/summary.json" ]
  [ ! -e "$(vm_fs "$dir/perf.xcresult")" ]
  called "^host-xcrun xcresulttool get test-results metrics --path $dir/perf.xcresult "
  out="$dir/results/SmokePerfTests.json"
  [ "$(field '.artifacts[] | select(.kind == "perf-result") | .path')" = "$out" ]
  python3 "$SB/tools/scripts/perf-compare/perf_compare.py" validate "$out"
  # The baseline key names the test VM, as for the Mac in it.
  [ "$(jq -r .runner.id "$out")" = "vm/meguri-mac/iPhone 17 Pro/24A434" ]
  [ "$(jq -r '"\(.runner.kind) \(.runner.model) \(.runner.os_build)"' "$out")" = "simulator iPhone 17 Pro 24A434" ]
  # The device went back to the pool, and the VM's lease is free.
  [ ! -e "$(vm_lease "$u")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  assert_host_untouched
}

@test "perf-ui --device takes a device this checkout leased from the VM's pool, checked there, and keeps it leased" {
  use_vm
  local u
  u="$(lease_in_sandbox iphone)"
  [ -n "$u" ]
  mg perf-ui --device "$u" --suite SmokePerfTests
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  [ "$(field .details.runtime.device)" = "$u" ]
  [ "$(field .details.runtime.actual.build)" = 24A434 ]
  called "^vm-xcodebuild .* -destination platform=iOS Simulator,id=$u .* -only-testing:MeguriUITests/SmokePerfTests\$"
  [ "$(jq -r .holder "$(vm_lease "$u")")" = test ]
  # A device the pool does not have is refused before anything is built.
  : >"$STUB_LOG"
  mg perf-ui --device 00000000-0000-4000-8000-000000000999
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = device ]
  not_called '^fake build'
  assert_host_untouched
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

@test "metrics records the drive with xctrace in the test VM, on its pool device, and brings the trace back" {
  use_vm
  grep -v review.notRegistered "$FX_SIGNPOSTS" >"$BATS_TEST_TMPDIR/os-signpost.xml"
  export FX_SIGNPOSTS="$BATS_TEST_TMPDIR/os-signpost.xml"
  local script="$SB/tools/drive-scripts/examples/perf-probe.json" u dir note
  mg metrics "$script"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  u="$(field .details.device.udid)"
  dir="$(dirname "$result")"
  note="com.antoinefink.meguri.mg.metrics.$(field .run_id)"
  called '^fake build ios-sim Debug$'
  called "^xcrun simctl status_bar $u override "
  # The waiter for the tracing-started notification and xctrace run in the
  # VM, on its simulator; the drive runs there (fake) while it records.
  called "^vm-notifyutil -1 $note\$"
  called "^vm-xctrace record --template Logging --instrument os_signpost --device $u --all-processes --output $dir/metrics.trace --no-prompt --notify-tracing-started $note\$"
  called "^fake drive $script ios $u Debug no runner=vm\$"
  [ "$(line '^vm-xctrace recording')" -lt "$(line '^fake drive')" ]
  [ "$(line '^fake drive')" -lt "$(line '^vm-xctrace saved')" ]
  # The trace came back; the VM keeps neither it nor xctrace's pid file.
  [ "$(cat "$dir/metrics.trace/data")" = trace ]
  [ ! -e "$(vm_fs "$dir/metrics.trace")" ]
  [ ! -e "$(vm_fs "$dir/metrics.trace.pid")" ]
  [ "$(field '.artifacts[] | select(.kind == "trace") | .path')" = "$dir/metrics.trace" ]
  # Its intervals were read here.
  called "^host-xcrun xctrace export --input $dir/metrics.trace "
  [ "$(field -c '[.details.intervals.intervals[] | "\(.name) x\(.count)"]')" = '["core_build_info x1","probe.syntheticDelay x2"]' ]
  [ ! -e "$(vm_lease "$u")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  assert_host_untouched
}

@test "metrics in the test VM: an xctrace that cannot start, also when started once more, is an error, and its waiter is ended there" {
  use_vm
  local dir
  FAKE_XCTRACE_FAILS=1 mg metrics "$SB/tools/drive-scripts/examples/perf-probe.json"
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = xctrace ]
  [[ "$(field '.failures[0].message')" == "xctrace did not start recording before it ended, twice; see "* ]]
  [ "$(field .message)" = "metrics: xctrace did not start recording, twice; see $(dirname "$result")/xctrace.log" ]
  [ "$(field .details.xctrace.starts)" = 2 ]
  dir="$(dirname "$result")"
  # One retry, no more; both starts are in the log.
  [ "$(grep -c '^vm-xctrace record ' "$STUB_LOG")" -eq 2 ]
  [ "$(grep -c 'the recording could not start' "$dir/xctrace.log")" -eq 2 ]
  not_called '^fake drive'
  called "^vm-pkill -9 -f notifyutil\[ \]-1 com.antoinefink.meguri.mg.metrics.$(field .run_id)\$"
  # Neither the pid file nor the partial trace is left there.
  [ ! -e "$(vm_fs "$dir/metrics.trace.pid")" ]
  [ ! -e "$(vm_fs "$dir/metrics.trace")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  assert_host_untouched
}

@test "metrics in the test VM: a start that stalls is killed there with its waiter, both are checked gone, and one more start records" {
  use_vm
  grep -v review.notRegistered "$FX_SIGNPOSTS" >"$BATS_TEST_TMPDIR/os-signpost.xml"
  export FX_SIGNPOSTS="$BATS_TEST_TMPDIR/os-signpost.xml"
  local script="$SB/tools/drive-scripts/examples/perf-probe.json" u dir note stalled killed waiter gone again
  FAKE_XCTRACE_STALLS=1 MEGURI_METRICS_START_SECONDS=2 mg metrics "$script"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.xctrace.starts)" = 2 ]
  u="$(field .details.device.udid)"
  dir="$(dirname "$result")"
  note="com.antoinefink.meguri.mg.metrics.$(field .run_id)"
  [ "$(grep -c "^vm-xctrace record --template Logging --instrument os_signpost --device $u .* --notify-tracing-started $note\$" "$STUB_LOG")" -eq 2 ]
  [ "$(grep -c "^vm-notifyutil -1 $note\$" "$STUB_LOG")" -eq 2 ]
  # The stalled xctrace ignores SIGINT: SIGKILL there by its pid file, its
  # waiter by its command line, then both checked gone there, all before the
  # second start (which would refuse to run beside them).
  stalled="$(line '^vm-xctrace stalled')"
  killed="$(line "^ssh meguri-mac p='$dir/metrics.trace.pid'; .* kill -KILL ")"
  waiter="$(line "^vm-pkill -9 -f notifyutil\[ \]-1 $note\$")"
  gone="$(line "^ssh meguri-mac pgrep -f 'notifyutil\[ \]-1 $note")"
  again="$(grep -n '^vm-xctrace record ' "$STUB_LOG" | sed -n 2p | cut -d: -f1)"
  [ "$stalled" -lt "$killed" ]
  [ "$killed" -lt "$gone" ]
  [ "$waiter" -lt "$gone" ]
  [ "$gone" -lt "$again" ]
  not_called 'overlap'
  grep -q "did not start recording within 2 s; ending it and its waiter, then starting it once more" "$dir/xctrace.log"
  # The second start records the drive, and its trace comes back.
  [ "$(line '^vm-xctrace recording')" -lt "$(line '^fake drive')" ]
  called "^fake drive $script ios $u Debug no runner=vm\$"
  [ "$(cat "$dir/metrics.trace/data")" = trace ]
  [ ! -e "$(vm_fs "$dir/metrics.trace")" ]
  [ ! -e "$(vm_fs "$dir/metrics.trace.pid")" ]
  assert_host_untouched
}

@test "metrics in the test VM: a stalled start whose end the VM does not confirm is an error, and xctrace is not started again" {
  use_vm
  local dir
  FAKE_XCTRACE_STALLS=1 MEGURI_METRICS_START_SECONDS=1 FAKE_VM_SSH_FAIL="^pgrep -f 'notifyutil" \
    mg metrics "$SB/tools/drive-scripts/examples/perf-probe.json"
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = xctrace ]
  [[ "$(field '.failures[0].message')" == "xctrace did not start recording within 1 s, and it or its waiter was not seen to end, so it was not started again; see "* ]]
  [ "$(field .details.xctrace.starts)" = 1 ]
  [ "$(grep -c '^vm-xctrace record ' "$STUB_LOG")" -eq 1 ]
  not_called '^fake drive'
  dir="$(dirname "$result")"
  [ ! -e "$(vm_fs "$dir/metrics.trace.pid")" ]
  [ ! -e "$(vm_fs "$dir/metrics.trace")" ]
  assert_host_untouched
}

@test "a cancelled metrics run ends xctrace and its waiter in the test VM" {
  use_vm
  local job dir
  FX_DRIVE_UNTIL="$BATS_TEST_TMPDIR/never" "$SB/tools/mg" metrics "$SB/tools/drive-scripts/examples/perf-probe.json" \
    >"$BATS_TEST_TMPDIR/job.out" 2>"$BATS_TEST_TMPDIR/job.err" 3>&- &
  job=$!
  bg_track "$job"
  wait_for "$BATS_TEST_TMPDIR/never.started"
  kill -TERM "$job"
  wait "$job" || true
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/job.out")"
  [ "$(field '[.failures[].id] | index("cancelled") != null')" = true ]
  dir="$(dirname "$result")"
  # Killed there by xctrace's pid file and the waiter's command line.
  called "^ssh meguri-mac p='$dir/metrics.trace.pid'; .* kill -KILL "
  called "^vm-pkill -9 -f notifyutil\[ \]-1 com.antoinefink.meguri.mg.metrics."
  [ ! -e "$(vm_fs "$dir/metrics.trace.pid")" ]
  [ ! -e "$(vm_fs "$dir/metrics.trace")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  assert_host_untouched
}
