#!/bin/bash
# Restarts meguri-mac in its normal headless mode, under the VM lease.
set -uo pipefail
W=/Users/antoine/.herdr/worktrees/meguri/design; M=/Users/antoine/code/meguri
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
CFG="$HOME/Library/Application Support/meguri/mac-vm.json"; TART="$(jq -r .tart "$CFG")"; SSH="$(jq -r .ssh "$CFG")"; VM=meguri-mac
LOCK="$M/build/mg/mac-vm.$VM.lock"
for i in $(seq 1 120); do mkdir "$LOCK" 2>/dev/null && break; [ "$i" = 120 ] && { echo "lease busy"; exit 3; }; sleep 5; done
trap 'rm -rf "$LOCK"; echo "lease released"' EXIT
jq -nc --argjson pid $$ '{pid: $pid, job: "vm-restart", holder: "design-session"}' > "$LOCK/holder.json"
"$TART" stop "$VM" --timeout 60 || exit 1
. "$W/tools/scripts/mac-vm-lib.sh"
mvl_start "$TART" "$VM" "$S/vm-restart.log"
mvl_wait_ready "$SSH" "$TART" "$VM" true "$S/vm-restart.log" "$S/vm-restart.log.ssh" || { echo "no answer: ${MVL_REASON:-}"; exit 1; }
sleep 10; "$SSH" 'echo "console: $(stat -f %Su /dev/console)"; df -h / | tail -1 | cut -c1-60' </dev/null
