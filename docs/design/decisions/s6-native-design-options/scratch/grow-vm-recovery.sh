#!/bin/bash
# One-off: finishes growing the meguri-mac VM disk through the VM's own Recovery
# mode (Tart's documented route). Holds the VM lease until the VM is back.
set -uo pipefail
W=/Users/antoine/.herdr/worktrees/meguri/design
M=/Users/antoine/code/meguri
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
CFG="$HOME/Library/Application Support/meguri/mac-vm.json"
TART="$(jq -r .tart "$CFG")"; SSH="$(jq -r .ssh "$CFG")"
VM=meguri-mac
LOCK="$M/build/mg/mac-vm.$VM.lock"
exec > >(tee "$S/grow-vm-recovery.log") 2>&1
say() { printf '\n== %s\n' "$*"; }

mkdir "$LOCK" || { say "The VM lease is held by a running job. Try again when it ends."; cat "$LOCK/holder.json" 2>/dev/null; exit 3; }
trap 'rm -rf "$LOCK"; say "lease released"' EXIT
jq -nc --argjson pid $$ '{pid: $pid, job: "vm-disk-resize", holder: "design-session"}' > "$LOCK/holder.json"

say "checking for running tests"
RUNNING="$("$SSH" 'ps ax -o command | grep -E "xcodebuild|xctest|XCTRunner" | grep -v grep | head -3' </dev/null)"
if [ -n "$RUNNING" ]; then
  echo "$RUNNING" | cut -c1-110
  read -r -p "A test is running in the VM. Press Return to interrupt it, or Ctrl-C to cancel. " _
fi

say "stopping $VM"
"$TART" stop "$VM" --timeout 90 || exit 1

cat <<'STEPS'

== A VM window opens now. In that window:

  1. Choose Options, then Continue. Log in if it asks.
  2. In the menu bar, choose Utilities > Terminal.
  3. Check that disk0s3 is the Apple_APFS_Recovery partition:

       diskutil list

  4. If it is, run these one at a time:

       diskutil eraseVolume free free disk0s3
       yes | diskutil repairDisk disk0
       diskutil apfs resizeContainer disk0s2 0

  5. Shut down: Apple menu > Shut Down.

If step 3 shows anything else, shut down without running step 4.
This script continues when the VM has shut down.

STEPS

"$TART" run "$VM" --recovery
say "recovery session ended (exit $?)"

say "starting $VM headless"
# shellcheck source=/dev/null
. "$W/tools/scripts/mac-vm-lib.sh"
mvl_start "$TART" "$VM" "$S/vm-restart.log"
mvl_wait_ready "$SSH" "$TART" "$VM" true "$S/vm-restart.log" "$S/vm-restart.log.ssh" || { say "VM did not answer: ${MVL_REASON:-unknown}"; exit 1; }
"$SSH" 'df -h / | tail -1 | cut -c1-70; diskutil list physical | cut -c1-80' </dev/null
say "done"
