#!/usr/bin/env bats
# Self-tests for the agent build-run-verify loop (U8): sim-setup and sim-reset
# against the fake simctl, the host lease and session check, the Mac UI gate
# (host runs: MEGURI_MAC_UI_ALLOW_HOST=1 without a test VM; mac_vm.bats
# covers the VM), record, and the drive jobs' argument checks. Each test runs tools/mg from a
# throwaway copy of tools/ in its own git repo; macOS commands (simctl, ioreg,
# system_profiler, caffeinate, xcodebuild, osascript) are stubs, and the pool,
# lease and gate directories are per test.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
RT27=com.apple.CoreSimulator.SimRuntime.iOS-27-0

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
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
  export FAKE_SIM_DB="$BATS_TEST_TMPDIR/sims.json"
  export MEGURI_SIM_POOL_DIR="$BATS_TEST_TMPDIR/pool"
  export MEGURI_SIM_DEVICES_DIR="$BATS_TEST_TMPDIR/Devices"
  export MEGURI_SIM_POOL_SETTLE_SECONDS=0 MEGURI_SIM_POOL_POLL_SECONDS=1 MEGURI_SIM_POOL_WAIT_SECONDS=2
  export MEGURI_HOST_LEASE_DIR="$BATS_TEST_TMPDIR/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1
  export MEGURI_MAC_UI_DIR="$BATS_TEST_TMPDIR/mac-ui"
  # No test VM: these tests cover the host path of Mac UI and simulator jobs
  # (U8); tests/mac_vm.bats and tests/sim_vm.bats cover the VM (U8b, U8c).
  export MEGURI_MAC_VM_CONFIG="$BATS_TEST_TMPDIR/no-mac-vm.json" MEGURI_MAC_UI_ALLOW_HOST=1 MEGURI_SIM_ALLOW_HOST=1
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN" "$MEGURI_SIM_DEVICES_DIR"
  ln -s "$REPO_ROOT/tools/tests/sim_pool_fake_xcrun.sh" "$BIN/xcrun"
  stub ioreg 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes${STUB_LOCKED:+,\"CGSSessionScreenIsLocked\"=Yes})"'
  stub system_profiler 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\"${STUB_ASLEEP:+, \"spdisplays_asleep\": \"spdisplays_yes\"}}]}]}"'
  stub caffeinate 'echo "caffeinate $*" >>"$STUB_LOG"; exec sleep 600'
  stub xcodebuild 'echo "xcodebuild $*" >>"$STUB_LOG"; exit 70'
  stub osascript 'echo false'
  export PATH="$BIN:$PATH"
  POOL="$SB/tools/scripts/sim-pool.sh"
}

teardown() {
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  [ ! -f "$BATS_TEST_TMPDIR/bg.pids" ] || xargs kill <"$BATS_TEST_TMPDIR/bg.pids" 2>/dev/null || true
}

# bg_live: starts a process that lives a minute, a stand-in holder; sets
# BG_PID. teardown kills it.
bg_live() {
  sleep 60 >/dev/null 2>&1 3>&- &
  BG_PID=$!
  bg_track "$BG_PID"
}

# bg_track <pid>: teardown terminates it (a tools/mg run cancels its job), so
# a failed test leaves no job waiting for a file. Start such a process with
# 3>&-: bats waits for whatever holds its fd 3.
bg_track() { echo "$1" >>"$BATS_TEST_TMPDIR/bg.pids"; }

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }

# mg <args>: run the sandbox's tools/mg. Sets status, output, stderr and
# result (the result path, the last stdout line).
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
sim() { xcrun simctl "$@"; }
called() { grep -Eq "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq "$1" "$STUB_LOG" || { echo "unexpected call: $(grep -E "$1" "$STUB_LOG")"; return 1; }; }

# A pool device leased from the sandbox checkout.
lease_in_sandbox() { (cd "$SB" && "$POOL" acquire "$1" --holder test 2>/dev/null); }

add_fixture_jobs() {
  cat >"$SB/tools/mg.d/zz-fixture.sh" <<'EOF'
mg_job fx-lease host fx_lease "fixture: hold the host lease for FX_HOLD seconds, or until the file FX_UNTIL exists"
mg_job fx-gate host fx_gate "fixture: take the Mac UI gate"
fx_lease() {
  mg_host_lease || return 0
  echo "start $1 $(date +%s.%N)" >>"$FX_LOG"
  if [ -n "${FX_UNTIL:-}" ]; then
    while [ ! -e "$FX_UNTIL" ]; do sleep 0.1; done
  else
    sleep "${FX_HOLD:-1}"
  fi
  echo "end $1 $(date +%s.%N)" >>"$FX_LOG"
  mg_status pass
}
fx_gate() {
  mg_mac_ui_gate || return 0
  echo "allowed ${MEGURI_ALLOW_MAC_UI:-unset}" >>"$FX_LOG"
  mg_status pass
}
EOF
  export FX_LOG="$BATS_TEST_TMPDIR/fx.log"
  : >"$FX_LOG"
}

# mutex_hold: holds the host lease's mutex (tools/scripts/host-lease-mutex.sh)
# from a background process until mutex_free, as a job taking its nested slot
# or a recording releasing its lease would.
mutex_hold() {
  rm -f "$BATS_TEST_TMPDIR/mutex.held" "$BATS_TEST_TMPDIR/mutex.free"
  perl -MFcntl=:flock -MTime::HiRes=sleep -e '
    open(my $f, ">>", $ARGV[0]) or die "$ARGV[0]: $!\n";
    flock($f, LOCK_EX) or die "flock: $!\n";
    open(my $h, ">", $ARGV[1]) or die; close $h;
    sleep 0.05 until -e $ARGV[2];' \
    "$MEGURI_HOST_LEASE_DIR.mutex" "$BATS_TEST_TMPDIR/mutex.held" "$BATS_TEST_TMPDIR/mutex.free" >/dev/null 2>&1 3>&- &
  MUTEX_PID=$!
  for _ in $(seq 1 50); do [ -e "$BATS_TEST_TMPDIR/mutex.held" ] && return 0; sleep 0.1; done
  echo "the test could not take the mutex"
  return 1
}
mutex_free() { touch "$BATS_TEST_TMPDIR/mutex.free" && wait "$MUTEX_PID"; }

# waiting_for_mutex: whether some process waits for the lease's mutex
# (host_lease_lock's perl), within 5 s.
waiting_for_mutex() {
  for _ in $(seq 1 50); do
    pgrep -f 'timed out after 30 s' >/dev/null && return 0
    sleep 0.1
  done
  echo "nothing waits for the host lease's mutex"
  return 1
}

# Fakes for a drive: the build, the XCUITest run and its result bundle, and
# the image-tools container, as functions that replace the real ones (the
# fixture file sorts after drive.sh). The fake run writes a drive log of
# three steps: launch, a tap marked "dismiss-sheet", a snapshot; with
# MEGURI_DRIVE_FRAMES=1 the tap gets FX_FRAMES (5) sampled frames.
# FX_STEP1_STATUS and FX_STEP2_STATUS set the launch's and the tap's status,
# FX_SCRIPT_ERROR the log's error, and with FX_ISSUE the hierarchy check
# finds a control without an identifier in the first hierarchy. It lasts
# FX_DRIVE_SECONDS (0.6), or with FX_DRIVE_UNTIL=<file> it touches
# <file>.started and runs until <file> exists. The fake
# contact sheet fails with FX_SHEET=fail and leaves out the sheets with
# FX_SHEET=missing; it copies its window list to <out>/input.json.
add_drive_fakes() {
  cat >"$SB/tools/mg.d/zz-drive-fakes.sh" <<'EOF'
fx_now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }
fx_host() { case "$1" in /work/*) printf '%s\n' "$MG_ROOT/${1#/work/}" ;; *) printf '%s\n' "$1" ;; esac; }
mg_app_build() { echo "fake build $*" >>"$STUB_LOG"; }
mg_app_xcodebuild_test() {
  local prefix="$1" platform="$2" t0 t1 n=0
  echo "fake xcodebuild-test $platform frames=${TEST_RUNNER_MEGURI_DRIVE_FRAMES:-unset} ${*:5}" >>"$STUB_LOG"
  rm -rf "$prefix.xcresult"
  mkdir -p "$prefix.xcresult"
  t0="$(fx_now)"
  if [ -n "${FX_DRIVE_UNTIL:-}" ]; then
    : >"$FX_DRIVE_UNTIL.started"
    while [ ! -e "$FX_DRIVE_UNTIL" ]; do sleep 0.1; done
  else
    sleep "${FX_DRIVE_SECONDS:-0.6}"
  fi
  t1="$(fx_now)"
  [ "${TEST_RUNNER_MEGURI_DRIVE_FRAMES:-0}" != 1 ] || n="${FX_FRAMES:-5}"
  jq -n --argjson t0 "$t0" --argjson t1 "$t1" --argjson n "$n" --arg p "$platform" --arg s2 "${FX_STEP2_STATUS:-ok}" \
    --arg s1 "${FX_STEP1_STATUS:-ok}" --arg err "${FX_SCRIPT_ERROR:-}" '
    def cap($b; $ph): {phase: $ph, hierarchy: "\($b)-\($ph).json", screenshot: "\($b)-\($ph).png"};
    {script: "fixture", path: "/fixture.json", platform: $p, error: (if $err == "" then null else $err end), steps: [
      {index: 1, kind: "launch", transition: null, started: $t0, ended: ($t0 + 0.05), status: $s1, captures: [cap("drive-01-launch"; "after")]},
      ({index: 2, kind: "tap", transition: "dismiss-sheet", started: ($t0 + 0.1), ended: ($t1 - 0.1), status: $s2,
        captures: [cap("drive-02-tap"; "before"), cap("drive-02-tap"; "after")]}
       + (if $n > 0 then {frames: [range($n) | {file: "drive-02-tap-frame-\("00\(. + 1)" | .[-3:]).png", time: ($t0 + 0.1 + . * 0.1)}]} else {} end)),
      {index: 3, kind: "snapshot", transition: null, started: $t1, ended: $t1, status: "ok", captures: [{phase: "home", hierarchy: "drive-03-snapshot-home.json", screenshot: "drive-03-snapshot-home.png"}]}
    ]}' >"$prefix.xcresult/drive-log.json"
  return "${FX_DRIVE_RC:-0}"
}
mg_drive__export_attachments() {
  local log="$1/drive-log.json" dir="$2" n=0 entries='[]' name
  mkdir -p "$dir"
  # FX_EXPORT_DROP, FX_EXPORT_EMPTY, FX_EXPORT_LOST: attachment names left
  # out of the export, exported empty, or listed without their file.
  fx_add() {
    n=$((n + 1))
    case " ${FX_EXPORT_DROP:-} " in *" $1 "*) return 0 ;; esac
    case " ${FX_EXPORT_EMPTY:-} ${FX_EXPORT_LOST:-} " in
      *" $1 "*) case " ${FX_EXPORT_EMPTY:-} " in *" $1 "*) : >"$dir/e$n" ;; esac ;;
      *) printf '%s' "$2" >"$dir/e$n" ;;
    esac
    entries="$(jq -c --arg s "${1%.*}_0_0A1B2C3D-0000-4000-8000-00000000000$((n % 10)).${1##*.}" --arg e "e$n" '. + [{suggestedHumanReadableName: $s, exportedFileName: $e}]' <<<"$entries")"
  }
  for name in $(jq -r '.steps[].captures[] | .hierarchy, .screenshot' "$log"); do
    case "$name" in
      *.json) fx_add "$name" '{"schema": 1, "platform": "mac", "window": {"x": 100, "y": 50, "width": 800, "height": 600}, "root": {}}' ;;
      *) fx_add "$name" 'png' ;;
    esac
  done
  for name in $(jq -r '.steps[].frames[]?.file' "$log"); do fx_add "$name" 'frame'; done
  fx_add drive-log.json "$(cat "$log")"
  jq -n --argjson a "$entries" '[{attachments: $a}]' >"$dir/manifest.json"
}
mg_image_tools() {
  echo "fake image-tools $*" >>"$STUB_LOG"
  local script="$2" report="" out="" input="" files=() names
  shift 2
  case "$script" in
    */hierarchy_check.py)
      while [ "$#" -gt 0 ]; do
        case "$1" in --report) report="$2" && shift 2 ;; *) files[${#files[@]}]="$1" && shift ;; esac
      done
      printf '%s\n' "${files[@]}" | jq -R . | jq -s --arg issue "${FX_ISSUE:-}" '
        {files: (to_entries | map({path: .value, issues: (if $issue != "" and .key == 0 then [{kind: "missing-identifier",
          message: "button Done has no accessibility identifier", type: "button", identifier: "", label: "Done",
          frame: {x: 10, y: 20, width: 30, height: 40}, path: "0"}] else [] end)}))}
        | .counts = {files: (.files | length), issues: ([.files[].issues[]] | length)}' >"$(fx_host "$report")"
      ;;
    */contact_sheet.py)
      [ "${FX_SHEET:-ok}" != fail ] || { echo "contact-sheet: fake failure" >&2; return 2; }
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --out) out="$(fx_host "$2")" && shift 2 ;;
          --marks | --frame-list) input="$(fx_host "$2")" && shift 2 ;;
          *) shift ;;
        esac
      done
      cp "$input" "$out/input.json"
      names="$(jq -r '.windows[].name' "$input")"
      for name in $names; do
        [ "${FX_SHEET:-ok}" = missing ] || printf 'sheet' >"$out/$name.png"
      done
      jq --arg out "$out" '{video: null, jump_cuts: [], windows: [.windows[] | {name, sheet: "\($out)/\(.name).png", jump_cuts: [], stalls: []}]}' \
        "$input" >"$out/report.json"
      ;;
  esac
}
EOF
}

# --- sim-setup --------------------------------------------------------------

@test "sim-setup creates one pool device per pinned kind and reuses them on the next run" {
  mg sim-setup
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  local kind type u
  for kind in iphone iphone-small ipad ipad-small; do
    type="$(jq -r --arg k "$kind" '.devices[] | select(.kind == $k) | .device_type' "$SB/tools/sim-devices.json")"
    called "^xcrun simctl create Meguri-Pool-$kind-1 $type $RT27\$"
    [ "$(field --arg k "$kind" '.details.devices[$k].status')" = created ]
  done
  called "^xcrun simctl create Meguri-Pool-iphone-1 com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro $RT27\$"
  u="$(jq -r '.devices.iphone.udids[0]' "$SB/build/sim-devices.local.json")"
  [ "$(sim __state "$u")" = Booted ]
  [ ! -e "$MEGURI_SIM_POOL_DIR/leases/$u" ]
  [ "$(field '.details.foldable | "\(.status) \(.reason)"')" = "unavailable toolchain" ]
  [[ "$(field .message)" == *"foldable: unavailable: toolchain"* ]]

  : >"$STUB_LOG"
  mg sim-setup
  [ "$status" -eq 0 ]
  not_called "simctl create"
  [ "$(jq -r '.devices.iphone.udids[0]' "$SB/build/sim-devices.local.json")" = "$u" ]
  [ "$(field '.details.devices.iphone.status')" = present ]
}

@test "sim-setup fails with the expected and actual builds when the runtime differs from the pin" {
  FAKE_SIM_RT27_BUILD=24A999 mg sim-setup
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
  [[ "$(field '.failures[0].message')" == *"24A999"*"pins iOS 27.0 (24A434"* ]]
  [ "$(field '.details.runtime | "\(.expected.build) \(.actual.build)"')" = "24A434 24A999" ]
  not_called "simctl create"
}

@test "sim-setup fails when Xcode lacks a pinned device type" {
  jq '.devices[1].device_type = "com.apple.CoreSimulator.SimDeviceType.iPhone-99e"' "$SB/tools/sim-devices.json" >"$SB/x" && mv "$SB/x" "$SB/tools/sim-devices.json"
  mg sim-setup
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = device-type-missing ]
  [[ "$(field '.failures[0].message')" == *"iphone-small (com.apple.CoreSimulator.SimDeviceType.iPhone-99e)"* ]]
  not_called "simctl create"
}

@test "sim-setup adds the foldable lane once the toolchain offers a foldable iPhone" {
  FAKE_SIM_FOLDABLE=1 mg sim-setup --kinds foldable
  [ "$status" -eq 0 ]
  [ "$(field '.details.foldable.status')" = available ]
  called "^xcrun simctl create Meguri-Pool-foldable-1 com.apple.CoreSimulator.SimDeviceType.iPhone-Fold $RT27\$"
  not_called "simctl create Meguri-Pool-iphone"
}

@test "sim-setup --kinds limits the kinds and reports an unavailable foldable lane without failing" {
  mg sim-setup --kinds iphone,foldable
  [ "$status" -eq 0 ]
  called "simctl create Meguri-Pool-iphone-1 "
  not_called "simctl create Meguri-Pool-ipad"
  [ "$(field '.details.devices.foldable | "\(.status) \(.reason)"')" = "unavailable toolchain" ]
}

# --- sim-reset and the leased-device rule --------------------------------------

@test "sim-reset resets a simulator this checkout leased, keeps the lease and pins the status bar" {
  local u
  u="$(lease_in_sandbox iphone)"
  sim __app "$u" com.antoinefink.meguri.dev User
  : >"$STUB_LOG"
  mg sim-reset "$u"
  [ "$status" -eq 0 ]
  called "^xcrun simctl uninstall $u com.antoinefink.meguri.dev\$"
  called "^xcrun simctl status_bar $u override --time 9:41 .*--batteryState charged --batteryLevel 100\$"
  [ -f "$MEGURI_SIM_POOL_DIR/leases/$u/lease.json" ]
}

@test "sim-reset, run, drive, screenshot and record wait for the host lease, then check again that the device is still this checkout's" {
  add_fixture_jobs
  add_drive_fakes
  local u args n=0 holder job theirs
  u="$(lease_in_sandbox iphone)"
  for args in "sim-reset $u" "run --platform iphone --device $u --route sample/sheet" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --device $u" \
    "screenshot --route sample/sheet --device $u" "screenshot --device $u" "record start --device $u"; do
    n=$((n + 1))
    : >"$FX_LOG"
    FX_UNTIL="$BATS_TEST_TMPDIR/free-$n" "$SB/tools/mg" fx-lease holder >/dev/null 2>&1 3>&- &
    holder=$!
    bg_track "$holder"
    for _ in $(seq 1 50); do [ -s "$FX_LOG" ] && break; sleep 0.1; done
    # shellcheck disable=SC2086
    "$SB/tools/mg" $args >"$BATS_TEST_TMPDIR/job.out" 2>"$BATS_TEST_TMPDIR/job.err" 3>&- &
    job=$!
    bg_track "$job"
    for _ in $(seq 1 50); do grep -q 'host lease: waiting' "$BATS_TEST_TMPDIR/job.err" && break; sleep 0.1; done
    grep -q 'host lease: waiting for fx-lease' "$BATS_TEST_TMPDIR/job.err" || { echo "tools/mg $args did not queue"; return 1; }
    # While it queues, the device goes back to the pool and another checkout
    # leases it.
    "$POOL" release "$u" 2>/dev/null
    theirs="$(cd "$BATS_TEST_TMPDIR" && "$POOL" acquire iphone --holder other 2>/dev/null)"
    [ "$theirs" = "$u" ]
    sim __app "$u" com.example.theirs User
    : >"$STUB_LOG"
    touch "$BATS_TEST_TMPDIR/free-$n"
    wait "$holder"
    wait "$job" || true
    result="$(tail -n 1 "$BATS_TEST_TMPDIR/job.out")"
    [ "$(field '.failures[0].id')" = device ] || { echo "tools/mg $args: $(field '.failures')"; return 1; }
    [[ "$(field '.failures[0].message')" == *"leased to other in $BATS_TEST_TMPDIR"* ]]
    not_called "simctl (install|launch|terminate|uninstall|privacy|keychain|ui|status_bar|io) |fake xcodebuild-test"
    sim __apps "$u" | grep -qx com.example.theirs
    # The device comes back to this checkout for the next job.
    "$POOL" release "$u" 2>/dev/null
    [ "$(lease_in_sandbox iphone)" = "$u" ]
  done
  [ ! -e "$SB/build/mg/record/active/$u.json" ]
}

@test "a simulator leased from another checkout or outside the pool is refused" {
  local theirs mine
  theirs="$(cd "$BATS_TEST_TMPDIR" && "$POOL" acquire iphone --holder other 2>/dev/null)"
  mg sim-reset "$theirs"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = device ]
  [[ "$(field '.failures[0].message')" == *"not leased from this checkout"* ]]
  mine="$(sim __add "My iPhone" Booted)"
  mg sim-reset "$mine"
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"not a pool simulator"* ]]
  not_called "simctl uninstall"
}

@test "run and drive refuse another checkout's simulator before building, so its app keeps running" {
  local theirs
  theirs="$(cd "$BATS_TEST_TMPDIR" && "$POOL" acquire iphone --holder other 2>/dev/null)"
  mg run --platform iphone --device "$theirs" --route sample/sheet
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"not leased from this checkout (leased to other in $BATS_TEST_TMPDIR)"* ]]
  mg drive "$SB/tools/drive-scripts/examples/open-sample-sheet.json" --device "$theirs"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = device ]
  not_called "xcodebuild|simctl (install|launch|terminate|uninstall)"
}

# --- host lease ---------------------------------------------------------------

@test "a second job queues for the host lease until the first releases it" {
  add_fixture_jobs
  FX_HOLD=2 "$SB/tools/mg" fx-lease a >/dev/null 2>"$BATS_TEST_TMPDIR/a.err" &
  local first=$!
  for _ in $(seq 1 50); do [ -s "$FX_LOG" ] && break; sleep 0.1; done
  FX_HOLD=0 mg fx-lease b
  wait "$first"
  [ "$status" -eq 0 ]
  [ "$(awk '{print $1 $2}' "$FX_LOG" | tr '\n' ' ')" = "starta enda startb endb " ]
  [ "$(awk '$1 == "start" && $2 == "b" {print $3}' "$FX_LOG")" \> "$(awk '$1 == "end" && $2 == "a" {print $3}' "$FX_LOG")" ]
  [[ "$stderr" == *"host lease: waiting for fx-lease in $SB"* ]]
  [ "$(field '.details.host_lease.job')" = fx-lease ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  called "^caffeinate -d -i -w "
}

@test "two worktrees of one repository share one host lease, in the main checkout" {
  add_fixture_jobs
  unset MEGURI_HOST_LEASE_DIR
  local wt="$BATS_TEST_TMPDIR/wt" first
  git -C "$SB" worktree add -q -b wt "$wt"
  wt="$(cd "$wt" && pwd -P)"
  cp "$SB/tools/mg.d/zz-fixture.sh" "$wt/tools/mg.d/"
  FX_HOLD=2 "$wt/tools/mg" fx-lease a >/dev/null 2>&1 &
  first=$!
  for _ in $(seq 1 50); do [ -s "$FX_LOG" ] && break; sleep 0.1; done
  [ "$(jq -r '"\(.job) \(.worktree)"' "$SB/build/mg/host.lock/holder.json")" = "fx-lease $wt" ]
  [ ! -e "$wt/build/mg/host.lock" ]
  FX_HOLD=0 mg fx-lease b
  wait "$first"
  [ "$status" -eq 0 ]
  [ "$(awk '{print $1 $2}' "$FX_LOG" | tr '\n' ' ')" = "starta enda startb endb " ]
  [[ "$stderr" == *"host lease: waiting for fx-lease in $wt"* ]]
  [ ! -e "$SB/build/mg/host.lock" ]
}

# bats test_tags=gate
@test "a locked session queues the job (user step host-session), neither fail nor error, and frees the lease" {
  add_fixture_jobs
  STUB_LOCKED=1 mg fx-lease a
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field .details.blocked)" = host ]
  [ "$(field .details.host_session)" = locked ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part) \(.reason)"')" = "host-session fx-lease host (locked)" ]
  [ "$(field '.user_steps[0] | "\(.id) \(.state)"')" = "host-session missing" ]
  [[ "$(field .message)" == *"queued: host (locked)"*"(user step host-session)"* ]]
  [ ! -s "$FX_LOG" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "a sleeping display queues the job the same way" {
  add_fixture_jobs
  STUB_ASLEEP=1 mg fx-lease a
  [ "$status" -eq 0 ]
  [ "$(field .details.queued)" = true ]
  [ "$(field .details.host_session)" = display-asleep ]
  [ "$(field '.details.queued_parts[0].user_step')" = host-session ]
  [ ! -s "$FX_LOG" ]
}

@test "a lease left by a dead job is taken over; a live holder past the wait is an error" {
  add_fixture_jobs
  mkdir -p "$MEGURI_HOST_LEASE_DIR"
  sh -c 'exit 0' &
  local dead=$!
  wait "$dead"
  jq -n --argjson p "$dead" '{job: "gone", pid: $p, worktree: "/elsewhere", started_at: "then"}' >"$MEGURI_HOST_LEASE_DIR/holder.json"
  FX_HOLD=0 mg fx-lease a
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"removing a stale lease (gone in /elsewhere"* ]]

  mkdir -p "$MEGURI_HOST_LEASE_DIR"
  sleep 30 &
  local live=$!
  jq -n --argjson p "$live" '{job: "busy", pid: $p, worktree: "/elsewhere", started_at: "now"}' >"$MEGURI_HOST_LEASE_DIR/holder.json"
  MEGURI_HOST_LEASE_WAIT_SECONDS=1 mg fx-lease b
  kill "$live"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = host-busy ]
  [[ "$(field '.failures[0].message')" == *"busy in /elsewhere"* ]]
}

@test "a .dev Mac app the job did not start blocks Mac launches; other Meguri apps do not" {
  add_fixture_jobs
  cat >>"$SB/tools/mg.d/zz-fixture.sh" <<'EOF'
mg_job fx-mac host fx_mac "fixture: the Mac app guard"
fx_mac() { mg_app_mac_guard || return 0; mg_status pass; }
EOF
  local dev="$BATS_TEST_TMPDIR/other/Debug/Meguri.app" daily="/Applications/Meguri.app"
  stub ps "printf '  101 /sbin/launchd\n  202 $dev/Contents/MacOS/Meguri -ApplePersistenceIgnoreState YES\n  303 $daily/Contents/MacOS/Meguri\n'"
  stub plutil 'case "$*" in *"/other/Debug/Meguri.app/"*) echo com.antoinefink.meguri.dev ;; *) echo com.antoinefink.meguri ;; esac'
  mg fx-mac
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = mac-app-in-use ]
  [ "$(field .details.blocked)" = mac-app ]
  [[ "$(field '.failures[0].message')" == *"202 $dev"* ]]
  [[ "$(field '.failures[0].message')" != *"303"* ]]

  stub ps "printf '  303 $daily/Contents/MacOS/Meguri\n'"
  mg fx-mac
  [ "$status" -eq 0 ]
}

@test "xcode-check builds without the host lease, then queues for it before launches, simulators and tests" {
  add_fixture_jobs
  cat >>"$SB/tools/mg.d/zz-fixture.sh" <<'EOF'
mg_lint_ui_first() { return 0; }
mg__xcframework() { return 0; }
EOF
  # The project check logs each phase it runs and where its numbering starts.
  cat >"$SB/tools/tests/xcode_project_check.sh" <<'EOF'
#!/bin/bash
phase=all first=0
while [ "$#" -gt 0 ]; do
  case "$1" in --phase) phase="$2" && shift ;; --continue-from) first="$2" && shift ;; esac
  shift
done
echo "phase $phase from $first" >>"$FX_LOG"
if [ "$phase" = builds ]; then
  echo "ok 1 mac-Debug builds"
  if [ -n "${FX_BUILDS_PASS:-}" ]; then echo "ok 2 mac-Profile builds" && exit 0; fi
  echo "not ok 2 mac-Profile builds"
  exit 1
fi
echo "ok $((first + 1)) mac-Debug launched hidden stays up: Home"
echo "1..$((first + 1))"
EOF
  FX_HOLD=3 "$SB/tools/mg" fx-lease a >/dev/null 2>&1 &
  local first=$!
  for _ in $(seq 1 50); do [ -s "$FX_LOG" ] && break; sleep 0.1; done
  mg xcode-check
  wait "$first"
  [ "$status" -eq 1 ]
  [ "$(awk '{print $1 $2}' "$FX_LOG" | tr '\n' ' ')" = "starta phasebuilds enda phasedevices " ]
  [ "$(awk '$2 == "devices" {print $4}' "$FX_LOG")" = 2 ]
  [[ "$stderr" == *"host lease: waiting for fx-lease in $SB"* ]]
  [ "$(field '.details.host_lease.job')" = xcode-check ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed)"')" = "3 2 1" ]
  [ "$(field '.failures[0].id')" = "mac-Profile builds" ]

  # A locked Mac keeps the builds' results: a failing build is a failure.
  : >"$FX_LOG"
  STUB_LOCKED=1 mg xcode-check
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(cat "$FX_LOG")" = "phase builds from 0" ]
  [ "$(field '[.failures[].id] | sort | join(" ")')" = "mac-Profile builds" ]
  [ "$(field '.summary.total')" = 2 ]
  [ "$(field .details.queued)" = false ]
}

# bats test_tags=gate
@test "xcode-check whose builds pass while the Mac is locked is queued, naming host-session and the devices phase (U19b)" {
  add_fixture_jobs
  cat >>"$SB/tools/mg.d/zz-fixture.sh" <<'EOF'
mg_lint_ui_first() { return 0; }
mg__xcframework() { return 0; }
EOF
  cat >"$SB/tools/tests/xcode_project_check.sh" <<'EOF'
#!/bin/bash
echo "phase $*" >>"$FX_LOG"
echo "ok 1 mac-Debug builds"
echo "ok 2 mac-Profile builds"
EOF
  STUB_LOCKED=1 mg xcode-check
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(grep -c phase "$FX_LOG")" = 1 ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed)"')" = "2 2 0" ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '[.details.queued_parts[] | "\(.user_step): \(.part)"] | join("; ")')" = \
    "host-session: the devices phase (hidden Mac launches, simulators, test runs)" ]
  [ "$(field '.user_steps[] | select(.id == "host-session") | .state')" = missing ]
  [[ "$(field .message)" == *"the builds passed"*"queued"*"host-session"* ]]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

# --- Mac UI gate ----------------------------------------------------------------

@test "the Mac UI gate allows one run, then queues runs for an hour after its start" {
  add_fixture_jobs
  mg fx-gate
  [ "$status" -eq 0 ]
  [ "$(cat "$FX_LOG")" = "allowed 1" ]
  [ "$(jq -r .job "$MEGURI_MAC_UI_DIR/last-start.json")" = fx-gate ]
  [ ! -e "$MEGURI_MAC_UI_DIR/lock" ]

  mg fx-gate
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [[ "$(field .message)" == *"queued by the hourly Mac UI gate (runs on this Mac): the previous Mac UI run started at"* ]]
  [ "$(wc -l <"$FX_LOG" | tr -d ' ')" = 1 ]

  jq '.epoch -= 3601' "$MEGURI_MAC_UI_DIR/last-start.json" >"$BATS_TEST_TMPDIR/l" && mv "$BATS_TEST_TMPDIR/l" "$MEGURI_MAC_UI_DIR/last-start.json"
  mg fx-gate
  [ "$(field .status)" = pass ]
  [ "$(wc -l <"$FX_LOG" | tr -d ' ')" = 2 ]
}

@test "the Mac UI gate queues a run while another holds it, and frees a gate left by a dead run" {
  add_fixture_jobs
  mkdir -p "$MEGURI_MAC_UI_DIR/lock"
  sleep 30 &
  local live=$!
  jq -n --argjson p "$live" '{job: "mac-ui", pid: $p, worktree: "/main"}' >"$MEGURI_MAC_UI_DIR/lock/holder.json"
  mg fx-gate
  [ "$(field .status)" = warn ]
  [[ "$(field .message)" == *"another Mac UI run is in progress (mac-ui in /main)"* ]]
  kill "$live"
  wait "$live" 2>/dev/null || true
  mg fx-gate
  [ "$(field .status)" = pass ]
}

@test "drive and mac-ui on the Mac are queued without building while the gate is closed" {
  jq -n --argjson e "$(date +%s)" '{epoch: $e, started_at: "now", job: "mac-ui", worktree: "/main"}' >"$BATS_TEST_TMPDIR/last.json"
  mkdir -p "$MEGURI_MAC_UI_DIR" && mv "$BATS_TEST_TMPDIR/last.json" "$MEGURI_MAC_UI_DIR/last-start.json"
  mg drive "$SB/tools/drive-scripts/examples/open-sample-sheet.json" --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  mg mac-ui
  [ "$(field .status)" = warn ]
  [ "$(field '.details.suites | index("test-hooks") != null and index("agent-driver") != null')" = true ]
  [ "$(field '[.details.suites[] | select(startswith("drive:"))] | length')" -ge 1 ]
  not_called "xcodebuild"
}

# --- drive argument checks ------------------------------------------------------

@test "drive, ui-tree and screenshot reject bad arguments before building" {
  printf '{"steps": "no"}' >"$BATS_TEST_TMPDIR/bad.json"
  local args
  for args in "drive" "drive $BATS_TEST_TMPDIR/none.json" "drive $BATS_TEST_TMPDIR/bad.json" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --platform watch" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --platform mac --device X" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --config Release" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --route sample/sheet" \
    "ui-tree" "screenshot" "screenshot --route" "build-app" "build-app --platform tv" "record start" "record pause --device X"; do
    # shellcheck disable=SC2086
    mg $args
    [ "$status" -eq 2 ] || { echo "tools/mg $args exited $status"; return 1; }
    [ "$(field '.failures[0].id')" = usage ] || { echo "tools/mg $args: $(field '.failures[0]')"; return 1; }
  done
  not_called "xcodebuild"
}

# --- record ---------------------------------------------------------------------

@test "record start and stop record a leased simulator into the start run's directory" {
  local u video
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  video="$(field .details.recording.video)"
  [[ "$video" == "$SB/build/mg/record/"*/video.mp4 ]]
  called "^xcrun simctl io $u recordVideo --codec h264 --force $video\$"
  [ -f "$SB/build/mg/record/active/$u.json" ]
  mg record start --device "$u"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = recording ]

  mg record stop --device "$u"
  [ "$status" -eq 0 ]
  [ "$(field '.artifacts[] | select(.kind == "video") | .path')" = "$video" ]
  [ -s "$video" ]
  [ ! -e "$SB/build/mg/record/active/$u.json" ]
  [ -z "$(pgrep -f "simctl io $u recordVideo" || true)" ]

  mg record stop --device "$u"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = not-recording ]
}

# --- record: the host lease ------------------------------------------------------

@test "record start checks the session under the host lease: a locked Mac blocks it before recording" {
  local u
  u="$(lease_in_sandbox iphone)"
  STUB_LOCKED=1 mg record start --device "$u"
  [ "$status" -eq 0 ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.details.queued_parts[0].user_step')" = host-session ]
  [ "$(field .details.blocked)" = host ]
  not_called "recordVideo"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ ! -e "$SB/build/mg/record/active/$u.json" ]
}

@test "a recording holds the host lease until stop: this checkout's jobs run nested, other checkouts queue" {
  add_fixture_jobs
  local u worker other
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  worker="$(jq -r .pid "$SB/build/mg/record/active/$u.json")"
  kill -0 "$worker"
  [ "$(jq -r '"\(.pid) \(.kind) \(.udid) \(.worktree)"' "$MEGURI_HOST_LEASE_DIR/holder.json")" = "$worker recording $u $SB" ]
  called "^caffeinate -d -i -w $worker\$"

  FX_HOLD=0 mg fx-lease mine
  [ "$status" -eq 0 ]
  [ "$(field .details.host_lease.nested)" = true ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  [ "$(jq -r .pid "$MEGURI_HOST_LEASE_DIR/holder.json")" = "$worker" ]

  other="$BATS_TEST_TMPDIR/other"
  cp -R "$SB" "$other"
  run --separate-stderr env MEGURI_HOST_LEASE_WAIT_SECONDS=1 "$other/tools/mg" fx-lease theirs
  [ "$(jq -r '.failures[0].id' "${lines[${#lines[@]} - 1]}")" = host-busy ]
  [[ "$(jq -r '.failures[0].message' "${lines[${#lines[@]} - 1]}")" == *"record in $SB"* ]]
  [ "$(grep -c '^start theirs' "$FX_LOG")" = 0 ]

  mg record stop --device "$u"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.recording.reason)" = stopped ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  ! kill -0 "$worker" 2>/dev/null
  [ -z "$(pgrep -f "simctl io $u recordVideo" || true)" ]
}

@test "a nested job still running when the recording stops inherits the lease" {
  add_fixture_jobs
  local u job
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  FX_HOLD=3 "$SB/tools/mg" fx-lease nested >/dev/null 2>&1 &
  job=$!
  for _ in $(seq 1 50); do [ -s "$FX_LOG" ] && break; sleep 0.1; done
  [ -d "$MEGURI_HOST_LEASE_DIR/inner" ]
  mg record stop --device "$u"
  [ "$status" -eq 0 ]
  [ "$(jq -r .job "$MEGURI_HOST_LEASE_DIR/holder.json")" = fx-lease ]
  [ "$(jq -r '.kind // "none"' "$MEGURI_HOST_LEASE_DIR/holder.json")" = none ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  wait "$job"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "a recording worker that dies during a nested drive leaves the lease to the drive; another checkout waits for it" {
  add_fixture_jobs
  add_drive_fakes
  local u worker drive drive_pid other theirs go="$BATS_TEST_TMPDIR/drive.go"
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  worker="$(jq -r .pid "$SB/build/mg/record/active/$u.json")"
  FX_DRIVE_UNTIL="$go" "$SB/tools/mg" drive "$SB/tools/drive-scripts/examples/open-sample-sheet.json" --device "$u" \
    >"$BATS_TEST_TMPDIR/drive.out" 2>/dev/null 3>&- &
  drive=$!
  bg_track "$drive"
  for _ in $(seq 1 100); do [ -e "$go.started" ] && break; sleep 0.1; done
  [ -e "$go.started" ]
  drive_pid="$(jq -r .pid "$MEGURI_HOST_LEASE_DIR/inner/holder.json")"
  [ "$(jq -r .job "$MEGURI_HOST_LEASE_DIR/inner/holder.json")" = drive ]
  # The worker dies without releasing anything (SIGKILL to its group).
  kill -9 -- "-$worker"
  for _ in $(seq 1 50); do kill -0 "$worker" 2>/dev/null || break; sleep 0.1; done
  ! kill -0 "$worker" 2>/dev/null
  # Another checkout finds the recording's lease stale: the drive, still
  # running, inherits it, and the other checkout queues behind the drive.
  other="$BATS_TEST_TMPDIR/other"
  cp -R "$SB" "$other"
  "$other/tools/mg" fx-lease theirs >/dev/null 2>"$BATS_TEST_TMPDIR/theirs.err" 3>&- &
  theirs=$!
  bg_track "$theirs"
  for _ in $(seq 1 50); do grep -q 'host lease: waiting' "$BATS_TEST_TMPDIR/theirs.err" && break; sleep 0.1; done
  grep -q "host lease: waiting for drive in $SB" "$BATS_TEST_TMPDIR/theirs.err"
  [ "$(jq -r '"\(.pid) \(.job) \(.nested // "none") \(.kind // "none")"' "$MEGURI_HOST_LEASE_DIR/holder.json")" = "$drive_pid drive none none" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  [ ! -s "$FX_LOG" ]
  touch "$go"
  wait "$drive"
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/drive.out")")" = pass ]
  wait "$theirs"
  [ "$(awk '{print $1 $2}' "$FX_LOG" | tr '\n' ' ')" = "starttheirs endtheirs " ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "record stop of a dead recording worker leaves the lease to a nested job still running" {
  add_fixture_jobs
  local u worker job job_pid go="$BATS_TEST_TMPDIR/job.go"
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  worker="$(jq -r .pid "$SB/build/mg/record/active/$u.json")"
  FX_UNTIL="$go" "$SB/tools/mg" fx-lease nested >/dev/null 2>&1 3>&- &
  job=$!
  bg_track "$job"
  for _ in $(seq 1 50); do [ -s "$FX_LOG" ] && break; sleep 0.1; done
  job_pid="$(jq -r .pid "$MEGURI_HOST_LEASE_DIR/inner/holder.json")"
  kill -9 -- "-$worker"
  for _ in $(seq 1 50); do kill -0 "$worker" 2>/dev/null || break; sleep 0.1; done
  mg record stop --device "$u"
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"ended before record stop (the worker died)"* ]]
  [ "$(jq -r '"\(.pid) \(.job) \(.nested // "none")"' "$MEGURI_HOST_LEASE_DIR/holder.json")" = "$job_pid fx-lease none" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  touch "$go"
  wait "$job"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "a recording releases its lease under the mutex: a nested slot half-taken meanwhile inherits the lease" {
  local u worker status_file stop nested
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  worker="$(jq -r .pid "$SB/build/mg/record/active/$u.json")"
  status_file="$(jq -r .status "$SB/build/mg/record/active/$u.json")"
  # A job of this checkout between the mkdir of its nested slot and its
  # holder.json, holding the mutex.
  mutex_hold
  mkdir "$MEGURI_HOST_LEASE_DIR/inner"
  "$SB/tools/mg" record stop --device "$u" >"$BATS_TEST_TMPDIR/stop.out" 2>/dev/null &
  stop=$!
  for _ in $(seq 1 50); do [ -s "$status_file" ] && break; sleep 0.1; done
  [ -s "$status_file" ]
  waiting_for_mutex
  # The recording waits for the mutex: it neither released nor deleted the lease.
  [ -d "$MEGURI_HOST_LEASE_DIR/inner" ]
  [ "$(jq -r .pid "$MEGURI_HOST_LEASE_DIR/holder.json")" = "$worker" ]
  bg_live
  nested=$BG_PID
  jq -n --argjson p "$nested" --arg wt "$SB" '{job: "fx-lease", pid: $p, worktree: $wt, started_at: "now", nested: true}' \
    >"$MEGURI_HOST_LEASE_DIR/inner/holder.json"
  mutex_free
  wait "$stop"
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/stop.out")")" = pass ]
  [ "$(jq -r '"\(.pid) \(.job) \(.nested // "none")"' "$MEGURI_HOST_LEASE_DIR/holder.json")" = "$nested fx-lease none" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  kill "$nested"
}

@test "a nested job waiting for the mutex starts over when the recording's lease changed meanwhile" {
  add_fixture_jobs
  local u job other result_b
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  mutex_hold
  MEGURI_HOST_LEASE_WAIT_SECONDS=3 FX_HOLD=0 "$SB/tools/mg" fx-lease nested >"$BATS_TEST_TMPDIR/nested.out" 2>/dev/null &
  job=$!
  waiting_for_mutex
  [ ! -s "$FX_LOG" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  # Meanwhile the recording ended and another checkout took the lease.
  bg_live
  other=$BG_PID
  jq -n --argjson p "$other" '{job: "busy", pid: $p, worktree: "/elsewhere", started_at: "now"}' >"$MEGURI_HOST_LEASE_DIR/holder.json"
  mutex_free
  wait "$job" || true
  result_b="$(tail -n 1 "$BATS_TEST_TMPDIR/nested.out")"
  [ "$(jq -r '.failures[0].id' "$result_b")" = host-busy ]
  [[ "$(jq -r '.failures[0].message' "$result_b")" == *"busy in /elsewhere"* ]]
  [ ! -s "$FX_LOG" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR/inner" ]
  kill "$other"
  mg record stop --device "$u"
  [ "$status" -eq 0 ]
  [ "$(jq -r .job "$MEGURI_HOST_LEASE_DIR/holder.json")" = busy ]
}

@test "a recording stops itself at the limit and frees the lease; record stop then warns" {
  local u
  u="$(lease_in_sandbox iphone)"
  MEGURI_RECORD_MAX_SECONDS=1 mg record start --device "$u"
  [ "$status" -eq 0 ]
  for _ in $(seq 1 60); do [ -e "$MEGURI_HOST_LEASE_DIR" ] || break; sleep 0.1; done
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  mg record stop --device "$u"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [[ "$(field .message)" == *"stopped at the 1 s limit"* ]]
  [ -n "$(field '.artifacts[] | select(.kind == "video") | .path')" ]
}

@test "record stop signals no process that took over the pid of a worker stopped at the limit" {
  local u state live
  u="$(lease_in_sandbox iphone)"
  MEGURI_RECORD_MAX_SECONDS=1 mg record start --device "$u"
  [ "$status" -eq 0 ]
  state="$SB/build/mg/record/active/$u.json"
  for _ in $(seq 1 60); do [ -e "$MEGURI_HOST_LEASE_DIR" ] || break; sleep 0.1; done
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  # The worker exited; its pid now runs something else.
  bg_live
  live=$BG_PID
  jq --argjson p "$live" '.pid = $p' "$state" >"$state.tmp" && mv "$state.tmp" "$state"
  mg record stop --device "$u"
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  kill -0 "$live"
  kill "$live"
}

@test "record stop of a killed worker whose pid was reused signals nothing and frees the worker's slot" {
  local u state worker live
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  [ "$status" -eq 0 ]
  state="$SB/build/mg/record/active/$u.json"
  worker="$(jq -r .pid "$state")"
  # SIGKILL: no status file, and the slot stays; the worker leads its group.
  kill -9 -- "-$worker"
  for _ in $(seq 1 50); do kill -0 "$worker" 2>/dev/null || break; sleep 0.1; done
  [ -d "$MEGURI_HOST_LEASE_DIR" ]
  # The pid now runs something else; the slot names it, so it never looks stale.
  bg_live
  live=$BG_PID
  jq --argjson p "$live" '.pid = $p' "$state" >"$state.tmp" && mv "$state.tmp" "$state"
  jq --argjson p "$live" '.pid = $p' "$MEGURI_HOST_LEASE_DIR/holder.json" >"$BATS_TEST_TMPDIR/h" && mv "$BATS_TEST_TMPDIR/h" "$MEGURI_HOST_LEASE_DIR/holder.json"
  mg record stop --device "$u"
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"ended before record stop (the worker died)"* ]]
  kill -0 "$live"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  kill "$live"
}

@test "a recorder that fails on its own ends the recording; record stop reports it as an error" {
  local u
  u="$(lease_in_sandbox iphone)"
  FAKE_SIM_RECORD_EXIT_AFTER=0.5 mg record start --device "$u"
  [ "$status" -eq 0 ]
  for _ in $(seq 1 60); do [ -e "$MEGURI_HOST_LEASE_DIR" ] || break; sleep 0.1; done
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  mg record stop --device "$u"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = record ]
  [[ "$(field '.failures[0].message')" == *"ended before record stop (recorder-exited)"* ]]
}

@test "a terminated recording worker stops the recorder and frees the lease" {
  local u worker
  u="$(lease_in_sandbox iphone)"
  mg record start --device "$u"
  worker="$(jq -r .pid "$SB/build/mg/record/active/$u.json")"
  kill -TERM "$worker"
  for _ in $(seq 1 60); do kill -0 "$worker" 2>/dev/null || break; sleep 0.1; done
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ -z "$(pgrep -f "simctl io $u recordVideo" || true)" ]
}

@test "record takes simulators only and points the Mac to drive --record" {
  mg record start --platform mac
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"tools/mg drive <script> --platform mac --record"* ]]
}

# --- the runtime pin -----------------------------------------------------------

@test "drive, run, screenshot and record refuse a simulator on another runtime build before using it" {
  add_drive_fakes
  local u script="$SB/tools/drive-scripts/examples/open-sample-sheet.json"
  u="$(lease_in_sandbox ipad)"
  export FAKE_SIM_RT27_BUILD=24A999
  : >"$STUB_LOG"
  mg drive "$script" --platform ipad
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
  [[ "$(field '.failures[0].message')" == *"iOS 27.0 (24A999"*"pins iOS 27.0 (24A434"* ]]
  [ "$(field '.details.runtime | "\(.expected.build) \(.actual.build)"')" = "24A434 24A999" ]
  mg drive "$script" --device "$u"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
  mg run --platform iphone --route sample/sheet
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
  mg screenshot --device "$u"
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
  mg record start --device "$u"
  [ "$(field '.failures[0].id')" = runtime-mismatch ]
  not_called "simctl (install|launch) |status_bar [^ ]+ override|simctl io |fake xcodebuild-test"
  # run gives back the simulator it leased for the refused launch.
  [ -z "$("$POOL" status 2>/dev/null | jq -r --arg w "$SB" '.devices[] | select(.kind == "iphone" and .lease.worktree == $w) | .udid')" ]
}

@test "a failed status-bar override stops run, drive, screenshot, record and sim-reset with an error" {
  add_drive_fakes
  local u args leased
  u="$(lease_in_sandbox iphone)"
  export FAKE_SIM_STATUS_BAR_FAIL=1
  : >"$STUB_LOG"
  for args in "run --platform iphone --device $u --route sample/sheet" "run --platform iphone --route sample/sheet" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --device $u" "screenshot --device $u" \
    "record start --device $u" "sim-reset $u"; do
    # shellcheck disable=SC2086
    mg $args
    [ "$status" -eq 2 ] || { echo "tools/mg $args exited $status"; return 1; }
    [ "$(field '.failures[0].id')" = status-bar ] || { echo "tools/mg $args: $(field '.failures')"; return 1; }
    [[ "$(field '.failures[0].message')" == *"status_bar override failed on "* ]]
  done
  called "simctl status_bar $u override "
  not_called "simctl (install|launch) |simctl io |fake xcodebuild-test"
  [ ! -e "$SB/build/mg/record/active/$u.json" ]
  # run gives back the simulator it leased itself.
  leased="$("$POOL" status 2>/dev/null | jq -r --arg w "$SB" '[.devices[] | select(.lease.worktree == $w) | .udid] | join(" ")')"
  [ "$leased" = "$u" ]
}

# --- drive: capture results ------------------------------------------------------

@test "drive --record on a simulator records, makes the marked step's sheet and passes" {
  add_drive_fakes
  mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform iphone --record
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  called "xcrun simctl io .* recordVideo --codec h264 --force $SB/build/mg/drive/.*/video.mp4"
  called "fake xcodebuild-test ios frames=0"
  [ "$(field '.details.drive | "\(.capture) \(.motion.windows[0].name)"')" = "video 02-dismiss-sheet" ]
  [ "$(field '[.artifacts[] | select(.kind == "contact-sheet")] | length')" = 1 ]
  local dir
  dir="$(dirname "$result")"
  [ "$(jq -r '.windows[0] | "\(.name) \(.start >= 0) \(.end > .start)"' "$dir/motion/input.json")" = "02-dismiss-sheet true true" ]
  [ "$(jq -r .motion.windows[0].name "$dir/manifest.json")" = 02-dismiss-sheet ]
}

@test "drive --record is an error, never a pass, when the sheet, the recording or the recorder fails" {
  add_drive_fakes
  local script="$SB/tools/drive-scripts/examples/sample-transition.json"
  FX_SHEET=fail mg drive "$script" --platform iphone --record
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '[.failures[].id] | index("contact-sheet") != null')" = true ]
  [ "$(field '[.failures[].id] | index("contact-sheet/02-dismiss-sheet") != null')" = true ]
  [[ "$(field .message)" == *"a capture or check it asked for could not be made"* ]]

  FX_SHEET=missing mg drive "$script" --platform iphone --record
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = contact-sheet/02-dismiss-sheet ]

  FAKE_SIM_RECORD_EMPTY=1 mg drive "$script" --platform iphone --record
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = record ]
  [[ "$(field '.failures[0].message')" == *"the recording is empty"* ]]

  FAKE_SIM_RECORD_EXIT_AFTER=0.1 mg drive "$script" --platform iphone --record
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"stopped before the drive ended"* ]]
}

@test "a drive whose result bundle lacks a capture its log lists, or holds it empty, is an error, never a pass" {
  add_drive_fakes
  local script="$SB/tools/drive-scripts/examples/sample-transition.json"
  FX_EXPORT_DROP=drive-02-tap-after.png mg drive "$script" --platform iphone
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '[.failures[].id] | join(" ")')" = capture/02-tap-after.png ]
  [[ "$(field '.failures[0].message')" == *"lists drive-02-tap-after.png, but the result bundle had no such attachment"* ]]
  [ "$(jq -r .hierarchy_checked "$(dirname "$result")/manifest.json")" = true ]

  FX_EXPORT_EMPTY=drive-03-snapshot-home.json mg drive "$script" --platform iphone
  [ "$status" -eq 2 ]
  [ "$(field '[.failures[].id] | join(" ")')" = capture/03-snapshot-home.json ]
  [[ "$(field '.failures[0].message')" == *"lists drive-03-snapshot-home.json, but its export is empty"* ]]

  # No hierarchy at all: nothing was checked, so nothing passes.
  : >"$STUB_LOG"
  FX_EXPORT_DROP="drive-01-launch-after.json drive-02-tap-before.json drive-02-tap-after.json drive-03-snapshot-home.json" \
    mg drive "$script" --platform iphone
  [ "$status" -eq 2 ]
  [ "$(field '[.failures[].id | select(startswith("capture/"))] | length')" = 4 ]
  [ "$(jq -r .hierarchy_checked "$(dirname "$result")/manifest.json")" = false ]
  not_called "hierarchy_check.py"

  # The bundle's manifest lists an attachment whose file is missing.
  FX_EXPORT_LOST=drive-02-tap-before.png mg drive "$script" --platform iphone
  [ "$status" -eq 2 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "export/02-tap-before.png capture/02-tap-before.png" ]
}

@test "a sampled frame the drive log lists but the export lacks makes a Mac drive an error" {
  add_drive_fakes
  FX_EXPORT_DROP=drive-02-tap-frame-003.png mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac --record
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = capture/02-tap-frame-003.png ]
}

@test "a failed step in the drive log fails the drive even when XCTest passed" {
  add_drive_fakes
  FX_STEP2_STATUS=failed mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform iphone
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = drive/step-2-tap ]
}

@test "failed steps keep a drive with a capture error a fail" {
  add_drive_fakes
  FX_DRIVE_RC=1 FX_SHEET=fail mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform iphone --record
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
}

@test "drive --record on the Mac without Screen Recording makes sheets from the driver's frames and lists the user step" {
  add_drive_fakes
  mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac --record
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  called "fake xcodebuild-test mac frames=1"
  called "fake image-tools .*contact_sheet.py --frame-list /work/build/mg/drive/.*/frames.json"
  [ "$(field '.user_steps[] | select(.id == "screen-recording") | .state')" = missing ]
  [ "$(field '.details.drive | "\(.capture) \(.steps[1].frames)"')" = "frames 5" ]
  local dir
  dir="$(dirname "$result")"
  [ "$(jq -r '.windows[0].frames | length' "$dir/motion/input.json")" = 5 ]
  [ "$(jq -r '.windows[0].frames[0].path' "$dir/motion/input.json")" = "/work/build/mg/drive/$(basename "$dir")/frames/02-tap-frame-001.png" ]
  [ -f "$dir/frames/02-tap-frame-005.png" ]
  [ "$(field '[.artifacts[] | select(.path | contains("/frames/"))] | length')" = 0 ]
}

# bats test_tags=gate
@test "a Mac drive or mac-ui run whose synthesized input macOS dropped is queued (mac-input), neither fail nor error" {
  add_drive_fakes
  export MEGURI_MAC_UI_INTERVAL_MINUTES=0
  # The unified log of testmanagerd when WindowServer refuses its input.
  stub log 'echo "log $*" >>"$STUB_LOG"; [ -z "${STUB_INPUT_DROPPED:-}" ] || echo "2026-10-14 08:21:19.336 E  testmanagerd[21945:bfc28d] [com.apple.dt.xctest:GesturePerformer] Timed out after waiting 5.0s for Recap playback, implicitly confirming."'
  STUB_INPUT_DROPPED=1 FX_STEP2_STATUS=failed mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  # The failed step is the dropped input's doing, not the app's.
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field .details.blocked)" = mac-input ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-input sample-transition.json" ]
  [[ "$(field '.details.queued_parts[0].repro')" == "log show --start "* ]]
  [ "$(field '.user_steps[] | select(.id == "mac-input") | .state')" = missing ]
  [[ "$(field .message)" == *"queued: macOS dropped the input"* ]]
  [ -f "$(dirname "$result")/manifest.json" ]
  called "^log show --style compact --start [0-9]{4}-[0-9]{2}-[0-9]{2} [0-9:]{8} --predicate process == \"testmanagerd\" AND eventMessage CONTAINS \"Recap playback\"\$"

  # The same failed step with the input delivered is a failure of the app.
  FX_STEP2_STATUS=failed mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac
  [ "$status" -eq 1 ]
  [ -z "$(field '.user_steps[] | select(.id == "mac-input") | .id')" ]

  STUB_INPUT_DROPPED=1 mg mac-ui --suite test-hooks
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '[.details.queued_parts[] | "\(.user_step) \(.part)"] | join(", ")')" = "mac-input test-hooks" ]
  [[ "$(field .message)" == *"mac-ui: 0 passed; 1 of 1 suite(s) queued"*"test-hooks (user step mac-input)"* ]]
}

# bats test_tags=gate
@test "a Mac drive whose input macOS dropped still fails or errs on what the drop cannot explain" {
  add_drive_fakes
  export MEGURI_MAC_UI_INTERVAL_MINUTES=0
  local drive="$SB/tools/drive-scripts/examples/sample-transition.json"
  stub log '[ -z "${STUB_INPUT_DROPPED:-}" ] || echo "2026-10-14 08:21:19.336 E  testmanagerd[21945:bfc28d] [com.apple.dt.xctest:GesturePerformer] Timed out after waiting 5.0s for Recap playback, implicitly confirming."'
  export STUB_INPUT_DROPPED=1 FX_STEP2_STATUS=failed

  # The launch failed before any input: the app's failure. The tap, whose
  # input was dropped, is not recorded.
  FX_STEP1_STATUS=failed FX_DRIVE_RC=1 mg drive "$drive" --platform mac
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(" ")')" = drive/step-1-launch ]
  [ "$(field .details.queued)" = false ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-input sample-transition.json" ]
  [ "$(field '.user_steps[] | select(.id == "mac-input") | .state')" = missing ]
  [[ "$(field .message)" == *"(user step mac-input)"* ]]

  # A hierarchy issue: the captures show the app as it was.
  FX_ISSUE=1 mg drive "$drive" --platform mac
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = drive/missing-identifier/Done ]
  [ "$(field .details.queued)" = false ]

  # The script's error.
  FX_SCRIPT_ERROR="unknown step kind 'hover'" mg drive "$drive" --platform mac
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = drive/script ]

  # A capture the drive log lists came out empty: an error, not queued.
  FX_EXPORT_EMPTY=drive-03-snapshot-home.png mg drive "$drive" --platform mac
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '[.failures[].id] | join(" ")')" = capture/03-snapshot-home.png ]
  [ "$(field .details.queued)" = false ]

  # mac-ui runs the drive as a suite: the same issue fails the suite.
  FX_ISSUE=1 mg mac-ui --suite "drive:$drive"
  [ "$status" -eq 1 ]
  [ "$(field '.summary | "\(.failed) \(.queued)"')" = "1 0" ]
  [ "$(field .details.queued)" = false ]
}

@test "mac-ui runs the agent driver's resize test as a suite of its own" {
  add_drive_fakes
  mg mac-ui --suite agent-driver
  [ "$status" -eq 0 ]
  [ "$(field '.summary | "\(.total) \(.passed)"')" = "1 1" ]
  called "^fake xcodebuild-test mac frames=unset -only-testing:MeguriUITests/AgentDriverTests/testResizeKeepsTheRunningAppAndItsState\$"
  mg mac-ui --suite resize
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"(test-hooks, agent-driver, a11y, drive:<script>)"* ]]
}

@test "too few frames for a marked Mac step is an error" {
  add_drive_fakes
  FX_FRAMES=1 mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac --record
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = frames/02-dismiss-sheet ]
}

@test "drive --record on the Mac with Screen Recording records the display, keeps only the window's cuts and deletes the movie" {
  add_drive_fakes
  export MEGURI_MAC_UI_INTERVAL_MINUTES=0
  stub osascript 'case "$*" in *CGPreflightScreenCaptureAccess*) echo true ;; *NSScreen*) echo "{\"width\": 1512, \"height\": 982}" ;; *) echo "" ;; esac'
  stub screencapture 'echo "screencapture $*" >>"$STUB_LOG"; for f; do :; done; IFS= read -r -n 1 c; if [ "${STUB_SCREEN_EMPTY:-}" = 1 ]; then : >"$f"; else printf movie >"$f"; fi; echo "screencapture stopped by ${c:-eof}" >>"$STUB_LOG"'
  mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac --record
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  called "^screencapture -v -x -D1 -V3600 $SB/build/mg/drive/.*/screen.mov\$"
  called "^screencapture stopped by q\$"
  called "fake xcodebuild-test mac frames=0"
  local dir
  dir="$(dirname "$result")"
  [ "$(jq -c '.windows[0].crop' "$dir/motion/input.json")" = '{"x":100,"y":50,"width":800,"height":600,"display_width":1512,"display_height":982}' ]
  [ ! -e "$dir/screen.mov" ]
  [ "$(jq -r '"\(.capture) \(.video)"' "$dir/manifest.json")" = "screen null" ]
  [ -z "$(field '.user_steps[] | select(.id == "screen-recording") | .id')" ]

  STUB_SCREEN_EMPTY=1 mg drive "$SB/tools/drive-scripts/examples/sample-transition.json" --platform mac --record
  [ "$status" -eq 2 ]
  [[ "$(field '.failures[0].message')" == *"the recording is empty"* ]]
}
