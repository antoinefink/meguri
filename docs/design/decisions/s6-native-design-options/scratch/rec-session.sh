#!/bin/bash
# Holds the VM lease, boots meguri-mac into Recovery with headless VNC, and restarts it
# normally when Recovery shuts down. A watchdog ends the session after 30 minutes.
set -uo pipefail
W=/Users/antoine/.herdr/worktrees/meguri/design
M=/Users/antoine/code/meguri
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
CFG="$HOME/Library/Application Support/meguri/mac-vm.json"
TART="$(jq -r .tart "$CFG")"; SSH="$(jq -r .ssh "$CFG")"; VM=meguri-mac
LOCK="$M/build/mg/mac-vm.$VM.lock"
say() { printf '== %s\n' "$*"; }
for _ in $(seq 1 120); do mkdir "$LOCK" 2>/dev/null && break; [ "$_" = 120 ] && { say "lease busy"; cat "$LOCK/holder.json"; exit 3; }; sleep 5; done
trap 'rm -rf "$LOCK"; say "lease released"' EXIT
jq -nc --argjson pid $$ '{pid: $pid, job: "vm-disk-resize", holder: "design-session"}' > "$LOCK/holder.json"
say "running in VM: $("$SSH" 'ps ax -o command | grep -E "xcodebuild|xctest|XCTRunner" | grep -v grep | wc -l' </dev/null)"
"$TART" stop "$VM" --timeout 90 || exit 1
say "booting recovery"
"$TART" run "$VM" --recovery --no-graphics --vnc-experimental > "$S/rec/tart.log" 2>&1 &
TP=$!
( sleep 1800; say "watchdog: stopping recovery"; "$TART" stop "$VM" --timeout 30 ) & WD=$!
wait "$TP"; say "recovery ended, exit $?"
kill "$WD" 2>/dev/null
. "$W/tools/scripts/mac-vm-lib.sh"
mvl_start "$TART" "$VM" "$S/vm-restart.log"
mvl_wait_ready "$SSH" "$TART" "$VM" true "$S/vm-restart.log" "$S/vm-restart.log.ssh" || { say "no answer: ${MVL_REASON:-}"; exit 1; }
"$SSH" 'df -h / | tail -1 | cut -c1-70; diskutil list physical | cut -c1-80' </dev/null
say done
