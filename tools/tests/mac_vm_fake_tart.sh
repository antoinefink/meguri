#!/usr/bin/env bash
# A fake tart for the Mac UI VM self-tests (tests/mac_vm.bats). Each VM is a
# directory $FAKE_VM_DIR/<name>/ whose `state` file holds running or stopped
# (no file: no such VM) and whose fs/ is the VM's file system (see
# mac_vm_fake_ssh.sh). Every call is logged to $STUB_LOG.
#
#   list --format json   the VMs, like tart 2.32
#   run [options] <name> marks the VM running and stays in the foreground,
#                        like the real one; FAKE_TART_RUN_FAILS=1 makes it
#                        fail at once. Its pid goes to $FAKE_VM_DIR/run.pids.
#   ip <name>            an address
set -u
printf 'tart %s\n' "$*" >>"${STUB_LOG:-/dev/null}"
case "${1:-}" in
  list)
    for d in "$FAKE_VM_DIR"/*/; do
      [ -f "$d/state" ] || continue
      jq -n --arg n "$(basename "$d")" --arg s "$(cat "$d/state")" \
        '{Source: "local", Name: $n, Size: 36, Running: ($s == "running"), State: $s, Disk: 150}'
    done | jq -s .
    ;;
  run)
    shift
    name=""
    for a in "$@"; do
      case "$a" in --*) ;; *) name="$a" ;; esac
    done
    if [ ! -f "$FAKE_VM_DIR/$name/state" ]; then
      echo "Error: the specified VM \"$name\" does not exist" >&2
      exit 1
    fi
    if [ "${FAKE_TART_RUN_FAILS:-}" = 1 ]; then
      echo "Error: Error Domain=VZErrorDomain Code=1 \"The virtual machine failed to start.\"" >&2
      exit 1
    fi
    echo "$$" >>"$FAKE_VM_DIR/run.pids"
    echo running >"$FAKE_VM_DIR/$name/state"
    exec sleep 600
    ;;
  ip) echo 192.168.64.2 ;;
  *) exit 97 ;;
esac
