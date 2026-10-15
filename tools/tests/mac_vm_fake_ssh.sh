#!/usr/bin/env bash
# A fake SSH wrapper for one VM of mac_vm_fake_tart.sh ($FAKE_VM_NAME), for
# tests/mac_vm.bats. Like /Volumes/T9/meguri-vm/bin/vm-ssh, it takes the
# remote command as its arguments. It runs that command here, in bash, as the
# VM would: paths under $FAKE_VM_MIRROR (the test's temp dir) move to
# $FAKE_VM_DIR/<vm>/fs/<path>, so "the same absolute path in the VM" is a
# separate copy, and the VM's commands (xcodebuild, sudo, log, stat) come
# first from $FAKE_VM_BIN, with FAKE_VM_FS set to the VM's file system root.
# The command gets a clean environment, as over SSH: only the fakes' own
# variables (FAKE_*, STUB_LOG, MEGURI_HOST_LEASE_DIR), HOME and PATH, so a
# variable the job forgot to pass never reaches it. A VM that is not running,
# or $FAKE_VM_DIR/ssh-down, refuses the connection (exit 255). Every call is
# logged to $STUB_LOG.
set -u
vm="$FAKE_VM_DIR/$FAKE_VM_NAME"
printf 'ssh %s %s\n' "$FAKE_VM_NAME" "$*" >>"${STUB_LOG:-/dev/null}"
if [ "$(cat "$vm/state" 2>/dev/null)" != running ] || [ -e "$FAKE_VM_DIR/ssh-down" ]; then
  echo "ssh: connect to host 192.168.64.2 port 22: Connection refused" >&2
  exit 255
fi
mkdir -p "$vm/fs"
cmd="$*"
cmd="${cmd//"$FAKE_VM_MIRROR"/"$vm/fs$FAKE_VM_MIRROR"}"
envs=()
for n in $(compgen -e); do
  case "$n" in FAKE_* | STUB_LOG | MEGURI_HOST_LEASE_DIR | HOME) envs+=("$n=${!n}") ;; esac
done
exec env -i "${envs[@]}" PATH="$FAKE_VM_BIN:$PATH" FAKE_VM_FS="$vm/fs" FAKE_VM="$vm" bash -c "$cmd"
