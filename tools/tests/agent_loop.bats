#!/usr/bin/env bats
# Self-tests for the agent build-run-verify loop (U8): sim-setup and sim-reset
# against the fake simctl, the host lease and session check, the Mac UI gate,
# record, and the drive jobs' argument checks. Each test runs tools/mg from a
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
}

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
mg_job fx-lease host fx_lease "fixture: hold the host lease for FX_HOLD seconds"
mg_job fx-gate host fx_gate "fixture: take the Mac UI gate"
fx_lease() {
  mg_host_lease || return 0
  echo "start $1 $(date +%s.%N)" >>"$FX_LOG"
  sleep "${FX_HOLD:-1}"
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

@test "a locked session makes the job report blocked: host, not fail, and frees the lease" {
  add_fixture_jobs
  STUB_LOCKED=1 mg fx-lease a
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = blocked-host ]
  [ "$(field .details.blocked)" = host ]
  [ "$(field .details.host_session)" = locked ]
  [ "$(field '.user_steps[0].id')" = host-session ]
  [[ "$(field .message)" == *"blocked: host (locked)"* ]]
  [ ! -s "$FX_LOG" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "a sleeping display blocks the job the same way" {
  add_fixture_jobs
  STUB_ASLEEP=1 mg fx-lease a
  [ "$status" -eq 2 ]
  [ "$(field .details.host_session)" = display-asleep ]
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
  [[ "$(field .message)" == *"queued for the hourly Mac UI sweep: the previous Mac UI run started at"* ]]
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
  [ "$(field '.details.suites | index("test-hooks") != null')" = true ]
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
