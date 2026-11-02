#!/bin/bash
# One-off: grows the meguri-mac VM disk to 300 GB. Holds the VM lease throughout,
# keeps an APFS clone as a backup, and stops at the first failed step.
set -euo pipefail
W=/Users/antoine/.herdr/worktrees/meguri/design
M=/Users/antoine/code/meguri
CFG="$HOME/Library/Application Support/meguri/mac-vm.json"
TART="$(jq -r .tart "$CFG")"; SSH="$(jq -r .ssh "$CFG")"
VM=meguri-mac; IMG=/Volumes/T9/meguri-vm/tart-home/vms/$VM/disk.img
LOCK="$M/build/mg/mac-vm.$VM.lock"
say() { printf '== %s\n' "$*"; }

mkdir "$LOCK" || { say "VM lease is held; not touching it"; cat "$LOCK/holder.json" 2>/dev/null; exit 3; }
trap 'rm -rf "$LOCK"; say "lease released"' EXIT
jq -nc --argjson pid $$ '{pid: $pid, job: "vm-disk-resize", holder: "design-session"}' > "$LOCK/holder.json"

say "checking for running tests"
"$SSH" 'ps ax -o command | grep -E "xcodebuild|xctest|XCTRunner" | grep -v grep | head -3' || true

say "stopping $VM"
"$TART" stop "$VM" --timeout 90
"$TART" list | grep "$VM"

say "backup clone"
"$TART" list | grep -q "$VM-preresize" || "$TART" clone "$VM" "$VM-preresize"

say "growing the disk image"
"$TART" set "$VM" --disk-size 300
ls -la "$IMG" | cut -c1-90

say "attaching without mounting"
DISK="$(hdiutil attach -imagekey diskimage-class=CRawDiskImage -nomount "$IMG" | head -1 | awk '{print $1}')"
say "attached at $DISK"
detach() { hdiutil detach "$DISK" -force >/dev/null 2>&1 || true; }
trap 'detach; rm -rf "$LOCK"; say "lease released"' EXIT
diskutil list "$DISK" | cut -c1-80
TYPE="$(diskutil info "${DISK}s3" | awk -F: '/Partition Type/{gsub(/^ +/,"",$2);print $2}')"
[ "$TYPE" = Apple_APFS_Recovery ] || { say "partition 3 is $TYPE, expected Apple_APFS_Recovery; stopping"; exit 4; }

say "removing the recovery partition"
diskutil eraseVolume free free "${DISK}s3" 2>&1 | tail -2 | cut -c1-110
say "repairing the partition map"
yes | diskutil repairDisk "$DISK" 2>&1 | tail -2 | cut -c1-110
say "expanding the system container"
diskutil apfs resizeContainer "${DISK}s2" 0 2>&1 | tail -3 | cut -c1-110
diskutil list "$DISK" | cut -c1-80
hdiutil detach "$DISK"
trap 'rm -rf "$LOCK"; say "lease released"' EXIT

say "starting $VM headless"
# shellcheck source=/dev/null
. "$W/tools/scripts/mac-vm-lib.sh"
mvl_start "$TART" "$VM" "/tmp/meguri-vm-grow.log"
for _ in $(seq 1 60); do
  if "$SSH" 'true' </dev/null >/dev/null 2>&1; then break; fi
  sleep 5
done
"$SSH" 'df -h / | tail -1 | cut -c1-70; xcrun simctl list runtimes | cut -c1-80' </dev/null
say "done"
