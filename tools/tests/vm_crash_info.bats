#!/usr/bin/env bats
# Synthetic checks only; authored unrun in the fixer session.
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "vm-crash-info synthetic report and original admission boundaries" {
  python3 -B "$REPO_ROOT/tools/tests/vm_crash_info_test.py"
}

@test "vm-crash-info rejects unrestricted input before VM access" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/vm-crash.sh"
    mg_failure() { :; }
    mg_status() { [ "$1" = error ]; }
    mg_vm_crash_private() { echo forbidden-vm; return 1; }
    for option in --pid --path --command --suite --restart; do
      mg_vm_crash_info "$option" generated
    done
    mg_vm_crash_info --mac-ui-run ../../generated --suite drive-generated
    mg_vm_crash_info --mac-ui-run 20260101T010000Z-321 --suite ../../generated
  ' _ "$REPO_ROOT"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

@test "vm-crash-info missing original inputs refuse before config lease or guest" {
  mkdir -m 700 "$BATS_TEST_TMPDIR/private"
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/vm-crash.sh"
    MG_ROOT="$2" MG_TOOLS="$1/tools"
    mvl_read_config() { echo forbidden-config; return 0; }
    mg__mac_vm_lease() { echo forbidden-lease; return 0; }
    mvl_state() { echo forbidden-state; return 0; }
    mg_vm_crash_private "$2/private" "$2/report.json" 20260101T010000Z-321 drive-generated
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -ne 0 ]
  [ -z "$output" ]
  [ ! -e "$BATS_TEST_TMPDIR/report.json" ]
}

@test "vm-crash-info has no take drain restart copy or unrestricted command option" {
  run python3 -B - "$REPO_ROOT" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1])
shell = (root / 'tools/mg.d/vm-crash.sh').read_text()
helper = (root / 'tools/scripts/vm-crash-info.py').read_text()
for forbidden in ('mg_mac_vm_take', 'mvl_stop_tests', 'mg__mac_vm_stop_orphans',
                  'mvl_start', 'mvl_ensure_ready', 'mg_mac_vm_copy', 'mvl_copy',
                  'MG_MAC_VM_TESTING=1'):
    assert forbidden not in shell
assert 'mg__mac_vm_lease' in shell
assert shell.index('--admit') < shell.index('mvl_read_config') < shell.index('mg__mac_vm_lease')
assert "'--pid'" not in helper and "'--path'" not in helper and "'--command'" not in helper
assert 'shlex.join' in helper and 'owned.command' in helper
assert 'subprocess.run(' not in helper
assert 'mg_status pass' not in shell
PY
  [ "$status" -eq 0 ]
}

@test "vm-crash-info cancellation joins original worker before removing guard and releasing" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/host.sh"
    source "$1/tools/mg.d/vm-xcode.sh"
    source "$1/tools/mg.d/vm-crash.sh"
    MG_EXIT_CMDS=()
    private="$2/private" MG_LEASE_DIR="$2/lease" log="$2/events"
    mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
    MG_VM_CRASH_GUARD=1 MG_VM_CRASH_STARTED=1 MG_VM_CRASH_FINISHED=""
    mg_on_exit '\''[ -f "$private/lease-safe.json" ] && [ ! -e "$MG_LEASE_DIR/vm-xcode-pending" ] && echo release >> "$log"'\''
    (trap '\''echo original-completed >> "$log"; echo safe > "$private/lease-safe.json"; exit 1'\'' TERM; echo ready > "$2/ready"; while :; do read -t 1 -r ignored || :; done) &
    MG_VM_CRASH_WORKER=$!
    mg_on_exit mg_vm_crash_join
    while [ ! -f "$2/ready" ]; do :; done
    mg__run_exit_cmds
    trap - EXIT
    [ "$(cat "$log")" = "$(printf "original-completed\nrelease")" ]
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
}

@test "vm-crash-info local join or earlier proof cannot release unknown original guest" {
  for mode in local-joined launch-unknown; do
    mkdir -p "$BATS_TEST_TMPDIR/$mode"
    run bash -c '
      mg_job() { :; }
      MG_TOOLS="$1/tools"
      source "$1/tools/mg.d/host.sh"
      source "$1/tools/mg.d/vm-xcode.sh"
      source "$1/tools/mg.d/vm-crash.sh"
      MG_EXIT_CMDS=()
      private="$2/private" MG_LEASE_DIR="$2/lease"
      mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
      echo joined > "$private/receiver-joined.json"
      MG_VM_CRASH_GUARD=1 MG_VM_CRASH_STARTED=1 MG_VM_CRASH_WORKER=""
      MG_VM_CRASH_FINISHED=""
      [ "$3" != local-joined ] || MG_VM_CRASH_FINISHED=1
      mg_on_exit "echo forbidden-release > \"$2/released\""
      mg_on_exit mg_vm_crash_join
      mg__run_exit_cmds
    ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR/$mode" "$mode"
    [ "$status" -eq 1 ]
    [ ! -e "$BATS_TEST_TMPDIR/$mode/released" ]
    [ -d "$BATS_TEST_TMPDIR/$mode/lease/vm-xcode-pending" ]
    run bash -c '
      mg_job() { :; }
      MG_TOOLS="$1/tools"
      source "$1/tools/mg.d/host.sh"
      if mg__lease_stale "$2/lease" '\''{"pid":2147483647}'\''; then exit 1; fi
      if host_lease_drop "$2/lease"; then exit 1; fi
      [ -d "$2/lease/vm-xcode-pending" ]
    ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR/$mode"
    [ "$status" -eq 0 ]
  done
}

@test "vm-crash-info original cancellation without guest proof preserves failure and lease" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/host.sh"
    source "$1/tools/mg.d/vm-xcode.sh"
    source "$1/tools/mg.d/vm-crash.sh"
    MG_EXIT_CMDS=()
    private="$2/private" MG_LEASE_DIR="$2/lease" log="$2/events"
    mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
    MG_VM_CRASH_GUARD=1 MG_VM_CRASH_STARTED=1 MG_VM_CRASH_FINISHED=""
    mg_on_exit "echo forbidden-release > \"$2/released\""
    (trap '\''echo local-stopped > "$log"; exit 1'\'' TERM; echo ready > "$2/ready"; while :; do read -t 1 -r ignored || :; done) &
    MG_VM_CRASH_WORKER=$!
    mg_on_exit mg_vm_crash_join
    while [ ! -f "$2/ready" ]; do :; done
    mg__run_exit_cmds
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 1 ]
  [ "$(cat "$BATS_TEST_TMPDIR/events")" = local-stopped ]
  [ ! -e "$BATS_TEST_TMPDIR/released" ]
  [ ! -e "$BATS_TEST_TMPDIR/private/lease-safe.json" ]
  [ -d "$BATS_TEST_TMPDIR/lease/vm-xcode-pending" ]
}
