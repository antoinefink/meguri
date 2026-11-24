#!/bin/bash
# Fixed guest-only posture run. The pool mutex covers validation and XCUI
# input; release/reset take this same mutex before touching a device.
set -u
[ "$#" -eq 3 ] || exit 2
source "$(cd "$(dirname "$0")/../scripts" && pwd -P)/sim-pool.sh"
trap sp_on_exit EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
trap 'exit 129' HUP
SP_KINDS="$(sp_kinds)" || exit 1
sp_lock
context="$(cat "$1")" || exit 1
request="${TEST_RUNNER_MEGURI_POSTURE_REQUEST:-null}"
# Compare the complete acquisition under the mutex, including its unique id.
# No earlier host inventory check authorizes this transaction on its own.
pool="$(sp_status)" || exit 1
if ! jq -e --argjson c "$context" --argjson r "$request" --argjson now "$(sp_now)" '
  . as $pool | [.devices[] | select(.udid == $r.udid)] | select(length == 1) | .[0] |
  ([$pool.devices[] | select(.name == $r.name)] | length) == 1 and
  .runtime == $c.device.runtime and .udid == $c.device.udid and .name == $c.device.name and .name == $r.name and
  .kind == "foldable" and .state == "Booted" and .available == true and .current_runtime == true and
  .leased == true and .lease == $c.device.lease and
  (.lease.acquisition_id | type == "string" and length > 0) and
  .lease.worktree == $c.root and .lease.vm == $c.vm and .lease.udid == .udid and .lease.kind == "foldable" and
  $r.mutationUntil > ($now + 5) and $r.mutationUntil <= ($now + 90) and
  .lease.expires_epoch > ($now + 120)' <<<"$pool" >/dev/null; then
  echo 'sim-posture: invalid-target-or-lease' >&2
  exit 65
fi
# A signal between spawning the child and recording its pid must not free
# the mutex. Defer cancellation until that child can be joined.
cancelled=""
test_pid=""
trap 'cancelled=1' TERM INT HUP
stop_test() {
  trap '' TERM INT HUP
  wait "$test_pid" 2>/dev/null || :
  exit 143
}
[ -z "$cancelled" ] || exit 143
xcodebuild test-without-building -xctestrun "$2" \
  -destination 'platform=macOS,arch=arm64' -resultBundlePath "$3.xcresult" -collect-test-diagnostics never \
  -only-testing:MeguriUITests/SimulatorPostureTests/testRequestedPosture \
  -parallel-testing-enabled NO -destination-timeout 15 \
  -maximum-test-execution-time-allowance 30 -test-timeouts-enabled YES &
test_pid=$!
# A disconnected SSH leaves this transaction holding its own lock until
# XCTest joins. Cancellation never hands another caller an active device.
trap stop_test TERM INT HUP
[ -z "$cancelled" ] || stop_test
wait "$test_pid"
exit "$?"
