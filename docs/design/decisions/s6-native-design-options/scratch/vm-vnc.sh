#!/bin/bash
# Restarts meguri-mac with Tart's headless VNC on, under the VM lease. The VM stays a normal
# running VM for tools/mg afterwards; VNC only adds a screen and input for the design session.
set -uo pipefail
W=/Users/antoine/.herdr/worktrees/meguri/design
M=/Users/antoine/code/meguri
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
CFG="$HOME/Library/Application Support/meguri/mac-vm.json"
TART="$(jq -r .tart "$CFG")"; SSH="$(jq -r .ssh "$CFG")"; VM=meguri-mac
LOCK="$M/build/mg/mac-vm.$VM.lock"
say() { printf '== %s\n' "$*"; }
for i in $(seq 1 120); do mkdir "$LOCK" 2>/dev/null && break; [ "$i" = 120 ] && { say "lease busy"; cat "$LOCK/holder.json"; exit 3; }; sleep 5; done
trap 'rm -rf "$LOCK"; say "lease released"' EXIT
jq -nc --argjson pid $$ '{pid: $pid, job: "vm-vnc-restart", holder: "design-session"}' > "$LOCK/holder.json"
true
"$TART" list | grep -E "^local +meguri-mac " | grep -q running && { "$TART" stop "$VM" --timeout 60 || exit 1; }
: > "$S/rec/tart.log"
( cd / && exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec: $!\n"' \
    "$TART" run --no-graphics --vnc-experimental "$VM" </dev/null >"$S/rec/tart.log" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&- )
. "$W/tools/scripts/mac-vm-lib.sh"
mvl_wait_ready "$SSH" "$TART" "$VM" true "$S/rec/tart.log" "$S/rec/ssh.err" || { say "no answer: ${MVL_REASON:-}"; exit 1; }
sleep 15
"$SSH" 'echo "console: $(stat -f %Su /dev/console)"; df -h / | tail -1 | cut -c1-60' </dev/null
say done
