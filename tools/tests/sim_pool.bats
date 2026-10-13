#!/usr/bin/env bats
# Self-tests for the shared simulator pool, tools/scripts/sim-pool.sh,
# against a fake `xcrun simctl` (sim_pool_fake_xcrun.sh).
# No real simulator is involved; the pool and device directories are
# per-test temporary directories.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
IPHONE_TYPE=com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro
IPAD_TYPE=com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB
RT27=com.apple.CoreSimulator.SimRuntime.iOS-27-0
RT18=com.apple.CoreSimulator.SimRuntime.iOS-18-2

setup() {
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  export FAKE_SIM_DB="$BATS_TEST_TMPDIR/sims.json"
  export MEGURI_SIM_POOL_DIR="$BATS_TEST_TMPDIR/pool"
  export MEGURI_SIM_DEVICES_DIR="$BATS_TEST_TMPDIR/Devices"
  export MEGURI_SIM_POOL_SETTLE_SECONDS=0 MEGURI_SIM_POOL_POLL_SECONDS=1 MEGURI_SIM_POOL_WAIT_SECONDS=2
  : >"$STUB_LOG"
  mkdir -p "$BATS_TEST_TMPDIR/bin" "$MEGURI_SIM_DEVICES_DIR"
  ln -s "$REPO_ROOT/tools/tests/sim_pool_fake_xcrun.sh" "$BATS_TEST_TMPDIR/bin/xcrun"
  export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
  POOL="$REPO_ROOT/tools/scripts/sim-pool.sh"
  LEASES="$MEGURI_SIM_POOL_DIR/leases"
}

# pool <args>: run sim-pool.sh. Sets status, output (stdout), stderr, lines.
pool() { run --separate-stderr "$POOL" "$@"; }

sim() { xcrun simctl "$@"; } # the fake's test-only commands are not logged

called() { grep -Eq "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
not_called() {
  if grep -Eq "$1" "$STUB_LOG"; then
    echo "unexpected call matching: $1"
    grep -E "$1" "$STUB_LOG"
    return 1
  fi
}
count_calls() { grep -Ec "$1" "$STUB_LOG" || true; }

lease() { jq -r "$2" "$LEASES/$1/lease.json"; }

age() { touch -t 202601010000 "$MEGURI_SIM_DEVICES_DIR/$1"; } # make a device directory old

# Pool devices are never shut down, erased or deleted.
assert_pool_devices_untouched() {
  local u
  for u in $(jq -r '.devices[] | select(.name | startswith("Meguri-Pool-")) | .udid' "$FAKE_SIM_DB"); do
    not_called "^xcrun simctl (shutdown|erase|delete) $u"
  done
}

# --- acquire ----------------------------------------------------------------

@test "acquire creates a pool iPhone on the newest iOS runtime, boots it and prints only its UDID" {
  pool acquire iphone --holder t1
  [ "$status" -eq 0 ]
  [ "${#lines[@]}" -eq 1 ]
  local u="${lines[0]}"
  called "^xcrun simctl create Meguri-Pool-iphone-1 $IPHONE_TYPE $RT27\$"
  called "^xcrun simctl boot $u\$"
  called "^xcrun simctl bootstatus $u -b\$"
  [ "$(sim __state "$u")" = Booted ]
  [ "$(lease "$u" .udid)" = "$u" ]
  [ "$(lease "$u" .kind)" = iphone ]
  [ "$(lease "$u" .holder)" = t1 ]
  [ "$(lease "$u" .cwd)" = "$(pwd -P)" ]
  [ "$(lease "$u" '.pid | type')" = number ]
  [ "$(lease "$u" '.expires_epoch - .acquired_epoch')" -eq 3600 ]
  [[ "$(lease "$u" .expires_at)" =~ ^20[0-9-]{8}T[0-9:]{8}Z$ ]]
  [[ "$stderr" == *"leased Meguri-Pool-iphone-1"* ]]
  assert_pool_devices_untouched
}

@test "acquire ipad uses the iPad Pro 13-inch (M5) device type" {
  pool acquire ipad --ttl-min 5
  [ "$status" -eq 0 ]
  called "^xcrun simctl create Meguri-Pool-ipad-1 $IPAD_TYPE $RT27\$"
  [ "$(lease "${lines[0]}" '.expires_epoch - .acquired_epoch')" -eq 300 ]
}

@test "a released device is reset, stays booted, and the next acquire reuses it without a reboot" {
  pool acquire iphone
  local u="${lines[0]}"
  sim __app "$u" com.example.meguri User
  sim __app "$u" com.example.meguri.tests.xctrunner User
  pool release "$u"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
  called "^xcrun simctl uninstall $u com\.example\.meguri\$"
  called "^xcrun simctl uninstall $u com\.example\.meguri\.tests\.xctrunner\$"
  not_called "^xcrun simctl uninstall $u com\.apple\."
  called "^xcrun simctl privacy $u reset all\$"
  called "^xcrun simctl keychain $u reset\$"
  called "^xcrun simctl ui $u appearance light\$"
  called "^xcrun simctl ui $u content_size large\$"
  called "^xcrun simctl status_bar $u clear\$"
  [ "$(sim __apps "$u" | tr '\n' ' ')" = "com.apple.Preferences com.apple.mobilesafari " ]
  [ "$(sim __state "$u")" = Booted ]
  [ ! -e "$LEASES/$u" ]
  assert_pool_devices_untouched

  : >"$STUB_LOG"
  pool acquire iphone
  [ "$status" -eq 0 ]
  [ "${lines[0]}" = "$u" ]
  not_called '^xcrun simctl (create|boot) '
  [ -d "$LEASES/$u" ]
}

@test "acquire prefers a booted free pool device and ignores pool devices on older runtimes" {
  local old shut booted
  old="$(sim __add Meguri-Pool-iphone-1 Booted "$IPHONE_TYPE" "$RT18")"
  shut="$(sim __add Meguri-Pool-iphone-2 Shutdown)"
  booted="$(sim __add Meguri-Pool-iphone-3 Booted)"
  pool acquire iphone
  [ "${lines[0]}" = "$booted" ]
  pool acquire iphone
  [ "${lines[0]}" = "$shut" ]
  called "^xcrun simctl boot $shut\$"
  # The 18.2 device does not count toward the cap and keeps its name.
  pool acquire iphone
  [ "$status" -eq 0 ]
  called "^xcrun simctl create Meguri-Pool-iphone-4 $IPHONE_TYPE $RT27\$"
  [ "${lines[0]}" != "$old" ]
  [ ! -e "$LEASES/$old" ]
}

@test "the pool grows to three devices per kind, then acquire waits and times out" {
  local a b c
  pool acquire iphone; a="${lines[0]}"
  pool acquire iphone; b="${lines[0]}"
  pool acquire iphone; c="${lines[0]}"
  [ "$a" != "$b" ] && [ "$b" != "$c" ] && [ "$a" != "$c" ]
  [ "$(count_calls '^xcrun simctl create ')" -eq 3 ]
  local start=$SECONDS
  pool acquire iphone
  [ "$status" -eq 3 ]
  [ -z "$output" ]
  [ $((SECONDS - start)) -ge 2 ]
  [[ "$stderr" == *"waiting up to 2 s"* ]]
  [ "$(count_calls '^xcrun simctl create ')" -eq 3 ]
  # Another kind has its own cap.
  pool acquire ipad
  [ "$status" -eq 0 ]
}

@test "a waiting acquire gets the device released meanwhile" {
  export MEGURI_SIM_POOL_MAX=1 MEGURI_SIM_POOL_WAIT_SECONDS=30
  pool acquire iphone
  local u="${lines[0]}"
  (sleep 2 && "$POOL" release "$u" 2>/dev/null) 3>&- &
  local start=$SECONDS
  pool acquire iphone --holder second
  wait
  [ "$status" -eq 0 ]
  [ "${lines[0]}" = "$u" ]
  [ $((SECONDS - start)) -lt 25 ]
  [ "$(lease "$u" .holder)" = second ]
}

@test "an expired lease is taken over and the device reset before it is handed out" {
  export MEGURI_SIM_POOL_MAX=1
  pool acquire iphone --holder old
  local u="${lines[0]}"
  sim __app "$u" com.example.left.behind User
  jq '.expires_epoch = 1' "$LEASES/$u/lease.json" >"$BATS_TEST_TMPDIR/l" && mv "$BATS_TEST_TMPDIR/l" "$LEASES/$u/lease.json"
  pool acquire iphone --holder new
  [ "$status" -eq 0 ]
  [ "${lines[0]}" = "$u" ]
  [[ "$stderr" == *"taking over the expired lease"* ]]
  called "^xcrun simctl uninstall $u com\.example\.left\.behind\$"
  called "^xcrun simctl privacy $u reset all\$"
  [ "$(lease "$u" .holder)" = new ]
  [ "$(count_calls '^xcrun simctl create ')" -eq 1 ]
  assert_pool_devices_untouched
}

@test "parallel acquires never share a device or create past the cap" {
  export MEGURI_SIM_POOL_WAIT_SECONDS=3
  local i
  for i in 1 2 3 4; do
    "$POOL" acquire iphone --holder "p$i" >"$BATS_TEST_TMPDIR/out$i" 2>"$BATS_TEST_TMPDIR/err$i" 3>&- &
  done
  wait || true
  cat "$BATS_TEST_TMPDIR"/out* | sort >"$BATS_TEST_TMPDIR/udids"
  [ "$(wc -l <"$BATS_TEST_TMPDIR/udids")" -eq 3 ]
  [ "$(sort -u "$BATS_TEST_TMPDIR/udids" | wc -l)" -eq 3 ]
  [ "$(count_calls '^xcrun simctl create ')" -eq 3 ]
  [ "$(ls "$LEASES" | wc -l)" -eq 3 ]
  [ ! -e "$MEGURI_SIM_POOL_DIR/pool.lock" ]
}

@test "a pool lock left by a dead process is removed" {
  mkdir -p "$MEGURI_SIM_POOL_DIR/pool.lock"
  sh -c 'exit 0' &
  local dead=$!
  wait "$dead"
  echo "$dead" >"$MEGURI_SIM_POOL_DIR/pool.lock/pid"
  pool acquire iphone
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"stale pool lock"* ]]
}

@test "a cancelled acquire drops the lease it had not handed out" {
  export MEGURI_SIM_POOL_SETTLE_SECONDS=30
  "$POOL" acquire iphone >"$BATS_TEST_TMPDIR/out" 2>"$BATS_TEST_TMPDIR/err" 3>&- &
  local pid=$!
  for _ in $(seq 100); do
    grep -q settle "$BATS_TEST_TMPDIR/err" && break
    sleep 0.1
  done
  grep -q settle "$BATS_TEST_TMPDIR/err"
  [ "$(ls "$LEASES" | wc -l)" -eq 1 ]
  kill -TERM "$pid"
  local rc=0
  wait "$pid" || rc=$?
  [ "$rc" -eq 143 ]
  [ ! -s "$BATS_TEST_TMPDIR/out" ]
  [ "$(ls "$LEASES" | wc -l)" -eq 0 ]
}

# --- renew, release, status -------------------------------------------------

@test "renew extends the lease" {
  pool acquire iphone --ttl-min 1
  local u="${lines[0]}"
  pool renew "$u" --ttl-min 90
  [ "$status" -eq 0 ]
  [ "$(lease "$u" '.expires_epoch - (now | floor) > 5300')" = true ]
  [ "$(lease "$u" .ttl_min)" = 90 ]
  [ "$(lease "$u" '.renewed_at | type')" = string ]
}

@test "release and renew refuse simulators outside the pool and unleased devices" {
  local own
  own="$(sim __add "iPhone 18 Pro" Booted)"
  sim __app "$own" com.antoine.app User
  pool release "$own"
  [ "$status" -eq 1 ]
  [[ "$stderr" == *"not a pool device"* ]]
  pool renew "$own"
  [ "$status" -eq 1 ]
  pool release 11111111-2222-3333-4444-555555555555
  [ "$status" -eq 1 ]
  not_called '^xcrun simctl (uninstall|privacy|keychain|ui|status_bar|boot|shutdown|erase|delete) '
  [ "$(sim __apps "$own" | grep -c com.antoine.app)" -eq 1 ]
  local free
  free="$(sim __add Meguri-Pool-iphone-1 Booted)"
  pool renew "$free"
  [ "$status" -eq 1 ]
  pool release "$free"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"not leased"* ]]
  not_called "^xcrun simctl (uninstall|privacy) $free"
}

@test "release boots a pool device found shut down, and never shuts one down" {
  pool acquire iphone
  local u="${lines[0]}"
  sim shutdown "$u"
  : >"$STUB_LOG"
  pool release "$u"
  [ "$status" -eq 0 ]
  called "^xcrun simctl boot $u\$"
  [ "$(sim __state "$u")" = Booted ]
  called "^xcrun simctl privacy $u reset all\$"
  assert_pool_devices_untouched
}

@test "status prints the pool and its leases as JSON" {
  sim __add "iPhone 18 Pro" Booted >/dev/null
  pool acquire iphone --holder s1
  local u="${lines[0]}"
  sim __add Meguri-Pool-ipad-1 Shutdown >/dev/null
  pool status
  [ "$status" -eq 0 ]
  jq -e . <<<"$output" >/dev/null
  [ "$(jq -r '.runtime.version' <<<"$output")" = 27.0 ]
  [ "$(jq -r '.devices | length' <<<"$output")" -eq 2 ]
  [ "$(jq -r --arg u "$u" '.devices[] | select(.udid == $u) | "\(.kind) \(.leased) \(.lease.holder) \(.lease_expired) \(.current_runtime)"' <<<"$output")" = "iphone true s1 false true" ]
  [ "$(jq -r '.devices[] | select(.name == "Meguri-Pool-ipad-1") | "\(.kind) \(.leased) \(.state)"' <<<"$output")" = "ipad false Shutdown" ]
  [ "$(jq -r '[.kinds[].kind] | join(" ")' <<<"$output")" = "iphone ipad" ]
  [[ "$stderr" == *"Meguri-Pool-iphone-1  $u  Booted  leased to s1"* ]]
}

@test "usage errors exit 2 without touching a simulator" {
  pool acquire watch
  [ "$status" -eq 2 ]
  [[ "$stderr" == *"unknown kind 'watch' (kinds: iphone ipad)"* ]]
  pool acquire
  [ "$status" -eq 2 ]
  pool acquire iphone --ttl-min 0
  [ "$status" -eq 2 ]
  pool gc --older-than-hours x
  [ "$status" -eq 2 ]
  pool frobnicate
  [ "$status" -eq 2 ]
  [ ! -s "$STUB_LOG" ]
}

# --- gc ---------------------------------------------------------------------

@test "gc deletes only old leftover Meguri simulators, shutting them down first" {
  local own check u7 fresh poolfree poolipad leased
  own="$(sim __add "iPhone 18 Pro" Shutdown)"
  check="$(sim __add Meguri-check-123-iPhone17Pro Booted)"
  u7="$(sim __add Meguri-U7-ipad Shutdown)"
  fresh="$(sim __add Meguri-U7-visual3-iphone Booted)"
  poolfree="$(sim __add Meguri-Pool-iphone-1 Booted)"
  poolipad="$(sim __add Meguri-Pool-ipad-7 Shutdown)"
  leased="$(sim __add Meguri-leased-elsewhere Booted)"
  mkdir -p "$LEASES/$leased"
  local u
  for u in "$own" "$check" "$u7" "$poolfree" "$poolipad" "$leased"; do age "$u"; done

  pool gc --list
  [ "$status" -eq 0 ]
  [ "$(jq -r '[.targets[].name] | sort | join(" ")' <<<"$output")" = "Meguri-U7-ipad Meguri-check-123-iPhone17Pro" ]
  [ "$(jq -r '.list_only, (.deleted | length)' <<<"$output" | tr '\n' ' ')" = "true 0 " ]
  [ ! -s "$STUB_LOG" ] || not_called '^xcrun simctl (shutdown|delete|erase) '
  [ "$(jq '.devices | length' "$FAKE_SIM_DB")" -eq 7 ]

  pool gc --older-than-hours 100000
  [ "$status" -eq 0 ]
  [ "$(jq '.targets | length' <<<"$output")" -eq 0 ]

  pool gc
  [ "$status" -eq 0 ]
  [ "$(jq -r '.deleted | sort | join(" ")' <<<"$output")" = "$(printf '%s\n' "$check" "$u7" | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//')" ]
  called "^xcrun simctl shutdown $check\$"
  not_called "^xcrun simctl shutdown $u7\$"
  called "^xcrun simctl delete $check\$"
  called "^xcrun simctl delete $u7\$"
  [ "$(count_calls '^xcrun simctl (shutdown|delete) ')" -eq 3 ]
  [ "$(jq -r '[.devices[].name] | sort | join(",")' "$FAKE_SIM_DB")" = "Meguri-Pool-ipad-7,Meguri-Pool-iphone-1,Meguri-U7-visual3-iphone,Meguri-leased-elsewhere,iPhone 18 Pro" ]
  [ ! -e "$MEGURI_SIM_DEVICES_DIR/$check" ]
}
