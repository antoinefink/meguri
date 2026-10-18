#!/usr/bin/env bats
# Self-tests for simulators in the macOS test VM (U8c): the simulator pool
# runs in the VM (sim-pool.sh runs itself there over SSH), simulator jobs
# take the VM's lease instead of the host's, run simctl, devicectl and
# xcodebuild test-without-building there and bring their captures back, and
# without the VM they are queued (or use this Mac's pool with
# MEGURI_SIM_ALLOW_HOST=1). tart and the VM's SSH wrapper are the fakes of
# tests/mac_vm.bats (mac_vm_fake_tart.sh, mac_vm_fake_ssh.sh): the "VM" runs
# remote commands here, in its own copy of the file system under
# $FAKE_VM_DIR/<vm>/fs, and its simulators are the fake simctl's
# (sim_pool_fake_xcrun.sh) with their own database. This Mac's xcrun knows no
# simctl: a simulator call on the host would be logged as host-xcrun. Each
# test runs tools/mg from a throwaway copy of tools/ in its own git repo; the
# build is a fake that writes a minimal Products directory.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/app/Snapshots/iPhone18,1-24A434"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp -R "$REPO_ROOT/app/Fixtures" "$t/app/Fixtures"
  echo "a baseline" >"$t/app/Snapshots/iPhone18,1-24A434/baseline.png"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$(new_checkout repo)"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  export MEGURI_HOST_LEASE_DIR="$BATS_TEST_TMPDIR/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1
  export MEGURI_MAC_UI_DIR="$BATS_TEST_TMPDIR/mac-ui"
  export MEGURI_MAC_VM_POLL_SECONDS=1 MEGURI_MAC_VM_BOOT_SECONDS=6 MEGURI_MAC_VM_STOP_SECONDS=2
  # A job that waits for a lease it should not need fails within a minute
  # instead of hanging the suite.
  export MEGURI_MAC_VM_LEASE_WAIT_SECONDS=60
  unset MEGURI_MAC_UI_ALLOW_HOST MEGURI_SIM_ALLOW_HOST MEGURI_MAC_UI_INTERVAL_MINUTES
  export FAKE_VM_DIR="$BATS_TEST_TMPDIR/vms" FAKE_VM_MIRROR="$BATS_TEST_TMPDIR" FAKE_VM_BIN="$BATS_TEST_TMPDIR/vm-bin"
  # The VM's simulators. The pool's directories are given here and go to the
  # VM with sim-pool.sh, which maps them into the VM's file system.
  export FAKE_SIM_DB="$FAKE_VM_DIR/meguri-mac/sims.json"
  export MEGURI_SIM_POOL_DIR="$BATS_TEST_TMPDIR/pool" MEGURI_SIM_DEVICES_DIR="$BATS_TEST_TMPDIR/Devices"
  export MEGURI_SIM_POOL_SETTLE_SECONDS=0 MEGURI_SIM_POOL_POLL_SECONDS=1 MEGURI_SIM_POOL_WAIT_SECONDS=2
  export TMPDIR="$BATS_TEST_TMPDIR/tmp"
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN" "$FAKE_VM_BIN" "$FAKE_VM_DIR" "$TMPDIR"
  host_stubs
  vm_stubs
  DRIVE="$BATS_TEST_TMPDIR/T9/meguri-vm/bin"
  mkdir -p "$DRIVE"
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_tart.sh" "$DRIVE/tart"
  add_vm meguri-mac running
  export MEGURI_MAC_VM_CONFIG="$BATS_TEST_TMPDIR/config/mac-vm.json"
  write_config meguri-mac
  export PATH="$BIN:$PATH"
}

teardown() {
  [ ! -f "$FAKE_VM_DIR/run.pids" ] || xargs kill <"$FAKE_VM_DIR/run.pids" 2>/dev/null || true
  [ ! -f "$BATS_TEST_TMPDIR/bg.pids" ] || xargs kill <"$BATS_TEST_TMPDIR/bg.pids" 2>/dev/null || true
  cat "$FAKE_VM_DIR"/*/tests.pids 2>/dev/null | xargs kill -9 2>/dev/null || true
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  pkill -f "$BATS_TEST_TMPDIR/.*sim-record.sh" 2>/dev/null || true
  pkill -f "sim_pool_fake_xcrun.sh simctl io" 2>/dev/null || true
  # Shown only when the test failed.
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures, details}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 30
}

# new_checkout <name>: a fresh copy of the template repository; prints its path.
new_checkout() {
  cp -R "$BATS_FILE_TMPDIR/template" "$BATS_TEST_TMPDIR/$1"
  add_fakes "$BATS_TEST_TMPDIR/$1"
  (cd "$BATS_TEST_TMPDIR/$1" && pwd -P)
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$1" && chmod +x "$1"; }

add_vm() {
  mkdir -p "$FAKE_VM_DIR/$1/fs"
  echo "$2" >"$FAKE_VM_DIR/$1/state"
  stub "$DRIVE/vm-ssh-$1" "FAKE_VM_NAME=$1 exec '$REPO_ROOT/tools/tests/mac_vm_fake_ssh.sh' \"\$@\""
}

write_config() {
  mkdir -p "$(dirname "$MEGURI_MAC_VM_CONFIG")"
  local v vms='[]'
  for v in "$@"; do
    vms="$(jq -c --arg n "$v" --arg s "$DRIVE/vm-ssh-$v" '. + [{name: $n, ssh: $s}]' <<<"$vms")"
  done
  jq -n --arg t "$DRIVE/tart" --argjson vms "$vms" '{tart: $t, vms: $vms}' >"$MEGURI_MAC_VM_CONFIG"
}

# This Mac: the session check, caffeinate, and an xcodebuild and xcrun that
# log any simulator or test call. xcrun answers xcresulttool from the result
# bundles the VM's xcodebuild writes; simctl here is a failure.
host_stubs() {
  stub "$BIN/ioreg" 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes${STUB_LOCKED:+,\"CGSSessionScreenIsLocked\"=Yes})"'
  stub "$BIN/system_profiler" 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\"${STUB_LOCKED:+, \"spdisplays_asleep\": \"spdisplays_yes\"}}]}]}"'
  stub "$BIN/caffeinate" 'echo "caffeinate $*" >>"$STUB_LOG"; exec sleep 600'
  stub "$BIN/osascript" 'echo false'
  stub "$BIN/xcodebuild" 'echo "host-xcodebuild $*" >>"$STUB_LOG"; exit 70'
  stub "$BIN/xcrun" '
a=("$@")
for ((i = 0; i < ${#a[@]}; i++)); do
  case "${a[$i]}" in --path) path="${a[$((i + 1))]}" ;; --output-path) out="${a[$((i + 1))]}" ;; esac
done
case "$*" in
  "xcresulttool get test-results summary "*) cat "$path/summary.json" ;;
  "xcresulttool get test-results tests "*) echo "{\"testNodes\": []}" ;;
  "xcresulttool export attachments "*)
    mkdir -p "$out"
    if [ -d "$path/attachments" ]; then cp -R "$path/attachments/." "$out/"; else echo "[]" >"$out/manifest.json"; fi ;;
  *) echo "host-xcrun $*" >>"$STUB_LOG"; exit 97 ;;
esac'
}

# The VM: simctl is the fake simctl (its database $FAKE_SIM_DB), devicectl
# logs, and xcodebuild checks the test products at the -xctestrun path (they
# must have been copied there) and the destination simulator (booted in the
# VM), logs its arguments, TEST_RUNNER_ environment and whether the host lease
# is held, and writes a result bundle: a summary; for a drive script the
# driver's attachments, once it found every fixture the script's launch steps
# name in the VM (logged as vm-fixture); for the Snapshot plan its output
# directory (TEST_RUNNER_MEGURI_SNAPSHOT_OUT). FAKE_VM_XCODEBUILD_UNTIL=<file>
# makes it touch <file>.started and wait for <file>; FAKE_VM_IGNORE_TERM=1
# makes it ignore SIGTERM, as a real VM's run never gets the SIGTERM that
# tools/mg sends the job's process tree when it cancels the job. Paths are
# logged as the VM sees them (without the fake file system's prefix).
vm_stubs() {
  stub "$FAKE_VM_BIN/sudo" 'exec "$@"'
  # The process check jobs make before they start work there (U19c).
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_procs.sh" "$FAKE_VM_BIN/sysctl"
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_procs.sh" "$FAKE_VM_BIN/ps"
  stub "$FAKE_VM_BIN/pgrep" '
rc=1
for pid in $(cat "$FAKE_VM/tests.pids" 2>/dev/null); do
  cmd="$(ps -o command= -p "$pid" 2>/dev/null)" || continue
  if printf "%s\n" "$cmd" | grep -Eq -- "${!#}"; then echo "$pid"; rc=0; fi
done
exit "$rc"'
  stub "$FAKE_VM_BIN/pkill" '
echo "vm-pkill $*" >>"$STUB_LOG"
sig=TERM
[ "$1" != -9 ] || sig=KILL
pids="$(pgrep -f "${!#}")" || exit 1
kill -s "$sig" $pids 2>/dev/null
exit 0'
  stub "$FAKE_VM_BIN/stat" '
if [ "$*" = "-f %Su /dev/console" ]; then echo meguri; exit 0; fi
exec '"$(command -v stat)"' "$@"'
  # FAKE_VM_SIMCTL_FORK_FAIL=1: simctl cannot be forked there (the VM's user
  # at its process limit, as on 2026-10-17).
  stub "$FAKE_VM_BIN/xcrun" '
s="$*"
case "$1" in
  simctl)
    if [ "${FAKE_VM_SIMCTL_FORK_FAIL:-}" = 1 ]; then
      echo "/Applications/Xcode.app/Contents/Developer/usr/bin/simctl: fork: Resource temporarily unavailable" >&2
      exit 1
    fi
    exec "'"$REPO_ROOT"'/tools/tests/sim_pool_fake_xcrun.sh" "$@" ;;
  devicectl) s="${*:2}"; echo "vm-devicectl ${s//"$FAKE_VM_FS"/}" >>"$STUB_LOG" ;;
  *) echo "vm-xcrun $s" >>"$STUB_LOG"; exit 97 ;;
esac'
  stub "$FAKE_VM_BIN/xcodebuild" '
unmap() { local s="$*"; printf "%s" "${s//"$FAKE_VM_FS"/}"; }
if [ "${1:-}" = -version ]; then printf "Xcode 27.0\nBuild version 27A266a\n"; exit 0; fi
echo "vm-xcodebuild $(unmap "$@")" >>"$STUB_LOG"
other="$(pgrep -f "xcodebuild[ ]test-without-building")" && echo "vm-overlap with $other" >>"$STUB_LOG"
echo "$$" >>"$FAKE_VM/tests.pids"
[ "${FAKE_VM_IGNORE_TERM:-}" != 1 ] || trap "" TERM
env | grep "^TEST_RUNNER_" | sort | while IFS= read -r e; do echo "vm-env $(unmap "$e")" >>"$STUB_LOG"; done
echo "vm-host-lease $([ -e "$MEGURI_HOST_LEASE_DIR" ] && echo held || echo free)" >>"$STUB_LOG"
while [ $# -gt 0 ]; do
  case "$1" in
    -xctestrun) run="$2" && shift 2 ;;
    -resultBundlePath) bundle="$2" && shift 2 ;;
    -destination) dest="$2" && shift 2 ;;
    *) shift ;;
  esac
done
products="$(dirname "$run")"
app="$products/Debug-iphonesimulator/Meguri.app/Info.plist"
[ -f "$run" ] && [ -f "$app" ] || { echo "xcodebuild: error: no test products at $(unmap "$products")"; exit 70; }
echo "vm-products $(unmap "$products") $(cat "$app")" >>"$STUB_LOG"
udid="${dest#id=}"
state="$(xcrun simctl __state "$udid" 2>/dev/null)"
[ "$state" = Booted ] || { echo "xcodebuild: error: Unable to find a destination matching $dest"; exit 70; }
# The driver launches the app with the fixtures its script names: they must
# be in the VM (launch.fixture relative to the checkout).
if [ -n "${TEST_RUNNER_MEGURI_DRIVE_SCRIPT:-}" ] && [ -f "$TEST_RUNNER_MEGURI_DRIVE_SCRIPT" ]; then
  for f in $(jq -r ".steps[] | objects | .launch? | objects | (.fixture? | strings),
      (.arguments? | arrays | . as \$a | range(1; length) as \$i | select(\$a[\$i - 1] == \"-MeguriFixture\") | \$a[\$i] | strings)" \
      "$TEST_RUNNER_MEGURI_DRIVE_SCRIPT"); do
    case "$f" in /*) p="$FAKE_VM_FS$f" ;; *) p="${products%/build/app/DerivedData/Build/Products}/$f" ;; esac
    [ -e "$p" ] || { echo "AgentDriverTests: the launch fixture $(unmap "$p") is not there"; exit 65; }
    echo "vm-fixture $(unmap "$p")" >>"$STUB_LOG"
  done
fi
if [ -n "${FAKE_VM_XCODEBUILD_UNTIL:-}" ]; then
  : >"$FAKE_VM_XCODEBUILD_UNTIL.started"
  while [ ! -e "$FAKE_VM_XCODEBUILD_UNTIL" ]; do sleep 0.1; done
fi
mkdir -p "$bundle"
echo "{\"result\": \"Passed\", \"passedTests\": 12, \"failedTests\": 0, \"skippedTests\": 0}" >"$bundle/summary.json"
if [ -n "${TEST_RUNNER_MEGURI_SNAPSHOT_OUT:-}" ]; then
  out="$TEST_RUNNER_MEGURI_SNAPSHOT_OUT"
  mkdir -p "$out"
  if [ "${TEST_RUNNER_MEGURI_SNAPSHOT_MODE:-}" = plan ]; then
    : >"$out/plan.jsonl"
  else
    printf "probe %s\n" "$TEST_RUNNER_MEGURI_SNAPSHOT_GROUP" >"$out/probe.png"
    jq -nc --arg a "$(unmap "$out")/probe.png" --arg g "$TEST_RUNNER_MEGURI_SNAPSHOT_GROUP" --arg t "$TEST_RUNNER_MEGURI_SNAPSHOT_TARGET" \
      "{name: \"matrix.probe/default/\(\$t)\", subject: \"matrix.probe\", status: \"pass\", platform: \$t, group: \$g, key: \"iPhone18,1-24A434\", actual: \$a}" >"$out/results.jsonl"
    echo "{\"screens\": [], \"problems\": []}" >"$out/state-coverage.json"
    [ -f "$TEST_RUNNER_MEGURI_INVENTORY_JSON" ] && echo "vm-inventory present" >>"$STUB_LOG"
  fi
fi
if [ -n "${TEST_RUNNER_MEGURI_DRIVE_SCRIPT:-}" ]; then
  [ -f "$TEST_RUNNER_MEGURI_DRIVE_SCRIPT" ] || { echo "AgentDriverTests: no drive script at $(unmap "$TEST_RUNNER_MEGURI_DRIVE_SCRIPT")"; exit 65; }
  att="$bundle/attachments"
  mkdir -p "$att"
  t0="$(date +%s)"
  sleep 1
  jq -n --argjson t0 "$t0" "
    def cap(\$b; \$ph): {phase: \$ph, hierarchy: \"\(\$b)-\(\$ph).json\", screenshot: \"\(\$b)-\(\$ph).png\"};
    {script: \"fixture\", path: \"/fixture.json\", platform: \"ios\", error: null, steps: [
      {index: 1, kind: \"launch\", transition: null, started: \$t0, ended: (\$t0 + 0.05), status: \"ok\", captures: [cap(\"drive-01-launch\"; \"after\")]},
      {index: 2, kind: \"tap\", transition: \"dismiss-sheet\", started: (\$t0 + 0.1), ended: (\$t0 + 0.5), status: \"ok\",
        captures: [cap(\"drive-02-tap\"; \"before\"), cap(\"drive-02-tap\"; \"after\")]}
    ]}" >"$att/drive-log.json"
  for f in $(jq -r ".steps[] | .captures[] | .hierarchy, .screenshot" "$att/drive-log.json"); do
    case "$f" in
      *.json) echo "{\"schema\": 1, \"platform\": \"ios\", \"window\": {\"x\": 0, \"y\": 0, \"width\": 402, \"height\": 874}, \"root\": {}}" >"$att/$f" ;;
      *) printf png >"$att/$f" ;;
    esac
  done
  (cd "$att" && ls) | jq -R "{suggestedHumanReadableName: ., exportedFileName: .}" | jq -s "[{attachments: .}]" >"$bundle/manifest.tmp"
  mv "$bundle/manifest.tmp" "$att/manifest.json"
fi
echo "** TEST SUCCEEDED **"
exit 0'
}

# Fakes that replace the real functions (the file sorts after the jobs): the
# build writes a minimal Products directory with the plan's simulator
# xctestrun, lint passes, the core XCFramework is up to date, and the
# image-tools scripts write their reports here.
add_fakes() {
  cat >"$1/tools/mg.d/zz-sim-vm-fakes.sh" <<'EOF'
mg_lint_ui_first() { return 0; }
mg__xcframework() { return 0; }
mg_app_build() {
  echo "fake build $*" >>"$STUB_LOG"
  local p="$MG_ROOT/build/app/DerivedData/Build/Products" plan="${3:-UI}"
  mkdir -p "$p/$2-iphonesimulator/Meguri.app" "$p/$2-iphonesimulator/MeguriUITests-Runner.app"
  printf 'built from %s\n' "$MG_ROOT" >"$p/$2-iphonesimulator/Meguri.app/Info.plist"
  printf '<plist><string>__TESTROOT__/%s-iphonesimulator/MeguriUITests-Runner.app</string></plist>\n' "$2" \
    >"$p/Meguri_${plan}_iphonesimulator27.0-arm64.xctestrun"
}
fx_host() { case "$1" in /work/*) printf '%s\n' "$MG_ROOT/${1#/work/}" ;; *) printf '%s\n' "$1" ;; esac; }
mg_image_tools() {
  echo "fake image-tools $*" >>"$STUB_LOG"
  local script="$2" report="" out="" input="" files=() name
  shift 2
  case "$script" in
    */screen_status.py)
      while [ "$#" -gt 0 ]; do case "$1" in --out) out="$(fx_host "$2")" && shift 2 ;; *) shift ;; esac; done
      echo '{"screens": []}' >"$out"
      ;;
    */hierarchy_check.py)
      while [ "$#" -gt 0 ]; do
        case "$1" in --report) report="$2" && shift 2 ;; *) files[${#files[@]}]="$1" && shift ;; esac
      done
      printf '%s\n' "${files[@]}" | jq -R . | jq -s '{files: map({path: ., issues: []}), counts: {files: length, issues: 0}}' >"$(fx_host "$report")"
      ;;
    */contact_sheet.py)
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --out) out="$(fx_host "$2")" && shift 2 ;;
          --marks | --frame-list) input="$(fx_host "$2")" && shift 2 ;;
          *) shift ;;
        esac
      done
      for name in $(jq -r '.windows[].name' "$input"); do printf 'sheet' >"$out/$name.png"; done
      jq --arg out "$out" '{video: null, jump_cuts: [], windows: [.windows[] | {name, sheet: "\($out)/\(.name).png", jump_cuts: [], stalls: []}]}' \
        "$input" >"$out/report.json"
      ;;
  esac
}
EOF
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG" || { echo "unexpected call: $(grep -E -- "$1" "$STUB_LOG")"; return 1; }; }
validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }
bg_track() { echo "$1" >>"$BATS_TEST_TMPDIR/bg.pids"; }
wait_for() {
  for _ in $(seq 1 100); do [ -e "$1" ] && return 0; sleep 0.1; done
  echo "$1 did not appear"
  return 1
}

vm_fs() { printf '%s/%s/fs%s\n' "$FAKE_VM_DIR" "$1" "$2"; }
# The VM's simulators and the pool's leases there.
vm_sim() { FAKE_SIM_DB="$FAKE_VM_DIR/$1/sims.json" MEGURI_SIM_DEVICES_DIR="$(vm_fs "$1" "$MEGURI_SIM_DEVICES_DIR")" "$REPO_ROOT/tools/tests/sim_pool_fake_xcrun.sh" simctl "${@:2}"; }
vm_lease() { printf '%s/leases/%s/lease.json\n' "$(vm_fs meguri-mac "$MEGURI_SIM_POOL_DIR")" "$1"; }
vm_tests_running() { FAKE_VM="$FAKE_VM_DIR/$1" "$FAKE_VM_BIN/pgrep" -f 'xcodebuild[ ]test-without-building' >/dev/null; }
# wait_log <file> <ERE>: waits until a line of the file matches.
wait_log() {
  for _ in $(seq 1 150); do grep -Eq -- "$2" "$1" 2>/dev/null && return 0; sleep 0.1; done
  echo "$1 has no line matching $2:"
  cat "$1"
  return 1
}

# lease_in_sandbox <kind>: a VM pool device leased from the sandbox checkout.
lease_in_sandbox() { (cd "$SB" && "$SB/tools/scripts/sim-pool.sh" acquire "$1" --holder test 2>/dev/null); }

# No simulator call, test run or lease on this Mac.
assert_host_untouched() {
  not_called '^host-xcrun simctl'
  not_called '^host-xcrun devicectl'
  not_called '^host-xcodebuild'
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ ! -d "$MEGURI_SIM_POOL_DIR" ]
}

# --- the pool in the VM -----------------------------------------------------------

@test "sim acquire leases a simulator of the VM's pool to this checkout; release resets it there" {
  mg sim acquire iphone --holder agent
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  local u
  u="$(field .details.udid)"
  [ "$(vm_sim meguri-mac __state "$u")" = Booted ]
  called "^xcrun simctl create Meguri-Pool-iphone-1 com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro com.apple.CoreSimulator.SimRuntime.iOS-27-0\$"
  # After the boot the pool checks, in the VM's simulator, that SpringBoard
  # applies appearance changes.
  called "^xcrun simctl spawn $u log show --start "
  # The lease is the VM's, and names this checkout.
  [ "$(jq -r '"\(.worktree) \(.holder) \(.vm)"' "$(vm_lease "$u")")" = "$SB agent meguri-mac" ]
  [ "$(field '.details.lease.worktree')" = "$SB" ]
  [ "$(field '.details.device.name')" = Meguri-Pool-iphone-1 ]
  assert_host_untouched
  validate "$result"

  # A script calls the pool directly: the same pool, in the VM.
  run --separate-stderr "$SB/tools/scripts/sim-pool.sh" status
  [ "$status" -eq 0 ]
  [ "$(jq -r '"\(.vm) \(.devices | length) \(.devices[0].lease.worktree)"' <<<"$output")" = "meguri-mac 1 $SB" ]

  mg sim release "$u"
  [ "$(field .status)" = pass ]
  [ ! -e "$(vm_lease "$u")" ]
  called "^xcrun simctl status_bar $u clear\$"
  assert_host_untouched
}

@test "sim-setup checks the VM's runtime and device types and creates the pool there" {
  mg sim-setup --kinds iphone,ipad
  [ "$(field .status)" = pass ]
  [ "$(field .details.runtime.actual.build)" = 24A434 ]
  [ "$(field '.details.devices | keys | join(" ")')" = "ipad iphone" ]
  [ "$(jq -r '"\(.runner) \(.vm)"' "$SB/build/sim-devices.local.json")" = "vm meguri-mac" ]
  [ "$(vm_sim meguri-mac list devices --json | jq '[.devices[][] | select(.name | startswith("Meguri-Pool-"))] | length')" = 2 ]
  assert_host_untouched

  # The pinned build is checked against the VM's runtime.
  FAKE_SIM_RT27_BUILD=24A999 mg sim-setup
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
}

@test "a stopped VM is started headless by a direct call of the pool script" {
  echo stopped >"$FAKE_VM_DIR/meguri-mac/state"
  run --separate-stderr "$SB/tools/scripts/sim-pool.sh" status
  [ "$status" -eq 0 ]
  called "^tart run --no-graphics meguri-mac\$"
  [ "$(jq -r .vm <<<"$output")" = meguri-mac ]
}

# --- jobs in the VM -------------------------------------------------------------

@test "a11y --platform iphone runs in the VM under its lease: products copied there, the result bundle back, no host lease" {
  mg a11y --platform iphone
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  [ "$(field .details.mac_vm.name)" = meguri-mac ]
  [ "$(field .details.host_lease)" = null ]
  [ "$(field '.summary | "\(.passed) \(.failed)"')" = "12 0" ]
  local u dir
  u="$(field .details.device.udid)"
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri_UI_iphonesimulator27.0-arm64.xctestrun -destination id=$u -resultBundlePath $SB/build/mg/a11y/[^ ]+/iphone.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/AccessibilityHelperTests\$"
  called "^vm-products $SB/build/app/DerivedData/Build/Products built from $SB\$"
  called "^vm-host-lease free\$"
  # The pinned status bar, in the VM.
  called "^xcrun simctl status_bar $u override --time 9:41 "
  dir="$(dirname "$result")"
  [ "$(jq -r .passedTests "$dir/iphone.xcresult/summary.json")" = 12 ]
  [ ! -e "$(vm_fs meguri-mac "$dir/iphone.xcresult")" ]
  # The device went back to the pool; the VM lease is released.
  [ ! -e "$(vm_lease "$u")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  assert_host_untouched
  validate "$result"
}

@test "ui-test --platform iphone runs the named suites in the VM under its lease and refuses a run with no suite" {
  mg ui-test --platform iphone --suite NavigationLayoutTests --suite TestHooksTests/testSampleScreenRoute
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  [ "$(field .details.host_lease)" = null ]
  [ "$(field '.details.suites | join(",")')" = NavigationLayoutTests,TestHooksTests/testSampleScreenRoute ]
  [ "$(field '.summary | "\(.passed) \(.failed)"')" = "12 0" ]
  local u dir
  u="$(field .details.device.udid)"
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri_UI_iphonesimulator27.0-arm64.xctestrun -destination id=$u -resultBundlePath $SB/build/mg/ui-test/[^ ]+/iphone.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/NavigationLayoutTests -only-testing:MeguriUITests/TestHooksTests/testSampleScreenRoute\$"
  dir="$(dirname "$result")"
  [ "$(jq -r .passedTests "$dir/iphone.xcresult/summary.json")" = 12 ]
  [ ! -e "$(vm_lease "$u")" ]
  assert_host_untouched
  validate "$result"

  mg ui-test --platform iphone
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = usage ]
  mg ui-test --platform iphone --suite 'Bad Suite'
  [ "$(field .status)" = error ]
}

@test "a locked Mac with its display asleep does not hold up simulator jobs in the VM" {
  export STUB_LOCKED=1
  mg a11y --platform ipad
  [ "$(field .status)" = pass ]
  [ "$(field .details.blocked)" = null ]
  [ -z "$(field '.user_steps[] | select(.id == "host-session") | .id')" ]
  not_called '^caffeinate'
}

@test "drive --record on an iPhone runs in the VM: the script and fixture go there, the video and captures come back" {
  local script="$SB/tools/drive-scripts/examples/open-sample-sheet.json"
  mg drive "$script" --platform iphone --record
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  cmp "$script" "$(vm_fs meguri-mac "$script")"
  cmp "$SB/app/Fixtures/TestHooks/single-file.fixture" "$(vm_fs meguri-mac "$SB/app/Fixtures/TestHooks/single-file.fixture")"
  called "^vm-env TEST_RUNNER_MEGURI_DRIVE_SCRIPT=$script\$"
  called "^vm-env TEST_RUNNER_MEGURI_DRIVE_FRAMES=0\$"
  called "^vm-xcodebuild .* -only-testing:MeguriUITests/AgentDriverTests/testDriveScript\$"
  local dir u
  dir="$(dirname "$result")"
  u="$(field .details.drive.device)"
  # simctl recorded in the VM; the video came back and the VM keeps none.
  called "^xcrun simctl io $u recordVideo --codec h264 --force .*$dir/video.mp4\$"
  [ "$(cat "$dir/video.mp4")" = "fake video" ]
  [ ! -e "$(vm_fs meguri-mac "$dir/video.mp4")" ]
  [ ! -e "$(vm_fs meguri-mac "$dir/video.mp4.pid")" ]
  [ "$(field .details.drive.capture)" = video ]
  [ -f "$dir/motion/02-dismiss-sheet.png" ]
  [ "$(field '[.artifacts[] | select(.kind == "png")] | length')" -ge 3 ]
  [ "$(field '[.artifacts[] | select(.kind == "video")] | length')" = 1 ]
  assert_host_untouched
  validate "$result"
}

@test "run, ui-tree and screenshot install, launch and capture in the VM" {
  local u
  u="$(lease_in_sandbox iphone)"
  [ -n "$u" ]
  mg run --platform iphone --device "$u" --route sample/sheet
  [ "$(field .status)" = pass ]
  called "^xcrun simctl install $u $(vm_fs meguri-mac "$SB")/build/app/DerivedData/Build/Products/Debug-iphonesimulator/Meguri.app\$"
  called "^xcrun simctl launch --terminate-running-process $u com.antoinefink.meguri.dev -ApplePersistenceIgnoreState YES -MeguriRoute sample/sheet\$"

  mg ui-tree --route sample/sheet --platform iphone --device "$u"
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  [ "$(field '[.artifacts[] | select(.kind == "hierarchy")] | length')" -ge 1 ]

  mg screenshot --device "$u"
  [ "$(field .status)" = pass ]
  local png
  png="$(dirname "$result")/screen.png"
  [ -s "$png" ]
  [ ! -e "$(vm_fs meguri-mac "$png")" ]
  assert_host_untouched
  validate "$result"
}

@test "record start holds the VM lease and records there; this checkout's drive runs nested; stop brings the video back" {
  local u start stop video
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$(field .status)" = pass ]
  start="$result"
  [ "$(field .details.recording.sim_runner)" = vm ]
  video="$(field .details.recording.video)"
  [ "$(jq -r '"\(.kind) \(.udid)"' "$BATS_TEST_TMPDIR/mac-vm.meguri-mac.lock/holder.json")" = "recording $u" ]
  [ -s "$(vm_fs meguri-mac "$video")" ]

  # A drive of this checkout runs inside the recording's lease.
  mg drive "$SB/tools/drive-scripts/examples/open-sample-sheet.json" --platform iphone --device "$u"
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_vm.lease.nested)" = true ]

  mg record stop --device "$u"
  [ "$(field .status)" = pass ]
  [ "$(cat "$video")" = "fake video" ]
  [ ! -e "$(vm_fs meguri-mac "$video")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  [ "$(jq -r .details.recording.reason "$result")" = stopped ]
  assert_host_untouched
  result="$start"
  validate "$result"
}

@test "record stop of a killed recording worker stops the recorder it left in the VM and brings the video back" {
  local u video pid
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$(field .status)" = pass ]
  video="$(field .details.recording.video)"
  pid="$(field .details.recording.pid)"
  kill -9 "$pid"
  pgrep -f "sim_pool_fake_xcrun.sh simctl io $u recordVideo" >/dev/null
  mg record stop --device "$u"
  [ "$(field .status)" = error ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"ended before record stop"* ]]
  [ "$(cat "$video")" = "fake video" ]
  ! pgrep -f "sim_pool_fake_xcrun.sh simctl io $u recordVideo" >/dev/null
  [ ! -e "$(vm_fs meguri-mac "$video")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
}

@test "snapshots --platform ipad render in the VM: baselines and inventory there, settings by devicectl there, results back" {
  mg snapshots --platform ipad
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = vm ]
  cmp "$SB/app/Snapshots/iPhone18,1-24A434/baseline.png" "$(vm_fs meguri-mac "$SB/app/Snapshots/iPhone18,1-24A434/baseline.png")"
  called '^vm-inventory present$'
  called "^vm-devicectl device settings appearance --device [^ ]+ --mode light "
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri_Snapshot_iphonesimulator27.0-arm64.xctestrun "
  local dir
  dir="$(dirname "$result")"
  [ -f "$dir/ipad/plan/plan.jsonl" ]
  [ "$(jq -r .status "$dir/ipad/rt0-rm0-t50/results.jsonl")" = pass ]
  [ -f "$dir/ipad/rt0-rm0-t50/probe.png" ]
  [ ! -e "$(vm_fs meguri-mac "$dir/ipad/rt0-rm0-t50")" ]
  [ "$(field .summary.passed)" = 1 ]
  assert_host_untouched
}

@test "two checkouts start simulator jobs at once: the second waits for the VM lease" {
  local sb2 first second
  sb2="$(new_checkout repo2)"
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/first-done" "$SB/tools/mg" a11y --platform iphone \
    >"$BATS_TEST_TMPDIR/first.out" 2>"$BATS_TEST_TMPDIR/first.err" 3>&- &
  first=$!
  bg_track "$first"
  wait_for "$BATS_TEST_TMPDIR/first-done.started"
  "$sb2/tools/mg" a11y --platform iphone >"$BATS_TEST_TMPDIR/second.out" 2>"$BATS_TEST_TMPDIR/second.err" 3>&- &
  second=$!
  bg_track "$second"
  for _ in $(seq 1 100); do grep -q 'waiting for the VM' "$BATS_TEST_TMPDIR/second.err" && break; sleep 0.1; done
  grep -q "VM lease: waiting for the VM (meguri-mac: a11y in $SB" "$BATS_TEST_TMPDIR/second.err"
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" = 1 ]
  touch "$BATS_TEST_TMPDIR/first-done"
  wait "$first"
  wait "$second"
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/first.out")")" = pass ]
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/second.out")")" = pass ]
  [ "$(jq -r .details.mac_vm.lease.waited_s "$(tail -n 1 "$BATS_TEST_TMPDIR/second.out")")" -ge 1 ]
  not_called '^vm-overlap'
}

@test "sim acquire, sim-setup and a script's own acquire wait for the VM's lease while a suite runs there; the suite's own acquire does not" {
  local sb2 suite acq setup direct u
  sb2="$(new_checkout repo2)"
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/suite-done" "$SB/tools/mg" a11y --platform iphone \
    >"$BATS_TEST_TMPDIR/suite.out" 2>"$BATS_TEST_TMPDIR/suite.err" 3>&- &
  suite=$!
  bg_track "$suite"
  wait_for "$BATS_TEST_TMPDIR/suite-done.started"
  "$sb2/tools/mg" sim acquire ipad >"$BATS_TEST_TMPDIR/acquire.out" 2>"$BATS_TEST_TMPDIR/acquire.err" 3>&- &
  acq=$!
  bg_track "$acq"
  "$sb2/tools/mg" sim-setup --kinds ipad-small >"$BATS_TEST_TMPDIR/setup.out" 2>"$BATS_TEST_TMPDIR/setup.err" 3>&- &
  setup=$!
  bg_track "$setup"
  (cd "$sb2" && exec "$sb2/tools/scripts/sim-pool.sh" acquire iphone-small --holder script) \
    >"$BATS_TEST_TMPDIR/direct.out" 2>"$BATS_TEST_TMPDIR/direct.err" 3>&- &
  direct=$!
  bg_track "$direct"
  wait_log "$BATS_TEST_TMPDIR/acquire.err" "VM lease: waiting for the VM \(meguri-mac: a11y in $SB"
  wait_log "$BATS_TEST_TMPDIR/setup.err" "VM lease: waiting for the VM \(meguri-mac: a11y in $SB"
  wait_log "$BATS_TEST_TMPDIR/direct.err" "VM lease: waiting for the VM \(meguri-mac: a11y in $SB"
  # Nothing was created or booted in the VM beside the suite: only the
  # suite's own iPhone.
  not_called '^xcrun simctl create Meguri-Pool-(ipad|ipad-small|iphone-small)-'
  [ "$(grep -c '^xcrun simctl boot ' "$STUB_LOG")" = 1 ]

  touch "$BATS_TEST_TMPDIR/suite-done"
  wait "$suite"
  wait "$acq"
  wait "$setup"
  wait "$direct" || { cat "$BATS_TEST_TMPDIR/direct.err"; return 1; }
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/suite.out")")" = pass ]
  # The suite leased its iPhone under its own lease, not through tools/mg sim.
  [ ! -e "$SB/build/mg/sim" ]
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/acquire.out")"
  [ "$(field .status)" = pass ]
  [ "$(field .details.device.kind)" = ipad ]
  [ "$(field .details.mac_vm.lease.waited_s)" -ge 1 ]
  validate "$result"
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/setup.out")"
  [ "$(field .status)" = pass ]
  [ "$(field '.details.devices["ipad-small"].status')" = created ]
  # The script's acquire went through tools/mg sim, under the lease, and
  # printed only the UDID, leased to its checkout.
  [ "$(wc -l <"$BATS_TEST_TMPDIR/direct.out" | tr -d ' ')" = 1 ]
  u="$(cat "$BATS_TEST_TMPDIR/direct.out")"
  [ "$(jq -r '"\(.kind) \(.holder) \(.worktree)"' "$(vm_lease "$u")")" = "iphone-small script $sb2" ]
  grep -q "under the VM's lease" "$BATS_TEST_TMPDIR/direct.err"
  [ "$(vm_sim meguri-mac __state "$u")" = Booted ]
  not_called '^vm-overlap'
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  assert_host_untouched
}

@test "run puts a -MeguriFixture fixture in the VM: in the checkout at its own path, outside it under the run; a missing one launches nothing" {
  local u inside outside dir
  u="$(lease_in_sandbox iphone)"
  [ -n "$u" ]
  inside="$SB/build/fixtures/recipes/sample-1"
  mkdir -p "$inside"
  echo collection >"$inside/collection.anki2"
  mg run --platform iphone --device "$u" --route sample/sheet -- -MeguriFixture "$inside"
  [ "$(field .status)" = pass ]
  diff -r "$inside" "$(vm_fs meguri-mac "$inside")"
  called "^xcrun simctl launch --terminate-running-process $u com.antoinefink.meguri.dev -ApplePersistenceIgnoreState YES -MeguriRoute sample/sheet -MeguriFixture $(vm_fs meguri-mac "$inside")\$"

  # Outside the checkout: the VM's paths there are not this Mac's.
  outside="$BATS_TEST_TMPDIR/elsewhere/deck.anki2"
  mkdir -p "$(dirname "$outside")"
  echo other >"$outside"
  mg run --platform iphone --device "$u" -- -MeguriFixture "$outside"
  [ "$(field .status)" = pass ]
  dir="$(dirname "$result")"
  [ "$(cat "$(vm_fs meguri-mac "$dir/vm-fixtures/1/deck.anki2")")" = other ]
  [ ! -e "$(vm_fs meguri-mac "$outside")" ]
  called "^xcrun simctl launch --terminate-running-process $u com.antoinefink.meguri.dev -ApplePersistenceIgnoreState YES -MeguriFixture $(vm_fs meguri-mac "$dir")/vm-fixtures/1/deck.anki2\$"

  : >"$STUB_LOG"
  mg run --platform iphone --device "$u" -- -MeguriFixture "$SB/build/fixtures/missing"
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = fixture ]
  not_called '^xcrun simctl (install|launch) '
  assert_host_untouched
  validate "$result"
}

@test "a drive's fixtures go to the VM from launch.fixture and from -MeguriFixture in its arguments; one outside the checkout is named by a copy of the script" {
  local inside outside script dir
  inside="$SB/build/fixtures/recipes/sample-1"
  mkdir -p "$inside"
  echo collection >"$inside/collection.anki2"
  outside="$BATS_TEST_TMPDIR/elsewhere/deck.anki2"
  mkdir -p "$(dirname "$outside")"
  echo other >"$outside"
  script="$SB/build/fixture-drive.json"
  jq -n --arg o "$outside" '{name: "fixtures", steps: [
    {launch: {fixture: "build/fixtures/recipes/sample-1", route: "sample/sheet"}},
    {launch: {arguments: ["-MeguriFixture", $o]}}, {snapshot: "after"}]}' >"$script"
  mg drive "$script" --platform iphone
  [ "$(field .status)" = pass ]
  dir="$(dirname "$result")"
  diff -r "$inside" "$(vm_fs meguri-mac "$inside")"
  [ "$(cat "$(vm_fs meguri-mac "$dir/vm-fixtures/1/deck.anki2")")" = other ]
  called "^vm-fixture $inside\$"
  called "^vm-fixture $dir/vm-fixtures/1/deck.anki2\$"
  # The driver read a copy of the script naming the VM's path; the relative
  # fixture stays as it was.
  called "^vm-env TEST_RUNNER_MEGURI_DRIVE_SCRIPT=$dir/script-in-vm.json\$"
  [ "$(jq -r '"\(.steps[0].launch.fixture) \(.steps[1].launch.arguments[1])"' "$dir/script-in-vm.json")" = \
    "build/fixtures/recipes/sample-1 $dir/vm-fixtures/1/deck.anki2" ]

  # ui-tree and screenshot pass their raw launch arguments the same way.
  inside="$SB/build/fixtures/recipes/sample-2"
  mkdir -p "$inside"
  echo collection >"$inside/collection.anki2"
  : >"$STUB_LOG"
  mg ui-tree --route sample/sheet --platform iphone -- -MeguriFixture "$inside"
  [ "$(field .status)" = pass ]
  called "^vm-fixture $inside\$"

  # A fixture that is not there: the drive does not run.
  jq -n '{name: "missing", steps: [{launch: {fixture: "build/fixtures/none"}}]}' >"$script"
  : >"$STUB_LOG"
  mg drive "$script" --platform iphone
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = fixture ]
  not_called '^vm-xcodebuild '
  assert_host_untouched
}

@test "a cancelled xcode-check stops its simulator run in the VM before releasing the VM's lease" {
  # The checks script is a fake whose devices phase, as the real one does,
  # leases an iPhone from the pool under the job's lease, copies its test
  # products into the VM and starts a test run there.
  cat >"$SB/tools/tests/xcode_project_check.sh" <<'EOF'
#!/bin/bash
. "$(dirname "$0")/../scripts/mac-vm-lib.sh"
mkdir -p "$1/logs"
work="$(cd "$1" && pwd -P)"
shift
phase="" ssh=""
while [ "$#" -gt 0 ]; do
  case "$1" in --phase) phase="$2" && shift ;; --vm-ssh) ssh="$2" && shift ;; esac
  shift
done
[ "$phase" != builds ] || { echo "ok 1 builds"; exit 0; }
udid="$("$(dirname "$0")/../scripts/sim-pool.sh" acquire iphone --holder xcode-check)" || { echo "not ok 2 lease"; exit 1; }
p="$work/DerivedData-tests/Build/Products"
mkdir -p "$p/Debug-iphonesimulator/Meguri.app"
echo built >"$p/Debug-iphonesimulator/Meguri.app/Info.plist"
echo run >"$p/Meguri_UI_iphonesimulator27.0-arm64.xctestrun"
mvl_copy_in "$ssh" vm "$work" "$p" || { echo "not ok 2 copy"; exit 1; }
"$ssh" "xcodebuild test-without-building -xctestrun $(mvl_q "$p/Meguri_UI_iphonesimulator27.0-arm64.xctestrun") -destination $(mvl_q "id=$udid") -resultBundlePath $(mvl_q "$work/t.xcresult")" \
  </dev/null >>"$work/logs/iphone.log" 2>&1
echo "ok 2 iphone test"
EOF
  local job
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/never" FAKE_VM_IGNORE_TERM=1 "$SB/tools/mg" xcode-check \
    >"$BATS_TEST_TMPDIR/job.out" 2>"$BATS_TEST_TMPDIR/job.err" 3>&- &
  job=$!
  bg_track "$job"
  wait_for "$BATS_TEST_TMPDIR/never.started"
  vm_tests_running meguri-mac
  kill -TERM "$job"
  wait "$job" || true
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/job.out")"
  [ "$(field '[.failures[].id] | index("cancelled") != null')" = true ]
  # SIGTERM does not end it; SIGKILL does.
  called "^vm-pkill -f "
  called "^vm-pkill -9 -f "
  ! vm_tests_running meguri-mac
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
}

# --- the VM is missing or fails ------------------------------------------------------

# bats test_tags=gate
@test "a VM that does not answer queues simulator jobs with the mac-vm step and the reason, and no simulator boots on this Mac (U19c)" {
  touch "$FAKE_VM_DIR/ssh-down"
  MEGURI_MAC_VM_BOOT_SECONDS=2 mg a11y --platform iphone
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-vm the simulator runs" ]
  [[ "$(field '.details.queued_parts[0].reason')" == *"meguri-mac did not answer over SSH in a logged-in session within 2 s"*"Connection refused"* ]]
  [ "$(field '.user_steps[] | select(.id == "mac-vm") | "\(.state) \(.required)"')" = "missing true" ]
  validate "$result"
  # It queues before building: nothing more goes to the VM.
  not_called '^fake build'
  local args
  for args in "sim acquire iphone" "sim-setup" "run --platform iphone" "snapshots --platform iphone" "perf-ui" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --platform iphone"; do
    # shellcheck disable=SC2086
    MEGURI_MAC_VM_BOOT_SECONDS=2 mg $args
    [ "$status" -eq 0 ] || { echo "tools/mg $args exited $status"; return 1; }
    [ "$(field .details.queued)" = true ] || { echo "tools/mg $args: $(field -c '{status, message}')"; return 1; }
    [[ "$(field '.details.queued_parts[0].reason')" == *"did not answer over SSH"* ]]
  done
  # The pool script, called directly: exit 5 (queued), with the reason.
  MEGURI_MAC_VM_BOOT_SECONDS=2 run --separate-stderr "$SB/tools/scripts/sim-pool.sh" acquire iphone
  [ "$status" -eq 5 ]
  [[ "$stderr" == *"sim-pool: queued: "*"meguri-mac did not answer over SSH"* ]]
  MEGURI_MAC_VM_BOOT_SECONDS=2 run --separate-stderr "$SB/tools/scripts/sim-pool.sh" status
  [ "$status" -eq 5 ]
  [[ "$stderr" == *"sim-pool: queued: meguri-mac did not answer over SSH"* ]]
  assert_host_untouched
}

# bats test_tags=gate
@test "a pool whose simctl cannot fork in the VM queues the simulator job, not an error (U19c: U28's merge)" {
  FAKE_VM_SIMCTL_FORK_FAIL=1 mg a11y --platform iphone
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-vm the simulator runs" ]
  [ "$(field '.details.queued_parts[0].reason')" = "sim-pool.sh acquire iphone: the test VM meguri-mac could not be reached or could not start the pool's commands (exit 1: /Applications/Xcode.app/Contents/Developer/usr/bin/simctl: fork: Resource temporarily unavailable)" ]
  not_called '^vm-xcodebuild '
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]

  FAKE_VM_SIMCTL_FORK_FAIL=1 mg sim acquire iphone
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field .details.pool.exit)" = 5 ]
  FAKE_VM_SIMCTL_FORK_FAIL=1 run --separate-stderr "$SB/tools/scripts/sim-pool.sh" acquire iphone
  [ "$status" -eq 5 ]
  [[ "$stderr" == *"fork: Resource temporarily unavailable"* ]]
  assert_host_untouched
}

# bats test_tags=gate
@test "xcode-check with the VM near its process limit keeps its builds and Mac launches and queues its simulator runs; runs the VM drops mid-phase are queued by name (U19c)" {
  # The checks script is a fake: it logs its phases and arguments, and in its
  # devices phase lists what $FAKE_XCODE_CHECK_QUEUED names as a run the VM
  # could not take, as the real one does.
  cat >"$SB/tools/tests/xcode_project_check.sh" <<'FAKE'
#!/bin/bash
work="$1"
shift
mkdir -p "$work"
echo "fake xcode_project_check $*" >>"$STUB_LOG"
case " $* " in
  *" --phase builds "*) echo "ok 1 builds" ;;
  *)
    if [ -n "${FAKE_XCODE_CHECK_QUEUED:-}" ]; then
      printf '%s\t%s\n' "$FAKE_XCODE_CHECK_QUEUED" "meguri-mac refused the run's SSH session" >"$work/vm-queued.tsv"
      echo "ok 2 $FAKE_XCODE_CHECK_QUEUED # SKIP queued: meguri-mac refused the run's SSH session (user step mac-vm)"
    else
      echo "ok 2 devices"
    fi
    echo "1..2" ;;
esac
FAKE
  FAKE_VM_PROCS=2800 MEGURI_MAC_VM_PROCS_POLL_SECONDS=1 MEGURI_MAC_VM_PROCS_WAIT_SECONDS=1 mg xcode-check
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-vm the simulator runs" ]
  [[ "$(field .message)" == "xcode-check: passed without the simulator runs, which are queued: meguri-mac is near its process limit"*"(user step mac-vm)" ]]
  # The devices phase ran without simulators, under the host lease alone.
  called '^fake xcode_project_check --phase devices --continue-from 1 --no-sims$'
  [ "$(field .details.host_lease.job)" = xcode-check ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]

  : >"$STUB_LOG"
  FAKE_XCODE_CHECK_QUEUED=iphone-Debug-TestHooksTests mg xcode-check
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part): \(.reason)"')" = "mac-vm iphone-Debug-TestHooksTests: meguri-mac refused the run's SSH session" ]
  called '^fake xcode_project_check --phase devices --continue-from 1 --vm-ssh '
}

@test "without the VM, simulator jobs are queued with the mac-vm step; MEGURI_SIM_ALLOW_HOST=1 uses this Mac's pool" {
  rm -f "$MEGURI_MAC_VM_CONFIG"
  local args
  for args in "sim acquire iphone" "sim status" "sim-setup" "a11y --platform iphone" "run --platform iphone" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --platform iphone" "snapshots --platform iphone"; do
    # shellcheck disable=SC2086
    mg $args
    [ "$status" -eq 0 ] || { echo "tools/mg $args exited $status"; return 1; }
    [ "$(field .status)" = warn ]
    [ "$(field .details.queued)" = true ]
    [ "$(field '.user_steps[] | select(.id == "mac-vm") | "\(.state) \(.required)"')" = "missing true" ]
    [[ "$(field .message)" == *"queued"*"simulators run only in the test VM"* ]] || { echo "tools/mg $args: $(field .message)"; return 1; }
  done
  run --separate-stderr "$SB/tools/scripts/sim-pool.sh" acquire iphone
  [ "$status" -eq 5 ]
  [[ "$stderr" == *"queued"* ]]
  not_called '^fake build'
  not_called '^(tart|ssh) '
  assert_host_untouched

  # With MEGURI_SIM_ALLOW_HOST=1, this Mac's pool, as before U8c.
  ln -sf "$REPO_ROOT/tools/tests/sim_pool_fake_xcrun.sh" "$BIN/xcrun"
  export FAKE_SIM_DB="$BATS_TEST_TMPDIR/host-sims.json"
  MEGURI_SIM_ALLOW_HOST=1 mg sim acquire iphone
  [ "$(field .status)" = pass ]
  [ "$(field .details.sim_runner)" = host ]
  [ -f "$MEGURI_SIM_POOL_DIR/leases/$(field .details.udid)/lease.json" ]
  [ "$(jq -r '.devices[0].name' "$FAKE_SIM_DB")" = Meguri-Pool-iphone-1 ]
}

@test "a config that is not valid is an error for simulator jobs" {
  echo '{"vms": [' >"$MEGURI_MAC_VM_CONFIG"
  mg sim status
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = mac-vm-config ]
  run --separate-stderr "$SB/tools/scripts/sim-pool.sh" status
  [ "$status" -eq 6 ]
}

# --- provisioning ---------------------------------------------------------------------

@test "add-sim-runtime copies this Mac's pinned runtime image into the VM and adds it there, once" {
  export FAKE_SIM_NO_RT27=1
  local asset="$BATS_TEST_TMPDIR/assets/abc.asset/AssetData"
  mkdir -p "$asset/Restore/Firmware"
  echo image >"$asset/Restore/043-70362-657.dmg"
  echo info >"$asset/Restore/Firmware/043-70362-657.dmg.cryptex_info"
  stub "$BIN/xcrun" "
case \"\$*\" in
  'simctl runtime list -j') jq -n --arg p '$asset' '[{build: \"24A434\", runtimeIdentifier: \"com.apple.CoreSimulator.SimRuntime.iOS-27-0\", state: \"Ready\", path: \$p}]' ;;
  *) echo \"host-xcrun \$*\" >>\"\$STUB_LOG\"; exit 97 ;;
esac"
  run --separate-stderr "$SB/tools/vm/add-sim-runtime.sh"
  [ "$status" -eq 0 ] || { echo "$stderr"; return 1; }
  called "^xcrun simctl runtime add .*/runtime-import/com.apple.CoreSimulator.SimRuntime.iOS-27-0-24A434/AssetData/Restore/043-70362-657.dmg\$"
  [ "$(vm_sim meguri-mac list runtimes --json | jq -r '[.runtimes[].buildversion] | index("24A434") != null')" = true ]
  [[ "$stderr" == *"meguri-mac has the iOS 27.0 (24A434) simulator runtime"* ]]
  # The import directory is gone.
  [ -z "$(ls "$FAKE_VM_DIR/meguri-mac/fs"/*/runtime-import 2>/dev/null)" ] || [ -z "$(ls -A "$(find "$FAKE_VM_DIR" -name runtime-import -type d | head -n 1)")" ]

  : >"$STUB_LOG"
  run --separate-stderr "$SB/tools/vm/add-sim-runtime.sh"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"already has the iOS 27.0 (24A434) simulator runtime"* ]]
  not_called '^xcrun simctl runtime add'
}
