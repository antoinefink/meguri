#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent delivery generated transport tests supply no signed proof" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_install_test.py"
}

@test "CloudAgent child requires prepared inputs and parent stays implementation missing" {
  run bash -c '
    mg_job() { :; }
    mg_failure() { printf "failure:%s\n" "$1"; }
    mg_status() { printf "status:%s\n" "$1"; }
    mg_detail() { printf "detail:%s:%s\n" "$1" "$2"; }
    MG_ROOT="$1" MG_RUN_DIR="$2"
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_agent_install
    mg_cloudkit_signed
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [[ "$output" == *"failure:usage"* ]]
  [[ "$output" == *"failure:implementation_missing"* ]]
  [[ "$output" != *"status:pass"* ]]
  [[ "$output" != *"awaiting_user"* ]]
}

@test "CloudAgent private VM selection queues without leaking routes or touching host" {
  run bash -c '
    mg_job() { :; }
    mg_mac_vm_probe() { MG_MAC_VMS="[{\"name\":\"private-fixture-route\"}]"; return 0; }
    mg_mac_vm_take() {
      [ "$*" = "--no-copy --running-only" ] || exit 9
      printf "private-fixture-wrapper-and-identifier\n" >&2
      return 2
    }
    MG_ROOT="$1" MG_RUN_DIR="$2" MG_RUN_ID=20261102T010100Z-102
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_agent_install_private "$2" 20261102T010200Z-103 "" >"$2/private.log" 2>&1
    rc=$?
    printf "private-phase-exit:%s\n" "$rc"
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = "private-phase-exit:75" ]
}

@test "CloudAgent running-only VM take never boots a stopped guest" {
  run bash -c '
    MG_TOOLS="$1/tools"
    source "$1/tools/mg.d/mac-vm.sh"
    mg__mac_vm_lease() { MG_MAC_VM="{\"name\":\"generated\",\"tart\":\"unused\"}"; }
    mg_on_exit() { :; }
    mvl_state() { printf "stopped\n"; }
    mg__mac_vm_boot() { printf "unexpected-boot\n"; return 9; }
    mvl_wait_ready() { printf "unexpected-wait\n"; return 9; }
    mg__mac_vm_unreachable() { printf "queued:%s\n" "$1"; }
    mg__mac_vm_release_queued() { :; }
    mg_mac_vm_take --no-copy --running-only
  ' _ "$REPO_ROOT"
  [ "$status" -eq 2 ]
  [ "$output" = "queued:selected VM is not running" ]
}
