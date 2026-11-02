#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "vm-xcode-stage generated complete bundles and maintenance boundaries" {
  python3 -B "$REPO_ROOT/tools/tests/vm_xcode_stage_test.py"
}

@test "vm-xcode-stage resolves logical temporary base before private directory creation" {
  mkdir -m 700 "$BATS_TEST_TMPDIR/physical temp"
  ln -s "$BATS_TEST_TMPDIR/physical temp" "$BATS_TEST_TMPDIR/logical temp"
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools" MG_RUN_DIR="$2"
    TMPDIR="$2/logical temp"
    source "$1/tools/mg.d/vm-xcode.sh"
    mg_vm_xcode_stage_private() {
      python3 -B -I - "$MG_TOOLS/scripts/vm-xcode-stage.py" "$1" "$TMPDIR" <<"PY"
import importlib.util
import os
from pathlib import Path
import stat
import sys
spec = importlib.util.spec_from_file_location("stage", sys.argv[1])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
private, base = sys.argv[2:]
assert private == os.path.realpath(private)
assert str(Path(private).parent) == os.path.realpath(base)
with m.Directory(private) as directory:
    s = os.fstat(directory.fd)
    assert s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700
PY
      [ "$?" -eq 0 ] || return 1
      printf "%s\n" "{\"scope\":\"VM-XCODE-MAINTENANCE-ONLY\",\"outcome\":\"already-selected\",\"checks\":{}}" >"$2"
    }
    mg_artifact() { :; }
    mg_message() { :; }
    mg_status() { [ "$1" = pass ]; }
    mg_failure() { return 1; }
    mg_vm_xcode_stage
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

@test "vm-xcode-stage never drains a recording holder through a nested slot" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/vm-xcode.sh"
    MG_LEASE_DIR="$2/lease" MG_LEASE_SLOT="$2/lease/inner"
    mvl_stop_tests() { echo forbidden; return 0; }
    mg_vm_xcode_stop_exclusive
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 1 ]
  [ -z "$output" ]
}

@test "vm-xcode-stage retains its lease until cancellation joins original worker" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/host.sh"
    source "$1/tools/mg.d/vm-xcode.sh"
    MG_EXIT_CMDS=()
    private="$2/private" MG_LEASE_DIR="$2/lease"
    mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
    MG_VM_XCODE_GUARD=1 MG_VM_XCODE_STARTED=1
    echo pending >"$private/receiver-pending.json"
    log="$2/events"
    mg_on_exit '\''[ -f "$private/receiver-joined.json" ] && [ ! -e "$MG_LEASE_DIR/vm-xcode-pending" ] && echo release >> "$log"'\''
    mg_on_exit "echo drain >> \"$log\""
    (trap '\''echo joined > "$private/receiver-joined.json"; echo safe > "$private/lease-safe.json"; echo joined >> "$log"; exit 0'\'' TERM; echo ready >"$2/ready"; while :; do read -t 1 -r ignored || :; done) &
    MG_VM_XCODE_WORKER=$!
    mg_on_exit mg_vm_xcode_join
    while [ ! -f "$2/ready" ]; do :; done
    mg__run_exit_cmds
    trap - EXIT
    [ "$(cat "$log")" = "$(printf "joined\ndrain\nrelease")" ]
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
}

@test "vm-xcode-stage pending operation retains the lease across dead-holder cleanup" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/host.sh"
    source "$1/tools/mg.d/vm-xcode.sh"
    private="$2/private" MG_LEASE_DIR="$2/lease"
    mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
    echo pending >"$private/receiver-pending.json"
    MG_EXIT_CMDS=()
    mg_on_exit "echo forbidden-release > \"$2/released\""
    mg_on_exit mg_vm_xcode_join
    mg__run_exit_cmds
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 1 ]
  [ ! -e "$BATS_TEST_TMPDIR/released" ]
  [ -d "$BATS_TEST_TMPDIR/lease/vm-xcode-pending" ]
  run bash -c '
    MG_TOOLS="$1/tools"
    mg_job() { :; }
    source "$1/tools/mg.d/host.sh"
    if mg__lease_stale "$2/lease" '\''{"pid":2147483647}'\''; then exit 1; fi
    if host_lease_drop "$2/lease"; then exit 1; fi
    [ -d "$2/lease/vm-xcode-pending" ]
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
}

@test "vm-xcode-stage receiver join cannot release an unresolved activation" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/host.sh"
    source "$1/tools/mg.d/vm-xcode.sh"
    private="$2/private" MG_LEASE_DIR="$2/lease"
    mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
    echo joined >"$private/receiver-joined.json"
    MG_VM_XCODE_GUARD=1 MG_VM_XCODE_STARTED=1 MG_VM_XCODE_FINISHED=1
    MG_EXIT_CMDS=()
    mg_on_exit "echo forbidden-release > \"$2/released\""
    mg_on_exit "echo forbidden-drain > \"$2/drained\""
    mg_on_exit mg_vm_xcode_join
    mg__run_exit_cmds
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 1 ]
  [ -f "$BATS_TEST_TMPDIR/private/receiver-joined.json" ]
  [ ! -e "$BATS_TEST_TMPDIR/private/lease-safe.json" ]
  [ ! -e "$BATS_TEST_TMPDIR/drained" ]
  [ ! -e "$BATS_TEST_TMPDIR/released" ]
  [ -d "$BATS_TEST_TMPDIR/lease/vm-xcode-pending" ]
}

@test "vm-xcode-stage confirmed activation removes guard before drain and release" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/host.sh"
    source "$1/tools/mg.d/vm-xcode.sh"
    private="$2/private" MG_LEASE_DIR="$2/lease" log="$2/events"
    mkdir -p "$private" "$MG_LEASE_DIR/vm-xcode-pending"
    echo joined >"$private/receiver-joined.json"
    MG_VM_XCODE_GUARD=1 MG_VM_XCODE_STARTED=1 MG_VM_XCODE_FINISHED=1
    MG_EXIT_CMDS=()
    mg_on_exit '\''[ ! -e "$MG_LEASE_DIR/vm-xcode-pending" ] && echo release >> "$log"'\''
    mg_on_exit '\''[ ! -e "$MG_LEASE_DIR/vm-xcode-pending" ] && echo drain >> "$log"'\''
    mg_on_exit mg_vm_xcode_join
    [ -d "$MG_LEASE_DIR/vm-xcode-pending" ] || exit 1
    echo activation-completed >> "$log"
    echo safe >"$private/lease-safe.json"
    mg__run_exit_cmds
    trap - EXIT
    [ "$(cat "$log")" = "$(printf "activation-completed\ndrain\nrelease")" ]
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ ! -d "$BATS_TEST_TMPDIR/lease/vm-xcode-pending" ]
}

@test "vm-xcode-stage queued report is not a passing activation or app gate" {
  run bash -c '
    mg_job() { :; }
    MG_TOOLS="$1/tools" MG_RUN_DIR="$2"
    source "$1/tools/mg.d/vm-xcode.sh"
    mg_vm_xcode_stage_private() {
      jq -nc '\''{scope:"VM-XCODE-MAINTENANCE-ONLY",outcome:"staged",version:"27.2",build:"18C123",checks:{selection_matches:false}}'\'' >"$2"
      return 75
    }
    mg_artifact() { :; }
    mg_user_step() { [ "$1" = mac-vm ]; }
    mg_queued_part() { [ "$1" = mac-vm ]; }
    mg_message() { :; }
    mg_status() { [ "$1" = warn ]; }
    mg_failure() { return 1; }
    mg_vm_xcode_stage
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
}
